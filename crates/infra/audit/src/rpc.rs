//! RPC server for the audit archiver.
//!
//! Exposes the `base_persistRejectedTransactionBatch` method for receiving batches
//! of rejected transactions from the builder and persisting them to storage.

use std::sync::Arc;

use base_bundles::RejectedTransaction;
use futures::stream::{self, StreamExt};
use jsonrpsee::{core::RpcResult, proc_macros::rpc, types::error::ErrorObjectOwned};
use jsonrpsee_types::error::ErrorCode;
use tracing::{error, info};

use crate::storage::RejectedTxStore;

const MAX_BATCH_SIZE: usize = 500;

/// RPC trait for the audit archiver.
#[rpc(server, namespace = "base")]
pub trait AuditArchiverApi {
    /// Persists a batch of rejected transactions to storage.
    /// Returns the number of items successfully persisted.
    #[method(name = "persistRejectedTransactionBatch")]
    async fn persist_rejected_transaction_batch(
        &self,
        batch: Vec<RejectedTransaction>,
    ) -> RpcResult<u32>;
}

/// RPC handler for audit archiver requests.
#[derive(Debug)]
pub struct AuditArchiverRpc<S> {
    storage: Arc<S>,
}

impl<S: RejectedTxStore> AuditArchiverRpc<S> {
    /// Creates a new `AuditArchiverRpc`.
    pub const fn new(storage: Arc<S>) -> Self {
        Self { storage }
    }
}

#[async_trait::async_trait]
impl<S: RejectedTxStore + 'static> AuditArchiverApiServer for AuditArchiverRpc<S> {
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
        // additional concurrent batch RPC calls while this batch's storage writes are in flight.
        let storage = Arc::clone(&self.storage);

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
}
