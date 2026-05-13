//! Contains trait [`DefaultUnstable`] used to create a default context.
use base_common_chains::UnstableUpgrade;
use revm::{
    Context, Journal, MainContext,
    context::{BlockEnv, CfgEnv, TxEnv},
    database_interface::EmptyDB,
};

use crate::{UnstableSpecId, UnstableTransaction, L1BlockInfo};

/// Type alias for the default context type of the `UnstableEvm`.
pub type UnstableContext<DB> =
    Context<BlockEnv, UnstableTransaction<TxEnv>, CfgEnv<UnstableSpecId>, DB, Journal<DB>, L1BlockInfo>;

/// Trait that allows for a default context to be created.
pub trait DefaultUnstable {
    /// Create a default context.
    fn base() -> UnstableContext<EmptyDB>;
}

impl DefaultUnstable for UnstableContext<EmptyDB> {
    fn base() -> Self {
        Context::mainnet()
            .with_tx(UnstableTransaction::builder().build_fill())
            .with_cfg(CfgEnv::new_with_spec(UnstableSpecId::new(UnstableUpgrade::Bedrock)))
            .with_chain(L1BlockInfo::default())
    }
}

#[cfg(test)]
mod tests {
    use revm::{ExecuteEvm, InspectEvm, inspector::NoOpInspector};

    use super::*;
    use crate::Builder;

    #[test]
    fn default_run_base() {
        let ctx = Context::base();
        let mut evm = ctx.build_with_inspector(NoOpInspector {});
        // execute without inspector
        let _ = evm.transact(UnstableTransaction::builder().build_fill());
        // execute with inspector callbacks
        let _ = evm.inspect_one_tx(UnstableTransaction::builder().build_fill());
    }
}
