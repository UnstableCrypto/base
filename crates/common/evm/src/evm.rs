use core::ops::{Deref, DerefMut};

use alloy_evm::{Database, Evm, EvmEnv};
use alloy_primitives::{Address, Bytes};
use revm::{
    DatabaseCommit, ExecuteCommitEvm, ExecuteEvm, InspectCommitEvm, InspectEvm,
    InspectSystemCallEvm, Inspector, SystemCallEvm,
    context::{
        BlockEnv, ContextError, ContextSetters, Evm as RevmEvm, FrameStack, TxEnv,
        result::ExecResultAndState,
    },
    context_interface::{
        ContextTr, JournalTr,
        result::{EVMError, ExecutionResult, ResultAndState},
    },
    handler::{
        EthFrame, EvmTr, FrameInitOrResult, Handler, ItemOrResult, PrecompileProvider,
        SystemCallTx, evm::FrameTr, instructions::EthInstructions,
    },
    inspector::{InspectorEvmTr, InspectorHandler, JournalExt},
    interpreter::{InterpreterResult, interpreter::EthInterpreter},
    state::EvmState,
};

use crate::{
    UnstableContext, UnstableHaltReason, UnstablePrecompiles, UnstableSpecId, UnstableTransaction,
    UnstableTransactionError, handler::UnstableHandler,
};

/// Type alias for the inner [`RevmEvm`] parameterized with Unstable-specific context and fixed
/// [`EthInstructions`] / [`EthFrame`], keeping [`UnstableEvm`] field and constructor signatures tidy.
type InnerEvm<DB, I, P> = RevmEvm<
    UnstableContext<DB>,
    I,
    EthInstructions<EthInterpreter, UnstableContext<DB>>,
    P,
    EthFrame<EthInterpreter>,
>;

/// The Unstable EVM, wrapping [`RevmEvm`] with a [`UnstableContext`] and an optional [`Inspector`].
///
/// Parameterized over a database [`DB`], inspector [`I`], and precompile set [`P`]
/// (defaulting to [`UnstablePrecompiles`]). All Unstable-specific context configuration —
/// [`UnstableSpecId`], [`UnstableTransaction`], and [`crate::L1BlockInfo`] — is fixed by [`UnstableContext`].
///
/// The `inspect` flag controls whether [`Inspector`] callbacks are invoked during
/// [`Evm::transact`]. When `false`, the inspector is present in the type but silent,
/// enabling zero-cost tracing toggling at runtime without type changes.
#[allow(missing_debug_implementations)] // revm::Context does not implement Debug
pub struct UnstableEvm<DB: Database, I, P = UnstablePrecompiles> {
    /// Inner revm EVM with Unstable-specific context, fixed [`EthInstructions`] and
    /// [`EthFrame`], and generic precompile set [`P`].
    pub(crate) inner: InnerEvm<DB, I, P>,
    /// Whether to invoke the [`Inspector`] on each [`Evm::transact`] call.
    pub(crate) inspect: bool,
}

impl<DB: Database, I, P> UnstableEvm<DB, I, P> {
    /// Constructs a [`UnstableEvm`] from a pre-built [`RevmEvm`] and an inspect flag.
    ///
    /// Prefer [`crate::Builder::build_base`] or [`crate::Builder::build_with_inspector`]
    /// to construct from a [`UnstableContext`] directly.
    pub const fn new(inner: InnerEvm<DB, I, P>, inspect: bool) -> Self {
        Self { inner, inspect }
    }

    /// Returns a reference to the underlying [`UnstableContext`].
    pub const fn ctx(&self) -> &UnstableContext<DB> {
        &self.inner.ctx
    }

    /// Returns a mutable reference to the underlying [`UnstableContext`].
    pub const fn ctx_mut(&mut self) -> &mut UnstableContext<DB> {
        &mut self.inner.ctx
    }

    /// Consumes `self` and returns the underlying [`UnstableContext`].
    pub fn into_context(self) -> UnstableContext<DB> {
        self.inner.ctx
    }

    /// Consumes `self` and returns the inspector.
    pub fn into_inspector(self) -> I {
        self.inner.inspector
    }

