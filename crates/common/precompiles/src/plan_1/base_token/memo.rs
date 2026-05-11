//! Memo overloads of transfer / mint / burn.
//!
//! Gated by `Feature::Memo` at dispatch. Each function performs the same balance work
//! as its non-memo sibling, then emits an additional `TransferWithMemo` event carrying
//! the 32-byte memo. The memo is never persisted — only emitted as an indexed log topic.

use alloy::primitives::{Address, B256, U256};
use base_precompiles_contracts::{BaseTokenEvent, IBaseToken};

use crate::{
    base_token::{BaseToken, TransferKind, authz::BURNER_ROLE, authz::ISSUER_ROLE},
    error::Result,
};

impl BaseToken {
    /// `transferWithMemo`.
    pub fn transfer_with_memo(
        &mut self,
        msg_sender: Address,
        call: IBaseToken::transferWithMemoCall,
    ) -> Result<bool> {
        self.move_balance(msg_sender, call.to, call.amount, TransferKind::Transfer)?;
        self.emit_memo(msg_sender, call.to, call.amount, call.memo)?;
        Ok(true)
    }

    /// `mintWithMemo`. Caller must hold `ISSUER_ROLE`.
    pub fn mint_with_memo(
        &mut self,
        msg_sender: Address,
        call: IBaseToken::mintWithMemoCall,
    ) -> Result<()> {
        self.check_role(msg_sender, *ISSUER_ROLE)?;
        self.move_balance(Address::ZERO, call.to, call.amount, TransferKind::Mint)?;
        self.emit_event(BaseTokenEvent::Mint(IBaseToken::Mint {
            to: call.to,
            amount: call.amount,
        }))?;
        self.emit_memo(Address::ZERO, call.to, call.amount, call.memo)
    }

    /// `burnWithMemo`. Caller must hold `BURNER_ROLE`.
    pub fn burn_with_memo(
        &mut self,
        msg_sender: Address,
        call: IBaseToken::burnWithMemoCall,
    ) -> Result<()> {
        self.check_role(msg_sender, *BURNER_ROLE)?;
        self.move_balance(call.from, Address::ZERO, call.amount, TransferKind::Burn)?;
        self.emit_event(BaseTokenEvent::Burn(IBaseToken::Burn {
            from: call.from,
            amount: call.amount,
        }))?;
        self.emit_memo(call.from, Address::ZERO, call.amount, call.memo)
    }

    fn emit_memo(&mut self, from: Address, to: Address, amount: U256, memo: B256) -> Result<()> {
        self.emit_event(BaseTokenEvent::TransferWithMemo(IBaseToken::TransferWithMemo {
            from,
            to,
            amount,
            memo,
        }))
    }
}
