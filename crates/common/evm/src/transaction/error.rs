//! Contains the `[UnstableTransactionError]` type.

use core::fmt::Display;

use alloy_evm::InvalidTxError;
use revm::{
    context::tx::TxEnvBuildError,
    context_interface::{
        result::{EVMError, InvalidTransaction},
        transaction::TransactionError,
    },
};

/// Error type for building [`TxEnv`]
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BuildError {
    /// Unstable transaction build error
    Unstable(TxEnvBuildError),
    /// Missing enveloped transaction bytes
    MissingEnvelopedTxBytes,
    /// Missing source hash for deposit transaction
    MissingSourceHashForDeposit,
}

impl From<TxEnvBuildError> for BuildError {
    fn from(error: TxEnvBuildError) -> Self {
        Self::Unstable(error)
    }
}

/// Unstable transaction validation error.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum UnstableTransactionError {
    /// Unstable transaction error.
    Unstable(InvalidTransaction),
    /// System transactions are not supported post-regolith hardfork.
    ///
    /// Before the Regolith hardfork, there was a special field in the `Deposit` transaction
    /// type that differentiated between `system` and `user` deposit transactions. This field
    /// was deprecated in the Regolith hardfork, and this error is thrown if a `Deposit` transaction
    /// is found with this field set to `true` after the hardfork activation.
    ///
    /// In addition, this error is internal, and bubbles up into a [`UnstableHaltReason::FailedDeposit`][crate::UnstableHaltReason::FailedDeposit] error
    /// in the `revm` handler for the consumer to easily handle. This is due to a state transition
    /// rule on Unstable where, if for any reason a deposit transaction fails, the transaction
    /// must still be included in the block, the sender nonce is bumped, the `mint` value persists, and
    /// special gas accounting rules are applied. Normally on L1, [`EVMError::Transaction`] errors
    /// are cause for non-inclusion, so a special [`UnstableHaltReason`][crate::UnstableHaltReason] variant was introduced to handle this
    /// case for failed deposit transactions.
    DepositSystemTxPostRegolith,
    /// Deposit transaction halts bubble up to the global main return handler, wiping state and
    /// only increasing the nonce + persisting the mint value.
    ///
    /// This is a catch-all error for any deposit transaction that results in a [`UnstableHaltReason`][crate::UnstableHaltReason] error
    /// post-regolith hardfork. This allows for a consumer to easily handle special cases where
    /// a deposit transaction fails during validation, but must still be included in the block.
    ///
    /// In addition, this error is internal, and bubbles up into a [`UnstableHaltReason::FailedDeposit`][crate::UnstableHaltReason::FailedDeposit] error
    /// in the `revm` handler for the consumer to easily handle. This is due to a state transition
    /// rule on Unstable where, if for any reason a deposit transaction fails, the transaction
    /// must still be included in the block, the sender nonce is bumped, the `mint` value persists, and
    /// special gas accounting rules are applied. Normally on L1, [`EVMError::Transaction`] errors
    /// are cause for non-inclusion, so a special [`UnstableHaltReason`][crate::UnstableHaltReason] variant was introduced to handle this
    /// case for failed deposit transactions.
    HaltedDepositPostRegolith,
    /// Missing enveloped transaction bytes for non-deposit transaction.
    ///
    /// Non-deposit transactions on Unstable must have `enveloped_tx` field set
    /// to properly calculate L1 costs.
    MissingEnvelopedTx,
}

impl TransactionError for UnstableTransactionError {}

impl Display for UnstableTransactionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unstable(error) => error.fmt(f),
            Self::DepositSystemTxPostRegolith => {
                write!(f, "deposit system transactions post regolith hardfork are not supported")
            }
            Self::HaltedDepositPostRegolith => {
                write!(
                    f,
                    "deposit transaction halted post-regolith; error will be bubbled up to main return handler"
                )
            }
            Self::MissingEnvelopedTx => {
                write!(f, "missing enveloped transaction bytes for non-deposit transaction")
            }
        }
    }
}

impl InvalidTxError for UnstableTransactionError {
    fn as_invalid_tx_err(&self) -> Option<&InvalidTransaction> {
        match self {
            Self::Unstable(tx) => Some(tx),
            _ => None,
        }
    }
}

impl core::error::Error for UnstableTransactionError {}

impl From<InvalidTransaction> for UnstableTransactionError {
    fn from(value: InvalidTransaction) -> Self {
        Self::Unstable(value)
    }
}

impl<DBError> From<UnstableTransactionError> for EVMError<DBError, UnstableTransactionError> {
    fn from(value: UnstableTransactionError) -> Self {
        Self::Transaction(value)
    }
}

#[cfg(test)]
mod tests {
    use std::string::ToString;

    use super::*;

    #[test]
    fn test_display_base_errors() {
        assert_eq!(
            UnstableTransactionError::Unstable(InvalidTransaction::NonceTooHigh { tx: 2, state: 1 })
                .to_string(),
            "nonce 2 too high, expected 1"
        );
        assert_eq!(
            UnstableTransactionError::DepositSystemTxPostRegolith.to_string(),
            "deposit system transactions post regolith hardfork are not supported"
        );
        assert_eq!(
            UnstableTransactionError::HaltedDepositPostRegolith.to_string(),
            "deposit transaction halted post-regolith; error will be bubbled up to main return handler"
        );
        assert_eq!(
            UnstableTransactionError::MissingEnvelopedTx.to_string(),
            "missing enveloped transaction bytes for non-deposit transaction"
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serialize_json_base_transaction_error() {
        let response = r#""DepositSystemTxPostRegolith""#;

        let base_transaction_error: UnstableTransactionError = serde_json::from_str(response).unwrap();
        assert_eq!(base_transaction_error, UnstableTransactionError::DepositSystemTxPostRegolith);
    }
}
