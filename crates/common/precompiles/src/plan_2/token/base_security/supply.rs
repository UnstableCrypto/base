//! Supply for BaseSecurity — mint, burn, burnBlocked. All structural-mandatory.

use alloy::primitives::Address;
use base_precompiles_contracts::{BaseSecurityError, BaseSecurityEvent, IBaseSecurity};

use crate::{
    error::{BasePrecompileError, Result},
    plan_2::{
        policy_registry::Base2PolicyRegistry,
        shared::TransferKind,
        token::base_security::{BaseSecurity, BURN_BLOCKED_ROLE, ISSUER_ROLE},
    },
    storage::Handler,
};

impl BaseSecurity {
    pub fn mint(&mut self, sender: Address, call: IBaseSecurity::mintCall) -> Result<()> {
        self.check_role(sender, *ISSUER_ROLE)?;
        self.move_balance(Address::ZERO, call.to, call.amount, TransferKind::Mint)?;
        self.emit_event(BaseSecurityEvent::Mint(IBaseSecurity::Mint { to: call.to, amount: call.amount }))
    }

    pub fn burn(&mut self, sender: Address, call: IBaseSecurity::burnCall) -> Result<()> {
        self.check_role(sender, *ISSUER_ROLE)?;
        self.move_balance(sender, Address::ZERO, call.amount, TransferKind::Burn)?;
        self.emit_event(BaseSecurityEvent::Burn(IBaseSecurity::Burn { from: sender, amount: call.amount }))
    }

    pub fn burn_blocked(
        &mut self, sender: Address, call: IBaseSecurity::burnBlockedCall,
    ) -> Result<()> {
        self.core.check_not_paused()
            .map_err(|_| BasePrecompileError::BaseSecurity(BaseSecurityError::contract_paused()))?;
        self.check_role(sender, *BURN_BLOCKED_ROLE)?;
        let policy_id = self.extra.policy_id.read()?;
        if Base2PolicyRegistry::new().is_authorized_internal(policy_id, call.from, call.from)? {
            return Err(BasePrecompileError::BaseSecurity(BaseSecurityError::policy_forbids()));
        }
        self.move_balance(call.from, Address::ZERO, call.amount, TransferKind::Burn)?;
        self.emit_event(BaseSecurityEvent::BurnBlocked(IBaseSecurity::BurnBlocked {
            from: call.from, amount: call.amount,
        }))
    }
}
