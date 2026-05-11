//! `BaseAsset` — permissionless onchain-native token class.
//!
//! Delegates all common behavior (ERC-20, RBAC, pause, permit) to [`TokenCore`].
//! Class-specific: optional SupplyControl (mint/burn), optional SupplyCap, optional Memo.
//! Structurally absent: PolicyHook, BurnBlocked, ForceTransfer, HolderLimit, Currency.
//! Asset class identifier: `assetClass() == 1`.

pub mod dispatch;
pub mod features;
pub mod supply;

pub use features::{ASSET_ALL_KNOWN, ASSET_MEMO, ASSET_SUPPLY_CAP, ASSET_SUPPLY_CONTROL,
    AssetFeatures};

use alloy::primitives::{Address, B256, U256};
pub use base_precompiles_contracts::{BaseAssetError, BaseAssetEvent, IBaseAsset};
use base_precompiles_macros::contract;

use crate::{
    address::{is_base_asset_prefix, is_base_security_prefix, is_base_stablecoin_prefix},
    error::{BasePrecompileError, Result},
    plan_2::shared::{TokenCore, TransferKind, DEFAULT_ADMIN_ROLE, UNGRANTABLE_ROLE},
    storage::Handler,
};

pub const ASSET_CLASS: u8 = 1;

/// Class-specific storage fields (slots 10-11), co-located with [`TokenCore`] at the same address.
#[contract]
struct BaseAssetExtra {
    #[slot(10)]
    asset_features: u8,
    #[slot(11)]
    supply_cap: U256,
}

/// BaseAsset precompile. Wraps [`TokenCore`] (slots 0-9) and adds class-specific slots (10-11).
pub struct BaseAsset {
    pub(crate) core: TokenCore,
    extra: BaseAssetExtra,
}

impl BaseAsset {
    pub fn from_address(address: Address) -> Result<Self> {
        if !is_base_asset_prefix(&address) {
            return Err(BasePrecompileError::BaseAsset(BaseAssetError::invalid_token()));
        }
        Ok(Self {
            core: TokenCore::new_at(address),
            extra: BaseAssetExtra::__new(address),
        })
    }

    /// Called once by `BaseAssetFactory::create_base_asset`.
    pub fn initialize(
        &mut self,
        msg_sender: Address,
        name: &str,
        symbol: &str,
        decimals: u8,
        admin: Address,
        features: u8,
        supply_cap: U256,
    ) -> Result<()> {
        self.core.mark_initialized()?;
        self.core.initialize_core(msg_sender, name, symbol, decimals, admin)?;
        self.extra.asset_features.write(features)?;
        self.extra.supply_cap.write(supply_cap)?;
        Ok(())
    }

    pub fn name(&self) -> Result<String> { self.core.name() }
    pub fn symbol(&self) -> Result<String> { self.core.symbol() }
    pub fn decimals(&self) -> Result<u8> { self.core.decimals() }
    pub fn asset_class(&self) -> Result<u8> { Ok(ASSET_CLASS) }
    pub fn is_initialized(&self) -> Result<bool> { self.core.is_initialized() }
    pub fn features_raw(&self) -> Result<u8> { self.extra.asset_features.read() }
    pub fn feature_set(&self) -> Result<AssetFeatures> {
        Ok(AssetFeatures::new(self.extra.asset_features.read()?))
    }
    pub fn supply_cap(&self) -> Result<U256> { self.extra.supply_cap.read() }
}

// ─────────────────────────────────────────────────── RBAC surface (delegates to core)

use std::sync::LazyLock;
use alloy::primitives::keccak256;

pub static ISSUER_ROLE: LazyLock<B256> =
    LazyLock::new(|| keccak256(b"BASE_ASSET_ISSUER_ROLE"));
pub static PAUSER_ROLE: LazyLock<B256> =
    LazyLock::new(|| keccak256(b"BASE_ASSET_PAUSER_ROLE"));

impl BaseAsset {
    pub fn issuer_role() -> B256 { *ISSUER_ROLE }
    pub fn pauser_role() -> B256 { *PAUSER_ROLE }

    pub fn has_role(&self, call: IBaseAsset::hasRoleCall) -> Result<bool> {
        self.core.has_role_internal(call.account, call.role)
            .map_err(|_| BasePrecompileError::BaseAsset(BaseAssetError::unauthorized()))
    }

    pub fn grant_role(&mut self, sender: Address, call: IBaseAsset::grantRoleCall) -> Result<()> {
        let admin_role = self.core.get_role_admin_internal(call.role)?;
        self.core.check_role_internal(sender, admin_role)
            .map_err(|_| BasePrecompileError::BaseAsset(BaseAssetError::unauthorized()))?;
        self.core.grant_role_internal(call.account, call.role)?;
        self.emit_event(BaseAssetEvent::RoleGranted(IBaseAsset::RoleGranted {
            role: call.role,
            account: call.account,
            sender,
        }))
    }

