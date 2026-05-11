//! Per-token `FeatureSet` bitmap.
//!
//! Each `BaseToken` instance is configured at `createToken` time with a `u64` bitmap of
//! enabled features. Feature-specific selectors (`mint`, `burn`, `pause`, `permit`, …)
//! check the bitmap at the dispatch site and revert with `FeatureNotEnabled` if the
//! issuer did not opt in. The transfer pipeline branches on the same bitmap.
//!
//! The enum is **append-only**: never reorder bits, never reuse retired bits. New
//! features are added in future hardforks at the next free bit position.

use base_precompiles_contracts::BaseTokenError;
use revm::precompile::PrecompileResult;

use crate::{
    base_token::BaseToken,
    error::{BasePrecompileError, Result},
    storage::ContractStorage,
};

/// Per-token feature flags. Each variant is a single bit position in the [`FeatureSet`]
/// bitmap stored on the token. Bit positions are part of the on-chain ABI — never reorder,
/// never reuse.
#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    /// `mint(...)` callable by holders of `ISSUER_ROLE`.
    Mint = 1 << 0,
    /// `burn(...)` callable by holders of `BURNER_ROLE`.
    Burn = 1 << 1,
    /// `pause()` / `unpause()` callable by holders of `PAUSER_ROLE`. When enabled, the
    /// transfer pipeline checks `paused` before mutating balances.
    Pause = 1 << 2,
    /// `permit(...)` / `nonces(...)` / `DOMAIN_SEPARATOR()` (EIP-2612) become callable.
    Permit = 1 << 3,
    /// `transferWithMemo` / `mintWithMemo` / `burnWithMemo` overloads become callable.
    Memo = 1 << 4,
    /// Transfer-policy enforcement: the pipeline calls into the policy registry to
    /// authorize sender + recipient. When disabled, transfers skip the cross-precompile
    /// hop entirely.
    Policy = 1 << 5,
}

impl Feature {
    /// Bitmap of all features defined as of the current binary. Used by the factory to
    /// reject `createToken` calls that set unknown feature bits.
    pub const ALL_KNOWN: u64 = (Feature::Mint as u64)
        | (Feature::Burn as u64)
        | (Feature::Pause as u64)
        | (Feature::Permit as u64)
        | (Feature::Memo as u64)
        | (Feature::Policy as u64);
}

/// Newtype wrapper around the `u64` feature bitmap stored on a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureSet(pub u64);

impl FeatureSet {
    /// Wraps a raw bitmap.
    #[inline]
    pub const fn new(bits: u64) -> Self {
        Self(bits)
    }

    /// Returns `true` if `feature` is enabled in this set.
    #[inline]
    pub const fn contains(&self, feature: Feature) -> bool {
        (self.0 & feature as u64) != 0
    }

    /// Returns the raw bitmap.
    #[inline]
    pub const fn bits(&self) -> u64 {
        self.0
    }

    /// Reverts with [`BaseTokenError::FeatureNotEnabled`] if `feature` is not set.
    #[inline]
    pub fn ensure(&self, feature: Feature) -> Result<()> {
        if self.contains(feature) {
            Ok(())
        } else {
            Err(BasePrecompileError::BaseToken(BaseTokenError::feature_not_enabled(feature as u64)))
        }
    }
}

impl BaseToken {
    /// Dispatch helper. Loads the per-token [`FeatureSet`] and verifies every flag in
    /// `required` is set; on the first failure returns the reverted [`PrecompileResult`]
    /// so the dispatch arm can short-circuit with `if let Some(err) = ... { return err; }`.
    ///
    /// Why a helper and not `?`: dispatch arm bodies must produce `PrecompileResult`,
    /// and `BasePrecompileError` does not auto-convert. This keeps each arm's gate a
    /// single visible line.
    #[inline]
    pub(super) fn ensure_features(&self, required: &[Feature]) -> Option<PrecompileResult> {
        let set = match self.feature_set() {
            Ok(s) => s,
            Err(e) => return Some(self.storage().error_result(e)),
        };
        for feature in required {
            if let Err(e) = set.ensure(*feature) {
                return Some(self.storage().error_result(e));
            }
        }
        None
    }
}
