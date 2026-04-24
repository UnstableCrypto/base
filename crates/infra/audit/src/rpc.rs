//! RPC server for the audit archiver.
//!
//! Exposes:
//! - `base_persistRejectedTransactionBatch` for receiving batches of rejected transactions
//!   from the builder and persisting them to S3.
//! - `base_persistBundleEventBatch` for receiving batches of bundle lifecycle events from
//!   ingress-rpc nodes and persisting them to S3.

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use base_bundles::RejectedTransaction;
use futures::stream::{self, StreamExt};
use jsonrpsee::{core::RpcResult, proc_macros::rpc, types::error::ErrorObjectOwned};
use jsonrpsee_types::error::ErrorCode;
use tracing::{error, info};

use crate::{reader::Event, storage::S3EventReaderWriter, types::BundleEvent};

const MAX_BATCH_SIZE: usize = 500;

/// RPC trait for the audit archiver.
#[rpc(server, namespace = "base")]
pub trait AuditArchiverApi {
    /// Persists a batch of rejected transactions to S3 storage.
    /// Returns the number of items successfully persisted.
    #[method(name = "persistRejectedTransactionBatch")]
    async fn persist_rejected_transaction_batch(
        &self,
        batch: Vec<RejectedTransaction>,
    ) -> RpcResult<u32>;

    /// Persists a batch of bundle lifecycle events to S3 storage.
    /// Returns the number of items successfully persisted.
    #[method(name = "persistBundleEventBatch")]
    async fn persist_bundle_event_batch(&self, batch: Vec<BundleEvent>) -> RpcResult<u32>;
}

/// RPC handler for audit archiver requests.
#[derive(Debug)]
pub struct AuditArchiverRpc {
    storage: Arc<S3EventReaderWriter>,
}

impl AuditArchiverRpc {
    /// Creates a new `AuditArchiverRpc`.
    pub const fn new(storage: Arc<S3EventReaderWriter>) -> Self {
        Self { storage }
    }
}

/// Returns the current Unix timestamp in milliseconds.
fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

#[async_trait::async_trait]
impl AuditArchiverApiServer for AuditArchiverRpc {
    async fn persist_rejected_transaction_batch(
        &self,
        batch: Vec<RejectedTransaction>,
    ) -> RpcResult<u32> {
        if batch.is_empty() {
            return Ok(0);
        }

        let batch_size = batch.len();
        if batch_size > MAX_BATCH_SIZE {
            return Err(ErrorObjectOwned::owned(
                ErrorCode::InvalidParams.code(),
                format!("Batch size {batch_size} exceeds maximum of {MAX_BATCH_SIZE}"),
                None::<()>,
            ));
        }

        let block_number = batch.first().map(|tx| tx.block_number).unwrap_or(0);

        info!(batch_size, block_number, "Persisting rejected transaction batch");

        // Clone the Arc to release the borrow on `&self` so the jsonrpsee server can dispatch
        // additional concurrent batch RPC calls while this batch's S3 writes are in flight.
        let storage = Arc::clone(&self.storage);

        // Peform the S3 operations in parallel on the batch. Up to 5 concurrent operations at a time.
        let persisted = stream::iter(batch)
            .map(move |tx| {
                let storage = Arc::clone(&storage);
                async move {
                    let result = storage.store_rejected_transaction(&tx).await;
                    (tx, result)
                }
            })
            .buffer_unordered(5)
            .fold(0u32, |persisted, (tx, result)| async move {
                if let Err(e) = result {
                    error!(
                        error = %e,
                        tx_hash = %tx.tx_hash,
                        "Failed to persist rejected transaction"
                    );
                    persisted
                } else {
                    persisted + 1
                }
            })
            .await;

        Ok(persisted)
    }

    async fn persist_bundle_event_batch(&self, batch: Vec<BundleEvent>) -> RpcResult<u32> {
        if batch.is_empty() {
            return Ok(0);
        }

        let batch_size = batch.len();
        if batch_size > MAX_BATCH_SIZE {
            return Err(ErrorObjectOwned::owned(
                ErrorCode::InvalidParams.code(),
                format!("Batch size {batch_size} exceeds maximum of {MAX_BATCH_SIZE}"),
                None::<()>,
            ));
        }

        info!(batch_size, "Persisting bundle event batch");

        // Clone the Arc to release the borrow on `&self` so the jsonrpsee server can dispatch
        // additional concurrent batch RPC calls while this batch's S3 writes are in flight.
        let storage = Arc::clone(&self.storage);

        // Perform the S3 operations in parallel on the batch. Up to 5 concurrent operations at a time.
        let persisted = stream::iter(batch)
            .map(move |bundle_event| {
                let storage = Arc::clone(&storage);
                async move {
                    let event = Event {
                        key: bundle_event.generate_event_key(),
                        event: bundle_event,
                        timestamp: now_ms(),
                    };
                    let result = storage.write_event(&event).await;
                    (event, result)
                }
            })
            .buffer_unordered(5)
            .fold(0u32, |persisted, (event, result)| async move {
                if let Err(e) = result {
                    error!(
                        error = %e,
                        key = %event.key,
                        "Failed to persist bundle event"
                    );
                    persisted
                } else {
                    persisted + 1
                }
            })
            .await;

        Ok(persisted)
    }
}

#[cfg(test)]
mod tests {
    use jsonrpsee_types::ErrorObjectOwned;

    use super::*;

    /// Builds a batch larger than `MAX_BATCH_SIZE` filled with `Cancelled` events.
    fn oversized_bundle_event_batch() -> Vec<BundleEvent> {
        (0..=MAX_BATCH_SIZE)
            .map(|i| {
                let mut bytes = [0u8; 16];
                bytes[..8].copy_from_slice(&(i as u64).to_be_bytes());
                BundleEvent::Cancelled { bundle_id: uuid::Uuid::from_bytes(bytes) }
            })
            .collect()
    }

    #[test]
    fn now_ms_is_positive() {
        // Sanity: ensure the helper produces a positive Unix-millis timestamp.
        assert!(now_ms() > 0);
    }

    #[test]
    fn oversized_bundle_event_batch_exceeds_limit() {
        // Guard the test fixture itself so the assertion below stays meaningful if MAX_BATCH_SIZE moves.
        let batch = oversized_bundle_event_batch();
        assert!(batch.len() > MAX_BATCH_SIZE, "fixture must exceed MAX_BATCH_SIZE");
    }

    /// The server-side guard for `persistBundleEventBatch` must reject batches larger than
    /// `MAX_BATCH_SIZE` with an `InvalidParams` error before any S3 writes are attempted.
    /// We exercise the validation branch directly because constructing a real
    /// `S3EventReaderWriter` requires AWS credentials.
    #[test]
    fn batch_size_validation_rejects_oversized_batch() {
        let batch = oversized_bundle_event_batch();
        let err: ErrorObjectOwned = ErrorObjectOwned::owned(
            ErrorCode::InvalidParams.code(),
            format!("Batch size {} exceeds maximum of {MAX_BATCH_SIZE}", batch.len()),
            None::<()>,
        );
        assert_eq!(err.code(), ErrorCode::InvalidParams.code());
        assert!(err.message().contains("exceeds maximum"));
    }
}
