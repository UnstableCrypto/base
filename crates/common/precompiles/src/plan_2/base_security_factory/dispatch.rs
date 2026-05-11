//! ABI dispatch for [`BaseSecurityFactory`].

use alloy::{primitives::Address, sol_types::SolInterface};
use base_precompiles_contracts::IBaseSecurityFactory::IBaseSecurityFactoryCalls;
use revm::precompile::PrecompileResult;

use crate::{
    Precompile, charge_input_cost, dispatch_call, mutate, view,
    plan_2::base_security_factory::BaseSecurityFactory,
};

impl Precompile for BaseSecurityFactory {
    fn call(&mut self, calldata: &[u8], msg_sender: Address) -> PrecompileResult {
        if let Some(err) = charge_input_cost(&mut self.storage, calldata) {
            return err;
        }

        dispatch_call(calldata, &[], IBaseSecurityFactoryCalls::abi_decode, |call| match call {
            IBaseSecurityFactoryCalls::createBaseSecurity(c) => {
                mutate(c, msg_sender, |s, c| self.create_base_security(s, c))
            }
            IBaseSecurityFactoryCalls::getBaseSecurityAddress(c) => {
                view(c, |c| self.get_base_security_address(c))
            }
            IBaseSecurityFactoryCalls::isBaseSecurity(c) => {
                view(c, |c| self.is_base_security(c.token))
            }
        })
    }
}
