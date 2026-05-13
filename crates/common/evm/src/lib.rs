#![doc = include_str!("../README.md")]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub use base_common_chains::UnstableUpgrade;

mod spec;
pub use spec::UnstableSpecId;

mod result;
pub use result::UnstableHaltReason;

mod l1block;
pub use l1block::L1BlockInfo;

mod transaction;
pub use transaction::{
    UnstableTransaction, UnstableTransactionBuilder, UnstableTransactionError, UnstableTxTr, BuildError,
    DEPOSIT_TRANSACTION_TYPE, DepositTransactionParts,
};

mod handler;
pub use handler::{UnstableHandler, IsTxError};

mod precompiles;
pub use precompiles::UnstablePrecompiles;

mod api;
pub use api::{UnstableContext, UnstableContextTr, UnstableError, Builder, DefaultUnstable};

mod evm;
pub use evm::UnstableEvm;

mod factory;
pub use factory::UnstableEvmFactory;

mod tx_env;
pub use tx_env::UnstableTxEnv;

mod error;
pub use error::UnstableBlockExecutionError;

mod receipt_builder;
pub use receipt_builder::{AlloyReceiptBuilder, UnstableReceiptBuilder};

mod canyon;
pub use canyon::ensure_create2_deployer;

mod executor;
pub use executor::{
    UnstableBlockExecutionCtx, UnstableBlockExecutor, UnstableBlockExecutorFactory, UnstableTxResult,
};
