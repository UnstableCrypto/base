//! ABI dispatch for [`BaseTokenPolicyRegistry`].

use alloy::{primitives::Address, sol_types::SolInterface};
use base_precompiles_contracts::IBaseTokenPolicyRegistry::IBaseTokenPolicyRegistryCalls;
use revm::precompile::PrecompileResult;

use crate::{
    Precompile, base_token_policy_registry::BaseTokenPolicyRegistry, charge_input_cost,
    dispatch_call, mutate, mutate_void, view,
};

impl Precompile for BaseTokenPolicyRegistry {
    fn call(&mut self, calldata: &[u8], msg_sender: Address) -> PrecompileResult {
        if let Some(err) = charge_input_cost(&mut self.storage, calldata) {
            return err;
        }

        dispatch_call(
            calldata,
            &[],
            IBaseTokenPolicyRegistryCalls::abi_decode,
            |call| match call {
                IBaseTokenPolicyRegistryCalls::policyIdCounter(c) => {
                    view(c, |_| self.policy_id_counter())
                }
                IBaseTokenPolicyRegistryCalls::policyExists(c) => {
                    view(c, |c| self.policy_exists(c))
                }
                IBaseTokenPolicyRegistryCalls::policyAdmin(c) => view(c, |c| self.policy_admin(c)),
                IBaseTokenPolicyRegistryCalls::policyKind(c) => view(c, |c| self.policy_kind(c)),
                IBaseTokenPolicyRegistryCalls::isAuthorized(c) => {
                    view(c, |c| self.is_authorized(c))
                }
                IBaseTokenPolicyRegistryCalls::createPolicy(c) => {
                    mutate(c, msg_sender, |s, c| self.create_policy(s, c))
                }
                IBaseTokenPolicyRegistryCalls::addToList(c) => {
                    mutate_void(c, msg_sender, |s, c| self.add_to_list(s, c))
                }
                IBaseTokenPolicyRegistryCalls::removeFromList(c) => {
                    mutate_void(c, msg_sender, |s, c| self.remove_from_list(s, c))
                }
                IBaseTokenPolicyRegistryCalls::setPolicyAdmin(c) => {
                    mutate_void(c, msg_sender, |s, c| self.set_policy_admin(s, c))
                }
            },
        )
    }
}
