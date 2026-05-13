//! Storage wrapper that records metrics for all operations.

use std::{
    fmt::Debug,
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

use alloy_eips::{BlockNumHash, eip1898::BlockWithParent};
use alloy_primitives::{B256, U256};
use derive_more::Constructor;
use reth_db::DatabaseError;
use reth_primitives_traits::Account;
use reth_trie::{
    hashed_cursor::{HashedCursor, HashedStorageCursor},
    trie_cursor::{TrieCursor, TrieStorageCursor},
};
use reth_trie_common::{BranchNodeCompact, Nibbles};

use crate::{
    UnstableProofsStorageResult, UnstableProofsStore, BlockStateDiff,
    api::{UnstableProofsInitialStateStore, InitialStateAnchor, OperationDurations, WriteCounts},
    cursor,
};

/// Alias for [`UnstableProofsStorageWithMetrics`].
pub type UnstableProofsStorage<S> = UnstableProofsStorageWithMetrics<S>;

/// Alias for [`TrieCursor`](cursor::UnstableProofsTrieCursor) with metrics layer.
pub type UnstableProofsTrieCursor<C> = cursor::UnstableProofsTrieCursor<UnstableProofsTrieCursorWithMetrics<C>>;

/// Alias for [`UnstableProofsHashedAccountCursor`](cursor::UnstableProofsHashedAccountCursor) with metrics
/// layer.
pub type UnstableProofsHashedAccountCursor<C> =
    cursor::UnstableProofsHashedAccountCursor<UnstableProofsHashedCursorWithMetrics<C>>;

/// Alias for [`UnstableProofsHashedStorageCursor`](cursor::UnstableProofsHashedStorageCursor) with metrics
/// layer.
pub type UnstableProofsHashedStorageCursor<C> =
    cursor::UnstableProofsHashedStorageCursor<UnstableProofsHashedCursorWithMetrics<C>>;

/// Types of storage operations that can be tracked.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum StorageOperation {
    /// Store account trie branch
    StoreAccountBranch,
    /// Store storage trie branch
    StoreStorageBranch,
    /// Store hashed account
    StoreHashedAccount,
    /// Store hashed storage
    StoreHashedStorage,
    /// Trie cursor seek exact operation
    TrieCursorSeekExact,
    /// Trie cursor seek
    TrieCursorSeek,
    /// Trie cursor next
    TrieCursorNext,
    /// Trie cursor current
    TrieCursorCurrent,
    /// Hashed cursor seek
    HashedCursorSeek,
    /// Hashed cursor next
    HashedCursorNext,
}

impl StorageOperation {
    /// Returns the operation as a string for metrics labels.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::StoreAccountBranch => "store_account_branch",
            Self::StoreStorageBranch => "store_storage_branch",
            Self::StoreHashedAccount => "store_hashed_account",
            Self::StoreHashedStorage => "store_hashed_storage",
            Self::TrieCursorSeekExact => "trie_cursor_seek_exact",
            Self::TrieCursorSeek => "trie_cursor_seek",
            Self::TrieCursorNext => "trie_cursor_next",
            Self::TrieCursorCurrent => "trie_cursor_current",
            Self::HashedCursorSeek => "hashed_cursor_seek",
            Self::HashedCursorNext => "hashed_cursor_next",
        }
    }
}

base_metrics::define_metrics! {
    base_trie.storage.operation,
    struct = OperationMetrics,
    #[describe("Duration of storage operations in seconds")]
    #[label(operation)]
    duration_seconds: histogram,
}

base_metrics::define_metrics! {
    base_trie.block,
    struct = BlockMetrics,
    #[describe("Total time to process a block (end-to-end) in seconds")]
    total_duration_seconds: histogram,
    #[describe("Time spent executing the block (EVM) in seconds")]
    execution_duration_seconds: histogram,
    #[describe("Time spent calculating state root in seconds")]
    state_root_duration_seconds: histogram,
    #[describe("Time spent writing trie updates to storage in seconds")]
    write_duration_seconds: histogram,
    #[describe("Number of trie updates written")]
    account_trie_updates_written_total: counter,
    #[describe("Number of storage trie updates written")]
    storage_trie_updates_written_total: counter,
    #[describe("Number of hashed accounts written")]
    hashed_accounts_written_total: counter,
    #[describe("Number of hashed storages written")]
    hashed_storages_written_total: counter,
    #[describe("Earliest block number that the proofs storage has stored")]
    earliest_number: gauge,
    #[describe("Latest block number that the proofs storage has stored")]
    latest_number: gauge,
}

