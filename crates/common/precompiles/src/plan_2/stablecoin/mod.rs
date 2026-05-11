//! `BaseStablecoin` — fiat-backed payment instrument token class.
//!
//! Delegates all common behavior to [`TokenCore`]. Mandatory: PolicyHook, SupplyControl,
//! BurnBlocked, Currency (immutable ISO 4217). Optional: Memo.
//! Structurally absent: SupplyCap, HolderLimit, ForceTransfer.
//! No dependency on `plan_2::token/`.
//! Asset class identifier: `assetClass() == 3`.

pub mod compliance;
pub mod dispatch;
pub mod features;
pub mod supply;

pub use features::{STABLECOIN_ALL_KNOWN, STABLECOIN_MEMO, StablecoinFeatures};

use alloy::primitives::{Address, B256, U256};
pub use base_precompiles_contracts::{BaseStablecoinError, BaseStablecoinEvent, IBaseStablecoin};
use base_precompiles_macros::contract;

use crate::{
    address::is_base_stablecoin_prefix,
    error::{BasePrecompileError, Result},
    plan_2::shared::{TokenCore, TransferKind, DEFAULT_ADMIN_ROLE},
    storage::Handler,
};

pub const STABLECOIN_CLASS: u8 = 3;

#[contract]
struct BaseStablecoinExtra {
    #[slot(10)]
    stablecoin_features: u8,
    #[slot(11)]
    policy_id: u64,
    #[slot(12)]
    currency: String,
}

pub struct BaseStablecoin {
    pub(crate) core: TokenCore,
    pub(crate) extra: BaseStablecoinExtra,
}

impl BaseStablecoin {
    pub fn from_address(address: Address) -> Result<Self> {
        if !is_base_stablecoin_prefix(&address) {
            return Err(BasePrecompileError::BaseStablecoin(BaseStablecoinError::invalid_token()));
        }
        Ok(Self {
            core: TokenCore::new_at(address),
            extra: BaseStablecoinExtra::__new(address),
        })
    }

    pub fn initialize(
        &mut self,
        msg_sender: Address,
        name: &str,
        symbol: &str,
        decimals: u8,
        admin: Address,
        currency: &str,
        policy_id: u64,
        features: u8,
    ) -> Result<()> {
        self.core.mark_initialized()?;
        self.core.initialize_core(msg_sender, name, symbol, decimals, admin)?;
        self.extra.stablecoin_features.write(features)?;
        self.extra.policy_id.write(policy_id)?;
        self.extra.currency.write(currency.to_string())?;
        Ok(())
    }

    pub fn name(&self) -> Result<String> { self.core.name() }
    pub fn symbol(&self) -> Result<String> { self.core.symbol() }
    pub fn decimals(&self) -> Result<u8> { self.core.decimals() }
    pub fn asset_class(&self) -> Result<u8> { Ok(STABLECOIN_CLASS) }
    pub fn is_initialized(&self) -> Result<bool> { self.core.is_initialized() }
    pub fn features_raw(&self) -> Result<u8> { self.extra.stablecoin_features.read() }
    pub fn feature_set(&self) -> Result<StablecoinFeatures> {
        Ok(StablecoinFeatures::new(self.extra.stablecoin_features.read()?))
    }
    pub fn currency(&self) -> Result<String> { self.extra.currency.read() }
}

// ─────────────────────────────────────────────────── RBAC

use std::sync::LazyLock;
use alloy::primitives::keccak256;

pub static ISSUER_ROLE: LazyLock<B256> =
    LazyLock::new(|| keccak256(b"BASE_STABLECOIN_ISSUER_ROLE"));
pub static PAUSER_ROLE: LazyLock<B256> =
    LazyLock::new(|| keccak256(b"BASE_STABLECOIN_PAUSER_ROLE"));
pub static BURN_BLOCKED_ROLE: LazyLock<B256> =
    LazyLock::new(|| keccak256(b"BASE_STABLECOIN_BURN_BLOCKED_ROLE"));
pub static POLICY_ADMIN_ROLE: LazyLock<B256> =
    LazyLock::new(|| keccak256(b"BASE_STABLECOIN_POLICY_ADMIN_ROLE"));

impl BaseStablecoin {
    pub fn issuer_role() -> B256 { *ISSUER_ROLE }
    pub fn pauser_role() -> B256 { *PAUSER_ROLE }
    pub fn burn_blocked_role() -> B256 { *BURN_BLOCKED_ROLE }
    pub fn policy_admin_role() -> B256 { *POLICY_ADMIN_ROLE }

    pub fn has_role(&self, call: IBaseStablecoin::hasRoleCall) -> Result<bool> {
        self.core.has_role_internal(call.account, call.role)
    }

