//! Mint and burn for BaseAsset. Gated by ASSET_SUPPLY_CONTROL at dispatch.

use alloy::primitives::Address;
use base_precompiles_contracts::{BaseAssetEvent, IBaseAsset};

use crate::{
    error::Result,
    plan_2::{
        shared::TransferKind,
        token::base_asset::{BaseAsset, ISSUER_ROLE},
    },
};

impl BaseAsset {
    pub fn mint(&mut self, sender: Address, call: IBaseAsset::mintCall) -> Result<()> {
        self.check_role(sender, *ISSUER_ROLE)?;
        self.move_balance(Address::ZERO, call.to, call.amount, TransferKind::Mint)?;
        self.emit_event(BaseAssetEvent::Mint(IBaseAsset::Mint { to: call.to, amount: call.amount }))
    }

    pub fn burn(&mut self, sender: Address, call: IBaseAsset::burnCall) -> Result<()> {
        self.check_role(sender, *ISSUER_ROLE)?;
        self.move_balance(sender, Address::ZERO, call.amount, TransferKind::Burn)?;
        self.emit_event(BaseAssetEvent::Burn(IBaseAsset::Burn { from: sender, amount: call.amount }))
    }
}
