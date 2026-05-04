//! Narrow provider trait for [`super::L1WatcherActor`].

use alloy_consensus::Header;
use alloy_eips::{BlockId, BlockNumHash};
use alloy_primitives::B256;
use alloy_provider::Provider;
use alloy_rpc_types_eth::{Block, Filter, Log};
use alloy_transport::{TransportError, TransportErrorKind};
use async_trait::async_trait;
use base_consensus_providers::AlloyChainProvider;

/// A narrow trait exposing only the two L1 RPC methods used by [`super::L1WatcherActor`].
///
/// Replacing the broad [`alloy_provider::Provider`] bound with this trait makes
/// in-process test implementations straightforward — a test double only needs
/// to implement `get_logs` and `get_block` rather than the full ~30-method
/// provider interface.
#[async_trait]
pub trait L1BlockFetcher: Send + Sync + 'static {
    /// Error type returned by all fetch operations.
    type Error: std::fmt::Display + std::fmt::Debug + Send;

    /// Return all logs matching `filter`.
    async fn get_logs(&self, filter: Filter) -> Result<Vec<Log>, Self::Error>;

    /// Return the block identified by `id`, or `None` if it does not exist.
    async fn get_block(&self, id: BlockId) -> Result<Option<Block>, Self::Error>;
}

/// Wraps an [`alloy_provider::Provider`] to implement [`L1BlockFetcher`].
///
/// Construct this with the production L1 provider and pass it to
/// [`super::L1WatcherActor::new`] in place of the bare provider.
#[derive(Debug)]
pub struct AlloyL1BlockFetcher<P> {
    /// The underlying L1 provider.
    pub provider: P,
    /// Whether to trust RPC responses without header-commitment verification.
    pub trust_rpc: bool,
}

impl<P> AlloyL1BlockFetcher<P> {
    /// Creates an L1 block fetcher with the configured trust mode.
    pub const fn new(provider: P, trust_rpc: bool) -> Self {
        Self { provider, trust_rpc }
    }

    /// Converts a custom validation failure into the fetcher's transport error type.
    pub fn custom_error(message: impl Into<String>) -> TransportError {
        alloy_transport::RpcError::Transport(TransportErrorKind::Custom(message.into().into()))
    }

    /// Verifies that a fetched header is the requested block hash.
    pub fn verify_header_hash(header: &Header, expected_hash: B256) -> Result<(), TransportError> {
        let actual_hash = header.hash_slow();
        if actual_hash != expected_hash {
            return Err(Self::custom_error(format!(
                "L1 header hash mismatch: expected {:?}, got {:?}",
                expected_hash, actual_hash
            )));
        }

        Ok(())
    }
}

#[async_trait]
impl<P> L1BlockFetcher for AlloyL1BlockFetcher<P>
where
    P: Provider + 'static,
{
    type Error = TransportError;

    async fn get_logs(&self, filter: Filter) -> Result<Vec<Log>, Self::Error> {
        if self.trust_rpc {
            return Ok(self.provider.get_logs(&filter).await?);
        }

        let block_hash = filter.get_block_hash().ok_or_else(|| {
            Self::custom_error(
                "cannot verify L1 logs without a block-hash-pinned filter when trust_rpc=false",
            )
        })?;

        let block = self
            .provider
            .get_block(BlockId::Hash(block_hash.into()))
            .await?
            .ok_or_else(|| Self::custom_error(format!("L1 block not found: {block_hash:?}")))?;
        let header: Header = block.header.clone().into_consensus();
        Self::verify_header_hash(&header, block_hash)?;
        let receipts =
            self.provider.get_block_receipts(BlockId::Hash(block_hash.into())).await?.ok_or_else(
                || Self::custom_error(format!("L1 block receipts not found: {block_hash:?}")),
            )?;
        let consensus_receipts = receipts
            .iter()
            .map(|receipt| receipt.inner.clone().into_primitives_receipt().as_receipt().cloned())
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| {
                Self::custom_error(format!(
                    "failed to convert L1 block receipts into consensus receipts: {block_hash:?}"
                ))
            })?;
        let receipt_envelopes = receipts
            .iter()
            .map(|receipt| receipt.inner.clone().into_primitives_receipt())
            .collect::<Vec<_>>();

        AlloyChainProvider::verify_receipts_root_and_logs_bloom(
            &header,
            block_hash,
            &receipt_envelopes,
        )
        .map_err(|error| Self::custom_error(error.to_string()))?;

        let tx_hashes_and_receipts = receipts
            .iter()
            .zip(consensus_receipts.iter())
            .map(|(receipt, consensus_receipt)| (receipt.transaction_hash, consensus_receipt));

        Ok(filter.matching_block_logs(
            BlockNumHash { number: header.number, hash: block_hash },
            header.timestamp,
            tx_hashes_and_receipts,
            false,
        ))
    }

    async fn get_block(&self, id: BlockId) -> Result<Option<Block>, Self::Error> {
        let block = self.provider.get_block(id).await?;

        if !self.trust_rpc
            && let (BlockId::Hash(expected_hash), Some(block)) = (id, &block)
        {
            let header = block.header.clone().into_consensus();
            Self::verify_header_hash(&header, expected_hash.block_hash)?;
        }

        Ok(block)
    }
}
