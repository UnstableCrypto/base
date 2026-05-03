//! Receipt root calculation helpers for consensus receipts.

use alloy_eips::eip2718::Encodable2718;
use alloy_primitives::B256;
use alloy_trie::root::ordered_trie_root_with_encoder;

/// Helper for calculating receipt roots from consensus receipts.
#[derive(Debug, Clone, Copy)]
pub struct ReceiptRoot;

impl ReceiptRoot {
    /// Calculates the receipt root for EIP-2718 encoded receipts.
    pub fn calculate<T>(receipts: &[T]) -> B256
    where
        T: Encodable2718,
    {
        ordered_trie_root_with_encoder(receipts, |receipt, buf| receipt.encode_2718(buf))
    }
}
