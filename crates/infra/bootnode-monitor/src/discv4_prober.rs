//! discv4 crawler: discovers peers via the Ethereum Discovery v4 protocol.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use bytes::Bytes;
use reth_discv4::{Discv4, Discv4Config, DiscoveryUpdate};
use reth_network_peers::{NodeRecord, pk2id};
use secp256k1::SECP256K1;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tracing::warn;

/// Discovers peers via discv4 and streams their [`NodeRecord`]s into a channel.
pub struct Discv4Crawler {
    disc: Discv4,
}

impl Discv4Crawler {
    /// Creates a new discv4 instance on a random UDP port with the given fork hash in its ENR.
    pub async fn new(fork_hash: [u8; 4]) -> anyhow::Result<Self> {
        let mut key_bytes = [0u8; 32];
        rand_08::RngCore::fill_bytes(&mut rand_08::thread_rng(), &mut key_bytes);
        let secret_key = secp256k1::SecretKey::from_slice(&key_bytes)
            .map_err(|e| anyhow::anyhow!("secret key gen: {e}"))?;
        let public_key = secp256k1::PublicKey::from_secret_key(SECP256K1, &secret_key);
        let id = pk2id(&public_key);

        let port = free_udp_port()?;
        let local_enr = NodeRecord {
            address: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            tcp_port: port,
            udp_port: port,
            id,
        };

        let config =
            Discv4Config::builder().lookup_interval(Duration::from_secs(3)).build();

        let disc = Discv4::spawn(
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)),
            local_enr,
            secret_key,
            config,
        )
        .await
        .map_err(|e| anyhow::anyhow!("discv4::spawn: {e:?}"))?;

        let [h0, h1, h2, h3] = fork_hash;
        let fork_id_rlp = Bytes::from(vec![0xc6, 0x84, h0, h1, h2, h3, 0x80]);
        disc.set_eip868_rlp_pair(b"opel".to_vec(), fork_id_rlp);

        Ok(Self { disc })
    }

    /// Adds a bootnode to the discv4 routing table.
    pub fn add_bootnode(&self, record: NodeRecord) {
        self.disc.add_node(record);
    }

    /// Spawns a background task that forwards newly discovered [`NodeRecord`]s into `tx`.
    ///
    /// Stops when `tx` is dropped.
    pub async fn spawn_into(self, tx: mpsc::Sender<NodeRecord>) -> anyhow::Result<()> {
        let mut stream = self
            .disc
            .update_stream()
            .await
            .map_err(|e| anyhow::anyhow!("discv4 update_stream: {e:?}"))?;

        tokio::spawn(async move {
            while let Some(update) = stream.next().await {
                let records: Vec<NodeRecord> = match update {
                    DiscoveryUpdate::Added(r) | DiscoveryUpdate::DiscoveredAtCapacity(r) => {
                        vec![r]
                    }
                    DiscoveryUpdate::Batch(updates) => updates
                        .into_iter()
                        .filter_map(|u| match u {
                            DiscoveryUpdate::Added(r)
                            | DiscoveryUpdate::DiscoveredAtCapacity(r) => Some(r),
                            _ => None,
                        })
                        .collect(),
                    _ => continue,
                };
                for record in records {
                    if tx.send(record).await.is_err() {
                        return;
                    }
                }
            }
        });

        Ok(())
    }
}

/// Converts a [`NodeRecord`] to an `enode://` URL string for use with discv5 `request_enr`.
///
/// Returns `None` for IPv6 nodes.
pub fn node_record_to_enode(record: &NodeRecord) -> Option<String> {
    let IpAddr::V4(ip) = record.address else {
        warn!(address = %record.address, "skipping IPv6 discv4 node");
        return None;
    };
    let id_hex = hex::encode(record.id.as_slice());
    Some(format!("enode://{id_hex}@{ip}:{}", record.udp_port))
}

fn free_udp_port() -> anyhow::Result<u16> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0")?;
    Ok(sock.local_addr()?.port())
}
