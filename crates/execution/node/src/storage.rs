//! Unstable node storage type aliases.

use alloy_consensus::Header;
use base_common_consensus::UnstableTransactionSigned;
use reth_storage_api::EmptyBodyStorage;

/// Unstable storage implementation.
pub type UnstableStorage<T = UnstableTransactionSigned, H = Header> = EmptyBodyStorage<T, H>;

#[cfg(test)]
mod tests {
    use reth_codecs::{test_utils::UnusedBits, validate_bitflag_backwards_compat};
    use reth_prune_types::{PruneCheckpoint, PruneMode, PruneSegment};

    #[test]
    fn test_ensure_backwards_compatibility() {
        assert_eq!(PruneMode::bitflag_encoded_bytes(), 1);
        assert_eq!(PruneSegment::bitflag_encoded_bytes(), 1);

        // In case of failure, refer to the documentation of the
        // [`validate_bitflag_backwards_compat`] macro for detailed instructions on handling it.
        validate_bitflag_backwards_compat!(PruneCheckpoint, UnusedBits::NotZero);
        validate_bitflag_backwards_compat!(PruneMode, UnusedBits::Zero);
        validate_bitflag_backwards_compat!(PruneSegment, UnusedBits::Zero);
    }
}
