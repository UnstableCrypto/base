//! ABI dispatch for [`BaseStablecoinFactory`].

use alloy::{primitives::Address, sol_types::SolInterface};
use base_precompiles_contracts::IBaseStablecoinFactory::IBaseStablecoinFactoryCalls;
use revm::precompile::PrecompileResult;

use crate::{
    Precompile, charge_input_cost, dispatch_call, mutate, view,
    plan_2::base_stablecoin_factory::BaseStablecoinFactory,
};

impl Precompile for BaseStablecoinFactory {
    fn call(&mut self, calldata: &[u8], msg_sender: Address) -> PrecompileResult {
        if let Some(err) = charge_input_cost(&mut self.storage, calldata) {
            return err;
        }

        dispatch_call(calldata, &[], IBaseStablecoinFactoryCalls::abi_decode, |call| match call {
            IBaseStablecoinFactoryCalls::createBaseStablecoin(c) => {
                mutate(c, msg_sender, |s, c| self.create_base_stablecoin(s, c))
            }
            IBaseStablecoinFactoryCalls::getBaseStablecoinAddress(c) => {
                view(c, |c| self.get_base_stablecoin_address(c))
            }
            IBaseStablecoinFactoryCalls::isBaseStablecoin(c) => {
                view(c, |c| self.is_base_stablecoin(c.token))
            }
        })
    }
}
