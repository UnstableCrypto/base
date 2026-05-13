use alloy_evm::{Database, EvmEnv, EvmFactory, precompiles::PrecompilesMap};
use revm::{
    Context, Inspector,
    context::{BlockEnv, TxEnv},
    context_interface::result::EVMError,
    inspector::NoOpInspector,
};

use crate::{
    UnstableContext, UnstableEvm, UnstableHaltReason, UnstablePrecompiles, UnstableSpecId, UnstableTransaction,
    UnstableTransactionError, Builder, DefaultUnstable,
};

/// Factory that produces [`UnstableEvm`] instances backed by a [`PrecompilesMap`].
///
/// [`UnstablePrecompiles`] are eagerly flattened into a [`PrecompilesMap`] on construction
/// so that precompile dispatch is a single hash-map lookup rather than a spec-aware
/// branch on every call.
#[derive(Debug, Default, Clone, Copy)]
#[non_exhaustive]
pub struct UnstableEvmFactory;

impl EvmFactory for UnstableEvmFactory {
    type Evm<DB: Database, I: Inspector<UnstableContext<DB>>> = UnstableEvm<DB, I, PrecompilesMap>;
    type Context<DB: Database> = UnstableContext<DB>;
    type Tx = UnstableTransaction<TxEnv>;
    type Error<DBError: core::error::Error + Send + Sync + 'static> =
        EVMError<DBError, UnstableTransactionError>;
    type HaltReason = UnstableHaltReason;
    type Spec = UnstableSpecId;
    type BlockEnv = BlockEnv;
    type Precompiles = PrecompilesMap;

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
            .with_precompiles(PrecompilesMap::from_static(
                UnstablePrecompiles::new_with_spec(spec_id).precompiles(),
            ))
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
            .with_precompiles(PrecompilesMap::from_static(
                UnstablePrecompiles::new_with_spec(spec_id).precompiles(),
            ))
    }
}
