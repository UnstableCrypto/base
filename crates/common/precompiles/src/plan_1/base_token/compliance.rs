//! **Compliance domain** — the integration boundary to the policy registry.
//!
//! This is the only file in the token that talks to a sibling precompile. Two concerns
//! live here:
//!
//! 1. **Policy administration on the token** — `policyId()` / `setPolicyId(...)` —
//!    governance-style writes to the per-token "which policy applies" pointer.
//!    Authorization rule: `POLICY_ADMIN_ROLE`.
//! 2. **The cross-precompile authorization call** — [`BaseToken::ensure_transfer_authorized`]
//!    invoked from the ledger pipeline when `Feature::Policy` is enabled.
//!
//! Keeping both in one file makes the **anti-corruption boundary** explicit: every
//! interaction with `BaseTokenPolicyRegistry` originates from this file. Readers
//! auditing how the token relates to compliance only need to read `compliance.rs`.

use alloy::primitives::Address;
use base_precompiles_contracts::{BaseTokenError, BaseTokenEvent, IBaseToken};

use crate::{
    base_token::{BaseToken, authz::POLICY_ADMIN_ROLE},
    base_token_policy_registry::BaseTokenPolicyRegistry,
    error::Result,
    storage::Handler,
};

impl BaseToken {
    // ────────────────────────────────────────────────────────────── policy admin

    /// Returns the active transfer policy id for this token.
    pub fn policy_id(&self) -> Result<u64> {
        self.policy_id.read()
    }

    /// Sets the active transfer policy id. Caller must hold `POLICY_ADMIN_ROLE`. The
    /// referenced policy must exist in the registry.
    pub fn set_policy_id(
        &mut self,
        msg_sender: Address,
        call: IBaseToken::setPolicyIdCall,
    ) -> Result<()> {
        self.check_role(msg_sender, *POLICY_ADMIN_ROLE)?;
        if !BaseTokenPolicyRegistry::new().policy_exists_internal(call.newPolicyId)? {
            return Err(BaseTokenError::invalid_policy_id().into());
        }
        self.policy_id.write(call.newPolicyId)?;
        self.emit_event(BaseTokenEvent::PolicyIdUpdate(IBaseToken::PolicyIdUpdate {
            updater: msg_sender,
            newPolicyId: call.newPolicyId,
        }))
    }

    // ────────────────────────────────────────────────────────────── registry boundary

    /// Reads the active policy id and asks the registry whether `(from, to)` is
    /// allowed. Called from the ledger pipeline only when `Feature::Policy` is enabled.
    /// Short-circuits cheaply when the active policy is the universal allow-all sentinel.
    pub(super) fn ensure_transfer_authorized(&self, from: Address, to: Address) -> Result<()> {
        let policy_id = self.policy_id.read()?;
        if !BaseTokenPolicyRegistry::new().is_authorized_internal(policy_id, from, to)? {
            return Err(BaseTokenError::policy_forbids().into());
        }
        Ok(())
    }
}
