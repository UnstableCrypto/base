//! Background DHT crawl service: BFS discovery of all peers on a target network.
//!
//! Runs discv5 BFS and discv4 stream discovery in parallel, sharing a single
//! seen-set so nodes found via either protocol feed into the other's traversal.

use std::collections::HashSet;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use discv5::enr::NodeId;
use discv5::Enr;
use reth_network_peers::NodeRecord;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::discv4_prober::{Discv4Crawler, node_record_to_enode};
use crate::prober::BootnodeProber;

/// A peer found during a DHT crawl.
#[derive(Debug)]
pub struct DiscoveredPeer {
    /// First 8 hex characters of the NodeId.
    pub node_id_prefix: String,
    /// `"ip:port"` address string.
    pub address: String,
    /// Network identification tag (e.g. `"base-zeronet/jovian"`).
    pub network_tag: &'static str,
    /// Discovery protocol used to find this peer (`"discv4"` or `"discv5"`).
    pub protocol: &'static str,
    /// Milliseconds elapsed since the crawl started when this peer was found.
    pub found_at_ms: u64,
}

/// Progress updates sent from the background crawl task to the UI.
#[derive(Debug)]
pub enum DiscoveryUpdate {
    /// A crawl is starting (or restarting) — the UI should clear all previous state.
    Reset,
    /// A peer on the target network was found.
    Peer(DiscoveredPeer),
    /// Periodic progress update.
    Progress {
        /// Nodes scanned so far.
        scanned: usize,
        /// Nodes currently waiting in the queue.
        queued: usize,
        /// Total unique nodes encountered in this crawl cycle.
        encountered: usize,
        /// Elapsed seconds since crawl start.
        elapsed_secs: f64,
    },
}

/// Runs the discovery background service.
///
/// Waits for an initial trigger on `trigger_rx`, then crawls the DHT
/// continuously via both discv5 and discv4. When the BFS queue is exhausted
/// it re-seeds from bootnodes and keeps going. A new trigger resets everything
/// and starts a fresh crawl. Only peers whose network tag starts with
/// `target_prefix` are reported as `DiscoveryUpdate::Peer`.
pub async fn run_discovery_service(
    fork_hash: [u8; 4],
    target_prefix: &'static str,
    bootnodes: Vec<String>,
    mut trigger_rx: mpsc::Receiver<()>,
    update_tx: mpsc::Sender<DiscoveryUpdate>,
) {
    if trigger_rx.recv().await.is_none() {
        return;
    }

    loop {
        let _ = update_tx.send(DiscoveryUpdate::Reset).await;
        info!(target_prefix = %target_prefix, "starting DHT crawl");

        let restarted =
            crawl(&fork_hash, target_prefix, &bootnodes, &update_tx, &mut trigger_rx).await;
        if !restarted {
            return;
        }
        info!(target_prefix = %target_prefix, "DHT crawl restarted by trigger");
    }
}

