use core::convert::Infallible;

use base_common_consensus::{UnstableReceipt, UnstableTxEnvelope};
use reth_rpc_convert::{TryFromReceiptResponse, TryFromTransactionResponse};

use crate::Unstable;

impl TryFromTransactionResponse<Unstable> for UnstableTxEnvelope {
    type Error = Infallible;

    fn from_transaction_response(
        transaction_response: base_common_rpc_types::Transaction,
    ) -> Result<Self, Self::Error> {
        Ok(transaction_response.inner.into_inner())
    }
}

impl TryFromReceiptResponse<Unstable> for UnstableReceipt {
    type Error = Infallible;

    fn from_receipt_response(
        receipt_response: base_common_rpc_types::UnstableTransactionReceipt,
    ) -> Result<Self, Self::Error> {
        Ok(receipt_response.inner.inner.into_components().0.map_logs(Into::into))
    }
}
