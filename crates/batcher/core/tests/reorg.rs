//! Integration tests for reorg handling in [`BatchDriver`].

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use alloy_primitives::{Address, B256};
use async_trait::async_trait;
use base_batcher_core::{
    BatchDriver, BatchDriverConfig, DaThrottle, LocalSafeHeadProvider, LocalSafeHeadResult,
    NoopThrottleClient, ThrottleController,
    test_utils::{
        ImmediateConfirmTxManager, OneBlockSource, OneReorgPipeline, PendingL1HeadSource, Recorded,
        ReorgPipeline, TrackingPipeline,
    },
};
use base_batcher_source::{ChannelBlockSource, L2BlockEvent, SourceError, UnsafeBlockSource};
use base_common_consensus::BaseBlock;
use base_protocol::{BlockInfo, L2BlockInfo};
use base_runtime::{
    Cancellation, Clock, Spawner,
    deterministic::{Config, Runner},
};
use futures::future::BoxFuture;
use tokio::sync::watch;

#[derive(Debug)]
struct StaticLocalSafeHeadProvider {
    number: u64,
}

impl LocalSafeHeadProvider for StaticLocalSafeHeadProvider {
    fn local_safe_l2_number(&self) -> BoxFuture<'_, LocalSafeHeadResult> {
        Box::pin(async move { Ok(self.number) })
    }
}

#[derive(Debug)]
struct ReorgThenPendingSource {
    event: Option<L2BlockEvent>,
    catchup_args: Arc<Mutex<Vec<u64>>>,
}

impl ReorgThenPendingSource {
    fn new(event: L2BlockEvent) -> (Self, Arc<Mutex<Vec<u64>>>) {
        let catchup_args = Arc::new(Mutex::new(Vec::new()));
        (Self { event: Some(event), catchup_args: Arc::clone(&catchup_args) }, catchup_args)
    }
}

#[async_trait]
impl UnsafeBlockSource for ReorgThenPendingSource {
    async fn next(&mut self) -> Result<L2BlockEvent, SourceError> {
        if let Some(event) = self.event.take() {
            return Ok(event);
        }

        std::future::pending().await
    }

    fn reset_catchup(&mut self, start_from: u64) {
        self.catchup_args.lock().unwrap().push(start_from);
    }
}

/// When `add_block` returns `ReorgError`, the driver must reset the pipeline and
/// call `reset_catchup` on the source so it re-delivers all post-reorg blocks
/// sequentially. The triggering block must NOT be re-added directly — the source
/// will re-deliver it via sequential catchup.
#[test]
fn test_reorg_triggers_pipeline_reset_and_catchup() {
    Runner::start(Config::seeded(0), |ctx| async move {
        let blocks_accepted = Arc::new(Mutex::new(0usize));
        let resets = Arc::new(Mutex::new(0usize));
        let pipeline = OneReorgPipeline::new(Arc::clone(&blocks_accepted), Arc::clone(&resets));

        let driver = BatchDriver::new(
            ctx.clone(),
            pipeline,
            OneBlockSource::new(),
            ImmediateConfirmTxManager { l1_block: 1 },
            BatchDriverConfig {
                inbox: Address::ZERO,
                max_pending_transactions: 1,
                drain_timeout: Duration::from_millis(10),
                force_blobs_when_throttling: true,
            },
            DaThrottle::new(ThrottleController::noop(), Arc::new(NoopThrottleClient)),
            PendingL1HeadSource,
        );
        let handle = ctx.spawn(driver.run());

        ctx.sleep(Duration::from_millis(50)).await;
        ctx.cancel();

        assert!(handle.await.unwrap().is_ok());
        assert_eq!(*resets.lock().unwrap(), 1, "pipeline must be reset on reorg");
        // The triggering block is NOT re-added directly; the source re-delivers it
        // via reset_catchup. In this test OneBlockSource is a no-op so blocks_accepted stays 0.
        assert_eq!(
            *blocks_accepted.lock().unwrap(),
            0,
            "block must not be re-added directly; source will re-deliver via catchup"
        );
    });
}

/// When `add_block` returns a `ReorgError`, the driver must reset the pipeline
/// and discard in-flight futures instead of propagating a fatal error. This
/// mirrors the `L2BlockEvent::Reorg` handling path.
#[test]
fn test_add_block_reorg_resets_pipeline_instead_of_fatal_error() {
    Runner::start(Config::seeded(0), |ctx| async move {
        let recorded = Arc::new(Mutex::new(Recorded::default()));
        let pipeline = ReorgPipeline::new(Arc::clone(&recorded));

        let driver = BatchDriver::new(
            ctx.clone(),
            pipeline,
            OneBlockSource::new(),
            ImmediateConfirmTxManager { l1_block: 1 },
            BatchDriverConfig {
                inbox: Address::ZERO,
                max_pending_transactions: 1,
                drain_timeout: Duration::from_millis(10),
                force_blobs_when_throttling: true,
            },
            DaThrottle::new(ThrottleController::noop(), Arc::new(NoopThrottleClient)),
            PendingL1HeadSource,
        );
        let handle = ctx.spawn(driver.run());

        ctx.sleep(Duration::from_millis(50)).await;
        ctx.cancel();

        let result = handle.await.unwrap();
        assert!(result.is_ok(), "driver must not return a fatal error on add_block reorg");
        assert_eq!(
            recorded.lock().unwrap().resets,
            1,
            "pipeline.reset() must be called when add_block returns ReorgError"
        );
    });
}