    /// Consumes `self` and returns a new [`UnstableEvm`] with the given inspector, preserving
    /// the inspect flag. Used to swap inspectors without rebuilding from context.
    pub fn with_inspector<J>(self, inspector: J) -> UnstableEvm<DB, J, P> {
        UnstableEvm { inner: self.inner.with_inspector(inspector), inspect: self.inspect }
    }

    /// Consumes `self` and returns a new [`UnstableEvm`] with the given precompile set,
    /// preserving the inspect flag. Used to substitute [`UnstablePrecompiles`] with
    /// custom implementations such as FPVM-accelerated precompiles in the proof system.
    pub fn with_precompiles<Q>(self, precompiles: Q) -> UnstableEvm<DB, I, Q> {
        UnstableEvm { inner: self.inner.with_precompiles(precompiles), inspect: self.inspect }
    }
}

impl<DB: Database, I, P> Deref for UnstableEvm<DB, I, P> {
    type Target = UnstableContext<DB>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.ctx()
    }
}

impl<DB: Database, I, P> DerefMut for UnstableEvm<DB, I, P> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.ctx_mut()
    }
}

impl<DB, I, P> EvmTr for UnstableEvm<DB, I, P>
where
    DB: Database,
    P: PrecompileProvider<UnstableContext<DB>, Output = InterpreterResult>,
{
    type Context = UnstableContext<DB>;
    type Instructions = EthInstructions<EthInterpreter, UnstableContext<DB>>;
    type Precompiles = P;
    type Frame = EthFrame<EthInterpreter>;

    #[inline]
    fn all(
        &self,
    ) -> (&Self::Context, &Self::Instructions, &Self::Precompiles, &FrameStack<Self::Frame>) {
        self.inner.all()
    }

    #[inline]
    fn all_mut(
        &mut self,
    ) -> (
        &mut Self::Context,
        &mut Self::Instructions,
        &mut Self::Precompiles,
        &mut FrameStack<Self::Frame>,
    ) {
        self.inner.all_mut()
    }

    fn frame_init(
        &mut self,
        frame_input: <Self::Frame as FrameTr>::FrameInit,
    ) -> Result<
        ItemOrResult<&mut Self::Frame, <Self::Frame as FrameTr>::FrameResult>,
        ContextError<DB::Error>,
    > {
        self.inner.frame_init(frame_input)
    }

    fn frame_run(&mut self) -> Result<FrameInitOrResult<Self::Frame>, ContextError<DB::Error>> {
        self.inner.frame_run()
    }

    fn frame_return_result(
        &mut self,
        result: <Self::Frame as FrameTr>::FrameResult,
    ) -> Result<Option<<Self::Frame as FrameTr>::FrameResult>, ContextError<DB::Error>> {
        self.inner.frame_return_result(result)
    }
}

impl<DB, I, P> InspectorEvmTr for UnstableEvm<DB, I, P>
where
    DB: Database,
    UnstableContext<DB>: ContextTr<Journal: JournalExt> + ContextSetters,
    I: Inspector<UnstableContext<DB>>,
    P: PrecompileProvider<UnstableContext<DB>, Output = InterpreterResult>,
{
    type Inspector = I;

    #[inline]
    fn all_inspector(
        &self,
    ) -> (
        &Self::Context,
        &Self::Instructions,
        &Self::Precompiles,
        &FrameStack<Self::Frame>,
        &Self::Inspector,
    ) {
        self.inner.all_inspector()
    }

    #[inline]
    fn all_mut_inspector(
        &mut self,
    ) -> (
        &mut Self::Context,
        &mut Self::Instructions,
        &mut Self::Precompiles,
        &mut FrameStack<Self::Frame>,
        &mut Self::Inspector,
    ) {
        self.inner.all_mut_inspector()
    }
}