    pub fn grant_role(&mut self, sender: Address, call: IBaseStablecoin::grantRoleCall) -> Result<()> {
        let admin_role = self.core.get_role_admin_internal(call.role)?;
        self.core.check_role_internal(sender, admin_role)
            .map_err(|_| BasePrecompileError::BaseStablecoin(BaseStablecoinError::unauthorized()))?;
        self.core.grant_role_internal(call.account, call.role)?;
        self.emit_event(BaseStablecoinEvent::RoleGranted(IBaseStablecoin::RoleGranted {
            role: call.role, account: call.account, sender,
        }))
    }

    pub fn revoke_role(&mut self, sender: Address, call: IBaseStablecoin::revokeRoleCall) -> Result<()> {
        let admin_role = self.core.get_role_admin_internal(call.role)?;
        self.core.check_role_internal(sender, admin_role)
            .map_err(|_| BasePrecompileError::BaseStablecoin(BaseStablecoinError::unauthorized()))?;
        self.core.revoke_role_internal(call.account, call.role)?;
        self.emit_event(BaseStablecoinEvent::RoleRevoked(IBaseStablecoin::RoleRevoked {
            role: call.role, account: call.account, sender,
        }))
    }

    pub fn renounce_role(&mut self, sender: Address, call: IBaseStablecoin::renounceRoleCall) -> Result<()> {
        self.core.check_role_internal(sender, call.role)
            .map_err(|_| BasePrecompileError::BaseStablecoin(BaseStablecoinError::unauthorized()))?;
        self.core.revoke_role_internal(sender, call.role)?;
        self.emit_event(BaseStablecoinEvent::RoleRevoked(IBaseStablecoin::RoleRevoked {
            role: call.role, account: sender, sender,
        }))
    }

    pub fn check_role(&self, account: Address, role: B256) -> Result<()> {
        self.core.check_role_internal(account, role)
            .map_err(|_| BasePrecompileError::BaseStablecoin(BaseStablecoinError::unauthorized()))
    }
}

// ─────────────────────────────────────────────────── ERC-20

impl BaseStablecoin {
    pub fn total_supply(&self) -> Result<U256> { self.core.get_total_supply() }
    pub fn balance_of(&self, call: IBaseStablecoin::balanceOfCall) -> Result<U256> {
        self.core.get_balance(call.account)
    }
    pub fn allowance(&self, call: IBaseStablecoin::allowanceCall) -> Result<U256> {
        self.core.get_allowance(call.owner, call.spender)
    }

    pub fn approve(&mut self, sender: Address, call: IBaseStablecoin::approveCall) -> Result<bool> {
        self.core.set_allowance(sender, call.spender, call.amount)?;
        self.emit_event(BaseStablecoinEvent::Approval(IBaseStablecoin::Approval {
            owner: sender, spender: call.spender, amount: call.amount,
        }))?;
        Ok(true)
    }

    pub fn transfer(&mut self, sender: Address, call: IBaseStablecoin::transferCall) -> Result<bool> {
        self.move_balance(sender, call.to, call.amount, TransferKind::Transfer)?;
        Ok(true)
    }

    pub fn transfer_from(
        &mut self, sender: Address, call: IBaseStablecoin::transferFromCall,
    ) -> Result<bool> {
        self.core.consume_allowance(call.from, sender, call.amount)
            .map_err(|_| BasePrecompileError::BaseStablecoin(BaseStablecoinError::insufficient_allowance()))?;
        self.move_balance(call.from, call.to, call.amount, TransferKind::Transfer)?;
        Ok(true)
    }
}

// ─────────────────────────────────────────────────── pause

impl BaseStablecoin {
    pub fn paused(&self) -> Result<bool> { self.core.get_paused() }

    pub fn pause(&mut self, sender: Address, _call: IBaseStablecoin::pauseCall) -> Result<()> {
        self.check_role(sender, *PAUSER_ROLE)?;
        self.core.set_paused(true)?;
        self.emit_event(BaseStablecoinEvent::PauseStateUpdate(IBaseStablecoin::PauseStateUpdate {
            updater: sender, isPaused: true,
        }))
    }

    pub fn unpause(&mut self, sender: Address, _call: IBaseStablecoin::unpauseCall) -> Result<()> {
        self.check_role(sender, *PAUSER_ROLE)?;
        self.core.set_paused(false)?;
        self.emit_event(BaseStablecoinEvent::PauseStateUpdate(IBaseStablecoin::PauseStateUpdate {
            updater: sender, isPaused: false,
        }))
    }
}

// ─────────────────────────────────────────────────── permit

