//! [`EvmFactory`] implementation for the EVM in the ZKVM environment.

use alloy_evm::{Database, EvmEnv, EvmFactory};
use base_common_evm::{
    UnstableContext, UnstableEvm, UnstableHaltReason, UnstableSpecId, UnstableTransaction, UnstableTransactionError,
    Builder, DefaultUnstable,
};
use revm::{
    Context, Inspector,
    context::{BlockEnv, TxEnv},
    context_interface::result::EVMError,
    inspector::NoOpInspector,
};

use super::UnstableZkvmPrecompiles;

/// Factory producing [`UnstableEvm`]s with ZKVM-accelerated precompile overrides enabled.
#[derive(Debug, Clone)]
pub struct ZkvmUnstableEvmFactory {}

impl ZkvmUnstableEvmFactory {
    /// Creates a new [`ZkvmUnstableEvmFactory`].
    pub const fn new() -> Self {
        Self {}
    }
}

impl Default for ZkvmUnstableEvmFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl EvmFactory for ZkvmUnstableEvmFactory {
    type Evm<DB: Database, I: Inspector<UnstableContext<DB>>> = UnstableEvm<DB, I, UnstableZkvmPrecompiles>;
    type Context<DB: Database> = UnstableContext<DB>;
    type Tx = UnstableTransaction<TxEnv>;
    type Error<DBError: core::error::Error + Send + Sync + 'static> =
        EVMError<DBError, UnstableTransactionError>;
    type HaltReason = UnstableHaltReason;
    type Spec = UnstableSpecId;
    type BlockEnv = BlockEnv;
    type Precompiles = UnstableZkvmPrecompiles;

    fn create_evm<DB: Database>(
        &self,
        db: DB,
        input: EvmEnv<UnstableSpecId>,
    ) -> Self::Evm<DB, NoOpInspector> {
        let spec_id = input.cfg_env.spec;
        Context::base()
            .with_db(db)
            .with_block(input.block_env)
            .with_cfg(input.cfg_env)
            .build_base()
            .with_inspector(NoOpInspector {})
            .with_precompiles(UnstableZkvmPrecompiles::new_with_spec(spec_id))
    }

    fn create_evm_with_inspector<DB: Database, I: Inspector<Self::Context<DB>>>(
        &self,
        db: DB,
        input: EvmEnv<UnstableSpecId>,
        inspector: I,
    ) -> Self::Evm<DB, I> {
        let spec_id = input.cfg_env.spec;
        Context::base()
            .with_db(db)
            .with_block(input.block_env)
            .with_cfg(input.cfg_env)
            .build_with_inspector(inspector)
            .with_precompiles(UnstableZkvmPrecompiles::new_with_spec(spec_id))
    }
}
