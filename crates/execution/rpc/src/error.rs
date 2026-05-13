//! RPC errors specific to Unstable.

use std::convert::Infallible;

use alloy_json_rpc::ErrorPayload;
use alloy_primitives::Bytes;
use alloy_rpc_types_eth::{BlockError, error::EthRpcErrorCode};
use alloy_transport::{RpcError, TransportErrorKind};
use base_common_evm::{UnstableHaltReason, UnstableTransactionError};
use base_execution_evm::UnstableBlockExecutionError;
use jsonrpsee_types::error::INTERNAL_ERROR_CODE;
use reth_evm::execute::ProviderError;
use reth_rpc_eth_api::{AsEthApiError, EthTxEnvError, TransactionConversionError};
use reth_rpc_eth_types::{
    EthApiError,
    error::api::{FromEvmHalt, FromRevert},
};
use reth_rpc_server_types::result::{internal_rpc_err, rpc_err};
use revm::context_interface::result::{EVMError, InvalidTransaction};

/// Unstable-specific errors, that extend [`EthApiError`].
#[derive(Debug, thiserror::Error)]
pub enum UnstableEthApiError {
    /// L1 ethereum error.
    #[error(transparent)]
    Eth(#[from] EthApiError),
    /// EVM error originating from invalid Unstable data.
    #[error(transparent)]
    Evm(#[from] UnstableBlockExecutionError),
    /// Thrown when calculating L1 gas fee.
    #[error("failed to calculate l1 gas fee")]
    L1BlockFeeError,
    /// Thrown when calculating L1 gas used
    #[error("failed to calculate l1 gas used")]
    L1BlockGasError,
    /// Wrapper for [`revm_primitives::InvalidTransaction`](InvalidTransaction).
    #[error(transparent)]
    InvalidTransaction(#[from] UnstableInvalidTransactionError),
    /// Sequencer client error.
    #[error(transparent)]
    Sequencer(#[from] SequencerClientError),
}

impl AsEthApiError for UnstableEthApiError {
    fn as_err(&self) -> Option<&EthApiError> {
        match self {
            Self::Eth(err) => Some(err),
            _ => None,
        }
    }
}

impl From<UnstableEthApiError> for jsonrpsee_types::error::ErrorObject<'static> {
    fn from(err: UnstableEthApiError) -> Self {
        match err {
            UnstableEthApiError::Eth(err) => err.into(),
            UnstableEthApiError::InvalidTransaction(err) => err.into(),
            UnstableEthApiError::Evm(_)
            | UnstableEthApiError::L1BlockFeeError
            | UnstableEthApiError::L1BlockGasError => internal_rpc_err(err.to_string()),
            UnstableEthApiError::Sequencer(err) => err.into(),
        }
    }
}

/// Unstable-specific invalid transaction errors
#[derive(thiserror::Error, Debug)]
pub enum UnstableInvalidTransactionError {
    /// A deposit transaction was submitted as a system transaction post-regolith.
    #[error("no system transactions allowed after regolith")]
    DepositSystemTxPostRegolith,
    /// A deposit transaction halted post-regolith
    #[error("deposit transaction halted after regolith")]
    HaltedDepositPostRegolith,
    /// The encoded transaction was missing during evm execution.
    #[error("missing enveloped transaction bytes")]
    MissingEnvelopedTx,
}

impl From<UnstableInvalidTransactionError> for jsonrpsee_types::error::ErrorObject<'static> {
    fn from(err: UnstableInvalidTransactionError) -> Self {
        match err {
            UnstableInvalidTransactionError::DepositSystemTxPostRegolith
            | UnstableInvalidTransactionError::HaltedDepositPostRegolith
            | UnstableInvalidTransactionError::MissingEnvelopedTx => {
                rpc_err(EthRpcErrorCode::TransactionRejected.code(), err.to_string(), None)
            }
        }
    }
}

impl TryFrom<UnstableTransactionError> for UnstableInvalidTransactionError {
    type Error = InvalidTransaction;

    fn try_from(err: UnstableTransactionError) -> Result<Self, Self::Error> {
        match err {
            UnstableTransactionError::DepositSystemTxPostRegolith => {
                Ok(Self::DepositSystemTxPostRegolith)
            }
            UnstableTransactionError::HaltedDepositPostRegolith => Ok(Self::HaltedDepositPostRegolith),
            UnstableTransactionError::MissingEnvelopedTx => Ok(Self::MissingEnvelopedTx),
            UnstableTransactionError::Unstable(err) => Err(err),
        }
    }
}

/// Error type when interacting with the Sequencer
#[derive(Debug, thiserror::Error)]
pub enum SequencerClientError {
    /// Wrapper around an [`RpcError<TransportErrorKind>`].
    #[error(transparent)]
    HttpError(#[from] RpcError<TransportErrorKind>),
}

impl From<SequencerClientError> for jsonrpsee_types::error::ErrorObject<'static> {
    fn from(err: SequencerClientError) -> Self {
        match err {
            SequencerClientError::HttpError(RpcError::ErrorResp(ErrorPayload {
                code,
                message,
                data,
            })) => jsonrpsee_types::error::ErrorObject::owned(code as i32, message, data),
            err => jsonrpsee_types::error::ErrorObject::owned(
                INTERNAL_ERROR_CODE,
                err.to_string(),
                None::<String>,
            ),
        }
    }
}

impl<T> From<EVMError<T, UnstableTransactionError>> for UnstableEthApiError
where
    T: Into<EthApiError>,
{
    fn from(error: EVMError<T, UnstableTransactionError>) -> Self {
        match error {
            EVMError::Transaction(err) => match err.try_into() {
                Ok(err) => Self::InvalidTransaction(err),
                Err(err) => Self::Eth(EthApiError::InvalidTransaction(err.into())),
            },
            EVMError::Database(err) => Self::Eth(err.into()),
            EVMError::Header(err) => Self::Eth(err.into()),
            EVMError::Custom(err) => Self::Eth(EthApiError::EvmCustom(err)),
        }
    }
}

impl FromEvmHalt<UnstableHaltReason> for UnstableEthApiError {
    fn from_evm_halt(halt: UnstableHaltReason, gas_limit: u64) -> Self {
        match halt {
            UnstableHaltReason::FailedDeposit => {
                UnstableInvalidTransactionError::HaltedDepositPostRegolith.into()
            }
            UnstableHaltReason::Unstable(halt) => EthApiError::from_evm_halt(halt, gas_limit).into(),
        }
    }
}

impl FromRevert for UnstableEthApiError {
    fn from_revert(output: Bytes) -> Self {
        Self::Eth(EthApiError::from_revert(output))
    }
}

impl From<TransactionConversionError> for UnstableEthApiError {
    fn from(value: TransactionConversionError) -> Self {
        Self::Eth(EthApiError::from(value))
    }
}

impl From<EthTxEnvError> for UnstableEthApiError {
    fn from(value: EthTxEnvError) -> Self {
        Self::Eth(EthApiError::from(value))
    }
}

impl From<ProviderError> for UnstableEthApiError {
    fn from(value: ProviderError) -> Self {
        Self::Eth(EthApiError::from(value))
    }
}

impl From<BlockError> for UnstableEthApiError {
    fn from(value: BlockError) -> Self {
        Self::Eth(EthApiError::from(value))
    }
}

impl From<Infallible> for UnstableEthApiError {
    fn from(value: Infallible) -> Self {
        match value {}
    }
}
