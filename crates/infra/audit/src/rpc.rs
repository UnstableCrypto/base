//! RPC server for the audit archiver.
//!
//! Exposes:
//! - `base_persistRejectedTransactionBatch` — receives batches of rejected
//!   transactions from the builder and persists them to S3.
//! - `base_persistEvent` — receives a single `BundleEvent` (e.g. from a
//!   mempool node forwarding a transaction) and persists it to S3 at the
//!   key derived from the event's content.
//! - `base_persistEventBatch` — receives a batch of `BundleEvent`s and
//!   persists each to S3 concurrently. Returns the count successfully
//!   persisted.

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use base_bundles::{BundleEvent, RejectedTransaction};
use futures::stream::{self, StreamExt};
use jsonrpsee::{core::RpcResult, proc_macros::rpc, types::error::ErrorObjectOwned};
use jsonrpsee_types::error::ErrorCode;
use tracing::{error, info};

use crate::{reader::Event, storage::S3EventReaderWriter};

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

    /// Persists a single bundle event to S3 storage at the key derived from
    /// the event's content (see `BundleEvent::s3_event_key`).
    #[method(name = "persistEvent")]
    async fn persist_event(&self, event: BundleEvent) -> RpcResult<()>;

    /// Persists a batch of bundle events to S3 storage concurrently.
    /// Returns the number of events successfully persisted.
    #[method(name = "persistEventBatch")]
    async fn persist_event_batch(&self, batch: Vec<BundleEvent>) -> RpcResult<u32>;
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

    async fn persist_event(&self, event: BundleEvent) -> RpcResult<()> {
        let evt = build_event(event);

        self.storage.write_event(&evt).await.map_err(|e| {
            error!(error = %e, "failed to persist bundle event");
            ErrorObjectOwned::owned(
                ErrorCode::InternalError.code(),
                format!("failed to persist event: {e}"),
                None::<()>,
            )
        })?;

        Ok(())
    }

    async fn persist_event_batch(&self, batch: Vec<BundleEvent>) -> RpcResult<u32> {
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

        let storage = Arc::clone(&self.storage);

        let persisted = stream::iter(batch)
            .map(move |event| {
                let storage = Arc::clone(&storage);
                async move {
                    let evt = build_event(event);
                    let result = storage.write_event(&evt).await;
                    (evt, result)
                }
            })
            .buffer_unordered(5)
            .fold(0u32, |persisted, (evt, result)| async move {
                if let Err(e) = result {
                    error!(error = %e, key = %evt.key, "Failed to persist bundle event");
                    persisted
                } else {
                    persisted + 1
                }
            })
            .await;

        Ok(persisted)
    }
}

fn build_event(event: BundleEvent) -> Event {
    let key = event.generate_event_key();
    let timestamp =
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    Event { key, event, timestamp }
}
