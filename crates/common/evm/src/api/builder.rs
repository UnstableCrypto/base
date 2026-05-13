//! [`Builder`] trait for constructing a [`UnstableEvm`] directly from a [`UnstableContext`].
use alloy_evm::Database;
use revm::{
    context::FrameStack,
    handler::{EthFrame, instructions::EthInstructions},
    interpreter::interpreter::EthInterpreter,
};

use crate::{UnstableContext, UnstableEvm, UnstablePrecompiles, UnstableSpecId};

/// Trait that allows constructing a [`UnstableEvm`] from a [`UnstableContext`].
///
/// Implemented for [`UnstableContext<DB>`] of any database. The resulting [`UnstableEvm`]
/// uses [`UnstablePrecompiles`] for the active [`UnstableSpecId`]; call
/// [`UnstableEvm::with_precompiles`] afterwards to substitute a custom precompile set.
pub trait Builder: Sized {
    /// The database type of the context.
    type Db: Database;

    /// Builds a [`UnstableEvm`] with a `()` inspector. The inspect flag is `false`,
    /// so [`Inspector`][revm::Inspector] callbacks are never invoked via
    /// [`alloy_evm::Evm::transact`].
    fn build_base(self) -> UnstableEvm<Self::Db, ()>;

    /// Builds a [`UnstableEvm`] with the given inspector. The inspect flag is `true`,
    /// so [`Inspector`][revm::Inspector] callbacks are invoked on every
    /// [`alloy_evm::Evm::transact`] call.
    fn build_with_inspector<INSP>(self, inspector: INSP) -> UnstableEvm<Self::Db, INSP>;
}

impl<DB: Database> Builder for UnstableContext<DB> {
    type Db = DB;

    fn build_base(self) -> UnstableEvm<DB, ()> {
        let spec: UnstableSpecId = self.cfg.spec;
        UnstableEvm::new(
            revm::context::Evm {
                ctx: self,
                inspector: (),
                instruction: EthInstructions::new_mainnet_with_spec(spec.into()),
                precompiles: UnstablePrecompiles::new_with_spec(spec),
                frame_stack: FrameStack::<EthFrame<EthInterpreter>>::new_prealloc(8),
            },
            false,
        )
    }

    fn build_with_inspector<INSP>(self, inspector: INSP) -> UnstableEvm<DB, INSP> {
        let spec: UnstableSpecId = self.cfg.spec;
        UnstableEvm::new(
            revm::context::Evm {
                ctx: self,
                inspector,
                instruction: EthInstructions::new_mainnet_with_spec(spec.into()),
                precompiles: UnstablePrecompiles::new_with_spec(spec),
                frame_stack: FrameStack::<EthFrame<EthInterpreter>>::new_prealloc(8),
            },
            true,
        )
    }
}
