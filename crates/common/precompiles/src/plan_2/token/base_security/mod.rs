//! `BaseSecurity` — regulated real-world asset token class.
//!
//! Delegates all common behavior to [`TokenCore`]. Mandatory: PolicyHook, SupplyCap,
//! SupplyControl, BurnBlocked. Optional: Memo, ForceTransfer, HolderLimit.
//! Structurally absent: Currency, Yield, VirtualAddress.
//! Asset class identifier: `assetClass() == 2`.

pub mod compliance;
pub mod dispatch;
pub mod features;
pub mod force_transfer;
pub mod holder_limit;
pub mod supply;

#[cfg(test)]
mod tests;

pub use features::{
    SECURITY_ALL_KNOWN, SECURITY_FORCE_TRANSFER, SECURITY_HOLDER_LIMIT, SECURITY_MEMO,
    SecurityFeatures,
};

use alloy::primitives::{Address, B256, U256};
pub use base_precompiles_contracts::{BaseSecurityError, BaseSecurityEvent, IBaseSecurity};
use base_precompiles_macros::contract;

use crate::{
    address::is_base_security_prefix,
    error::{BasePrecompileError, Result},
    plan_2::shared::{TokenCore, TransferKind, DEFAULT_ADMIN_ROLE},
    storage::Handler,
};

pub const SECURITY_CLASS: u8 = 2;

#[contract]
struct BaseSecurityExtra {
    #[slot(10)]
    security_features: u8,
    #[slot(11)]
    supply_cap: U256,
    #[slot(12)]
    policy_id: u64,
    #[slot(13)]
    holder_count: u64,
    #[slot(14)]
    holder_limit_cap: u64,
}

pub struct BaseSecurity {
    pub(crate) core: TokenCore,
    pub(crate) extra: BaseSecurityExtra,
}

impl BaseSecurity {
    pub fn from_address(address: Address) -> Result<Self> {
        if !is_base_security_prefix(&address) {
            return Err(BasePrecompileError::BaseSecurity(BaseSecurityError::invalid_token()));
        }
        Ok(Self {
            core: TokenCore::new_at(address),
            extra: BaseSecurityExtra::__new(address),
        })
    }

    pub fn initialize(
        &mut self,
        msg_sender: Address,
        name: &str,
        symbol: &str,
        decimals: u8,
        admin: Address,
        policy_id: u64,
        supply_cap: U256,
        features: u8,
        holder_limit: u64,
    ) -> Result<()> {
        self.core.mark_initialized()?;
        self.core.initialize_core(msg_sender, name, symbol, decimals, admin)?;
        self.extra.security_features.write(features)?;
        self.extra.supply_cap.write(supply_cap)?;
        self.extra.policy_id.write(policy_id)?;
        if (features & SECURITY_HOLDER_LIMIT) != 0 {
            self.extra.holder_limit_cap.write(holder_limit)?;
        }
        Ok(())
    }

    pub fn name(&self) -> Result<String> { self.core.name() }
    pub fn symbol(&self) -> Result<String> { self.core.symbol() }
    pub fn decimals(&self) -> Result<u8> { self.core.decimals() }
    pub fn asset_class(&self) -> Result<u8> { Ok(SECURITY_CLASS) }
    pub fn is_initialized(&self) -> Result<bool> { self.core.is_initialized() }
    pub fn features_raw(&self) -> Result<u8> { self.extra.security_features.read() }
    pub fn feature_set(&self) -> Result<SecurityFeatures> {
        Ok(SecurityFeatures::new(self.extra.security_features.read()?))
    }
    pub fn supply_cap(&self) -> Result<U256> { self.extra.supply_cap.read() }
}

// ─────────────────────────────────────────────────── RBAC

use std::sync::LazyLock;
use alloy::primitives::keccak256;

pub static ISSUER_ROLE: LazyLock<B256> =
    LazyLock::new(|| keccak256(b"BASE_SECURITY_ISSUER_ROLE"));
pub static PAUSER_ROLE: LazyLock<B256> =
    LazyLock::new(|| keccak256(b"BASE_SECURITY_PAUSER_ROLE"));
pub static BURN_BLOCKED_ROLE: LazyLock<B256> =
    LazyLock::new(|| keccak256(b"BASE_SECURITY_BURN_BLOCKED_ROLE"));
pub static FORCE_TRANSFER_ROLE: LazyLock<B256> =
    LazyLock::new(|| keccak256(b"BASE_SECURITY_FORCE_TRANSFER_ROLE"));
pub static POLICY_ADMIN_ROLE: LazyLock<B256> =
    LazyLock::new(|| keccak256(b"BASE_SECURITY_POLICY_ADMIN_ROLE"));

impl BaseSecurity {
    pub fn issuer_role() -> B256 { *ISSUER_ROLE }
    pub fn pauser_role() -> B256 { *PAUSER_ROLE }
    pub fn burn_blocked_role() -> B256 { *BURN_BLOCKED_ROLE }
    pub fn force_transfer_role() -> B256 { *FORCE_TRANSFER_ROLE }
    pub fn policy_admin_role() -> B256 { *POLICY_ADMIN_ROLE }

