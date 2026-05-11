//! `TokenCore` — the shared base for all plan-2 token classes.
//!
//! Holds the common storage layout (slots 0–9) and implements all behavior that is
//! identical across BaseAsset, BaseSecurity, and BaseStablecoin: ERC-20 state
//! management, RBAC mechanics, pause, EIP-2612 permit, memo helpers, and the
//! balance-move primitive. TokenCore does NOT emit events — the calling class
//! emits its own typed events after calling into the core.

use std::sync::LazyLock;

use alloy::{
    primitives::{Address, B256, U256, keccak256},
    sol_types::SolValue,
};
use base_precompiles_macros::contract;

use alloy::sol_types::private::IntoLogData;

use crate::{
    error::{BasePrecompileError, Result},
    storage::{ContractStorage, Handler, Mapping},
};

// ────────────────────────────────────────────────────────── role constants

/// Root admin role (zero hash). Holds all role-admin privileges by default.
pub const DEFAULT_ADMIN_ROLE: B256 = B256::ZERO;

/// Self-administered sentinel that can never be granted externally. Used to
/// detect whether the roles tree has been initialized.
pub const UNGRANTABLE_ROLE: B256 = B256::new([0xff; 32]);

// ────────────────────────────────────────────────────────── permit constants

pub static PERMIT_TYPEHASH: LazyLock<B256> = LazyLock::new(|| {
    keccak256(b"Permit(address owner,address spender,uint256 value,uint256 nonce,uint256 deadline)")
});

pub static EIP712_DOMAIN_TYPEHASH: LazyLock<B256> = LazyLock::new(|| {
    keccak256(b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)")
});

pub static VERSION_HASH: LazyLock<B256> = LazyLock::new(|| keccak256(b"1"));

// ────────────────────────────────────────────────────────── TransferKind

/// Discriminator for the kind of balance movement used in the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    /// Standard ERC-20 transfer: debit `from`, credit `to`.
    Transfer,
    /// Mint: credit `to` only; `from` is `Address::ZERO`.
    Mint,
    /// Burn: debit `from` only; `to` is `Address::ZERO`.
    Burn,
}

// ────────────────────────────────────────────────────────── TokenCore struct

/// Common storage layout shared by all plan-2 token classes.
///
/// Slots 0–9 are identical for BaseAsset, BaseSecurity, and BaseStablecoin.
/// Class-specific slots (10+) are defined in each class's own `Extra` struct.
#[contract]
pub struct TokenCore {
    // 0-1: RBAC
    roles: Mapping<Address, Mapping<B256, bool>>,
    role_admins: Mapping<B256, B256>,

    // 2-4: metadata
    name: String,
    symbol: String,
    decimals: u8,

    // 5-9: ERC-20 state
    total_supply: U256,
    balances: Mapping<Address, U256>,
    allowances: Mapping<Address, Mapping<Address, U256>>,
    permit_nonces: Mapping<Address, U256>,
    paused: bool,
}

// ────────────────────────────────────────────────────────── dispatch helpers

impl TokenCore {
    /// Creates a new `TokenCore` handle at `address`. Public wrapper around macro-generated `__new`.
    pub fn new_at(address: Address) -> Self {
        Self::__new(address)
    }

    /// Marks this address as initialized (sets code). Public wrapper around macro-generated `__initialize`.
    pub fn mark_initialized(&mut self) -> crate::error::Result<()> {
        self.__initialize()
    }

    /// Emits a typed EVM event from this token's address. Public wrapper around private `emit_event`.
    pub fn emit<E: IntoLogData>(&mut self, event: E) -> crate::error::Result<()> {
        self.emit_event(event)
    }

    /// Charges calldata input gas. Used by class dispatchers since `storage` is private.
    pub fn charge_input(&mut self, calldata: &[u8]) -> Option<crate::PrecompileResult> {
        crate::charge_input_cost(&mut self.storage, calldata)
    }

    /// Converts an error into a reverted [`PrecompileResult`].
    pub fn err_result(
        &self,
        e: crate::error::BasePrecompileError,
    ) -> crate::PrecompileResult {
        self.storage.error_result(e)
    }

    /// Returns whether this contract address has been initialized.
    pub fn is_initialized(&self) -> crate::error::Result<bool> {
        self.storage.with_account_info(self.address, |info| Ok(!info.is_empty_code_hash()))
    }

    /// Returns the current block timestamp as `U256` for permit deadline comparisons.
    pub fn timestamp_u256(&self) -> U256 {
        U256::from(self.storage.timestamp())
    }
}

