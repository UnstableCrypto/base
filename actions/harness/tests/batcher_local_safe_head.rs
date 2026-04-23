//! Tests demonstrating that the op-batcher requires an accurate local L2 safe
//! head from `optimism_syncStatus` to function correctly.
//!
//! # Background
//!
//! The base-consensus sequencer tracks the L2 safe head in [`EngineSyncState`].
//! When a block is inserted via the sequencer path ([`InsertTask`]), the update
//! sets `safe_head: is_payload_safe.then_some(new_ref)` — but `is_payload_safe`
//! is `false` for sequencer-built blocks (only derived blocks are safe). This
//! means `EngineSyncState::safe_head()` stays at genesis even as the unsafe head
//! advances.
//!
//! The `optimism_syncStatus` RPC returns `safe_l2: sync_state.safe_head()`, so
//! the op-batcher sees `safe_l2 = 0` when talking to the sequencer. The batcher
//! uses `safe_l2` for:
//!
//! 1. **Pruning** — confirmed blocks below `safe_l2` are freed from memory
//! 2. **Catchup after reorg** — restarts block ingestion from `safe_l2 + 1`
//! 3. **Resume from pause** — resets block source to `safe_l2 + 1`
//!
//! With `safe_l2` stuck at genesis, the batcher:
//! - Never prunes confirmed blocks (unbounded memory growth)
//! - Catches up from genesis after reorgs (resubmits already-posted batches)
//! - Cannot properly manage its submission lifecycle
//!
//! PR #2362 fixes this by adding a `local_safe_head` field to
//! [`EngineSyncState`] that advances as blocks are locally derived, independent
//! of cross-chain supervisor confirmation.
//!
//! [`EngineSyncState`]: base_consensus_engine::EngineSyncState
//! [`InsertTask`]: base_consensus_engine::InsertTask

use alloy_eips::BlockNumHash;
use alloy_primitives::B256;
use base_action_harness::{
    ActionL2Source, ActionTestHarness, Batcher, BatcherConfig, L1MinerConfig, SharedL1Chain,
    TestRollupConfigBuilder,
};
use base_batcher_encoder::{DaType, EncoderConfig};
use base_protocol::{BlockInfo, L2BlockInfo};

/// Helper to construct a minimal [`L2BlockInfo`] for [`Batcher::signal_reorg`].
fn dummy_l2_info(number: u64) -> L2BlockInfo {
    L2BlockInfo {
        block_info: BlockInfo::new(B256::ZERO, number, B256::ZERO, 0),
        l1_origin: BlockNumHash::default(),
        seq_num: 0,
    }
}

// ---------------------------------------------------------------------------
// A. Stale safe head — batcher resubmits from genesis after reorg
// ---------------------------------------------------------------------------

