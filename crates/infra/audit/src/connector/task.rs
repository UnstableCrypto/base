use std::{sync::Arc, time::Instant};

use jsonrpsee::{
    core::{ClientError, client::ClientT},
    http_client::HttpClient,
    rpc_params,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};
use url::Url;

use super::{config::AuditConnectorConfig, metrics::AuditConnectorMetrics};
use crate::BundleEvent;

/// Async connector task that receives [`BundleEvent`]s from an mpsc channel
/// and forwards them to the audit-archiver via batched RPC calls.
///
/// Buffers events and flushes a batch via `base_persistBundleEventBatch`
/// whenever the buffer reaches `max_batch_size`. On shutdown (cancellation
/// or channel close), drains any remaining buffered events before exiting.
pub struct AuditConnector {
    audit_url: Url,
    client: HttpClient,
    receiver: mpsc::Receiver<BundleEvent>,
    config: Arc<AuditConnectorConfig>,
    cancel: CancellationToken,
    buffer: Vec<BundleEvent>,
}

impl AuditConnector {
    /// Creates a new audit connector.
    pub fn new(
        client: HttpClient,
        receiver: mpsc::Receiver<BundleEvent>,
        config: Arc<AuditConnectorConfig>,
        cancel: CancellationToken,
    ) -> Self {
        let audit_url = config.audit_url.clone();
        let buffer = Vec::with_capacity(config.max_batch_size.max(1));
        Self { audit_url, client, receiver, config, cancel, buffer }
    }

    /// Runs the connector loop until cancelled or the channel closes,
    /// then drains any remaining buffered events before returning.
    pub async fn run(mut self) {
        info!(
            audit_url = %self.audit_url,
            max_batch_size = self.config.max_batch_size,
            "starting audit connector",
        );

        loop {
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => {
                    info!(
                        audit_url = %self.audit_url,
                        buffered = self.buffer.len(),
                        "audit connector cancelled, draining buffer",
                    );
                    break;
                }
                result = self.receiver.recv() => {
                    match result {
                        Some(event) => {
                            self.buffer.push(event);
                            AuditConnectorMetrics::buffer_size().set(self.buffer.len() as f64);
                            if self.buffer.len() >= self.config.max_batch_size {
                                self.flush_buffer().await;
                            }
                        }
                        None => {
                            AuditConnectorMetrics::channel_closed().increment(1);
                            info!(
                                audit_url = %self.audit_url,
                                buffered = self.buffer.len(),
                                "audit event channel closed, draining buffer",
                            );
                            break;
                        }
                    }
                }
            }
        }