    pub fn has_role(&self, call: IBaseSecurity::hasRoleCall) -> Result<bool> {
        self.core.has_role_internal(call.account, call.role)
    }

    pub fn grant_role(&mut self, sender: Address, call: IBaseSecurity::grantRoleCall) -> Result<()> {
        let admin_role = self.core.get_role_admin_internal(call.role)?;
        self.core.check_role_internal(sender, admin_role)
            .map_err(|_| BasePrecompileError::BaseSecurity(BaseSecurityError::unauthorized()))?;
        self.core.grant_role_internal(call.account, call.role)?;
        self.emit_event(BaseSecurityEvent::RoleGranted(IBaseSecurity::RoleGranted {
            role: call.role, account: call.account, sender,
        }))
    }

    pub fn revoke_role(&mut self, sender: Address, call: IBaseSecurity::revokeRoleCall) -> Result<()> {
        let admin_role = self.core.get_role_admin_internal(call.role)?;
        self.core.check_role_internal(sender, admin_role)
            .map_err(|_| BasePrecompileError::BaseSecurity(BaseSecurityError::unauthorized()))?;
        self.core.revoke_role_internal(call.account, call.role)?;
        self.emit_event(BaseSecurityEvent::RoleRevoked(IBaseSecurity::RoleRevoked {
            role: call.role, account: call.account, sender,
        }))
    }

    pub fn renounce_role(&mut self, sender: Address, call: IBaseSecurity::renounceRoleCall) -> Result<()> {
        self.core.check_role_internal(sender, call.role)
            .map_err(|_| BasePrecompileError::BaseSecurity(BaseSecurityError::unauthorized()))?;
        self.core.revoke_role_internal(sender, call.role)?;
        self.emit_event(BaseSecurityEvent::RoleRevoked(IBaseSecurity::RoleRevoked {
            role: call.role, account: sender, sender,
        }))
    }

    pub fn check_role(&self, account: Address, role: B256) -> Result<()> {
        self.core.check_role_internal(account, role)
            .map_err(|_| BasePrecompileError::BaseSecurity(BaseSecurityError::unauthorized()))
    }
}

// ─────────────────────────────────────────────────── ERC-20

impl BaseSecurity {
    pub fn total_supply(&self) -> Result<U256> { self.core.get_total_supply() }
    pub fn balance_of(&self, call: IBaseSecurity::balanceOfCall) -> Result<U256> {
        self.core.get_balance(call.account)
    }
    pub fn allowance(&self, call: IBaseSecurity::allowanceCall) -> Result<U256> {
        self.core.get_allowance(call.owner, call.spender)
    }

    pub fn approve(&mut self, sender: Address, call: IBaseSecurity::approveCall) -> Result<bool> {
        self.core.set_allowance(sender, call.spender, call.amount)?;
        self.emit_event(BaseSecurityEvent::Approval(IBaseSecurity::Approval {
            owner: sender, spender: call.spender, amount: call.amount,
        }))?;
        Ok(true)
    }

    pub fn transfer(&mut self, sender: Address, call: IBaseSecurity::transferCall) -> Result<bool> {
        self.move_balance(sender, call.to, call.amount, TransferKind::Transfer)?;
        Ok(true)
    }

    pub fn transfer_from(
        &mut self, sender: Address, call: IBaseSecurity::transferFromCall,
    ) -> Result<bool> {
        self.core.consume_allowance(call.from, sender, call.amount)
            .map_err(|_| BasePrecompileError::BaseSecurity(BaseSecurityError::insufficient_allowance()))?;
        self.move_balance(call.from, call.to, call.amount, TransferKind::Transfer)?;
        Ok(true)
    }
}

// ─────────────────────────────────────────────────── pause

impl BaseSecurity {
    pub fn paused(&self) -> Result<bool> { self.core.get_paused() }

    pub fn pause(&mut self, sender: Address, _call: IBaseSecurity::pauseCall) -> Result<()> {
        self.check_role(sender, *PAUSER_ROLE)?;
        self.core.set_paused(true)?;
        self.emit_event(BaseSecurityEvent::PauseStateUpdate(IBaseSecurity::PauseStateUpdate {
            updater: sender, isPaused: true,
        }))
    }

    pub fn unpause(&mut self, sender: Address, _call: IBaseSecurity::unpauseCall) -> Result<()> {
        self.check_role(sender, *PAUSER_ROLE)?;
        self.core.set_paused(false)?;
        self.emit_event(BaseSecurityEvent::PauseStateUpdate(IBaseSecurity::PauseStateUpdate {
            updater: sender, isPaused: false,
        }))
    }
}

// ─────────────────────────────────────────────────── permit

impl BaseSecurity {
    pub fn nonces(&self, call: IBaseSecurity::noncesCall) -> Result<U256> {
        self.core.get_permit_nonce(call.owner)
    }