/// Runs a continuous DHT crawl using both discv5 BFS and discv4 stream discovery.
///
/// Returns `true` if a trigger caused an early restart, `false` if the
/// trigger channel was closed (caller should exit).
async fn crawl(
    fork_hash: &[u8; 4],
    target_prefix: &'static str,
    bootnodes: &[String],
    update_tx: &mpsc::Sender<DiscoveryUpdate>,
    trigger_rx: &mut mpsc::Receiver<()>,
) -> bool {
    let start = Instant::now();

    let mut prober = match BootnodeProber::new(*fork_hash).await {
        Ok(p) => p,
        Err(e) => {
            warn!(error = %e, "failed to create BootnodeProber for crawl");
            return false;
        }
    };

    // Channel for discv4-discovered NodeRecords. Discv4 init runs in a
    // background task so it never blocks the discv5 BFS loop.
    let (discv4_tx, mut discv4_rx) = mpsc::channel::<NodeRecord>(256);

    let enode_bootnodes: Vec<String> =
        bootnodes.iter().filter(|b| b.starts_with("enode://")).cloned().collect();
    if !enode_bootnodes.is_empty() {
        let fh = *fork_hash;
        tokio::spawn(async move {
            match Discv4Crawler::new(fh).await {
                Ok(crawler) => {
                    for bn in &enode_bootnodes {
                        match bn.parse::<NodeRecord>() {
                            Ok(record) => crawler.add_bootnode(record),
                            Err(e) => warn!(error = %e, bootnode = %bn, "failed to parse enode"),
                        }
                    }
                    if let Err(e) = crawler.spawn_into(discv4_tx).await {
                        warn!(error = %e, "failed to spawn discv4 crawler");
                    }
                }
                Err(e) => warn!(error = %e, "failed to create discv4 crawler"),
            }
        });
    }

    let mut queue: Vec<Enr> = Vec::new();
    let mut seen: HashSet<NodeId> = HashSet::new();
    let mut emitted: HashSet<NodeId> = HashSet::new();
    let mut discv4_seen: HashSet<String> = HashSet::new();
    let mut scanned = 0usize;
    let mut encountered = 0usize;

    encountered += seed_queue(&mut prober, bootnodes, &mut seen, &mut queue).await;

    let mut i = 0;
    loop {
        // Process at most one discv4-discovered node per BFS iteration so the
        // discv5 queue keeps making progress (each resolve_bootnode can take 5s).
        if let Ok(record) = discv4_rx.try_recv() {
            let addr_key = discv4_node_key(&record);
            if discv4_seen.insert(addr_key) {
                encountered += 1;
                if let Some(enode_str) = node_record_to_enode(&record) {
                    if let Some(enr) = prober.resolve_bootnode(&enode_str).await {
                        let node_id = enr.node_id();
                        let tag = crate::fork_id::network_tag(&enr);
                        let raw = node_id.raw();
                        let prefix = format!(
                            "{:02x}{:02x}{:02x}{:02x}",
                            raw[0], raw[1], raw[2], raw[3]
                        );
                        let address = enr
                            .ip4()
                            .map(|ip| format!("{}:{}", ip, enr.udp4().unwrap_or(0)))
                            .unwrap_or_else(|| enode_addr_str(&record));
                        let found_at_ms = start.elapsed().as_millis() as u64;

                        if tag.starts_with(target_prefix) && emitted.insert(node_id) {
                            let _ = update_tx
                                .send(DiscoveryUpdate::Peer(DiscoveredPeer {
                                    node_id_prefix: prefix,
                                    address,
                                    network_tag: tag,
                                    protocol: "discv4",
                                    found_at_ms,
                                }))
                                .await;
                        }

                        if seen.insert(node_id) {
                            queue.push(enr);
                        }
                    }
                }
            }
        }

        if i >= queue.len() {
            // Queue exhausted — clear seen set and re-seed to keep crawling.
            seen.clear();
            queue.clear();
            i = 0;
            encountered += seed_queue(&mut prober, bootnodes, &mut seen, &mut queue).await;
            if queue.is_empty() {
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        }

        // Non-blocking check for a restart trigger.
        match trigger_rx.try_recv() {
            Ok(()) => return true,
            Err(mpsc::error::TryRecvError::Disconnected) => return false,
            Err(mpsc::error::TryRecvError::Empty) => {}
        }

        let enr = queue[i].clone();
        i += 1;
        scanned += 1;

        let peers = prober.query_routing_table(enr).await;

        for peer in peers {
            let node_id = peer.node_id();
            if seen.insert(node_id) {
                encountered += 1;
                let tag = crate::fork_id::network_tag(&peer);
                let raw = node_id.raw();
                let prefix =
                    format!("{:02x}{:02x}{:02x}{:02x}", raw[0], raw[1], raw[2], raw[3]);
                let address = peer
                    .ip4()
                    .map(|ip| format!("{}:{}", ip, peer.udp4().unwrap_or(0)))
                    .unwrap_or_else(|| "unknown".to_string());
                let found_at_ms = start.elapsed().as_millis() as u64;

                if tag.starts_with(target_prefix) && emitted.insert(node_id) {
                    let _ = update_tx
                        .send(DiscoveryUpdate::Peer(DiscoveredPeer {
                            node_id_prefix: prefix,
                            address,
                            network_tag: tag,
                            protocol: "discv5",
                            found_at_ms,
                        }))
                        .await;
                }

                queue.push(peer);
            }
        }

        let elapsed_secs = start.elapsed().as_secs_f64();
        let queued = queue.len().saturating_sub(i);
        let _ = update_tx
            .send(DiscoveryUpdate::Progress { scanned, queued, encountered, elapsed_secs })
            .await;
    }
}

async fn seed_queue(
    prober: &mut BootnodeProber,
    bootnodes: &[String],
    seen: &mut HashSet<NodeId>,
    queue: &mut Vec<Enr>,
) -> usize {
    let mut added = 0;
    for bn in bootnodes {
        if let Some(enr) = prober.resolve_bootnode(bn).await {
            let node_id = enr.node_id();
            if seen.insert(node_id) {
                queue.push(enr);
                added += 1;
            }
        }
    }
    added
}

fn discv4_node_key(record: &NodeRecord) -> String {
    match record.address {
        IpAddr::V4(ip) => format!("{ip}:{}", record.udp_port),
        IpAddr::V6(ip) => format!("[{ip}]:{}", record.udp_port),
    }
}

fn enode_addr_str(record: &NodeRecord) -> String {
    match record.address {
        IpAddr::V4(ip) => format!("{ip}:{}", record.udp_port),
        IpAddr::V6(ip) => format!("[{ip}]:{}", record.udp_port),
    }
}
