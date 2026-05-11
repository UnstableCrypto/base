//! ForceTransfer for BaseSecurity. Gated by SECURITY_FORCE_TRANSFER bit.

use alloy::primitives::Address;
use base_precompiles_contracts::{BaseSecurityError, BaseSecurityEvent, IBaseSecurity};

use crate::{
    error::{BasePrecompileError, Result},
    plan_2::token::base_security::{BaseSecurity, FORCE_TRANSFER_ROLE},
    storage::Handler,
};

impl BaseSecurity {
    pub fn force_transfer(
        &mut self, sender: Address, call: IBaseSecurity::forceTransferCall,
    ) -> Result<()> {
        self.core.check_not_paused()
            .map_err(|_| BasePrecompileError::BaseSecurity(BaseSecurityError::contract_paused()))?;
        self.check_role(sender, *FORCE_TRANSFER_ROLE)?;

        let from_balance = self.core.get_balance(call.from)?;
        if call.amount > from_balance {
            return Err(BasePrecompileError::BaseSecurity(
                BaseSecurityError::insufficient_balance(from_balance, call.amount),
            ));
        }
        let new_from = from_balance.checked_sub(call.amount).ok_or(BasePrecompileError::under_overflow())?;
        self.core.set_balance(call.from, new_from)?;
        let to_balance = self.core.get_balance(call.to)?;
        let new_to = to_balance.checked_add(call.amount).ok_or(BasePrecompileError::under_overflow())?;
        self.core.set_balance(call.to, new_to)?;

        self.emit_event(BaseSecurityEvent::ForceTransfer(IBaseSecurity::ForceTransfer {
            from: call.from, to: call.to, amount: call.amount, reason: call.reason,
        }))?;
        self.emit_event(BaseSecurityEvent::Transfer(IBaseSecurity::Transfer {
            from: call.from, to: call.to, amount: call.amount,
        }))
    }
}
