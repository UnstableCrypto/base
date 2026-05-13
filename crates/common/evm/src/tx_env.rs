use alloy_primitives::Bytes;

use crate::UnstableTransaction;

/// Trait for Unstable transaction environments. Allows to recover the transaction encoded bytes if
/// they're available.
pub trait UnstableTxEnv {
    /// Returns the encoded bytes of the transaction.
    fn encoded_bytes(&self) -> Option<&Bytes>;
}

impl<T: revm::context::Transaction> UnstableTxEnv for UnstableTransaction<T> {
    fn encoded_bytes(&self) -> Option<&Bytes> {
        self.enveloped_tx.as_ref()
    }
}