impl BlockMetrics {
    /// Record operation durations for the processing of a block.
    pub fn record_operation_durations(durations: &OperationDurations) {
        Self::total_duration_seconds().record(durations.total_duration_seconds);
        Self::execution_duration_seconds().record(durations.execution_duration_seconds);
        Self::state_root_duration_seconds().record(durations.state_root_duration_seconds);
        Self::write_duration_seconds().record(durations.write_duration_seconds);
    }

    /// Increment write counts of historical trie updates for a single block.
    pub fn increment_write_counts(counts: &WriteCounts) {
        Self::account_trie_updates_written_total()
            .increment(counts.account_trie_updates_written_total);
        Self::storage_trie_updates_written_total()
            .increment(counts.storage_trie_updates_written_total);
        Self::hashed_accounts_written_total().increment(counts.hashed_accounts_written_total);
        Self::hashed_storages_written_total().increment(counts.hashed_storages_written_total);
    }
}

/// Metrics for storage operations.
#[derive(Debug, Default, Clone)]
pub struct StorageMetrics;

impl StorageMetrics {
    /// Record a storage operation with timing.
    pub fn record_operation<R>(&self, operation: StorageOperation, f: impl FnOnce() -> R) -> R {
        base_metrics::time!(OperationMetrics::duration_seconds(operation.as_str()), { f() })
    }

    /// Record a storage operation with timing (async version).
    pub async fn record_operation_async<F, R>(&self, operation: StorageOperation, f: F) -> R
    where
        F: Future<Output = R>,
    {
        base_metrics::time!(OperationMetrics::duration_seconds(operation.as_str()), { f.await })
    }

    /// Record a pre-measured duration for an operation.
    pub fn record_duration(&self, operation: StorageOperation, duration: Duration) {
        OperationMetrics::duration_seconds(operation.as_str()).record(duration);
    }

    /// Record multiple items with the same duration.
    pub fn record_duration_per_item(
        &self,
        operation: StorageOperation,
        duration: Duration,
        count: usize,
    ) {
        if count > 0
            && let Some(count_u32) = u32::try_from(count).ok()
        {
            OperationMetrics::duration_seconds(operation.as_str())
                .record_many(duration / count_u32, count);
        }
    }
}

/// Wrapper for [`TrieCursor`] that records metrics.
#[derive(Debug, Constructor, Clone)]
pub struct UnstableProofsTrieCursorWithMetrics<C> {
    cursor: C,
    metrics: Arc<StorageMetrics>,
}

impl<C: TrieCursor> TrieCursor for UnstableProofsTrieCursorWithMetrics<C> {
    #[inline]
    fn seek_exact(
        &mut self,
        path: Nibbles,
    ) -> Result<Option<(Nibbles, BranchNodeCompact)>, DatabaseError> {
        self.metrics.record_operation(StorageOperation::TrieCursorSeekExact, || {
            self.cursor.seek_exact(path)
        })
    }

    #[inline]
    fn seek(
        &mut self,
        path: Nibbles,
    ) -> Result<Option<(Nibbles, BranchNodeCompact)>, DatabaseError> {
        self.metrics.record_operation(StorageOperation::TrieCursorSeek, || self.cursor.seek(path))
    }

    #[inline]
    fn next(&mut self) -> Result<Option<(Nibbles, BranchNodeCompact)>, DatabaseError> {
        self.metrics.record_operation(StorageOperation::TrieCursorNext, || self.cursor.next())
    }

    #[inline]
    fn current(&mut self) -> Result<Option<Nibbles>, DatabaseError> {
        self.metrics.record_operation(StorageOperation::TrieCursorCurrent, || self.cursor.current())
    }

    #[inline]
    fn reset(&mut self) {
        self.cursor.reset()
    }
}

impl<C: TrieStorageCursor> TrieStorageCursor for UnstableProofsTrieCursorWithMetrics<C> {
    #[inline]
    fn set_hashed_address(&mut self, _hashed_address: B256) {
        self.cursor.set_hashed_address(_hashed_address)
    }
}

/// Wrapper for [`HashedCursor`] type that records metrics.
#[derive(Debug, Constructor, Clone)]
pub struct UnstableProofsHashedCursorWithMetrics<C> {
    cursor: C,
    metrics: Arc<StorageMetrics>,
}

impl<C: HashedCursor> HashedCursor for UnstableProofsHashedCursorWithMetrics<C> {
    type Value = C::Value;

    #[inline]
    fn seek(&mut self, key: B256) -> Result<Option<(B256, Self::Value)>, DatabaseError> {
        self.metrics.record_operation(StorageOperation::HashedCursorSeek, || self.cursor.seek(key))
    }

