//! Contains the [`UnstableTxResult`] type.

use alloy_evm::{block::TxResult as TxResultTrait, eth::EthTxResult};
use alloy_primitives::Address;
use revm::context::result::ResultAndState;

/// The result of executing a Unstable transaction.
#[derive(Debug)]
pub struct UnstableTxResult<H, T> {
    /// The inner result of the transaction execution.
    pub inner: EthTxResult<H, T>,
    /// Whether the transaction is a deposit transaction.
    pub is_deposit: bool,
    /// The sender of the transaction.
    pub sender: Address,
}

impl<H, T> TxResultTrait for UnstableTxResult<H, T> {
    type HaltReason = H;

    fn result(&self) -> &ResultAndState<Self::HaltReason> {
        &self.inner.result
    }
}