impl<DB, I, P> ExecuteEvm for UnstableEvm<DB, I, P>
where
    DB: Database,
    UnstableContext<DB>: crate::UnstableContextTr
        + ContextSetters
        + ContextTr<Db = DB, Tx = UnstableTransaction<TxEnv>, Block = BlockEnv>,
    P: PrecompileProvider<UnstableContext<DB>, Output = InterpreterResult>,
{
    type Tx = UnstableTransaction<TxEnv>;
    type Block = BlockEnv;
    type State = EvmState;
    type Error = EVMError<DB::Error, UnstableTransactionError>;
    type ExecutionResult = ExecutionResult<UnstableHaltReason>;

    fn set_block(&mut self, block: Self::Block) {
        self.inner.ctx.set_block(block);
    }

    fn transact_one(&mut self, tx: Self::Tx) -> Result<Self::ExecutionResult, Self::Error> {
        self.inner.ctx.set_tx(tx);
        let mut h = UnstableHandler::<_, _, EthFrame<EthInterpreter>>::new();
        h.run(self)
    }

    fn finalize(&mut self) -> Self::State {
        self.inner.ctx.journal_mut().finalize()
    }

    fn replay(
        &mut self,
    ) -> Result<ExecResultAndState<Self::ExecutionResult, Self::State>, Self::Error> {
        let mut h = UnstableHandler::<_, _, EthFrame<EthInterpreter>>::new();
        h.run(self).map(|result| {
            let state = self.finalize();
            ExecResultAndState::new(result, state)
        })
    }
}

impl<DB, I, P> ExecuteCommitEvm for UnstableEvm<DB, I, P>
where
    DB: Database + DatabaseCommit,
    UnstableContext<DB>: crate::UnstableContextTr
        + ContextSetters
        + ContextTr<Db = DB, Tx = UnstableTransaction<TxEnv>, Block = BlockEnv>,
    P: PrecompileProvider<UnstableContext<DB>, Output = InterpreterResult>,
{
    fn commit(&mut self, state: Self::State) {
        self.inner.ctx.db_mut().commit(state);
    }
}

impl<DB, I, P> InspectEvm for UnstableEvm<DB, I, P>
where
    DB: Database,
    UnstableContext<DB>: crate::UnstableContextTr<Journal: JournalExt>
        + ContextSetters
        + ContextTr<Db = DB, Tx = UnstableTransaction<TxEnv>, Block = BlockEnv>,
    I: Inspector<UnstableContext<DB>>,
    P: PrecompileProvider<UnstableContext<DB>, Output = InterpreterResult>,
{
    type Inspector = I;

    fn set_inspector(&mut self, inspector: I) {
        self.inner.inspector = inspector;
    }

    fn inspect_one_tx(&mut self, tx: Self::Tx) -> Result<Self::ExecutionResult, Self::Error> {
        self.inner.ctx.set_tx(tx);
        let mut h = UnstableHandler::<_, _, EthFrame<EthInterpreter>>::new();
        h.inspect_run(self)
    }
}

impl<DB, I, P> InspectCommitEvm for UnstableEvm<DB, I, P>
where
    DB: Database + DatabaseCommit,
    UnstableContext<DB>: crate::UnstableContextTr<Journal: JournalExt>
        + ContextSetters
        + ContextTr<Db = DB, Tx = UnstableTransaction<TxEnv>, Block = BlockEnv>,
    I: Inspector<UnstableContext<DB>>,
    P: PrecompileProvider<UnstableContext<DB>, Output = InterpreterResult>,
{
}

impl<DB, I, P> SystemCallEvm for UnstableEvm<DB, I, P>
where
    DB: Database,
    UnstableContext<DB>: crate::UnstableContextTr<Tx: SystemCallTx>
        + ContextSetters
        + ContextTr<Db = DB, Tx = UnstableTransaction<TxEnv>, Block = BlockEnv>,
    P: PrecompileProvider<UnstableContext<DB>, Output = InterpreterResult>,
{
    fn system_call_one_with_caller(
        &mut self,
        caller: Address,
        system_contract_address: Address,
        data: Bytes,
    ) -> Result<Self::ExecutionResult, Self::Error> {
        self.inner.ctx.set_tx(<UnstableContext<DB> as ContextTr>::Tx::new_system_tx_with_caller(
            caller,
            system_contract_address,
            data,
        ));
        let mut h = UnstableHandler::<_, _, EthFrame<EthInterpreter>>::new();

        // load caller account into the journal (necessary for Geth proofs compatibility)
        // remove once https://github.com/bluealloy/revm/issues/3484 is fixed
        self.inner.ctx.journal_mut().load_account_with_code_mut(caller)?;

        h.run_system_call(self)
    }
}

