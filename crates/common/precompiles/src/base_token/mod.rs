//! `BaseToken` — sibling per-token precompile to [`B20Token`](crate::b20::B20Token).
//!
//! Plan-2 implementation of "one global token standard" delivered as a native precompile.
//! Each token is a distinct precompile instance addressed at `0xBA5E_PREFIX || keccak256(deployer, salt)[..8]`.
//! Per-token features (supply, lifecycle, permit, memo, policy) are opted into via an
//! immutable `FeatureSet` bitmap chosen by the issuer at `createToken` time.
//!
//! # Module layout (DDD bounded contexts)
//!
//! Each file is a domain — a coherent slice of token behavior with its own invariants.
//! New methods land in the file matching their domain; new files appear only when a
//! genuinely new domain emerges.
//!
//! - [`mod`] (this file) — the **aggregate root**: storage layout + lifecycle (`initialize`,
//!   `from_address`). No business logic.
//! - [`ledger`] — core money model: balances, allowances, ERC-20 surface, the
//!   single transfer pipeline (`move_balance`).
//! - [`supply`] — issuance and destruction (`mint`, `burn`).
//! - [`lifecycle`] — operational state of the token (currently: pause).
//! - [`compliance`] — the boundary to `BaseTokenPolicyRegistry`: `setPolicyId` plus
//!   the cross-precompile `ensure_transfer_authorized` call.
//! - [`permit`] — EIP-2612 signature-based authentication.
//! - [`memo`] — cross-cutting decoration adding a 32-byte memo to ledger / supply ops.
//! - [`authz`] — RBAC plumbing: role constants + `check_role`.
//! - [`features`] — `Feature` enum + `FeatureSet` + `ensure_features` dispatch helper.
//! - [`dispatch`] — ABI routing table; the only file that names selectors.
//!
//! # Where do gates live?
//!
//! - **Feature gates** at the dispatch arm (issuer-configured concern → routing layer).
//! - **Role gates** inside the domain (domain rule → domain code).
//!
//! Don't move them.

pub mod dispatch;

pub mod authz;
pub use authz::{
    BURNER_ROLE, DEFAULT_ADMIN_ROLE, ISSUER_ROLE, PAUSER_ROLE, POLICY_ADMIN_ROLE, UNGRANTABLE_ROLE,
};

pub mod compliance;

pub mod features;
pub use features::{Feature, FeatureSet};

pub mod ledger;
pub use ledger::TransferKind;

pub mod lifecycle;

pub mod memo;

pub mod permit;
pub use permit::{EIP712_DOMAIN_TYPEHASH, PERMIT_TYPEHASH, VERSION_HASH};

pub mod supply;

use alloy::primitives::{Address, B256, U256};
pub use base_precompiles_contracts::{BaseTokenError, BaseTokenEvent, IBaseToken};
use base_precompiles_macros::contract;
// Re-export the generated slot-constants module so external tests can reference layout.
pub use slots as base_token_slots;

use crate::{
    BaseBAddressExt, base_token_policy_registry::ALLOW_ALL_POLICY_ID, error::Result,
    storage::{Handler, Mapping},
};

/// `BaseToken` storage layout. **Append-only forever.** Never reorder, never retype.
/// Deprecated fields stay (mark with `_` prefix); reuse is forbidden — it would corrupt
/// every existing token's state. Slots `15..=19` are reserved for future fork additions.
#[contract]
pub struct BaseToken {
    // 0  metadata
    name: String,
    // 1
    symbol: String,
    // 2
    decimals: u8,

    // 3..6  ERC-20 state
    total_supply: U256,
    balances: Mapping<Address, U256>,
    allowances: Mapping<Address, Mapping<Address, U256>>,
    permit_nonces: Mapping<Address, U256>,

    // 7  pause flag (only meaningful when Feature::Pause is enabled)
    paused: bool,

    // 8  active transfer policy id (only meaningful when Feature::Policy is enabled)
    policy_id: u64,

    // 9, 10  RBAC
    roles: Mapping<Address, Mapping<B256, bool>>,
    role_admins: Mapping<B256, B256>,

    // 11  immutable per-token feature bitmap, set once at initialize()
    features: u64,
    // 12  per-feature config blob (e.g. supply cap value when SupplyCap is added)
    feature_config: Mapping<u64, B256>,
}

impl BaseToken {
    /// Creates a `BaseToken` handle from a raw address. Errors if the address does not
    /// carry the [`BASE_TOKEN_PREFIX_BYTES`](crate::address::BASE_TOKEN_PREFIX_BYTES).
    pub fn from_address(address: Address) -> Result<Self> {
        if !address.is_base_token() {
            return Err(BaseTokenError::invalid_token().into());
        }
        Ok(Self::__new(address))
    }

    /// Creates a `BaseToken` handle without prefix validation.
    ///
    /// # Safety
    /// Caller must guarantee `address.is_base_token()` is true.
    #[inline]
    pub fn from_address_unchecked(address: Address) -> Self {
        debug_assert!(address.is_base_token(), "address must have BaseToken prefix");
        Self::__new(address)
    }

    /// One-shot initialization called by `BaseTokenFactory::create_token`. Writes
    /// metadata, persists the immutable feature bitmap, sets `policy_id = ALLOW_ALL`,
    /// and grants `DEFAULT_ADMIN_ROLE` to `admin`.
    pub fn initialize(
        &mut self,
        msg_sender: Address,
        name: &str,
        symbol: &str,
        decimals: u8,
        admin: Address,
        features: u64,
    ) -> Result<()> {
        self.__initialize()?;

        self.name.write(name.to_string())?;
        self.symbol.write(symbol.to_string())?;
        self.decimals.write(decimals)?;
        self.features.write(features)?;
        self.policy_id.write(ALLOW_ALL_POLICY_ID)?;

        self.initialize_roles()?;
        self.grant_default_admin(msg_sender, admin)
    }

    /// Returns the token name.
    pub fn name(&self) -> Result<String> {
        self.name.read()
    }

    /// Returns the token symbol.
    pub fn symbol(&self) -> Result<String> {
        self.symbol.read()
    }

    /// Returns the token decimals.
    pub fn decimals(&self) -> Result<u8> {
        self.decimals.read()
    }

    /// Returns the per-token feature bitmap.
    pub fn features(&self) -> Result<u64> {
        self.features.read()
    }

    /// Loads the [`FeatureSet`] from storage.
    #[inline]
    pub fn feature_set(&self) -> Result<FeatureSet> {
        Ok(FeatureSet::new(self.features()?))
    }
}

#[cfg(test)]
mod tests;