    pub fn revoke_role(&mut self, sender: Address, call: IBaseAsset::revokeRoleCall) -> Result<()> {
        let admin_role = self.core.get_role_admin_internal(call.role)?;
        self.core.check_role_internal(sender, admin_role)
            .map_err(|_| BasePrecompileError::BaseAsset(BaseAssetError::unauthorized()))?;
        self.core.revoke_role_internal(call.account, call.role)?;
        self.emit_event(BaseAssetEvent::RoleRevoked(IBaseAsset::RoleRevoked {
            role: call.role,
            account: call.account,
            sender,
        }))
    }

    pub fn renounce_role(&mut self, sender: Address, call: IBaseAsset::renounceRoleCall) -> Result<()> {
        self.core.check_role_internal(sender, call.role)
            .map_err(|_| BasePrecompileError::BaseAsset(BaseAssetError::unauthorized()))?;
        self.core.revoke_role_internal(sender, call.role)?;
        self.emit_event(BaseAssetEvent::RoleRevoked(IBaseAsset::RoleRevoked {
            role: call.role,
            account: sender,
            sender,
        }))
    }

    pub fn check_role(&self, account: Address, role: B256) -> Result<()> {
        self.core.check_role_internal(account, role)
            .map_err(|_| BasePrecompileError::BaseAsset(BaseAssetError::unauthorized()))
    }
}

// ─────────────────────────────────────────────────── ERC-20 (delegates to core)

impl BaseAsset {
    pub fn total_supply(&self) -> Result<U256> { self.core.get_total_supply() }
    pub fn balance_of(&self, call: IBaseAsset::balanceOfCall) -> Result<U256> {
        self.core.get_balance(call.account)
    }
    pub fn allowance(&self, call: IBaseAsset::allowanceCall) -> Result<U256> {
        self.core.get_allowance(call.owner, call.spender)
    }

    pub fn approve(&mut self, sender: Address, call: IBaseAsset::approveCall) -> Result<bool> {
        self.core.set_allowance(sender, call.spender, call.amount)?;
        self.emit_event(BaseAssetEvent::Approval(IBaseAsset::Approval {
            owner: sender,
            spender: call.spender,
            amount: call.amount,
        }))?;
        Ok(true)
    }

    pub fn transfer(&mut self, sender: Address, call: IBaseAsset::transferCall) -> Result<bool> {
        self.move_balance(sender, call.to, call.amount, TransferKind::Transfer)?;
        Ok(true)
    }

    pub fn transfer_from(
        &mut self,
        sender: Address,
        call: IBaseAsset::transferFromCall,
    ) -> Result<bool> {
        self.core.consume_allowance(call.from, sender, call.amount)
            .map_err(|_| BasePrecompileError::BaseAsset(BaseAssetError::insufficient_allowance()))?;
        self.move_balance(call.from, call.to, call.amount, TransferKind::Transfer)?;
        Ok(true)
    }

    pub fn set_supply_cap(
        &mut self,
        sender: Address,
        call: IBaseAsset::setSupplyCapCall,
    ) -> Result<()> {
        self.check_role(sender, DEFAULT_ADMIN_ROLE)?;
        if call.newCap < self.core.get_total_supply()? {
            return Err(BasePrecompileError::under_overflow());
        }
        self.extra.supply_cap.write(call.newCap)?;
        self.emit_event(BaseAssetEvent::SupplyCapUpdate(IBaseAsset::SupplyCapUpdate {
            updater: sender,
            newSupplyCap: call.newCap,
        }))
    }
}

// ─────────────────────────────────────────────────── pause (delegates to core)

impl BaseAsset {
    pub fn paused(&self) -> Result<bool> { self.core.get_paused() }

    pub fn pause(&mut self, sender: Address, _call: IBaseAsset::pauseCall) -> Result<()> {
        self.check_role(sender, *PAUSER_ROLE)?;
        self.core.set_paused(true)?;
        self.emit_event(BaseAssetEvent::PauseStateUpdate(IBaseAsset::PauseStateUpdate {
            updater: sender,
            isPaused: true,
        }))
    }

    pub fn unpause(&mut self, sender: Address, _call: IBaseAsset::unpauseCall) -> Result<()> {
        self.check_role(sender, *PAUSER_ROLE)?;
        self.core.set_paused(false)?;
        self.emit_event(BaseAssetEvent::PauseStateUpdate(IBaseAsset::PauseStateUpdate {
            updater: sender,
            isPaused: false,
        }))
    }
}

// ─────────────────────────────────────────────────── permit (delegates to core)

impl BaseAsset {
    pub fn nonces(&self, call: IBaseAsset::noncesCall) -> Result<U256> {
        self.core.get_permit_nonce(call.owner)
    }

    pub fn domain_separator(&self) -> Result<B256> {
        self.core.compute_domain_separator(&self.core.name()?)
    }