    #[inline]
    fn next(&mut self) -> Result<Option<(B256, Self::Value)>, DatabaseError> {
        self.metrics.record_operation(StorageOperation::HashedCursorNext, || self.cursor.next())
    }

    #[inline]
    fn reset(&mut self) {
        self.cursor.reset()
    }
}

impl<C: HashedStorageCursor> HashedStorageCursor for UnstableProofsHashedCursorWithMetrics<C> {
    #[inline]
    fn is_storage_empty(&mut self) -> Result<bool, DatabaseError> {
        self.cursor.is_storage_empty()
    }

    #[inline]
    fn set_hashed_address(&mut self, _hashed_address: B256) {
        self.cursor.set_hashed_address(_hashed_address)
    }
}

/// Wrapper around [`UnstableProofsStore`] type that records metrics for all operations.
#[derive(Debug, Clone)]
pub struct UnstableProofsStorageWithMetrics<S> {
    storage: S,
    metrics: Arc<StorageMetrics>,
}

impl<S> UnstableProofsStorageWithMetrics<S> {
    /// Initializes new [`StorageMetrics`] and wraps given storage instance.
    pub fn new(storage: S) -> Self {
        Self { storage, metrics: Arc::new(StorageMetrics) }
    }

    /// Get the underlying storage.
    pub const fn inner(&self) -> &S {
        &self.storage
    }

    /// Get the metrics.
    pub const fn metrics(&self) -> &Arc<StorageMetrics> {
        &self.metrics
    }
}

impl<S> UnstableProofsStore for UnstableProofsStorageWithMetrics<S>
where
    S: UnstableProofsStore,
{
    type StorageTrieCursor<'tx>
        = UnstableProofsTrieCursorWithMetrics<S::StorageTrieCursor<'tx>>
    where
        Self: 'tx;
    type AccountTrieCursor<'tx>
        = UnstableProofsTrieCursorWithMetrics<S::AccountTrieCursor<'tx>>
    where
        Self: 'tx;
    type StorageCursor<'tx>
        = UnstableProofsHashedCursorWithMetrics<S::StorageCursor<'tx>>
    where
        Self: 'tx;
    type AccountHashedCursor<'tx>
        = UnstableProofsHashedCursorWithMetrics<S::AccountHashedCursor<'tx>>
    where
        Self: 'tx;

    #[inline]
    fn get_earliest_block_number(&self) -> UnstableProofsStorageResult<Option<(u64, B256)>> {
        self.storage.get_earliest_block_number()
    }

    #[inline]
    fn get_latest_block_number(&self) -> UnstableProofsStorageResult<Option<(u64, B256)>> {
        self.storage.get_latest_block_number()
    }

    #[inline]
    fn storage_trie_cursor<'tx>(
        &self,
        hashed_address: B256,
        max_block_number: u64,
    ) -> UnstableProofsStorageResult<Self::StorageTrieCursor<'tx>> {
        let cursor = self.storage.storage_trie_cursor(hashed_address, max_block_number)?;
        Ok(UnstableProofsTrieCursorWithMetrics::new(cursor, Arc::clone(&self.metrics)))
    }

    #[inline]
    fn account_trie_cursor<'tx>(
        &self,
        max_block_number: u64,
    ) -> UnstableProofsStorageResult<Self::AccountTrieCursor<'tx>> {
        let cursor = self.storage.account_trie_cursor(max_block_number)?;
        Ok(UnstableProofsTrieCursorWithMetrics::new(cursor, Arc::clone(&self.metrics)))
    }

    #[inline]
    fn storage_hashed_cursor<'tx>(
        &self,
        hashed_address: B256,
        max_block_number: u64,
    ) -> UnstableProofsStorageResult<Self::StorageCursor<'tx>> {
        let cursor = self.storage.storage_hashed_cursor(hashed_address, max_block_number)?;
        Ok(UnstableProofsHashedCursorWithMetrics::new(cursor, Arc::clone(&self.metrics)))
    }

    #[inline]
    fn account_hashed_cursor<'tx>(
        &self,
        max_block_number: u64,
    ) -> UnstableProofsStorageResult<Self::AccountHashedCursor<'tx>> {
        let cursor = self.storage.account_hashed_cursor(max_block_number)?;
        Ok(UnstableProofsHashedCursorWithMetrics::new(cursor, Arc::clone(&self.metrics)))
    }

    #[inline]
    fn store_trie_updates(
        &self,
        block_ref: BlockWithParent,
        block_state_diff: BlockStateDiff,
    ) -> UnstableProofsStorageResult<WriteCounts> {
        let result = self.storage.store_trie_updates(block_ref, block_state_diff)?;
        BlockMetrics::latest_number().set(block_ref.block.number as f64);
        Ok(result)
    }

    #[inline]
    fn fetch_trie_updates(&self, block_number: u64) -> UnstableProofsStorageResult<BlockStateDiff> {
        self.storage.fetch_trie_updates(block_number)
    }
    #[inline]
    fn prune_earliest_state(
        &self,
        new_earliest_block_ref: BlockWithParent,
    ) -> UnstableProofsStorageResult<WriteCounts> {
        BlockMetrics::earliest_number().set(new_earliest_block_ref.block.number as f64);
        self.storage.prune_earliest_state(new_earliest_block_ref)
    }

    #[inline]
    fn unwind_history(&self, to: BlockWithParent) -> UnstableProofsStorageResult<()> {
        self.storage.unwind_history(to)
    }

    #[inline]
    fn replace_updates(
        &self,
        latest_common_block: BlockNumHash,
        blocks_to_add: Vec<(BlockWithParent, BlockStateDiff)>,
    ) -> UnstableProofsStorageResult<()> {
        self.storage.replace_updates(latest_common_block, blocks_to_add)
    }

    #[inline]
    fn set_earliest_block_number(
        &self,
        block_number: u64,
        hash: B256,
    ) -> UnstableProofsStorageResult<()> {
        BlockMetrics::earliest_number().set(block_number as f64);
        self.storage.set_earliest_block_number(block_number, hash)
    }
}