impl<DB, I, P> InspectSystemCallEvm for UnstableEvm<DB, I, P>
where
    DB: Database,
    UnstableContext<DB>: crate::UnstableContextTr<Journal: JournalExt, Tx: SystemCallTx>
        + ContextSetters
        + ContextTr<Db = DB, Tx = UnstableTransaction<TxEnv>, Block = BlockEnv>,
    I: Inspector<UnstableContext<DB>>,
    P: PrecompileProvider<UnstableContext<DB>, Output = InterpreterResult>,
{
    fn inspect_one_system_call_with_caller(
        &mut self,
        caller: Address,
        system_contract_address: Address,
        data: Bytes,
    ) -> Result<Self::ExecutionResult, Self::Error> {
        self.inner.ctx.set_tx(<UnstableContext<DB> as ContextTr>::Tx::new_system_tx_with_caller(
            caller,
            system_contract_address,
            data,
        ));
        let mut h = UnstableHandler::<_, _, EthFrame<EthInterpreter>>::new();

        // load caller account into the journal (necessary for Geth proofs compatibility)
        // remove once https://github.com/bluealloy/revm/issues/3484 is fixed
        self.inner.ctx.journal_mut().load_account_with_code_mut(caller)?;

        h.inspect_run_system_call(self)
    }
}

impl<DB, I, P> Evm for UnstableEvm<DB, I, P>
where
    DB: Database,
    I: Inspector<UnstableContext<DB>>,
    P: PrecompileProvider<UnstableContext<DB>, Output = InterpreterResult>,
    UnstableContext<DB>: crate::UnstableContextTr
        + ContextSetters
        + ContextTr<Db = DB, Tx = UnstableTransaction<TxEnv>, Block = BlockEnv, Journal: JournalExt>,
{
    type DB = DB;
    type Tx = UnstableTransaction<TxEnv>;
    type Error = EVMError<DB::Error, UnstableTransactionError>;
    type HaltReason = UnstableHaltReason;
    type Spec = UnstableSpecId;
    type BlockEnv = BlockEnv;
    type Precompiles = P;
    type Inspector = I;

    fn block(&self) -> &BlockEnv {
        &self.block
    }

    fn chain_id(&self) -> u64 {
        self.cfg.chain_id
    }

    /// Executes `tx`, invoking the [`Inspector`] iff `self.inspect` is `true`.
    /// Uses [`InspectEvm::inspect_tx`] for the instrumented path and [`ExecuteEvm::transact`]
    /// for the uninstrumented path; both finalize the journal and return [`ResultAndState`].
    fn transact_raw(
        &mut self,
        tx: Self::Tx,
    ) -> Result<ResultAndState<Self::HaltReason>, Self::Error> {
        if self.inspect { InspectEvm::inspect_tx(self, tx) } else { ExecuteEvm::transact(self, tx) }
    }

    fn transact_system_call(
        &mut self,
        caller: Address,
        contract: Address,
        data: Bytes,
    ) -> Result<ResultAndState<Self::HaltReason>, Self::Error> {
        SystemCallEvm::system_call_with_caller(self, caller, contract, data)
    }

    fn finish(self) -> (Self::DB, EvmEnv<Self::Spec>) {
        let revm::Context { block: block_env, cfg: cfg_env, journaled_state, .. } = self.inner.ctx;

        (journaled_state.database, EvmEnv { block_env, cfg_env })
    }

    fn set_inspector_enabled(&mut self, enabled: bool) {
        self.inspect = enabled;
    }

    fn components(&self) -> (&Self::DB, &Self::Inspector, &Self::Precompiles) {
        (&self.inner.ctx.journaled_state.database, &self.inner.inspector, &self.inner.precompiles)
    }

    fn components_mut(&mut self) -> (&mut Self::DB, &mut Self::Inspector, &mut Self::Precompiles) {
        (
            &mut self.inner.ctx.journaled_state.database,
            &mut self.inner.inspector,
            &mut self.inner.precompiles,
        )
    }
}
