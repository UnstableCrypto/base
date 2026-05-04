//! Receipt root calculation helpers for consensus receipts.

use alloc::boxed::Box;

use alloy_consensus::{Header, TxReceipt};
use alloy_eips::eip2718::Encodable2718;
use alloy_primitives::{B256, Bloom, Log, logs_bloom};
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

    /// Calculates the receipt root and aggregate logs bloom for receipts.
    pub fn calculate_root_and_logs_bloom<T>(receipts: &[T]) -> (B256, Bloom)
    where
        T: TxReceipt<Log = Log> + Encodable2718,
    {
        let receipts_root = Self::calculate(receipts);
        let logs_bloom = logs_bloom(receipts.iter().flat_map(|receipt| receipt.logs()));

        (receipts_root, logs_bloom)
    }

    /// Verifies receipts against the commitments in the block header.
    pub fn verify_root_and_logs_bloom<T>(
        header: &Header,
        block_hash: B256,
        receipts: &[T],
    ) -> Result<(), ReceiptRootError>
    where
        T: TxReceipt<Log = Log> + Encodable2718,
    {
        let (actual_receipts_root, actual_logs_bloom) =
            Self::calculate_root_and_logs_bloom(receipts);

        if actual_receipts_root != header.receipts_root {
            return Err(ReceiptRootError::RootMismatch {
                block_hash,
                expected: header.receipts_root,
                actual: actual_receipts_root,
            });
        }

        if actual_logs_bloom != header.logs_bloom {
            return Err(ReceiptRootError::LogsBloomMismatch {
                block_hash,
                expected: Box::new(header.logs_bloom),
                actual: Box::new(actual_logs_bloom),
            });
        }

        Ok(())
    }
}

/// Receipt commitment verification error.
#[derive(Debug, thiserror::Error)]
pub enum ReceiptRootError {
    /// Receipt root does not match the block header.
    #[error(
        "Receipt root mismatch for block {block_hash:?}: expected {expected:?}, got {actual:?}"
    )]
    RootMismatch {
        /// The block hash whose receipts were checked.
        block_hash: B256,
        /// The receipt root committed in the header.
        expected: B256,
        /// The receipt root calculated from the returned receipts.
        actual: B256,
    },
    /// Logs bloom does not match the block header.
    #[error("Logs bloom mismatch for block {block_hash:?}: expected {expected:?}, got {actual:?}")]
    LogsBloomMismatch {
        /// The block hash whose receipts were checked.
        block_hash: B256,
        /// The logs bloom committed in the header.
        expected: Box<Bloom>,
        /// The logs bloom calculated from the returned receipts.
        actual: Box<Bloom>,
    },
}

#[cfg(test)]
mod tests {
    use alloy_consensus::{EthereumReceipt, Header, ReceiptEnvelope, TxType};
    use alloy_primitives::{Address, B256, Bloom, Bytes, Log, LogData, b256};
    use rstest::{fixture, rstest};

    use super::*;

    fn receipt_with_log() -> ReceiptEnvelope {
        EthereumReceipt {
            tx_type: TxType::Legacy,
            success: true,
            cumulative_gas_used: 21_000,
            logs: vec![Log {
                address: Address::repeat_byte(0x11),
                data: LogData::new_unchecked(
                    vec![b256!(
                        "0x1111111111111111111111111111111111111111111111111111111111111111"
                    )],
                    Bytes::from_static(&[0x22]),
                ),
            }],
        }
        .into()
    }

    fn header_matching_receipts(receipts: &[ReceiptEnvelope]) -> Header {
        let (receipts_root, logs_bloom) = ReceiptRoot::calculate_root_and_logs_bloom(receipts);

        Header { receipts_root, logs_bloom, ..Default::default() }
    }

    #[fixture]
    fn receipts() -> Vec<ReceiptEnvelope> {
        vec![receipt_with_log()]
    }

    #[rstest]
    fn verify_root_and_logs_bloom_accepts_matching_header(receipts: Vec<ReceiptEnvelope>) {
        let header = header_matching_receipts(&receipts);

        ReceiptRoot::verify_root_and_logs_bloom(&header, B256::ZERO, &receipts)
            .expect("matching receipts should verify");
    }

    fn make_root_mismatch(header: &mut Header) {
        header.receipts_root = B256::repeat_byte(0xff);
    }

    fn make_logs_bloom_mismatch(header: &mut Header) {
        header.logs_bloom = Bloom::ZERO;
    }

    fn is_root_mismatch(error: &ReceiptRootError) -> bool {
        matches!(error, ReceiptRootError::RootMismatch { .. })
    }

    fn is_logs_bloom_mismatch(error: &ReceiptRootError) -> bool {
        matches!(error, ReceiptRootError::LogsBloomMismatch { .. })
    }

    #[rstest]
    #[case::root_mismatch(make_root_mismatch, is_root_mismatch)]
    #[case::logs_bloom_mismatch(make_logs_bloom_mismatch, is_logs_bloom_mismatch)]
    fn verify_root_and_logs_bloom_rejects_mismatches(
        receipts: Vec<ReceiptEnvelope>,
        #[case] mutate_header: fn(&mut Header),
        #[case] matches_expected_error: fn(&ReceiptRootError) -> bool,
    ) {
        let mut header = header_matching_receipts(&receipts);
        mutate_header(&mut header);

        let err = ReceiptRoot::verify_root_and_logs_bloom(&header, B256::ZERO, &receipts)
            .expect_err("mismatched receipts should fail");

        assert!(matches_expected_error(&err));
    }
}
