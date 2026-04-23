//! Tests demonstrating that the Go op-batcher fails to post batches when
//! connected to a base-consensus sequencer that is missing the local L2 safe
//! head.
//!
//! The base-consensus sequencer's `optimism_syncStatus` RPC reports `safe_l2`
//! stuck at genesis because `EngineSyncState::safe_head()` is never updated
//! for sequencer-built blocks (`InsertTask` sets `safe_head` only when
//! `is_payload_safe` is true — always false for sequencer blocks).
//!
//! The op-batcher reads `safe_l2` to determine:
//! 1. Which blocks have been safely derived (pruning cursor)
//! 2. Where to restart after reorgs (`safe_l2 + 1`)
//! 3. Whether the node is synced
//!
//! With `safe_l2` stuck at 0, the op-batcher cannot properly advance its
//! channel manager state: it never prunes confirmed blocks, resubmits from
//! genesis on every sync cycle, and its internal cursor never progresses.
//! As a result, the client's safe head never advances because the op-batcher
//! cannot produce well-formed batches over time.

use std::time::Duration;

use alloy_provider::Provider;
use base_consensus_rpc::SyncStatusApiClient;
use devnet::{
    DevnetBuilder,
    host::{host_address, with_host_port_if_needed},
    images::OP_BATCHER_IMAGE,
    network::{ensure_network_exists, network_name},
};
use eyre::{Result, WrapErr, eyre};
use jsonrpsee::http_client::HttpClientBuilder;
use testcontainers::{GenericImage, ImageExt, core::WaitFor, runners::AsyncRunner};
use tokio::time::{sleep, timeout};

const L1_CHAIN_ID: u64 = 1337;
const L2_CHAIN_ID: u64 = 84538453;

/// Spins up the devnet WITHOUT a batcher, starts the Go op-batcher container
/// pointing at the base-consensus sequencer, and demonstrates that the client's
/// safe head does not advance because the op-batcher cannot properly manage
/// batch submission when `safe_l2` is stuck at genesis.
///
/// The test asserts two things:
/// 1. The sequencer's `optimism_syncStatus.safe_l2` stays at 0 (root cause)
/// 2. The client's safe head never advances past 0 (consequence: op-batcher
///    fails to produce usable batches)
#[tokio::test]
async fn op_batcher_fails_with_stale_safe_l2() -> Result<()> {
    base_node_runner::test_utils::init_silenced_tracing();

    // Start the devnet WITHOUT the in-process batcher.
    let devnet = DevnetBuilder::new()
        .with_l1_chain_id(L1_CHAIN_ID)
        .with_l2_chain_id(L2_CHAIN_ID)
        .with_skip_batcher()
        .build()
        .await?;

    let l2_builder_provider = devnet.l2_builder_provider()?;

    // Wait for the sequencer to produce at least 3 L2 blocks.
    timeout(Duration::from_secs(30), async {
        loop {
            let block = l2_builder_provider.get_block_number().await?;
            if block >= 3 {
                return Ok::<_, eyre::Error>(block);
            }
            sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .map_err(|_| eyre!("timed out waiting for L2 block production"))??;

    // Verify the root cause: sequencer reports safe_l2 = 0.
    let builder_consensus_url = devnet.l2_stack().builder_consensus_rpc_url();
    let op_client = HttpClientBuilder::default().build(builder_consensus_url.as_str())?;
    let sync_status = op_client.sync_status().await?;

    assert!(
        sync_status.unsafe_l2.block_info.number >= 3,
        "unsafe_l2 should be >= 3, got {}",
        sync_status.unsafe_l2.block_info.number
    );
    assert_eq!(
        sync_status.safe_l2.block_info.number, 0,
        "safe_l2 must be 0 (stale) — this is the root cause"
    );

    // Start the Go op-batcher container pointing at the in-process services.
    // The op-batcher connects via host gateway to reach in-process L1, builder,
    // and builder-consensus.
    let l1_url = devnet.l1_rpc_url().await?;
    let l2_url = devnet.l2_rpc_url()?;

    // Parse ports from the URLs for host gateway exposure.
    let l1_port = l1_url.port().unwrap_or(8545);
    let l2_port = l2_url.port().unwrap_or(8545);
    let consensus_port = builder_consensus_url.port().unwrap_or(7545);

    let host = host_address();
    let l1_rpc = format!("http://{host}:{l1_port}");
    let l2_rpc = format!("http://{host}:{l2_port}");
    let rollup_rpc = format!("http://{host}:{consensus_port}");

    // Batcher private key (Anvil account 6 — same key the devnet uses).
    let batcher_key = format!("0x{}", hex::encode(devnet::config::BATCHER.private_key.as_slice()));

    ensure_network_exists()?;
    let network = network_name().to_string();

    let (image_name, image_tag) =
        OP_BATCHER_IMAGE.split_once(':').ok_or_else(|| eyre!("op-batcher image tag missing"))?;

    let image = GenericImage::new(image_name, image_tag).with_wait_for(WaitFor::seconds(2));

    let container_builder = image
        .with_container_name(format!("op-batcher-test-{}", nanoid::nanoid!(6)))
        .with_network(&network)
        .with_cmd(vec![
            "--l1-eth-rpc".to_string(),
            l1_rpc,
            "--l2-eth-rpc".to_string(),
            l2_rpc.clone(),
            "--rollup-rpc".to_string(),
            rollup_rpc,
            "--private-key".to_string(),
            batcher_key,
            "--max-channel-duration".to_string(),
            "2".to_string(),
            "--poll-interval".to_string(),
            "1s".to_string(),
            "--sub-safety-margin".to_string(),
            "0".to_string(),
            "--num-confirmations".to_string(),
            "1".to_string(),
        ]);

    // Expose the host ports so the container can reach in-process services.
    let container_builder = with_host_port_if_needed(container_builder, l1_port);
    let container_builder = with_host_port_if_needed(container_builder, l2_port);
    let container_builder = with_host_port_if_needed(container_builder, consensus_port);

    let _op_batcher = container_builder.start().await.wrap_err("Failed to start op-batcher")?;

    // Wait for the op-batcher to attempt batch submission.
    // Give it enough time (30s) for multiple sync cycles (poll_interval=1s).
    sleep(Duration::from_secs(30)).await;

    // Check the client consensus node's sync status.
    // If the op-batcher were working correctly, it would have posted batches to
    // L1 and the client's derivation pipeline would have advanced the safe head.
    let client_consensus_url = devnet.l2_stack().client_consensus_rpc_url();
    let client_op_client = HttpClientBuilder::default().build(client_consensus_url.as_str())?;
    let client_sync = client_op_client.sync_status().await?;

    // The client's safe_l2 should still be at 0 because the op-batcher cannot
    // properly manage batches when the sequencer's safe_l2 is stuck at genesis.
    // Even if the op-batcher posts initial batches, it re-posts them every
    // cycle (since safe_l2 never advances), and the channel manager state
    // becomes inconsistent over time.
    assert_eq!(
        client_sync.safe_l2.block_info.number, 0,
        "client safe_l2 should be 0 — op-batcher cannot produce well-formed batches \
         when the sequencer reports safe_l2=0. unsafe_l2={}, safe_l2={}",
        client_sync.unsafe_l2.block_info.number, client_sync.safe_l2.block_info.number
    );

    // Confirm the sequencer's safe_l2 is STILL at 0 after 30s of op-batcher running.
    let final_sync = op_client.sync_status().await?;
    assert_eq!(
        final_sync.safe_l2.block_info.number, 0,
        "sequencer safe_l2 must remain at 0 — confirming the root cause persists"
    );

    Ok(())
}
