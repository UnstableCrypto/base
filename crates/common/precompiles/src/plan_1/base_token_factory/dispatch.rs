//! ABI dispatch for [`BaseTokenFactory`].

use alloy::{primitives::Address, sol_types::SolInterface};
use base_precompiles_contracts::IBaseTokenFactory::IBaseTokenFactoryCalls;
use revm::precompile::PrecompileResult;

use crate::{
    Precompile, base_token_factory::BaseTokenFactory, charge_input_cost, dispatch_call, mutate,
    view,
};

impl Precompile for BaseTokenFactory {
    fn call(&mut self, calldata: &[u8], msg_sender: Address) -> PrecompileResult {
        if let Some(err) = charge_input_cost(&mut self.storage, calldata) {
            return err;
        }

        dispatch_call(calldata, &[], IBaseTokenFactoryCalls::abi_decode, |call| match call {
            IBaseTokenFactoryCalls::createToken(c) => {
                mutate(c, msg_sender, |s, c| self.create_token(s, c))
            }
            IBaseTokenFactoryCalls::getTokenAddress(c) => view(c, |c| self.get_token_address(c)),
            IBaseTokenFactoryCalls::isBaseToken(c) => view(c, |c| self.is_base_token(c.token)),
        })
    }
}