// ────────────────────────────────────────────────────────── core init

impl TokenCore {
    /// Writes all common metadata slots and sets up the RBAC tree.
    pub fn initialize_core(
        &mut self,
        msg_sender: Address,
        name: &str,
        symbol: &str,
        decimals: u8,
        admin: Address,
    ) -> Result<()> {
        self.name.write(name.to_string())?;
        self.symbol.write(symbol.to_string())?;
        self.decimals.write(decimals)?;
        self.initialize_roles_internal()?;
        self.grant_default_admin_internal(msg_sender, admin)
    }

    pub fn name(&self) -> Result<String> {
        self.name.read()
    }

    pub fn symbol(&self) -> Result<String> {
        self.symbol.read()
    }

    pub fn decimals(&self) -> Result<u8> {
        self.decimals.read()
    }
}

// ────────────────────────────────────────────────────────── RBAC

impl TokenCore {
    pub fn initialize_roles_internal(&mut self) -> Result<()> {
        self.set_role_admin_internal(UNGRANTABLE_ROLE, UNGRANTABLE_ROLE)
    }

    pub fn grant_default_admin_internal(
        &mut self,
        _msg_sender: Address,
        admin: Address,
    ) -> Result<()> {
        self.grant_role_internal(admin, DEFAULT_ADMIN_ROLE)
    }

    pub fn has_role_internal(&self, account: Address, role: B256) -> Result<bool> {
        self.roles[account][role].read()
    }

    pub fn grant_role_internal(&mut self, account: Address, role: B256) -> Result<()> {
        self.roles[account][role].write(true)
    }

    pub fn revoke_role_internal(&mut self, account: Address, role: B256) -> Result<()> {
        self.roles[account][role].write(false)
    }

    pub fn get_role_admin_internal(&self, role: B256) -> Result<B256> {
        self.role_admins[role].read()
    }

    pub fn set_role_admin_internal(&mut self, role: B256, admin_role: B256) -> Result<()> {
        self.role_admins[role].write(admin_role)
    }

    /// Reverts with `Panic(UnderOverflow)` (the shared "Unauthorized" sentinel) if
    /// `account` does not hold `role`. Callers remap this to their typed error.
    pub fn check_role_internal(&self, account: Address, role: B256) -> Result<()> {
        if !self.has_role_internal(account, role)? {
            return Err(BasePrecompileError::under_overflow());
        }
        Ok(())
    }
}

// ────────────────────────────────────────────────────────── ERC-20 state

impl TokenCore {
    pub fn get_total_supply(&self) -> Result<U256> {
        self.total_supply.read()
    }

    pub fn set_total_supply(&mut self, value: U256) -> Result<()> {
        self.total_supply.write(value)
    }

    pub fn get_balance(&self, account: Address) -> Result<U256> {
        self.balances[account].read()
    }

    pub fn set_balance(&mut self, account: Address, value: U256) -> Result<()> {
        self.balances[account].write(value)
    }

    pub fn get_allowance(&self, owner: Address, spender: Address) -> Result<U256> {
        self.allowances[owner][spender].read()
    }

    pub fn set_allowance(&mut self, owner: Address, spender: Address, value: U256) -> Result<()> {
        self.allowances[owner][spender].write(value)
    }

    /// Decrements the allowance from `owner` to `spender` by `amount`.
    /// `U256::MAX` allowances are treated as infinite and left unchanged.
    /// Returns `Err(under_overflow())` if `amount > allowed`.
    pub fn consume_allowance(
        &mut self,
        owner: Address,
        spender: Address,
        amount: U256,
    ) -> Result<()> {
        let allowed = self.get_allowance(owner, spender)?;
        if amount > allowed {
            return Err(BasePrecompileError::under_overflow());
        }
        if allowed != U256::MAX {
            let new_allowed =
                allowed.checked_sub(amount).ok_or(BasePrecompileError::under_overflow())?;
            self.set_allowance(owner, spender, new_allowed)?;
        }
        Ok(())
    }
}

// ────────────────────────────────────────────────────────── pause

impl TokenCore {
    pub fn get_paused(&self) -> Result<bool> {
        self.paused.read()
    }

    pub fn set_paused(&mut self, value: bool) -> Result<()> {
        self.paused.write(value)
    }

    /// Returns `Err(under_overflow())` (the shared "ContractPaused" sentinel) when paused.
    pub fn check_not_paused(&self) -> Result<()> {
        if self.get_paused()? {
            return Err(BasePrecompileError::under_overflow());
        }
        Ok(())
    }
}

