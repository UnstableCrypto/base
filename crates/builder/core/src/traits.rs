//! Trait bounds for Unstable builder components.

use alloy_consensus::Header;
use base_common_consensus::{UnstablePrimitives, UnstableTransactionSigned};
use base_execution_chainspec::UnstableChainSpec;
use base_execution_txpool::{UnstablePooledTx, BundleTransaction, TimestampedTransaction};
use base_node_core::UnstableEngineTypes;
use reth_node_api::{FullNodeTypes, NodeTypes};
use reth_payload_util::PayloadTransactions;
use reth_provider::{BlockReaderIdExt, ChainSpecProvider, StateProviderFactory};
use reth_transaction_pool::{TransactionPool, TransactionPoolExt};

/// Composite trait bound for a full node type compatible with the Unstable builder.
pub trait NodeBounds:
    FullNodeTypes<
    Types: NodeTypes<
        Payload = UnstableEngineTypes,
        ChainSpec = UnstableChainSpec,
        Primitives = UnstablePrimitives,
    >,
>
{
}

impl<T> NodeBounds for T where
    T: FullNodeTypes<
        Types: NodeTypes<
            Payload = UnstableEngineTypes,
            ChainSpec = UnstableChainSpec,
            Primitives = UnstablePrimitives,
        >,
    >
{
}

/// Composite trait bound for a transaction pool compatible with the Unstable builder.
pub trait PoolBounds:
    TransactionPool<
        Transaction: UnstablePooledTx<Consensus = UnstableTransactionSigned>
                         + BundleTransaction
                         + TimestampedTransaction,
    > + TransactionPoolExt
    + Unpin
    + 'static
where
    <Self as TransactionPool>::Transaction:
        UnstablePooledTx + BundleTransaction + TimestampedTransaction,
{
}

impl<T> PoolBounds for T
where
    T: TransactionPool<
            Transaction: UnstablePooledTx<Consensus = UnstableTransactionSigned>
                             + BundleTransaction
                             + TimestampedTransaction,
        > + TransactionPoolExt
        + Unpin
        + 'static,
    <Self as TransactionPool>::Transaction:
        UnstablePooledTx + BundleTransaction + TimestampedTransaction,
{
}

/// Composite trait bound for state provider clients used by the Unstable builder.
pub trait ClientBounds:
    StateProviderFactory
    + ChainSpecProvider<ChainSpec = UnstableChainSpec>
    + BlockReaderIdExt<Header = Header>
    + Clone
{
}

impl<T> ClientBounds for T where
    T: StateProviderFactory
        + ChainSpecProvider<ChainSpec = UnstableChainSpec>
        + BlockReaderIdExt<Header = Header>
        + Clone
{
}

/// Composite trait bound for payload transaction iterators used by the Unstable builder.
pub trait PayloadTxsBounds:
    PayloadTransactions<
    Transaction: UnstablePooledTx<Consensus = UnstableTransactionSigned>
                     + BundleTransaction
                     + TimestampedTransaction,
>
{
}

impl<T> PayloadTxsBounds for T where
    T: PayloadTransactions<
        Transaction: UnstablePooledTx<Consensus = UnstableTransactionSigned>
                         + BundleTransaction
                         + TimestampedTransaction,
    >
{
}
