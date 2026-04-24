//! RPC-based audit connector.
//!
//! Replaces the legacy Kafka-publisher-driven connector. Events are buffered
//! and forwarded to the audit-archiver service via the
//! `base_persistBundleEventBatch` JSON-RPC method.

use std::{sync::Arc, time::Duration};

use jsonrpsee::http_client::HttpClientBuilder;
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::warn;

mod config;
pub use config::AuditConnectorConfig;

mod metrics;
pub use metrics::AuditConnectorMetrics;

mod task;
pub use task::AuditConnector;

use crate::BundleEvent;

/// Handle to a spawned [`AuditConnector`] task.
///
/// Holds the cancellation token and join handle so callers can wire up
/// graceful shutdown.
pub struct SpawnedAuditConnector {
    cancel: CancellationToken,
    handle: JoinHandle<()>,
    shutdown_timeout: Duration,
}

impl SpawnedAuditConnector {
    /// Spawns the audit connector task.
    ///
    /// Returns the inbound `BundleEvent` sender (used to enqueue events) and
    /// the [`SpawnedAuditConnector`] handle (used to drive shutdown).
    pub fn spawn(
        config: AuditConnectorConfig,
        channel_capacity: usize,
    ) -> eyre::Result<(mpsc::Sender<BundleEvent>, Self)> {
        let client = HttpClientBuilder::default()
            .request_timeout(config.request_timeout)
            .build(config.audit_url.as_str())
            .map_err(|e| eyre::eyre!("failed to build audit-archiver HTTP client: {e}"))?;

        let shutdown_timeout = config.shutdown_timeout;
        let cancel = CancellationToken::new();
        let (tx, rx) = mpsc::channel(channel_capacity);

        let connector =
            AuditConnector::new(client, rx, Arc::new(config), cancel.clone());

        let handle = tokio::spawn(connector.run());

        Ok((tx, Self { cancel, handle, shutdown_timeout }))
    }

    /// Cancels the connector and waits up to the configured shutdown timeout
    /// for the task to drain its buffer and exit.
    pub async fn shutdown(self) {
        self.cancel.cancel();
        if tokio::time::timeout(self.shutdown_timeout, self.handle).await.is_err() {
            warn!("audit connector did not finish within shutdown timeout");
        }
    }
}

impl std::fmt::Debug for SpawnedAuditConnector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpawnedAuditConnector")
            .field("cancelled", &self.cancel.is_cancelled())
            .field("shutdown_timeout", &self.shutdown_timeout)
            .finish_non_exhaustive()
    }
}
