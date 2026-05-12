//! Background poller that keeps the local safe L2 head watch channel up to date.

use std::{error::Error, time::Duration};

use base_batcher_core::{LocalSafeHeadProvider, LocalSafeHeadResult};
use base_consensus_rpc::RollupNodeApiClient;
use futures::future::BoxFuture;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::warn;

/// RPC-backed provider for the current local safe L2 head.
#[derive(Clone)]
pub struct RpcLocalSafeHeadProvider {
    client: jsonrpsee::http_client::HttpClient,
}

impl std::fmt::Debug for RpcLocalSafeHeadProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcLocalSafeHeadProvider").finish_non_exhaustive()
    }
}

impl RpcLocalSafeHeadProvider {
    /// Create a new RPC-backed local safe head provider.
    pub const fn new(client: jsonrpsee::http_client::HttpClient) -> Self {
        Self { client }
    }

    /// Fetch the current local safe L2 block number.
    pub async fn fetch_local_safe_l2_number(&self) -> Result<u64, Box<dyn Error + Send + Sync>> {
        let status = self.client.sync_status().await?;
        Ok(status.local_safe_l2.block_info.number)
    }
}

impl LocalSafeHeadProvider for RpcLocalSafeHeadProvider {
    fn local_safe_l2_number(&self) -> BoxFuture<'_, LocalSafeHeadResult> {
        Box::pin(self.fetch_local_safe_l2_number())
    }
}

/// Polls a [`LocalSafeHeadProvider`] at a fixed interval and updates a watch
/// channel when the local safe L2 head changes.
///
/// The poller waits `poll_interval` before the first call, then loops.
/// When the local safe head changes, it calls [`watch::Sender::send_if_modified`]
/// so receivers are only woken when the value actually changes.
///
/// Stops cleanly when the [`CancellationToken`] passed to [`run`](Self::run)
/// is cancelled — at most one in-flight RPC call is waited for before exit.
#[derive(Debug)]
pub struct SafeHeadPoller<C: LocalSafeHeadProvider> {
    provider: C,
    poll_interval: Duration,
    safe_head_tx: watch::Sender<u64>,
}

impl<C: LocalSafeHeadProvider> SafeHeadPoller<C> {
    /// Create a new [`SafeHeadPoller`].
    pub const fn new(
        provider: C,
        poll_interval: Duration,
        safe_head_tx: watch::Sender<u64>,
    ) -> Self {
        Self { provider, poll_interval, safe_head_tx }
    }