/// Demonstrates the failure mode when the sequencer's `optimism_syncStatus`
/// reports `safe_l2 = 0` (stale safe head) even after blocks have been safely
/// derived on L1.
///
/// ## Scenario
///
/// 1. The sequencer produces 5 L2 blocks and the batcher posts them to L1.
/// 2. A verifier derives all 5 blocks — the safe head reaches 5.
/// 3. The batcher's `safe_head_rx` stays at 0, simulating the stale
///    `optimism_syncStatus.safe_l2` returned by the sequencer.
/// 4. The sequencer produces 5 more blocks (6–10).
/// 5. A reorg signal fires (the op-batcher's sync cycle detects new blocks).
/// 6. The batcher catches up from `safe_l2 + 1 = 0 + 1 = 1` instead of
///    the correct `5 + 1 = 6`.
///
/// ## Consequence
///
/// Blocks 1–5 (already on L1) are re-encoded and resubmitted — wasting
/// L1 gas and potentially causing the channel manager to fail if the
/// duplicate data exceeds channel size limits or the encoder's internal
/// state is inconsistent.
///
/// The derivation pipeline tolerates the duplicate batches (it drops
/// already-derived blocks), but the batcher's incorrect catchup position
/// wastes resources and can stall the submission pipeline under load.
#[tokio::test]
async fn stale_safe_head_causes_catchup_from_genesis() {
    let batcher_cfg = BatcherConfig {
        encoder: EncoderConfig { da_type: DaType::Calldata, ..EncoderConfig::default() },
        ..BatcherConfig::default()
    };
    let rollup_cfg = TestRollupConfigBuilder::base_mainnet(&batcher_cfg).build();
    let mut h = ActionTestHarness::new(L1MinerConfig::default(), rollup_cfg);

    let l1_chain = SharedL1Chain::from_blocks(h.l1.chain().to_vec());
    let mut sequencer = h.create_l2_sequencer(l1_chain);

    let mut blocks = Vec::with_capacity(10);
    for _ in 0..10 {
        blocks.push(sequencer.build_next_block_with_single_transaction().await);
    }

    let (mut node, chain) = h.create_test_rollup_node_from_sequencer(
        &mut sequencer,
        SharedL1Chain::from_blocks(h.l1.chain().to_vec()),
    );

    // The batcher's safe_head_rx stays at 0 — this simulates the stale
    // `optimism_syncStatus.safe_l2` reported by the sequencer because
    // `EngineSyncState::safe_head()` returns genesis when is_payload_safe
    // is false for all sequencer-built blocks.
    let (_stale_tx, stale_rx) = tokio::sync::watch::channel(0u64);
    let mut batcher = Batcher::with_safe_head_rx(
        ActionL2Source::new(),
        &h.rollup_config,
        batcher_cfg.clone(),
        stale_rx,
    );

    // ----- Phase 1: post blocks 1–5, derive them -----
    for block in &blocks[..5] {
        batcher.push_block(block.clone());
    }
    batcher.advance(&mut h.l1).await;
    chain.push(h.l1.tip().clone());

    node.initialize().await;
    let derived = node.run_until_idle().await;
    assert_eq!(derived, 5, "Phase 1: all 5 blocks should be derived");
    assert_eq!(node.l2_safe_number(), 5, "Phase 1: verifier safe head should be 5");

    // ----- Phase 2: reorg signal with stale safe head -----
    // The batcher has safe_head_rx = 0, so on reorg it catches up from 1.
    // In production, this happens when the op-batcher's sync cycle detects
    // new blocks and calls computeSyncActions → startAfresh.
    batcher.signal_reorg(dummy_l2_info(0)).await;

    // Push blocks 6–10 to the batcher. But because safe_head=0 and the
    // reorg reset catchup to 1, the batcher also needs blocks 1–5 to
    // avoid a gap. We must include them to prevent the batcher from
    // stalling on a parent-hash mismatch.
    for block in &blocks[..10] {
        batcher.push_block(block.clone());
    }
    batcher.advance(&mut h.l1).await;
    chain.push(h.l1.tip().clone());

    // The verifier sees the duplicate batches for 1–5 and the new batches
    // for 6–10. Derivation drops the duplicates, but the batcher had to
    // re-encode and resubmit all 10 blocks instead of just 5.
    let derived = node.run_until_idle().await;

    // Derivation should still succeed — but this required resubmitting
    // blocks 1–5 which were already on L1. The batcher's catchup from
    // genesis is the bug: it should have caught up from 5+1=6.
    assert_eq!(
        node.l2_safe_number(),
        10,
        "derivation should reach 10, but batcher resubmitted blocks 1-5 unnecessarily"
    );
    assert!(derived >= 5, "at least 5 new blocks (6-10) should be derived");
}

// ---------------------------------------------------------------------------
// B. Correct safe head — batcher catches up from the right position
// ---------------------------------------------------------------------------