/// When the source delivers `L2BlockEvent::Reorg`, the driver must reset the
/// pipeline and discard in-flight submissions. This is distinct from the
/// `add_block`-triggered reorg path tested in
/// `test_reorg_block_is_readded_after_reset`.
#[test]
fn test_l2_reorg_event_resets_pipeline() {
    Runner::start(Config::seeded(0), |ctx| async move {
        let recorded = Arc::new(Mutex::new(Recorded::default()));
        let pipeline = TrackingPipeline::new(Arc::clone(&recorded));
        let (source, source_tx) = ChannelBlockSource::new();

        let driver = BatchDriver::new(
            ctx.clone(),
            pipeline,
            source,
            ImmediateConfirmTxManager { l1_block: 1 },
            BatchDriverConfig {
                inbox: Address::ZERO,
                max_pending_transactions: 1,
                drain_timeout: Duration::from_millis(10),
                force_blobs_when_throttling: true,
            },
            DaThrottle::new(ThrottleController::noop(), Arc::new(NoopThrottleClient)),
            PendingL1HeadSource,
        );
        let handle = ctx.spawn(driver.run());

        let reorg_head =
            L2BlockInfo::new(BlockInfo::new(B256::ZERO, 5, B256::ZERO, 0), Default::default(), 0);
        source_tx.send(L2BlockEvent::Reorg { new_safe_head: reorg_head }).unwrap();
        ctx.sleep(Duration::from_millis(50)).await;
        ctx.cancel();

        assert!(handle.await.unwrap().is_ok());
        assert_eq!(
            recorded.lock().unwrap().resets,
            1,
            "pipeline must be reset when source delivers a Reorg event"
        );
    });
}

/// When the safe-head watch still contains a stale pre-reorg value, the driver
/// must fetch the current local safe head for the reset boundary instead of
/// skipping ahead to the stale watch value.
#[test]
fn test_l2_reorg_event_uses_fresh_local_safe_head_over_stale_watch() {
    Runner::start(Config::seeded(0), |ctx| async move {
        let recorded = Arc::new(Mutex::new(Recorded::default()));
        let pipeline = TrackingPipeline::new(Arc::clone(&recorded));
        let reorg_head =
            L2BlockInfo::new(BlockInfo::new(B256::ZERO, 150, B256::ZERO, 0), Default::default(), 0);
        let (source, catchup_args) =
            ReorgThenPendingSource::new(L2BlockEvent::Reorg { new_safe_head: reorg_head });
        let (safe_head_tx, safe_head_rx) = watch::channel::<u64>(150);

        let driver = BatchDriver::new(
            ctx.clone(),
            pipeline,
            source,
            ImmediateConfirmTxManager { l1_block: 1 },
            BatchDriverConfig {
                inbox: Address::ZERO,
                max_pending_transactions: 1,
                drain_timeout: Duration::from_millis(10),
                force_blobs_when_throttling: true,
            },
            DaThrottle::new(ThrottleController::noop(), Arc::new(NoopThrottleClient)),
            PendingL1HeadSource,
        )
        .with_safe_head_rx(safe_head_rx)
        .with_local_safe_head_provider(Arc::new(StaticLocalSafeHeadProvider { number: 100 }));
        let handle = ctx.spawn(driver.run());

        ctx.sleep(Duration::from_millis(50)).await;
        ctx.cancel();

        drop(safe_head_tx);
        assert!(handle.await.unwrap().is_ok());
        assert_eq!(
            *catchup_args.lock().unwrap(),
            vec![101],
            "reorg catchup must start from fresh local_safe_l2 + 1"
        );
        assert_eq!(recorded.lock().unwrap().resets, 1, "pipeline must still be reset on reorg");
    });
}

/// Parent-hash mismatch reorgs use the same reset boundary as explicit source
/// reorg events: fresh local safe head, not the potentially stale pruning watch.
#[test]
fn test_add_block_reorg_uses_fresh_local_safe_head_over_stale_watch() {
    Runner::start(Config::seeded(0), |ctx| async move {
        let recorded = Arc::new(Mutex::new(Recorded::default()));
        let pipeline = ReorgPipeline::new(Arc::clone(&recorded));
        let (source, catchup_args) =
            ReorgThenPendingSource::new(L2BlockEvent::Block(Box::<BaseBlock>::default()));
        let (safe_head_tx, safe_head_rx) = watch::channel::<u64>(150);

        let driver = BatchDriver::new(
            ctx.clone(),
            pipeline,
            source,
            ImmediateConfirmTxManager { l1_block: 1 },
            BatchDriverConfig {
                inbox: Address::ZERO,
                max_pending_transactions: 1,
                drain_timeout: Duration::from_millis(10),
                force_blobs_when_throttling: true,
            },
            DaThrottle::new(ThrottleController::noop(), Arc::new(NoopThrottleClient)),
            PendingL1HeadSource,
        )
        .with_safe_head_rx(safe_head_rx)
        .with_local_safe_head_provider(Arc::new(StaticLocalSafeHeadProvider { number: 100 }));
        let handle = ctx.spawn(driver.run());

        ctx.sleep(Duration::from_millis(50)).await;
        ctx.cancel();

        drop(safe_head_tx);
        assert!(handle.await.unwrap().is_ok());
        assert_eq!(
            *catchup_args.lock().unwrap(),
            vec![101],
            "add_block reorg catchup must start from fresh local_safe_l2 + 1"
        );
        assert_eq!(
            recorded.lock().unwrap().resets,
            1,
            "pipeline must still be reset on add_block reorg"
        );
    });
}