        self.flush_remaining().await;
    }

    async fn flush_remaining(&mut self) {
        while !self.buffer.is_empty() {
            self.flush_buffer().await;
        }
    }

    async fn flush_buffer(&mut self) {
        let batch_size = self.buffer.len().min(self.config.max_batch_size);
        if batch_size == 0 {
            return;
        }
        let batch: Vec<BundleEvent> = self.buffer.drain(..batch_size).collect();
        AuditConnectorMetrics::buffer_size().set(self.buffer.len() as f64);

        trace!(
            audit_url = %self.audit_url,
            events = batch.len(),
            remaining = self.buffer.len(),
            "flushing audit batch",
        );

        self.send_with_retries(batch).await;
    }

    async fn send_with_retries(&self, batch: Vec<BundleEvent>) {
        let batch_size = batch.len();
        let overall_start = Instant::now();
        for attempt in 0..=self.config.max_retries {
            if self.cancel.is_cancelled() && attempt > 0 {
                warn!(
                    audit_url = %self.audit_url,
                    batch_size,
                    "cancellation observed during retry, abandoning batch",
                );
                AuditConnectorMetrics::rpc_errors().increment(1);
                return;
            }

            let result = self
                .client
                .request::<u32, _>("base_persistBundleEventBatch", rpc_params![&batch])
                .await;

            match result {
                Ok(persisted) if (persisted as usize) == batch_size => {
                    AuditConnectorMetrics::rpc_latency()
                        .record(overall_start.elapsed().as_secs_f64());
                    AuditConnectorMetrics::batches_sent().increment(1);
                    AuditConnectorMetrics::events_forwarded().increment(batch_size as u64);
                    debug!(
                        audit_url = %self.audit_url,
                        batch_size,
                        "audit batch sent",
                    );
                    return;
                }
                Ok(persisted) => {
                    let dropped = batch_size.saturating_sub(persisted as usize) as u64;
                    AuditConnectorMetrics::rpc_latency()
                        .record(overall_start.elapsed().as_secs_f64());
                    AuditConnectorMetrics::batches_sent().increment(1);
                    AuditConnectorMetrics::events_forwarded().increment(persisted as u64);
                    AuditConnectorMetrics::events_dropped().increment(dropped);
                    warn!(
                        audit_url = %self.audit_url,
                        persisted,
                        dropped,
                        batch_size,
                        "partial failure persisting audit batch",
                    );
                    return;
                }
                Err(err) if Self::is_retryable(&err) && attempt < self.config.max_retries => {
                    let backoff = self.config.retry_backoff * 2u32.saturating_pow(attempt);
                    debug!(
                        audit_url = %self.audit_url,
                        attempt = attempt + 1,
                        max_retries = self.config.max_retries,
                        backoff_ms = backoff.as_millis() as u64,
                        error = %err,
                        "audit RPC send failed, retrying",
                    );
                    tokio::select! {
                        _ = self.cancel.cancelled() => {
                            AuditConnectorMetrics::rpc_errors().increment(1);
                            return;
                        }
                        _ = tokio::time::sleep(backoff) => {}
                    }
                }
                Err(err) => {
                    AuditConnectorMetrics::rpc_latency()
                        .record(overall_start.elapsed().as_secs_f64());
                    AuditConnectorMetrics::events_dropped().increment(batch_size as u64);
                    AuditConnectorMetrics::rpc_errors().increment(1);
                    error!(
                        audit_url = %self.audit_url,
                        error = %err,
                        batch_size,
                        retryable = Self::is_retryable(&err),
                        "audit RPC send failed, dropping batch",
                    );
                    return;
                }
            }
        }
    }

    const fn is_retryable(err: &ClientError) -> bool {
        matches!(
            err,
            ClientError::Transport(_) | ClientError::RequestTimeout | ClientError::RestartNeeded(_)
        )
    }
}

