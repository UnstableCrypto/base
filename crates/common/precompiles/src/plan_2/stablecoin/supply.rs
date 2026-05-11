//! Supply for BaseStablecoin — mint, burn, burnBlocked. All structural-mandatory.

use alloy::primitives::Address;
use base_precompiles_contracts::{BaseStablecoinError, BaseStablecoinEvent, IBaseStablecoin};

use crate::{
    error::{BasePrecompileError, Result},
    plan_2::{
        policy_registry::Base2PolicyRegistry,
        shared::TransferKind,
        stablecoin::{BaseStablecoin, BURN_BLOCKED_ROLE, ISSUER_ROLE},
    },
    storage::Handler,
};

impl BaseStablecoin {
    pub fn mint(&mut self, sender: Address, call: IBaseStablecoin::mintCall) -> Result<()> {
        self.check_role(sender, *ISSUER_ROLE)?;
        self.move_balance(Address::ZERO, call.to, call.amount, TransferKind::Mint)?;
        self.emit_event(BaseStablecoinEvent::Mint(IBaseStablecoin::Mint { to: call.to, amount: call.amount }))
    }

    pub fn burn(&mut self, sender: Address, call: IBaseStablecoin::burnCall) -> Result<()> {
        self.check_role(sender, *ISSUER_ROLE)?;
        self.move_balance(sender, Address::ZERO, call.amount, TransferKind::Burn)?;
        self.emit_event(BaseStablecoinEvent::Burn(IBaseStablecoin::Burn { from: sender, amount: call.amount }))
    }

    pub fn burn_blocked(
        &mut self, sender: Address, call: IBaseStablecoin::burnBlockedCall,
    ) -> Result<()> {
        self.core.check_not_paused()
            .map_err(|_| BasePrecompileError::BaseStablecoin(BaseStablecoinError::contract_paused()))?;
        self.check_role(sender, *BURN_BLOCKED_ROLE)?;
        let policy_id = self.extra.policy_id.read()?;
        if Base2PolicyRegistry::new().is_authorized_internal(policy_id, call.from, call.from)? {
            return Err(BasePrecompileError::BaseStablecoin(BaseStablecoinError::policy_forbids()));
        }
        self.move_balance(call.from, Address::ZERO, call.amount, TransferKind::Burn)?;
        self.emit_event(BaseStablecoinEvent::BurnBlocked(IBaseStablecoin::BurnBlocked {
            from: call.from, amount: call.amount,
        }))
    }
}
