//! Mandatory policy compliance for BaseStablecoin — integration boundary to Base2PolicyRegistry.

use alloy::primitives::Address;
use base_precompiles_contracts::{BaseStablecoinError, BaseStablecoinEvent, IBaseStablecoin};

use crate::{
    error::{BasePrecompileError, Result},
    plan_2::{
        policy_registry::Base2PolicyRegistry,
        stablecoin::{BaseStablecoin, POLICY_ADMIN_ROLE},
    },
    storage::Handler,
};

impl BaseStablecoin {
    pub fn policy_id(&self) -> Result<u64> { self.extra.policy_id.read() }

    pub fn set_policy_id(
        &mut self, sender: Address, call: IBaseStablecoin::setPolicyIdCall,
    ) -> Result<()> {
        self.check_role(sender, *POLICY_ADMIN_ROLE)?;
        if !Base2PolicyRegistry::new().policy_exists_internal(call.newPolicyId)? {
            return Err(BasePrecompileError::BaseStablecoin(BaseStablecoinError::invalid_policy_id()));
        }
        self.extra.policy_id.write(call.newPolicyId)?;
        self.emit_event(BaseStablecoinEvent::PolicyIdUpdate(IBaseStablecoin::PolicyIdUpdate {
            updater: sender, newPolicyId: call.newPolicyId,
        }))
    }

    pub(super) fn ensure_transfer_authorized(&self, from: Address, to: Address) -> Result<()> {
        let policy_id = self.extra.policy_id.read()?;
        if !Base2PolicyRegistry::new().is_authorized_internal(policy_id, from, to)? {
            return Err(BasePrecompileError::BaseStablecoin(BaseStablecoinError::policy_forbids()));
        }
        Ok(())
    }
}
