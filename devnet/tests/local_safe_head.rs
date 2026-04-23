//! Tests demonstrating that the Go op-batcher fails to post batches when
//! connected to a base-consensus sequencer that omits `local_safe_l2` from the
//! `optimism_syncStatus` response.
//!
//! The op-batcher reads `local_safe_l2` from `optimism_syncStatus` to track
//! which blocks have been locally derived. When the field is missing from the
//! JSON response, Go's unmarshaler leaves it at its zero value (`Number: 0`),
//! so the op-batcher believes no blocks have been safely derived — even when
//! `safe_l2` and `unsafe_l2` are well past genesis.
//!
//! With `local_safe_l2` stuck at 0, the op-batcher:
//! - Never prunes confirmed blocks from its channel manager
//! - Resubmits all blocks from genesis on every sync cycle
//! - Cannot properly advance its internal submission cursor
//!
//! PR #2362 fixes this by adding `local_safe_l2` to `SyncStatus` and
//! populating it from `EngineSyncState::local_safe_head()`.

use std::time::Duration;

use alloy_provider::Provider;
use devnet::{
    DevnetBuilder,
    host::{host_address, with_host_port_if_needed},
    images::OP_BATCHER_IMAGE,
    network::{ensure_network_exists, network_name},
};
use eyre::{Result, WrapErr, eyre};
use jsonrpsee::{core::client::ClientT, http_client::HttpClientBuilder};
use testcontainers::{GenericImage, ImageExt, core::WaitFor, runners::AsyncRunner};
use tokio::time::{sleep, timeout};

const L1_CHAIN_ID: u64 = 1337;
const L2_CHAIN_ID: u64 = 84538453;

/// Spins up the devnet WITHOUT a batcher, starts the Go op-batcher container
/// pointing at the base-consensus sequencer, and demonstrates that the client's
/// safe head does not advance because the op-batcher cannot function without
/// `local_safe_l2` in the sync status response.
///
/// The test asserts three things:
/// 1. The `optimism_syncStatus` response JSON does NOT contain `local_safe_l2`
/// 2. The sequencer's `safe_l2` is **not** stuck at 0 (derivation advances it)
/// 3. The client's safe head never advances past 0 — the op-batcher cannot
///    produce usable batches because it reads the missing `local_safe_l2` as 0
#[tokio::test]
async fn op_batcher_fails_without_local_safe_l2() -> Result<()> {
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

    // ---------------------------------------------------------------
    // Assert the root cause: `local_safe_l2` is missing from the JSON
    // ---------------------------------------------------------------
    let builder_consensus_url = devnet.l2_stack().builder_consensus_rpc_url();
    let raw_client = HttpClientBuilder::default().build(builder_consensus_url.as_str())?;

    // Make a raw JSON-RPC call to inspect the response shape.
    let raw: serde_json::Value =
        raw_client.request("optimism_syncStatus", Vec::<()>::new()).await?;

    assert!(
        raw.get("local_safe_l2").is_none(),
        "optimism_syncStatus must NOT contain local_safe_l2 — \
         this is the field the op-batcher needs but base-consensus omits. \
         Response keys: {:?}",
        raw.as_object().map(|o| o.keys().collect::<Vec<_>>())
    );

    // Confirm unsafe_l2 is advancing (the sequencer is producing blocks).
    let unsafe_number = raw["unsafe_l2"]["block_info"]["number"].as_u64().unwrap_or(0);
    assert!(unsafe_number >= 3, "unsafe_l2 should be >= 3, got {unsafe_number}");

    // ---------------------------------------------------------------
    // Start the Go op-batcher container
    // ---------------------------------------------------------------
    let l1_url = devnet.l1_rpc_url().await?;
    let l2_url = devnet.l2_rpc_url()?;

    let l1_port = l1_url.port().unwrap_or(8545);
    let l2_port = l2_url.port().unwrap_or(8545);
    let consensus_port = builder_consensus_url.port().unwrap_or(7545);

    let host = host_address();
    let l1_rpc = format!("http://{host}:{l1_port}");
    let l2_rpc = format!("http://{host}:{l2_port}");
    let rollup_rpc = format!("http://{host}:{consensus_port}");

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
            l2_rpc,
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

    let container_builder = with_host_port_if_needed(container_builder, l1_port);
    let container_builder = with_host_port_if_needed(container_builder, l2_port);
    let container_builder = with_host_port_if_needed(container_builder, consensus_port);

    let _op_batcher = container_builder.start().await.wrap_err("Failed to start op-batcher")?;

    // Wait for the op-batcher to attempt batch submission over multiple sync
    // cycles (poll_interval=1s).
    sleep(Duration::from_secs(30)).await;

    // ---------------------------------------------------------------
    // Assert the consequence: client safe head stays at 0
    // ---------------------------------------------------------------
    let client_consensus_url = devnet.l2_stack().client_consensus_rpc_url();
    let client_raw: serde_json::Value = {
        let c = HttpClientBuilder::default().build(client_consensus_url.as_str())?;
        c.request("optimism_syncStatus", Vec::<()>::new()).await?
    };

    let client_safe = client_raw["safe_l2"]["block_info"]["number"].as_u64().unwrap_or(0);

    // The client's safe head should still be at 0: the op-batcher reads
    // local_safe_l2 as 0 (missing field), so its channel manager cannot
    // properly track which blocks have been derived. It resubmits from
    // genesis every cycle, and the resulting batches do not advance the
    // client's derivation.
    assert_eq!(
        client_safe, 0,
        "client safe_l2 should be 0 — the op-batcher cannot produce well-formed \
         batches when local_safe_l2 is missing from optimism_syncStatus"
    );

    Ok(())
}
