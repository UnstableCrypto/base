//! Custom EVM inspector for metering per-opcode and precompile gas usage.

use std::collections::HashSet;

use alloy_primitives::{Address, map::HashMap};
use revm::{
    Inspector,
    context::ContextTr,
    interpreter::{CallInputs, CallOutcome, CreateInputs, CreateOutcome, Interpreter},
};
use revm_inspectors::opcode::OpcodeGasInspector;

/// EVM inspector that tracks per-opcode gas usage and precompile call costs.
///
/// Wraps [`OpcodeGasInspector`] for opcode-level tracking and adds gas
/// attribution for calls to precompile addresses. Precompile execution
/// bypasses the interpreter (no `step`/`step_end` callbacks), so their
/// gas cost is invisible to the opcode inspector alone.
#[derive(Debug)]
pub(crate) struct MeteringInspector {
    inner: OpcodeGasInspector,
    precompile_gas: HashMap<Address, (u64, u64)>,
    metered_precompiles: HashSet<Address>,
}

impl MeteringInspector {
    /// Creates a new inspector that tracks the given precompile addresses.
    pub(crate) fn new(metered_precompiles: HashSet<Address>) -> Self {
        Self {
            inner: OpcodeGasInspector::new(),
            precompile_gas: HashMap::default(),
            metered_precompiles,
        }
    }

    /// Returns the inner opcode gas inspector.
    pub(crate) const fn inner(&self) -> &OpcodeGasInspector {
        &self.inner
    }

    /// Returns per-precompile gas data.
    pub(crate) const fn precompile_gas(&self) -> &HashMap<Address, (u64, u64)> {
        &self.precompile_gas
    }
}

impl<CTX> Inspector<CTX> for MeteringInspector
where
    CTX: ContextTr,
{
    fn step(&mut self, interp: &mut Interpreter, context: &mut CTX) {
        self.inner.step(interp, context);
    }

    fn step_end(&mut self, interp: &mut Interpreter, context: &mut CTX) {
        self.inner.step_end(interp, context);
    }

    fn call(&mut self, context: &mut CTX, inputs: &mut CallInputs) -> Option<CallOutcome> {
        self.inner.call(context, inputs)
    }

    fn call_end(&mut self, _context: &mut CTX, inputs: &CallInputs, outcome: &mut CallOutcome) {
        let target = inputs.bytecode_address;
        if self.metered_precompiles.contains(&target) {
            let gas_used = outcome.result.gas.spent();
            let entry = self.precompile_gas.entry(target).or_default();
            entry.0 += 1;
            entry.1 += gas_used;
        }
    }

    fn create(&mut self, context: &mut CTX, inputs: &mut CreateInputs) -> Option<CreateOutcome> {
        self.inner.create(context, inputs)
    }
}
