//! Transaction types for Unstable chains.

mod deposit;
pub use deposit::{DepositTransaction, TxDeposit};

mod tx_type;
pub use tx_type::DEPOSIT_TX_TYPE_ID;

mod envelope;
pub use envelope::{UnstableTransaction, UnstableTxEnvelope, OpTxType};

mod typed;
pub use typed::UnstableTypedTransaction;

mod pooled;
#[cfg(feature = "serde")]
pub use deposit::serde_deposit_tx_rpc;
pub use pooled::UnstablePooledTransaction;

mod meta;
pub use meta::{UnstableTransactionInfo, DepositInfo};

/// Bincode-compatible serde implementations for transaction types.
#[cfg(all(feature = "serde", feature = "serde-bincode-compat"))]
pub(super) mod serde_bincode_compat {
    pub use super::{deposit::serde_bincode_compat::TxDeposit, envelope::serde_bincode_compat::*};
}
