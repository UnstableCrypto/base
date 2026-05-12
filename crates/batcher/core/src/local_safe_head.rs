//! Provider trait for fetching the current local safe L2 head.

use std::error::Error;

use futures::future::BoxFuture;

/// Result type returned by local-safe-head providers.
pub type LocalSafeHeadResult = Result<u64, Box<dyn Error + Send + Sync>>;

/// Fetches the current local safe L2 block number from the rollup node.
///
/// Reset and catchup decisions must use a fresh local safe head rather than a
/// cached monotonic safe-head watch value, because L1 reorgs can move the local
/// safe head backward.
pub trait LocalSafeHeadProvider: std::fmt::Debug + Send + Sync + 'static {
    /// Return the current local safe L2 block number.
    fn local_safe_l2_number(&self) -> BoxFuture<'_, LocalSafeHeadResult>;
}