impl<S> UnstableProofsInitialStateStore for UnstableProofsStorageWithMetrics<S>
where
    S: UnstableProofsInitialStateStore,
{
    #[inline]
    fn initial_state_anchor(&self) -> UnstableProofsStorageResult<InitialStateAnchor> {
        self.storage.initial_state_anchor()
    }

    #[inline]
    fn set_initial_state_anchor(&self, anchor: BlockNumHash) -> UnstableProofsStorageResult<()> {
        self.storage.set_initial_state_anchor(anchor)
    }

    #[inline]
    fn store_account_branches(
        &self,
        account_nodes: Vec<(Nibbles, Option<BranchNodeCompact>)>,
    ) -> UnstableProofsStorageResult<()> {
        let count = account_nodes.len();
        let start = Instant::now();
        let result = self.storage.store_account_branches(account_nodes);
        let duration = start.elapsed();

        if count > 0 {
            self.metrics.record_duration_per_item(
                StorageOperation::StoreAccountBranch,
                duration,
                count,
            );
        }

        result
    }

    #[inline]
    fn store_storage_branches(
        &self,
        hashed_address: B256,
        storage_nodes: Vec<(Nibbles, Option<BranchNodeCompact>)>,
    ) -> UnstableProofsStorageResult<()> {
        let count = storage_nodes.len();
        let start = Instant::now();
        let result = self.storage.store_storage_branches(hashed_address, storage_nodes);
        let duration = start.elapsed();

        if count > 0 {
            self.metrics.record_duration_per_item(
                StorageOperation::StoreStorageBranch,
                duration,
                count,
            );
        }

        result
    }

    #[inline]
    fn store_hashed_accounts(
        &self,
        accounts: Vec<(B256, Option<Account>)>,
    ) -> UnstableProofsStorageResult<()> {
        let count = accounts.len();
        let start = Instant::now();
        let result = self.storage.store_hashed_accounts(accounts);
        let duration = start.elapsed();

        if count > 0 {
            self.metrics.record_duration_per_item(
                StorageOperation::StoreHashedAccount,
                duration,
                count,
            );
        }

        result
    }

    #[inline]
    fn store_hashed_storages(
        &self,
        hashed_address: B256,
        storages: Vec<(B256, U256)>,
    ) -> UnstableProofsStorageResult<()> {
        let count = storages.len();
        let start = Instant::now();
        let result = self.storage.store_hashed_storages(hashed_address, storages);
        let duration = start.elapsed();

        if count > 0 {
            self.metrics.record_duration_per_item(
                StorageOperation::StoreHashedStorage,
                duration,
                count,
            );
        }

        result
    }

    #[inline]
    fn commit_initial_state(&self) -> UnstableProofsStorageResult<BlockNumHash> {
        let block = self.storage.commit_initial_state()?;
        BlockMetrics::earliest_number().set(block.number as f64);
        Ok(block)
    }
}

impl<S> From<S> for UnstableProofsStorageWithMetrics<S>
where
    S: UnstableProofsStore + Clone + 'static,
{
    fn from(storage: S) -> Self {
        Self::new(storage)
    }
}