impl std::fmt::Debug for AuditConnector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditConnector")
            .field("audit_url", &self.audit_url)
            .field("config", &self.config)
            .field("buffered", &self.buffer.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use alloy_primitives::TxHash;
    use base_bundles::test_utils::create_bundle_from_txn_data;
    use jsonrpsee::http_client::HttpClientBuilder;
    use serde_json::json;
    use uuid::Uuid;
    use wiremock::{
        Match, Mock, MockServer, ResponseTemplate,
        matchers::{body_partial_json, method, path},
    };

    use super::*;

    fn test_event(seed: u8) -> BundleEvent {
        BundleEvent::BlockIncluded {
            bundle_id: Uuid::from_bytes([seed; 16]),
            block_number: u64::from(seed),
            block_hash: TxHash::from([seed; 32]),
        }
    }

    fn test_received_event(seed: u8) -> BundleEvent {
        BundleEvent::Received {
            bundle_id: Uuid::from_bytes([seed; 16]),
            bundle: Box::new(create_bundle_from_txn_data()),
        }
    }

    fn rpc_ok(persisted: u32) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 0,
            "result": persisted,
        }))
    }

    fn rpc_error() -> ResponseTemplate {
        ResponseTemplate::new(500).set_body_string("internal error")
    }

    async fn build_client(server: &MockServer) -> HttpClient {
        HttpClientBuilder::default()
            .request_timeout(Duration::from_millis(500))
            .build(server.uri())
            .expect("build client")
    }

    fn config(server: &MockServer, max_batch_size: usize, max_retries: u32) -> AuditConnectorConfig {
        AuditConnectorConfig::new(server.uri().parse().expect("parse url"))
            .with_max_batch_size(max_batch_size)
            .with_max_retries(max_retries)
            .with_retry_backoff(Duration::from_millis(1))
            .with_request_timeout(Duration::from_millis(500))
    }

    fn match_method(name: &str) -> impl Match {
        body_partial_json(json!({ "method": name }))
    }

    #[tokio::test]
    async fn batch_full_flush_sends_immediately() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .and(match_method("base_persistBundleEventBatch"))
            .respond_with(rpc_ok(2))
            .expect(1)
            .mount(&server)
            .await;

        let (tx, rx) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        let client = build_client(&server).await;
        let connector = AuditConnector::new(client, rx, Arc::new(config(&server, 2, 0)), cancel.clone());

        let handle = tokio::spawn(connector.run());

        tx.send(test_event(1)).await.unwrap();
        tx.send(test_event(2)).await.unwrap();

        // Allow time for the batch to flush before cancelling.
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();
        drop(tx);
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn cancellation_drains_partial_buffer() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .and(match_method("base_persistBundleEventBatch"))
            .respond_with(rpc_ok(1))
            .expect(1)
            .mount(&server)
            .await;

        let (tx, rx) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        let client = build_client(&server).await;
        let connector =
            AuditConnector::new(client, rx, Arc::new(config(&server, 100, 0)), cancel.clone());

        let handle = tokio::spawn(connector.run());

        tx.send(test_event(1)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        cancel.cancel();
        drop(tx);
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn channel_close_drains_buffer() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .and(match_method("base_persistBundleEventBatch"))
            .respond_with(rpc_ok(1))
            .expect(1)
            .mount(&server)
            .await;

        let (tx, rx) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        let client = build_client(&server).await;
        let connector =
            AuditConnector::new(client, rx, Arc::new(config(&server, 100, 0)), cancel.clone());

        let handle = tokio::spawn(connector.run());

        tx.send(test_event(1)).await.unwrap();
        drop(tx);
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn empty_buffer_on_cancellation_is_noop() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(rpc_ok(0))
            .expect(0)
            .mount(&server)
            .await;

        let (tx, rx) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        let client = build_client(&server).await;
        let connector =
            AuditConnector::new(client, rx, Arc::new(config(&server, 10, 0)), cancel.clone());

        let handle = tokio::spawn(connector.run());

        cancel.cancel();
        drop(tx);
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn rpc_error_after_retries_drops_batch() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .and(match_method("base_persistBundleEventBatch"))
            .respond_with(rpc_error())
            .expect(3) // initial + 2 retries
            .mount(&server)
            .await;

        let (tx, rx) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        let client = build_client(&server).await;
        let connector =
            AuditConnector::new(client, rx, Arc::new(config(&server, 1, 2)), cancel.clone());

        let handle = tokio::spawn(connector.run());

        tx.send(test_event(1)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        drop(tx);
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn partial_persistence_recorded() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .and(match_method("base_persistBundleEventBatch"))
            .respond_with(rpc_ok(1)) // server claims 1 of 3 persisted
            .expect(1)
            .mount(&server)
            .await;

        let (tx, rx) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        let client = build_client(&server).await;
        let connector =
            AuditConnector::new(client, rx, Arc::new(config(&server, 3, 0)), cancel.clone());

        let handle = tokio::spawn(connector.run());

        tx.send(test_event(1)).await.unwrap();
        tx.send(test_event(2)).await.unwrap();
        tx.send(test_event(3)).await.unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();
        drop(tx);
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn received_event_serializes_through_rpc() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .and(match_method("base_persistBundleEventBatch"))
            .respond_with(rpc_ok(1))
            .expect(1)
            .mount(&server)
            .await;

        let (tx, rx) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        let client = build_client(&server).await;
        let connector =
            AuditConnector::new(client, rx, Arc::new(config(&server, 1, 0)), cancel.clone());

        let handle = tokio::spawn(connector.run());

        tx.send(test_received_event(7)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();
        drop(tx);
        handle.await.unwrap();
    }
}