    pub fn domain_separator(&self) -> Result<B256> {
        self.core.compute_domain_separator(&self.core.name()?)
    }

    pub fn permit(&mut self, call: IBaseSecurity::permitCall) -> Result<()> {
        if self.core.timestamp_u256() > call.deadline {
            return Err(BasePrecompileError::BaseSecurity(BaseSecurityError::permit_expired()));
        }
        let nonce = self.core.get_permit_nonce(call.owner)?;
        let domain_separator = self.domain_separator()?;
        self.core
            .verify_permit_sig(call.owner, call.spender, call.value, call.deadline,
                call.v, call.r, call.s, nonce, domain_separator)
            .map_err(|_| BasePrecompileError::BaseSecurity(BaseSecurityError::invalid_signature()))?;
        self.core.increment_permit_nonce(call.owner)?;
        self.core.set_allowance(call.owner, call.spender, call.value)?;
        self.emit_event(BaseSecurityEvent::Approval(IBaseSecurity::Approval {
            owner: call.owner, spender: call.spender, amount: call.value,
        }))
    }
}

// ─────────────────────────────────────────────────── memo

impl BaseSecurity {
    pub fn transfer_with_memo(
        &mut self, sender: Address, call: IBaseSecurity::transferWithMemoCall,
    ) -> Result<bool> {
        self.move_balance(sender, call.to, call.amount, TransferKind::Transfer)?;
        self.emit_memo(sender, call.to, call.amount, call.memo)?;
        Ok(true)
    }

    pub fn mint_with_memo(
        &mut self, sender: Address, call: IBaseSecurity::mintWithMemoCall,
    ) -> Result<()> {
        self.check_role(sender, *ISSUER_ROLE)?;
        self.move_balance(Address::ZERO, call.to, call.amount, TransferKind::Mint)?;
        self.emit_event(BaseSecurityEvent::Mint(IBaseSecurity::Mint { to: call.to, amount: call.amount }))?;
        self.emit_memo(Address::ZERO, call.to, call.amount, call.memo)
    }

    pub fn burn_with_memo(
        &mut self, sender: Address, call: IBaseSecurity::burnWithMemoCall,
    ) -> Result<()> {
        self.check_role(sender, *ISSUER_ROLE)?;
        self.move_balance(sender, Address::ZERO, call.amount, TransferKind::Burn)?;
        self.emit_event(BaseSecurityEvent::Burn(IBaseSecurity::Burn { from: sender, amount: call.amount }))?;
        self.emit_memo(sender, Address::ZERO, call.amount, call.memo)
    }

    fn emit_memo(&mut self, from: Address, to: Address, amount: U256, memo: B256) -> Result<()> {
        self.emit_event(BaseSecurityEvent::TransferWithMemo(IBaseSecurity::TransferWithMemo {
            from, to, amount, memo,
        }))
    }
}

// ─────────────────────────────────────────────────── balance pipeline

impl BaseSecurity {
    /// BaseSecurity pipeline: pause → recipient → mandatory policy → supply cap → apply → holder count → emit.
    pub(crate) fn move_balance(
        &mut self, from: Address, to: Address, amount: U256, kind: TransferKind,
    ) -> Result<()> {
        self.core.check_not_paused()
            .map_err(|_| BasePrecompileError::BaseSecurity(BaseSecurityError::contract_paused()))?;

        match kind {
            TransferKind::Transfer | TransferKind::Mint => {
                self.core.validate_recipient_for(to, is_base_security_prefix)
                    .map_err(|_| BasePrecompileError::BaseSecurity(BaseSecurityError::invalid_recipient()))?;
            }
            TransferKind::Burn => {}
        }

        if kind != TransferKind::Burn {
            self.ensure_transfer_authorized(from, to)?;
        }

        if kind == TransferKind::Mint {
            let cap = self.extra.supply_cap.read()?;
            let total = self.core.get_total_supply()?;
            let new_total = total.checked_add(amount).ok_or(BasePrecompileError::under_overflow())?;
            if new_total > cap {
                return Err(BasePrecompileError::BaseSecurity(BaseSecurityError::supply_cap_exceeded()));
            }
        }

        self.core.apply_balance_move(from, to, amount, kind)
            .map_err(|_| BasePrecompileError::BaseSecurity(
                BaseSecurityError::insufficient_balance(U256::ZERO, amount)
            ))?;

        let features = self.feature_set()?;
        if features.has(SECURITY_HOLDER_LIMIT) {
            self.update_holder_count(from, to, amount, kind)?;
        }

        self.emit_event(BaseSecurityEvent::Transfer(IBaseSecurity::Transfer { from, to, amount }))
    }
}

// ─────────────────────────────────────────────────── emit_event forwarding

impl BaseSecurity {
    pub(crate) fn emit_event(&mut self, event: BaseSecurityEvent) -> Result<()> {
        self.core.emit(event)
    }
}