// ────────────────────────────────────────────────────────── permit

impl TokenCore {
    pub fn get_permit_nonce(&self, owner: Address) -> Result<U256> {
        self.permit_nonces[owner].read()
    }

    pub fn increment_permit_nonce(&mut self, owner: Address) -> Result<()> {
        let nonce = self.permit_nonces[owner].read()?;
        let new_nonce =
            nonce.checked_add(U256::from(1)).ok_or(BasePrecompileError::under_overflow())?;
        self.permit_nonces[owner].write(new_nonce)
    }

    /// Computes the EIP-712 domain separator for a token with the given `name`.
    pub fn compute_domain_separator(&self, name: &str) -> Result<B256> {
        let name_hash = self.storage.keccak256(name.as_bytes())?;
        let chain_id = U256::from(self.storage.chain_id());
        let encoded =
            (*EIP712_DOMAIN_TYPEHASH, name_hash, *VERSION_HASH, chain_id, self.address)
                .abi_encode();
        self.storage.keccak256(&encoded)
    }

    /// Validates an EIP-2612 permit signature. Returns `Err(under_overflow())` on failure.
    /// Does NOT update the nonce or allowance — callers do that after verifying.
    pub fn verify_permit_sig(
        &self,
        owner: Address,
        spender: Address,
        value: U256,
        deadline: U256,
        v: u8,
        r: B256,
        s: B256,
        nonce: U256,
        domain_separator: B256,
    ) -> Result<()> {
        let struct_hash = self.storage.keccak256(
            &(*PERMIT_TYPEHASH, owner, spender, value, nonce, deadline).abi_encode(),
        )?;
        let digest = self.storage.keccak256(
            &[&[0x19, 0x01], domain_separator.as_slice(), struct_hash.as_slice()].concat(),
        )?;
        let recovered = self
            .storage
            .recover_signer(digest, v, r, s)?
            .ok_or(BasePrecompileError::under_overflow())?;
        if recovered != owner {
            return Err(BasePrecompileError::under_overflow());
        }
        Ok(())
    }
}

// ────────────────────────────────────────────────────────── recipient validation

impl TokenCore {
    /// Validates that `to` is not the zero address and not a token precompile address.
    /// `is_self_prefix` is provided by the calling class to check its own address range.
    /// Returns `Err(under_overflow())` on failure.
    pub fn validate_recipient_for(
        &self,
        to: Address,
        is_self_prefix: fn(&Address) -> bool,
    ) -> Result<()> {
        if to.is_zero() || is_self_prefix(&to) {
            return Err(BasePrecompileError::under_overflow());
        }
        Ok(())
    }
}

// ────────────────────────────────────────────────────────── balance pipeline primitive

impl TokenCore {
    /// Applies the debit/credit/supply math for a balance movement.
    /// Does NOT check pause, policy, supply cap, or holder count — those are
    /// the responsibility of the calling class.
    /// Does NOT emit any events — the calling class emits its own typed events.
    ///
    /// Returns `Err(under_overflow())` on arithmetic failure or insufficient balance.
    pub fn apply_balance_move(
        &mut self,
        from: Address,
        to: Address,
        amount: U256,
        kind: TransferKind,
    ) -> Result<()> {
        // Supply mutation
        match kind {
            TransferKind::Mint => {
                let total = self.get_total_supply()?;
                let new_total =
                    total.checked_add(amount).ok_or(BasePrecompileError::under_overflow())?;
                self.set_total_supply(new_total)?;
            }
            TransferKind::Burn => {
                let total = self.get_total_supply()?;
                let new_total =
                    total.checked_sub(amount).ok_or(BasePrecompileError::under_overflow())?;
                self.set_total_supply(new_total)?;
            }
            TransferKind::Transfer => {}
        }

        // Balance debit
        if matches!(kind, TransferKind::Transfer | TransferKind::Burn) {
            let from_balance = self.get_balance(from)?;
            if amount > from_balance {
                return Err(BasePrecompileError::under_overflow());
            }
            let new_from = from_balance
                .checked_sub(amount)
                .ok_or(BasePrecompileError::under_overflow())?;
            self.set_balance(from, new_from)?;
        }

        // Balance credit
        if matches!(kind, TransferKind::Transfer | TransferKind::Mint) {
            let to_balance = self.get_balance(to)?;
            let new_to =
                to_balance.checked_add(amount).ok_or(BasePrecompileError::under_overflow())?;
            self.set_balance(to, new_to)?;
        }

        Ok(())
    }
}
