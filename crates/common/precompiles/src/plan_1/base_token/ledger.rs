//! **Ledger domain** — the core money model.
//!
//! Owns the ERC-20 surface (`transfer`, `transferFrom`, `approve`, balance + allowance
//! reads, `totalSupply`) and the single transfer pipeline ([`BaseToken::move_balance`])
//! through which **every** balance / supply mutation flows — including those originated
//! by the supply domain (`mint` / `burn`) and the memo decorator.
//!
//! Pipeline branches read the per-token [`FeatureSet`](super::FeatureSet) and consult
//! sibling domains (`lifecycle::check_not_paused`, `compliance::ensure_transfer_authorized`)
//! when the corresponding feature is on. The ledger does not own those checks — it owns
//! the *order* in which they run.

use alloy::primitives::{Address, U256};
use base_precompiles_contracts::{BaseTokenError, BaseTokenEvent, IBaseToken};

use crate::{
    base_token::{BaseToken, Feature},
    error::{BasePrecompileError, Result},
    storage::Handler,
};

/// Discriminator for the kind of balance movement. The pipeline uses this to skip the
/// debit (mint) or the credit (burn) branch and to enforce policy semantics correctly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    /// Standard transfer: debit `from`, credit `to`.
    Transfer,
    /// Mint: credit `to`, no debit. `from` MUST be `Address::ZERO`.
    Mint,
    /// Burn: debit `from`, no credit. `to` MUST be `Address::ZERO`.
    Burn,
}

impl BaseToken {
    // ────────────────────────────────────────────────────────────── ERC-20 reads

    /// ERC-20 `totalSupply`.
    pub fn total_supply(&self) -> Result<U256> {
        self.total_supply.read()
    }

    /// ERC-20 `balanceOf(account)`.
    pub fn balance_of(&self, call: IBaseToken::balanceOfCall) -> Result<U256> {
        self.balances[call.account].read()
    }

    /// ERC-20 `allowance(owner, spender)`.
    pub fn allowance(&self, call: IBaseToken::allowanceCall) -> Result<U256> {
        self.allowances[call.owner][call.spender].read()
    }

    // ────────────────────────────────────────────────────────────── ERC-20 writes

    /// ERC-20 `approve(spender, amount)`.
    pub fn approve(&mut self, msg_sender: Address, call: IBaseToken::approveCall) -> Result<bool> {
        self.allowances[msg_sender][call.spender].write(call.amount)?;
        self.emit_event(BaseTokenEvent::Approval(IBaseToken::Approval {
            owner: msg_sender,
            spender: call.spender,
            amount: call.amount,
        }))?;
        Ok(true)
    }

    /// ERC-20 `transfer(to, amount)`.
    pub fn transfer(
        &mut self,
        msg_sender: Address,
        call: IBaseToken::transferCall,
    ) -> Result<bool> {
        self.move_balance(msg_sender, call.to, call.amount, TransferKind::Transfer)?;
        Ok(true)
    }

    /// ERC-20 `transferFrom(from, to, amount)`. Decrements caller allowance unless it
    /// was set to `U256::MAX` (the "infinite approval" convention).
    pub fn transfer_from(
        &mut self,
        msg_sender: Address,
        call: IBaseToken::transferFromCall,
    ) -> Result<bool> {
        self.consume_allowance(call.from, msg_sender, call.amount)?;
        self.move_balance(call.from, call.to, call.amount, TransferKind::Transfer)?;
        Ok(true)
    }

    /// Internal helper: deducts `amount` from `owner→spender` allowance, leaving
    /// `U256::MAX` allowances unchanged.
    pub(super) fn consume_allowance(
        &mut self,
        owner: Address,
        spender: Address,
        amount: U256,
    ) -> Result<()> {
        let allowed = self.allowances[owner][spender].read()?;
        if amount > allowed {
            return Err(BaseTokenError::insufficient_allowance().into());
        }
        if allowed != U256::MAX {
            let new_allowed = allowed
                .checked_sub(amount)
                .ok_or(BaseTokenError::insufficient_allowance())?;
            self.allowances[owner][spender].write(new_allowed)?;
        }
        Ok(())
    }

    // ────────────────────────────────────────────────────────────── recipient validation

    /// Validates a recipient address: not zero, not a BaseToken precompile address.
    /// Used by transfer / mint paths; burn intentionally skips this (its `to` is zero).
    #[inline]
    pub(super) fn validate_recipient(&self, to: Address) -> Result<()> {
        if to.is_zero() || crate::address::is_base_token_prefix(&to) {
            return Err(BaseTokenError::invalid_recipient().into());
        }
        Ok(())
    }

    // ────────────────────────────────────────────────────────────── transfer pipeline

    /// Single source of truth for balance + supply movement.
    ///
    /// Branch order (audited):
    ///   1. `Pause`  guard if enabled — delegated to lifecycle domain
    ///   2. `Policy` guard if enabled — delegated to compliance domain
    ///      (skipped on burn — burner already holds the tokens; mint is gated by the
    ///      recipient check)
    ///   3. recipient validation (transfer/mint only)
    ///   4. supply update (mint/burn)
    ///   5. balance debit (transfer/burn)
    ///   6. balance credit (transfer/mint)
    ///   7. emit `Transfer(from, to, amount)`
    pub(super) fn move_balance(
        &mut self,
        from: Address,
        to: Address,
        amount: U256,
        kind: TransferKind,
    ) -> Result<()> {
        let features = self.feature_set()?;

        if features.contains(Feature::Pause) {
            self.check_not_paused()?;
        }

        match kind {
            TransferKind::Transfer | TransferKind::Mint => self.validate_recipient(to)?,
            TransferKind::Burn => {}
        }

        if features.contains(Feature::Policy) && kind != TransferKind::Burn {
            // For mint, `from` is zero — the registry treats zero as the protocol-side
            // mint origin; the recipient check still gates on the active policy.
            self.ensure_transfer_authorized(from, to)?;
        }

        match kind {
            TransferKind::Mint => {
                let total = self.total_supply.read()?;
                let new_total =
                    total.checked_add(amount).ok_or(BasePrecompileError::under_overflow())?;
                self.total_supply.write(new_total)?;
            }
            TransferKind::Burn => {
                let total = self.total_supply.read()?;
                let new_total = total.checked_sub(amount).ok_or(
                    BaseTokenError::insufficient_balance(total, amount),
                )?;
                self.total_supply.write(new_total)?;
            }
            TransferKind::Transfer => {}
        }

        if matches!(kind, TransferKind::Transfer | TransferKind::Burn) {
            let from_balance = self.balances[from].read()?;
            if amount > from_balance {
                return Err(BaseTokenError::insufficient_balance(from_balance, amount).into());
            }
            let new_from = from_balance
                .checked_sub(amount)
                .ok_or(BasePrecompileError::under_overflow())?;
            self.balances[from].write(new_from)?;
        }

        if matches!(kind, TransferKind::Transfer | TransferKind::Mint) {
            let to_balance = self.balances[to].read()?;
            let new_to = to_balance
                .checked_add(amount)
                .ok_or(BasePrecompileError::under_overflow())?;
            self.balances[to].write(new_to)?;
        }

        self.emit_event(BaseTokenEvent::Transfer(IBaseToken::Transfer { from, to, amount }))
    }
}
