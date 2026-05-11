//! ABI dispatch for [`Base2PolicyRegistry`].

use alloy::{primitives::Address, sol_types::SolInterface};
use base_precompiles_contracts::IBase2PolicyRegistry::IBase2PolicyRegistryCalls;
use revm::precompile::PrecompileResult;

use crate::{
    Precompile, charge_input_cost, dispatch_call, metadata, mutate, mutate_void, view,
    plan_2::policy_registry::{Base2PolicyRegistry, IBase2PolicyRegistry},
};

impl Precompile for Base2PolicyRegistry {
    fn call(&mut self, calldata: &[u8], msg_sender: Address) -> PrecompileResult {
        if let Some(err) = charge_input_cost(&mut self.storage, calldata) {
            return err;
        }

        // Lazy bootstrap: revm discards all SSTORE writes on EIP-161 empty accounts
        // (no code, no balance, zero nonce). The registry has no genesis bytecode
        // unless Plan2Bootstrap ran, so the first write must mark it non-empty first.
        // Idempotent — re-setting the same 0xef bytecode is harmless.
        if let Ok(false) = self.is_initialized() {
            if let Err(e) = self.initialize() {
                return self.storage.error_result(e);
            }
        }

        dispatch_call(calldata, &[], IBase2PolicyRegistryCalls::abi_decode, |call| match call {
            IBase2PolicyRegistryCalls::policyIdCounter(_) => {
                metadata::<IBase2PolicyRegistry::policyIdCounterCall>(|| self.policy_id_counter())
            }
            IBase2PolicyRegistryCalls::policyExists(c) => view(c, |c| self.policy_exists(c)),
            IBase2PolicyRegistryCalls::isAuthorized(c) => view(c, |c| self.is_authorized(c)),
            IBase2PolicyRegistryCalls::policyAdmin(c) => view(c, |c| self.policy_admin(c)),
            IBase2PolicyRegistryCalls::policyKind(c) => view(c, |c| self.policy_kind(c)),
            IBase2PolicyRegistryCalls::createPolicy(c) => {
                mutate(c, msg_sender, |s, c| self.create_policy(s, c))
            }
            IBase2PolicyRegistryCalls::addToList(c) => {
                mutate_void(c, msg_sender, |s, c| self.add_to_list(s, c))
            }
            IBase2PolicyRegistryCalls::removeFromList(c) => {
                mutate_void(c, msg_sender, |s, c| self.remove_from_list(s, c))
            }
            IBase2PolicyRegistryCalls::setPolicyAdmin(c) => {
                mutate_void(c, msg_sender, |s, c| self.set_policy_admin(s, c))
            }
        })
    }
}
