use alloy_evm::{Database, EvmEnv, EvmFactory, precompiles::PrecompilesMap};
#[cfg(feature = "native-dex")]
use base_common_chains::BaseUpgrade;
#[cfg(feature = "native-dex")]
use base_common_precompiles::{BASE_DEX_ADDRESS, BaseDexPrecompile};
use revm::{
    Context, Inspector,
    context::{BlockEnv, TxEnv},
    context_interface::result::EVMError,
    inspector::NoOpInspector,
};

use crate::{
    BaseContext, BaseEvm, BaseHaltReason, BasePrecompiles, BaseSpecId, BaseTransaction,
    BaseTransactionError, Builder, DefaultBase,
};

/// Factory that produces [`BaseEvm`] instances backed by a [`PrecompilesMap`].
///
/// [`BasePrecompiles`] are eagerly flattened into a [`PrecompilesMap`] on construction
/// so that precompile dispatch is a single hash-map lookup rather than a spec-aware
/// branch on every call.
#[derive(Debug, Default, Clone, Copy)]
#[non_exhaustive]
pub struct BaseEvmFactory;

impl BaseEvmFactory {
    fn precompiles(input: &EvmEnv<BaseSpecId>) -> PrecompilesMap {
        Self::precompiles_for_spec(input.cfg_env.spec)
    }

    fn precompiles_for_spec(spec: BaseSpecId) -> PrecompilesMap {
        let precompiles =
            PrecompilesMap::from_static(BasePrecompiles::new_with_spec(spec).precompiles());

        #[cfg(feature = "native-dex")]
        let mut precompiles = precompiles;

        #[cfg(feature = "native-dex")]
        if spec.is_enabled_in(BaseUpgrade::Beryl) {
            precompiles.extend_precompiles([(BASE_DEX_ADDRESS, BaseDexPrecompile::precompile())]);
        }

        precompiles
    }
}

impl EvmFactory for BaseEvmFactory {
    type Evm<DB: Database, I: Inspector<BaseContext<DB>>> = BaseEvm<DB, I, PrecompilesMap>;
    type Context<DB: Database> = BaseContext<DB>;
    type Tx = BaseTransaction<TxEnv>;
    type Error<DBError: core::error::Error + Send + Sync + 'static> =
        EVMError<DBError, BaseTransactionError>;
    type HaltReason = BaseHaltReason;
    type Spec = BaseSpecId;
    type BlockEnv = BlockEnv;
    type Precompiles = PrecompilesMap;

    fn create_evm<DB: Database>(
        &self,
        db: DB,
        input: EvmEnv<BaseSpecId>,
    ) -> Self::Evm<DB, NoOpInspector> {
        let precompiles = Self::precompiles(&input);
        Context::base()
            .with_db(db)
            .with_block(input.block_env)
            .with_cfg(input.cfg_env)
            .build_base()
            .with_inspector(NoOpInspector {})
            .with_precompiles(precompiles)
    }

    fn create_evm_with_inspector<DB: Database, I: Inspector<Self::Context<DB>>>(
        &self,
        db: DB,
        input: EvmEnv<BaseSpecId>,
        inspector: I,
    ) -> Self::Evm<DB, I> {
        let precompiles = Self::precompiles(&input);
        Context::base()
            .with_db(db)
            .with_block(input.block_env)
            .with_cfg(input.cfg_env)
            .build_with_inspector(inspector)
            .with_precompiles(precompiles)
    }
}

#[cfg(all(test, feature = "native-dex"))]
mod tests {
    use super::*;

    #[test]
    fn native_dex_precompile_is_absent_before_beryl() {
        let precompiles = BaseEvmFactory::precompiles_for_spec(BaseSpecId::new(BaseUpgrade::Azul));

        assert!(precompiles.get(&BASE_DEX_ADDRESS).is_none());
    }

    #[test]
    fn native_dex_precompile_is_present_at_beryl() {
        let precompiles = BaseEvmFactory::precompiles_for_spec(BaseSpecId::new(BaseUpgrade::Beryl));

        assert!(precompiles.get(&BASE_DEX_ADDRESS).is_some());
    }
}