impl BaseStablecoin {
    pub fn nonces(&self, call: IBaseStablecoin::noncesCall) -> Result<U256> {
        self.core.get_permit_nonce(call.owner)
    }

    pub fn domain_separator(&self) -> Result<B256> {
        self.core.compute_domain_separator(&self.core.name()?)
    }

    pub fn permit(&mut self, call: IBaseStablecoin::permitCall) -> Result<()> {
        if self.core.timestamp_u256() > call.deadline {
            return Err(BasePrecompileError::BaseStablecoin(BaseStablecoinError::permit_expired()));
        }
        let nonce = self.core.get_permit_nonce(call.owner)?;
        let domain_separator = self.domain_separator()?;
        self.core
            .verify_permit_sig(call.owner, call.spender, call.value, call.deadline,
                call.v, call.r, call.s, nonce, domain_separator)
            .map_err(|_| BasePrecompileError::BaseStablecoin(BaseStablecoinError::invalid_signature()))?;
        self.core.increment_permit_nonce(call.owner)?;
        self.core.set_allowance(call.owner, call.spender, call.value)?;
        self.emit_event(BaseStablecoinEvent::Approval(IBaseStablecoin::Approval {
            owner: call.owner, spender: call.spender, amount: call.value,
        }))
    }
}

// ─────────────────────────────────────────────────── memo

impl BaseStablecoin {
    pub fn transfer_with_memo(
        &mut self, sender: Address, call: IBaseStablecoin::transferWithMemoCall,
    ) -> Result<bool> {
        self.move_balance(sender, call.to, call.amount, TransferKind::Transfer)?;
        self.emit_memo(sender, call.to, call.amount, call.memo)?;
        Ok(true)
    }

    pub fn mint_with_memo(
        &mut self, sender: Address, call: IBaseStablecoin::mintWithMemoCall,
    ) -> Result<()> {
        self.check_role(sender, *ISSUER_ROLE)?;
        self.move_balance(Address::ZERO, call.to, call.amount, TransferKind::Mint)?;
        self.emit_event(BaseStablecoinEvent::Mint(IBaseStablecoin::Mint { to: call.to, amount: call.amount }))?;
        self.emit_memo(Address::ZERO, call.to, call.amount, call.memo)
    }

    pub fn burn_with_memo(
        &mut self, sender: Address, call: IBaseStablecoin::burnWithMemoCall,
    ) -> Result<()> {
        self.check_role(sender, *ISSUER_ROLE)?;
        self.move_balance(sender, Address::ZERO, call.amount, TransferKind::Burn)?;
        self.emit_event(BaseStablecoinEvent::Burn(IBaseStablecoin::Burn { from: sender, amount: call.amount }))?;
        self.emit_memo(sender, Address::ZERO, call.amount, call.memo)
    }

    fn emit_memo(&mut self, from: Address, to: Address, amount: U256, memo: B256) -> Result<()> {
        self.emit_event(BaseStablecoinEvent::TransferWithMemo(IBaseStablecoin::TransferWithMemo {
            from, to, amount, memo,
        }))
    }
}

// ─────────────────────────────────────────────────── balance pipeline

impl BaseStablecoin {
    /// BaseStablecoin pipeline: pause → recipient → mandatory policy → apply → emit.
    /// No supply cap (elastic by design).
    pub(crate) fn move_balance(
        &mut self, from: Address, to: Address, amount: U256, kind: TransferKind,
    ) -> Result<()> {
        self.core.check_not_paused()
            .map_err(|_| BasePrecompileError::BaseStablecoin(BaseStablecoinError::contract_paused()))?;

        match kind {
            TransferKind::Transfer | TransferKind::Mint => {
                self.core.validate_recipient_for(to, is_base_stablecoin_prefix)
                    .map_err(|_| BasePrecompileError::BaseStablecoin(BaseStablecoinError::invalid_recipient()))?;
            }
            TransferKind::Burn => {}
        }

        if kind != TransferKind::Burn {
            self.ensure_transfer_authorized(from, to)?;
        }

        self.core.apply_balance_move(from, to, amount, kind)
            .map_err(|_| BasePrecompileError::BaseStablecoin(
                BaseStablecoinError::insufficient_balance(U256::ZERO, amount)
            ))?;

        self.emit_event(BaseStablecoinEvent::Transfer(IBaseStablecoin::Transfer { from, to, amount }))
    }
}

// ─────────────────────────────────────────────────── emit_event forwarding

impl BaseStablecoin {
    pub(crate) fn emit_event(&mut self, event: BaseStablecoinEvent) -> Result<()> {
        self.core.emit(event)
    }
}