    /// Run the polling loop until `cancellation` fires.
    ///
    /// Cancellation is checked before every sleep, so the poller exits within
    /// one poll interval of the token being cancelled.
    pub async fn run(self, cancellation: CancellationToken) {
        loop {
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => break,
                _ = tokio::time::sleep(self.poll_interval) => {}
            }
            match self.provider.local_safe_l2_number().await {
                Ok(n) => {
                    self.safe_head_tx.send_if_modified(|old| {
                        if n != *old {
                            *old = n;
                            true
                        } else {
                            false
                        }
                    });
                }
                Err(e) => {
                    warn!(error = %e, "failed to poll optimism_syncStatus for local safe head");
                }
            }
        }
    }

    /// Spawn the polling loop as a background tokio task.
    pub fn spawn(self, cancellation: CancellationToken) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run(cancellation))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    use base_batcher_core::{LocalSafeHeadProvider, LocalSafeHeadResult};
    use futures::future::BoxFuture;
    use tokio::sync::watch;
    use tokio_util::sync::CancellationToken;

    use super::SafeHeadPoller;

    // ---- Mock providers ----

    /// Returns values from a pre-loaded queue; returns `0` when exhausted.
    #[derive(Debug)]
    struct MockProvider {
        values: Arc<Mutex<Vec<u64>>>,
    }

    impl LocalSafeHeadProvider for MockProvider {
        fn local_safe_l2_number(&self) -> BoxFuture<'_, LocalSafeHeadResult> {
            Box::pin(async move {
                let mut v = self.values.lock().unwrap();
                Ok(if v.is_empty() { 0 } else { v.remove(0) })
            })
        }
    }

    /// Always returns an error.
    struct ErrorProvider;

    impl std::fmt::Debug for ErrorProvider {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("ErrorProvider").finish()
        }
    }

    impl LocalSafeHeadProvider for ErrorProvider {
        fn local_safe_l2_number(&self) -> BoxFuture<'_, LocalSafeHeadResult> {
            Box::pin(async move { Err("rpc error".into()) })
        }
    }

    // ---- Tests ----

    /// When the provider returns a higher block number, the watch channel must
    /// be updated and receivers notified.
    #[tokio::test]
    async fn poll_advances_watch_channel() {
        let (tx, mut rx) = watch::channel(0u64);
        let provider = MockProvider { values: Arc::new(Mutex::new(vec![5, 10])) };
        let cancellation = CancellationToken::new();

        let poller = SafeHeadPoller::new(provider, Duration::from_millis(1), tx);
        let handle = poller.spawn(cancellation.clone());

        // Wait for at least one advance.
        tokio::time::timeout(Duration::from_millis(200), rx.changed())
            .await
            .expect("watch should fire within 200 ms")
            .expect("sender should still be alive");

        cancellation.cancel();
        handle.await.unwrap();

        assert!(*rx.borrow() >= 5, "local safe head must have advanced to at least 5");
    }

    /// When the local safe head regresses after an L1 reorg, the watch channel
    /// must be updated and receivers notified.
    #[tokio::test]
    async fn poll_regresses_watch_channel() {
        let (tx, mut rx) = watch::channel(10u64);
        let provider = MockProvider { values: Arc::new(Mutex::new(vec![5])) };
        let cancellation = CancellationToken::new();

        let poller = SafeHeadPoller::new(provider, Duration::from_millis(1), tx);
        let handle = poller.spawn(cancellation.clone());

        tokio::time::timeout(Duration::from_millis(200), rx.changed())
            .await
            .expect("watch should fire within 200 ms")
            .expect("sender should still be alive");

        cancellation.cancel();
        handle.await.unwrap();

        assert_eq!(*rx.borrow(), 5, "local safe head must regress to the provider value");
    }

    /// When the cancellation token fires, the poller must exit within one poll
    /// interval. It must not leak as a background task.
    #[tokio::test]
    async fn cancellation_stops_poller() {
        let (tx, _rx) = watch::channel(0u64);
        let provider = MockProvider { values: Arc::new(Mutex::new(vec![])) };
        let cancellation = CancellationToken::new();

        let poller = SafeHeadPoller::new(provider, Duration::from_millis(50), tx);
        let handle = poller.spawn(cancellation.clone());

        cancellation.cancel();

        tokio::time::timeout(Duration::from_millis(200), handle)
            .await
            .expect("poller must stop within 200 ms of cancellation")
            .unwrap();
    }

    /// Provider errors must be logged and swallowed — the poller must keep
    /// running and not advance the watch channel.
    #[tokio::test]
    async fn provider_errors_are_non_fatal() {
        let (tx, rx) = watch::channel(0u64);
        let cancellation = CancellationToken::new();

        let poller = SafeHeadPoller::new(ErrorProvider, Duration::from_millis(1), tx);
        let handle = poller.spawn(cancellation.clone());

        tokio::time::sleep(Duration::from_millis(30)).await;
        cancellation.cancel();
        handle.await.unwrap();

        assert_eq!(*rx.borrow(), 0, "watch must not advance when provider errors");
    }

    /// When the provider returns the same value, `send_if_modified` must not
    /// notify receivers. Check while the poller is still running (sender alive)
    /// so a dropped-sender signal cannot mask a missing change.
    #[tokio::test]
    async fn watch_not_notified_when_value_unchanged() {
        let (tx, mut rx) = watch::channel(10u64);
        // Mark the initial value as seen so `changed()` only fires on a new send.
        let _ = rx.borrow_and_update();

        let provider = MockProvider { values: Arc::new(Mutex::new(vec![10])) };
        let cancellation = CancellationToken::new();

        let poller = SafeHeadPoller::new(provider, Duration::from_millis(50), tx);
        let handle = poller.spawn(cancellation.clone());

        // Let the poller run multiple cycles, then check *before* cancelling so
        // the sender is still alive and a Err(RecvError) cannot mask an absent change.
        tokio::time::sleep(Duration::from_millis(60)).await;
        let changed = tokio::time::timeout(Duration::from_millis(5), rx.changed()).await;
        assert!(changed.is_err(), "watch must not fire when local safe head does not change");

        cancellation.cancel();
        handle.await.unwrap();
    }
}
