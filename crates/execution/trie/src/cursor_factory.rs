//! Implements [`TrieCursorFactory`] and [`HashedCursorFactory`] for [`UnstableProofsStore`] types.

use std::marker::PhantomData;

use alloy_primitives::B256;
use reth_db::DatabaseError;
use reth_trie::{hashed_cursor::HashedCursorFactory, trie_cursor::TrieCursorFactory};

use crate::{
    UnstableProofsHashedAccountCursor, UnstableProofsHashedStorageCursor, UnstableProofsStorage,
    UnstableProofsStore, UnstableProofsTrieCursor,
};

/// Factory for creating trie cursors for [`UnstableProofsStore`].
#[derive(Debug, Clone)]
pub struct UnstableProofsTrieCursorFactory<'tx, S: UnstableProofsStore> {
    storage: &'tx UnstableProofsStorage<S>,
    block_number: u64,
    _marker: PhantomData<&'tx ()>,
}

impl<'tx, S: UnstableProofsStore> UnstableProofsTrieCursorFactory<'tx, S> {
    /// Initializes new `UnstableProofsTrieCursorFactory`
    pub const fn new(storage: &'tx UnstableProofsStorage<S>, block_number: u64) -> Self {
        Self { storage, block_number, _marker: PhantomData }
    }
}

impl<'tx, S> TrieCursorFactory for UnstableProofsTrieCursorFactory<'tx, S>
where
    for<'a> S: UnstableProofsStore + 'tx,
{
    type AccountTrieCursor<'a>
        = UnstableProofsTrieCursor<S::AccountTrieCursor<'a>>
    where
        Self: 'a;
    type StorageTrieCursor<'a>
        = UnstableProofsTrieCursor<S::StorageTrieCursor<'a>>
    where
        Self: 'a;

    fn account_trie_cursor(&self) -> Result<Self::AccountTrieCursor<'_>, DatabaseError> {
        Ok(UnstableProofsTrieCursor::new(
            self.storage
                .account_trie_cursor(self.block_number)
                .map_err(Into::<DatabaseError>::into)?,
        ))
    }

    fn storage_trie_cursor(
        &self,
        hashed_address: B256,
    ) -> Result<Self::StorageTrieCursor<'_>, DatabaseError> {
        Ok(UnstableProofsTrieCursor::new(
            self.storage
                .storage_trie_cursor(hashed_address, self.block_number)
                .map_err(Into::<DatabaseError>::into)?,
        ))
    }
}

/// Factory for creating hashed account cursors for [`UnstableProofsStore`].
#[derive(Debug, Clone)]
pub struct UnstableProofsHashedAccountCursorFactory<'tx, S: UnstableProofsStore> {
    storage: &'tx UnstableProofsStorage<S>,
    block_number: u64,
    _marker: PhantomData<&'tx ()>,
}

impl<'tx, S: UnstableProofsStore> UnstableProofsHashedAccountCursorFactory<'tx, S> {
    /// Creates a new `UnstableProofsHashedAccountCursorFactory` instance.
    pub const fn new(storage: &'tx UnstableProofsStorage<S>, block_number: u64) -> Self {
        Self { storage, block_number, _marker: PhantomData }
    }
}

impl<'tx, S> HashedCursorFactory for UnstableProofsHashedAccountCursorFactory<'tx, S>
where
    S: UnstableProofsStore + 'tx,
{
    type AccountCursor<'a>
        = UnstableProofsHashedAccountCursor<S::AccountHashedCursor<'a>>
    where
        Self: 'a;
    type StorageCursor<'a>
        = UnstableProofsHashedStorageCursor<S::StorageCursor<'a>>
    where
        Self: 'a;

    fn hashed_account_cursor(&self) -> Result<Self::AccountCursor<'_>, DatabaseError> {
        Ok(UnstableProofsHashedAccountCursor::new(
            self.storage.account_hashed_cursor(self.block_number)?,
        ))
    }

    fn hashed_storage_cursor(
        &self,
        hashed_address: B256,
    ) -> Result<Self::StorageCursor<'_>, DatabaseError> {
        Ok(UnstableProofsHashedStorageCursor::new(
            self.storage.storage_hashed_cursor(hashed_address, self.block_number)?,
        ))
    }
}