    pub fn permit(&mut self, call: IBaseAsset::permitCall) -> Result<()> {
        if self.core.timestamp_u256() > call.deadline {
            return Err(BasePrecompileError::BaseAsset(BaseAssetError::permit_expired()));
        }
        let nonce = self.core.get_permit_nonce(call.owner)?;
        let domain_separator = self.domain_separator()?;
        self.core
            .verify_permit_sig(
                call.owner,
                call.spender,
                call.value,
                call.deadline,
                call.v,
                call.r,
                call.s,
                nonce,
                domain_separator,
            )
            .map_err(|_| BasePrecompileError::BaseAsset(BaseAssetError::invalid_signature()))?;
        self.core.increment_permit_nonce(call.owner)?;
        self.core.set_allowance(call.owner, call.spender, call.value)?;
        self.emit_event(BaseAssetEvent::Approval(IBaseAsset::Approval {
            owner: call.owner,
            spender: call.spender,
            amount: call.value,
        }))
    }
}

// ─────────────────────────────────────────────────── memo (delegates to core)

impl BaseAsset {
    pub fn transfer_with_memo(
        &mut self,
        sender: Address,
        call: IBaseAsset::transferWithMemoCall,
    ) -> Result<bool> {
        self.move_balance(sender, call.to, call.amount, TransferKind::Transfer)?;
        self.emit_memo(sender, call.to, call.amount, call.memo)?;
        Ok(true)
    }

    pub fn mint_with_memo(
        &mut self,
        sender: Address,
        call: IBaseAsset::mintWithMemoCall,
    ) -> Result<()> {
        self.check_role(sender, *ISSUER_ROLE)?;
        self.move_balance(Address::ZERO, call.to, call.amount, TransferKind::Mint)?;
        self.emit_event(BaseAssetEvent::Mint(IBaseAsset::Mint { to: call.to, amount: call.amount }))?;
        self.emit_memo(Address::ZERO, call.to, call.amount, call.memo)
    }

    pub fn burn_with_memo(
        &mut self,
        sender: Address,
        call: IBaseAsset::burnWithMemoCall,
    ) -> Result<()> {
        self.check_role(sender, *ISSUER_ROLE)?;
        self.move_balance(sender, Address::ZERO, call.amount, TransferKind::Burn)?;
        self.emit_event(BaseAssetEvent::Burn(IBaseAsset::Burn { from: sender, amount: call.amount }))?;
        self.emit_memo(sender, Address::ZERO, call.amount, call.memo)
    }

    fn emit_memo(
        &mut self,
        from: Address,
        to: Address,
        amount: U256,
        memo: B256,
    ) -> Result<()> {
        self.emit_event(BaseAssetEvent::TransferWithMemo(IBaseAsset::TransferWithMemo {
            from,
            to,
            amount,
            memo,
        }))
    }
}

// ─────────────────────────────────────────────────── balance pipeline

impl BaseAsset {
    /// BaseAsset pipeline: pause → recipient → supply cap → apply_balance_move → emit Transfer.
    /// No policy check — structurally absent from this class.
    pub(crate) fn move_balance(
        &mut self,
        from: Address,
        to: Address,
        amount: U256,
        kind: TransferKind,
    ) -> Result<()> {
        self.core
            .check_not_paused()
            .map_err(|_| BasePrecompileError::BaseAsset(BaseAssetError::contract_paused()))?;

        match kind {
            TransferKind::Transfer | TransferKind::Mint => {
                self.core
                    .validate_recipient_for(to, is_base_asset_prefix)
                    .map_err(|_| BasePrecompileError::BaseAsset(BaseAssetError::invalid_recipient()))?;
            }
            TransferKind::Burn => {}
        }

        // Optional supply cap check at mint
        if kind == TransferKind::Mint {
            let features = self.feature_set()?;
            if features.has(ASSET_SUPPLY_CAP) {
                let cap = self.extra.supply_cap.read()?;
                if cap > U256::ZERO {
                    let total = self.core.get_total_supply()?;
                    let new_total = total
                        .checked_add(amount)
                        .ok_or(BasePrecompileError::under_overflow())?;
                    if new_total > cap {
                        return Err(BasePrecompileError::BaseAsset(BaseAssetError::supply_cap_exceeded()));
                    }
                }
            }
        }

        self.core.apply_balance_move(from, to, amount, kind).map_err(|_| {
            BasePrecompileError::BaseAsset(BaseAssetError::insufficient_balance(
                U256::ZERO,
                amount,
            ))
        })?;

        self.emit_event(BaseAssetEvent::Transfer(IBaseAsset::Transfer { from, to, amount }))
    }
}

// ─────────────────────────────────────────────────── emit_event forwarding

impl BaseAsset {
    fn emit_event(&mut self, event: BaseAssetEvent) -> Result<()> {
        self.core.emit(event)
    }
}
