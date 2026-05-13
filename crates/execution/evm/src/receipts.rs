use alloy_consensus::{Eip658Value, Receipt};
use alloy_evm::eth::receipt_builder::ReceiptBuilderCtx;
use base_common_consensus::{UnstableReceipt, UnstableTransactionSigned, OpTxType};
use base_common_evm::UnstableReceiptBuilder;
use reth_evm::Evm;

/// A builder that operates on Unstable primitive types, specifically [`UnstableTransactionSigned`] and
/// [`UnstableReceipt`].
#[derive(Debug, Default, Clone, Copy)]
#[non_exhaustive]
pub struct UnstableRethReceiptBuilder;

impl UnstableReceiptBuilder for UnstableRethReceiptBuilder {
    type Transaction = UnstableTransactionSigned;
    type Receipt = UnstableReceipt;

    fn build_receipt<'a, E: Evm>(
        &self,
        ctx: ReceiptBuilderCtx<'a, OpTxType, E>,
    ) -> Result<Self::Receipt, ReceiptBuilderCtx<'a, OpTxType, E>> {
        match ctx.tx_type {
            OpTxType::Deposit => Err(ctx),
            ty => {
                let receipt = Receipt {
                    // Success flag was added in `EIP-658: Embedding transaction status code in
                    // receipts`.
                    status: Eip658Value::Eip658(ctx.result.is_success()),
                    cumulative_gas_used: ctx.cumulative_gas_used,
                    logs: ctx.result.into_logs(),
                };

                Ok(match ty {
                    OpTxType::Legacy => UnstableReceipt::Legacy(receipt),
                    OpTxType::Eip1559 => UnstableReceipt::Eip1559(receipt),
                    OpTxType::Eip2930 => UnstableReceipt::Eip2930(receipt),
                    OpTxType::Eip7702 => UnstableReceipt::Eip7702(receipt),
                    OpTxType::Deposit => unreachable!(),
                })
            }
        }
    }

    fn build_deposit_receipt(&self, inner: base_common_consensus::DepositReceipt) -> Self::Receipt {
        UnstableReceipt::Deposit(inner)
    }
}
