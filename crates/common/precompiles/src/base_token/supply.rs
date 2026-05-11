//! **Supply domain** — issuance and destruction of token units.
//!
//! Owns `mint(to, amount)` and `burn(from, amount)`. Both operate on the same
//! aggregate state (`total_supply`) and funnel through the ledger pipeline. Authorization
//! is a domain rule (issuance requires `ISSUER_ROLE`; destruction requires `BURNER_ROLE`)
//! and lives here, not at dispatch — dispatch only knows the *feature gate* (whether the
//! token enabled mint / burn at all).
//!
//! Memo-bearing variants live in `memo.rs` because they cross the supply and ledger
//! domains with an additional event decoration.

use alloy::primitives::Address;
use base_precompiles_contracts::{BaseTokenEvent, IBaseToken};

use crate::{
    base_token::{BaseToken, TransferKind, authz::BURNER_ROLE, authz::ISSUER_ROLE},
    error::Result,
};

impl BaseToken {
    /// Mints `amount` tokens to `to`. Caller must hold `ISSUER_ROLE`.
    pub fn mint(&mut self, msg_sender: Address, call: IBaseToken::mintCall) -> Result<()> {
        self.check_role(msg_sender, *ISSUER_ROLE)?;
        self.move_balance(Address::ZERO, call.to, call.amount, TransferKind::Mint)?;
        self.emit_event(BaseTokenEvent::Mint(IBaseToken::Mint {
            to: call.to,
            amount: call.amount,
        }))
    }

    /// Burns `amount` tokens from `from`. Caller must hold `BURNER_ROLE`.
    pub fn burn(&mut self, msg_sender: Address, call: IBaseToken::burnCall) -> Result<()> {
        self.check_role(msg_sender, *BURNER_ROLE)?;
        self.move_balance(call.from, Address::ZERO, call.amount, TransferKind::Burn)?;
        self.emit_event(BaseTokenEvent::Burn(IBaseToken::Burn {
            from: call.from,
            amount: call.amount,
        }))
    }
}