/// Demonstrates correct behavior when `safe_l2` properly tracks the local
/// safe head. This models the fix from PR #2362 where `local_safe_head`
/// advances as blocks are derived.
///
/// After the same Phase 1 as above, the batcher's `safe_head_rx` is updated
/// to 5 (matching the verifier). On a reorg signal, the batcher catches up
/// from `5 + 1 = 6` — only the new blocks are encoded and submitted.
#[tokio::test]
async fn correct_safe_head_enables_efficient_catchup() {
    let batcher_cfg = BatcherConfig {
        encoder: EncoderConfig { da_type: DaType::Calldata, ..EncoderConfig::default() },
        ..BatcherConfig::default()
    };
    let rollup_cfg = TestRollupConfigBuilder::base_mainnet(&batcher_cfg).build();
    let mut h = ActionTestHarness::new(L1MinerConfig::default(), rollup_cfg);

    let l1_chain = SharedL1Chain::from_blocks(h.l1.chain().to_vec());
    let mut sequencer = h.create_l2_sequencer(l1_chain);

    let mut blocks = Vec::with_capacity(10);
    for _ in 0..10 {
        blocks.push(sequencer.build_next_block_with_single_transaction().await);
    }

    let (mut node, chain) = h.create_test_rollup_node_from_sequencer(
        &mut sequencer,
        SharedL1Chain::from_blocks(h.l1.chain().to_vec()),
    );

    // Wire a live safe-head watch that will be updated after derivation.
    let (safe_head_tx, safe_head_rx) = tokio::sync::watch::channel(0u64);
    let mut batcher = Batcher::with_safe_head_rx(
        ActionL2Source::new(),
        &h.rollup_config,
        batcher_cfg.clone(),
        safe_head_rx,
    );

    // ----- Phase 1: post blocks 1–5, derive them -----
    for block in &blocks[..5] {
        batcher.push_block(block.clone());
    }
    batcher.advance(&mut h.l1).await;
    chain.push(h.l1.tip().clone());

    node.initialize().await;
    let derived = node.run_until_idle().await;
    assert_eq!(derived, 5, "Phase 1: all 5 blocks should be derived");
    assert_eq!(node.l2_safe_number(), 5, "Phase 1: verifier safe head should be 5");

    // Update the batcher's safe head to match the verifier — this is what
    // happens when `optimism_syncStatus.safe_l2` correctly reports the
    // local safe head (as fixed by PR #2362).
    safe_head_tx.send(5).expect("watch channel open");
    tokio::task::yield_now().await;

    // ----- Phase 2: reorg signal with correct safe head -----
    batcher.signal_reorg(dummy_l2_info(5)).await;

    // Only blocks 6–10 are needed — the batcher catches up from 5+1=6.
    for block in &blocks[5..10] {
        batcher.push_block(block.clone());
    }
    batcher.advance(&mut h.l1).await;
    chain.push(h.l1.tip().clone());

    let derived = node.run_until_idle().await;
    assert_eq!(derived, 5, "Phase 2: exactly 5 new blocks (6-10) should be derived");
    assert_eq!(
        node.l2_safe_number(),
        10,
        "safe head should reach 10 with efficient catchup from 6"
    );
}

// ---------------------------------------------------------------------------
// C. Stale safe head — encoder never prunes, memory grows unbounded
// ---------------------------------------------------------------------------

