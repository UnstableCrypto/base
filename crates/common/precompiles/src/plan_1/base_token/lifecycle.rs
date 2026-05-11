//! **Lifecycle domain** — operational state of the token (currently: pause).
//!
//! Owns the pause flag and its read/write surface. The transfer pipeline calls
//! [`BaseToken::check_not_paused`] when `Feature::Pause` is enabled — the pause
//! invariant is enforced *by* the ledger, but *defined* here. This file is the place
//! to look when adding any future operational-state concept (e.g. emergency-shutdown,
//! migration-frozen) that gates behavior across multiple domains.

use alloy::primitives::Address;
use base_precompiles_contracts::{BaseTokenError, BaseTokenEvent, IBaseToken};

use crate::{
    base_token::{BaseToken, authz::PAUSER_ROLE},
    error::Result,
    storage::Handler,
};

impl BaseToken {
    /// Returns whether the token is currently paused.
    pub fn paused(&self) -> Result<bool> {
        self.paused.read()
    }

    /// Pauses the token. Caller must hold `PAUSER_ROLE`.
    pub fn pause(&mut self, msg_sender: Address, _call: IBaseToken::pauseCall) -> Result<()> {
        self.check_role(msg_sender, *PAUSER_ROLE)?;
        self.paused.write(true)?;
        self.emit_event(BaseTokenEvent::PauseStateUpdate(IBaseToken::PauseStateUpdate {
            updater: msg_sender,
            isPaused: true,
        }))
    }

    /// Unpauses the token. Caller must hold `PAUSER_ROLE`.
    pub fn unpause(&mut self, msg_sender: Address, _call: IBaseToken::unpauseCall) -> Result<()> {
        self.check_role(msg_sender, *PAUSER_ROLE)?;
        self.paused.write(false)?;
        self.emit_event(BaseTokenEvent::PauseStateUpdate(IBaseToken::PauseStateUpdate {
            updater: msg_sender,
            isPaused: false,
        }))
    }

    /// Reverts with `ContractPaused` if the token is paused. Called from the ledger
    /// pipeline only when `Feature::Pause` is enabled.
    pub(super) fn check_not_paused(&self) -> Result<()> {
        if self.paused()? {
            return Err(BaseTokenError::contract_paused().into());
        }
        Ok(())
    }
}
