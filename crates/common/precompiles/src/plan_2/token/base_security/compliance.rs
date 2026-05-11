//! Policy compliance for BaseSecurity — integration boundary to Base2PolicyRegistry.

use alloy::primitives::Address;
use base_precompiles_contracts::{BaseSecurityError, BaseSecurityEvent, IBaseSecurity};

use crate::{
    error::{BasePrecompileError, Result},
    plan_2::{
        policy_registry::Base2PolicyRegistry,
        token::base_security::{BaseSecurity, POLICY_ADMIN_ROLE},
    },
    storage::Handler,
};

impl BaseSecurity {
    pub fn policy_id(&self) -> Result<u64> { self.extra.policy_id.read() }

    pub fn set_policy_id(
        &mut self, sender: Address, call: IBaseSecurity::setPolicyIdCall,
    ) -> Result<()> {
        self.check_role(sender, *POLICY_ADMIN_ROLE)?;
        if !Base2PolicyRegistry::new().policy_exists_internal(call.newPolicyId)? {
            return Err(BasePrecompileError::BaseSecurity(BaseSecurityError::invalid_policy_id()));
        }
        self.extra.policy_id.write(call.newPolicyId)?;
        self.emit_event(BaseSecurityEvent::PolicyIdUpdate(IBaseSecurity::PolicyIdUpdate {
            updater: sender, newPolicyId: call.newPolicyId,
        }))
    }

    pub(super) fn ensure_transfer_authorized(&self, from: Address, to: Address) -> Result<()> {
        let policy_id = self.extra.policy_id.read()?;
        if !Base2PolicyRegistry::new().is_authorized_internal(policy_id, from, to)? {
            return Err(BasePrecompileError::BaseSecurity(BaseSecurityError::policy_forbids()));
        }
        Ok(())
    }
}