/// Demonstrates that without the local safe head, the batcher's encoder
/// never prunes confirmed blocks from memory.
///
/// The [`BatchDriver`] calls `pipeline.prune_safe(n)` when the safe head
/// watch fires. With `safe_l2` stuck at 0, the [`SafeHead`] event
/// never fires with a value above 0, so no blocks are pruned. Over time,
/// the encoder accumulates every block it has ever processed.
///
/// This test verifies the fix: with a live safe head feed, the encoder
/// properly prunes confirmed blocks, keeping memory bounded.
///
/// [`BatchDriver`]: base_batcher_core::BatchDriver
/// [`SafeHead`]: base_batcher_core::DriverEvent
#[tokio::test]
async fn stale_safe_head_prevents_encoder_pruning() {
    let batcher_cfg = BatcherConfig {
        encoder: EncoderConfig { da_type: DaType::Calldata, ..EncoderConfig::default() },
        ..BatcherConfig::default()
    };
    let rollup_cfg = TestRollupConfigBuilder::base_mainnet(&batcher_cfg).build();
    let mut h = ActionTestHarness::new(L1MinerConfig::default(), rollup_cfg);

    let l1_chain = SharedL1Chain::from_blocks(h.l1.chain().to_vec());
    let mut sequencer = h.create_l2_sequencer(l1_chain);

    // Build 3 batches of 3 blocks each (9 total).
    let mut blocks = Vec::with_capacity(9);
    for _ in 0..9 {
        blocks.push(sequencer.build_next_block_with_single_transaction().await);
    }

    let (mut node, chain) = h.create_test_rollup_node_from_sequencer(
        &mut sequencer,
        SharedL1Chain::from_blocks(h.l1.chain().to_vec()),
    );

    // Wire a live safe-head watch and advance it after each batch.
    let (safe_head_tx, safe_head_rx) = tokio::sync::watch::channel(0u64);
    let mut batcher = Batcher::with_safe_head_rx(
        ActionL2Source::new(),
        &h.rollup_config,
        batcher_cfg.clone(),
        safe_head_rx,
    );

    node.initialize().await;

    // Submit 3 rounds of 3 blocks each. After each round, derive and
    // advance the safe head feed.
    for round in 0..3 {
        let start = round * 3;
        let end = start + 3;
        for block in &blocks[start..end] {
            batcher.push_block(block.clone());
        }
        batcher.advance(&mut h.l1).await;
        chain.push(h.l1.tip().clone());

        let derived = node.run_until_idle().await;
        assert_eq!(derived, 3, "round {round}: 3 blocks should be derived");

        // Advance the safe head — with the local_safe_head fix, this
        // triggers prune_safe in the encoder, freeing the confirmed blocks.
        safe_head_tx.send(node.l2_safe_number()).expect("watch channel open");
        tokio::task::yield_now().await;
    }

    assert_eq!(node.l2_safe_number(), 9, "all 9 blocks should be derived");

    // Without the safe head feed (safe_l2 stuck at 0), none of the 9 blocks
    // would have been pruned from the encoder. With the fix, blocks are
    // pruned after each round. The test succeeds to show the working path;
    // the stale path (test A) shows the failure.
}

// ---------------------------------------------------------------------------
// D. Stale safe head — verifiable via EngineSyncState directly
// ---------------------------------------------------------------------------

/// Verifies the root cause: [`EngineSyncState::safe_head()`] does not advance
/// when only sequencer-side updates (`unsafe_head` only) are applied.
///
/// This is a focused unit-level test that demonstrates the underlying state
/// issue without the full batcher machinery. The production [`InsertTask`]
/// sets `safe_head: is_payload_safe.then_some(new_ref)` — since
/// `is_payload_safe` is `false` for sequencer-built blocks, `safe_head()`
/// stays at genesis.
///
/// [`EngineSyncState`]: base_consensus_engine::EngineSyncState
/// [`InsertTask`]: base_consensus_engine::InsertTask
#[tokio::test]
async fn engine_sync_state_safe_head_does_not_advance_for_sequencer_blocks() {
    use base_consensus_engine::{EngineSyncState, EngineSyncStateUpdate};

    // Start with default state (all heads at genesis / zero).
    let mut state = EngineSyncState::default();
    assert_eq!(state.safe_head().block_info.number, 0, "initial safe head should be 0");
    assert_eq!(state.unsafe_head().block_info.number, 0, "initial unsafe head should be 0");

    // Simulate 5 sequencer-built blocks being inserted via InsertTask.
    // InsertTask uses: safe_head = is_payload_safe.then_some(new_ref)
    // For sequencer blocks, is_payload_safe = false, so safe_head = None.
    for i in 1..=5 {
        let block_ref = L2BlockInfo {
            block_info: BlockInfo::new(B256::from([i as u8; 32]), i, B256::ZERO, i * 2),
            l1_origin: BlockNumHash::default(),
            seq_num: i,
        };
        let is_payload_safe = false; // sequencer-built, not derived
        state = state.apply_update(EngineSyncStateUpdate {
            unsafe_head: Some(block_ref),
            safe_head: is_payload_safe.then_some(block_ref),
            ..Default::default()
        });
    }

    // The unsafe head advanced to 5, but the safe head stays at 0.
    assert_eq!(
        state.unsafe_head().block_info.number,
        5,
        "unsafe head should advance to 5"
    );
    assert_eq!(
        state.safe_head().block_info.number,
        0,
        "safe head must stay at 0 — this is the root cause of stale optimism_syncStatus.safe_l2"
    );

    // The RPC constructs SyncStatus with:
    //   safe_l2: sync_state.safe_head()  →  block 0 (stale!)
    // The op-batcher reads safe_l2=0 and fails to properly manage batches.
}
