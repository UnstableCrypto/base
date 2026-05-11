//! ABI dispatch for [`BaseAsset`].
//!
//! Structurally absent: policy selectors, burnBlocked, forceTransfer, currency, holderLimit.

use alloy::{primitives::Address, sol_types::SolInterface};
use base_precompiles_contracts::{
    BaseAssetError,
    IBaseAsset::{self, IBaseAssetCalls},
};
use revm::precompile::PrecompileResult;

use crate::{
    Precompile, dispatch_call, metadata, mutate, mutate_void, view,
    error::BasePrecompileError,
    plan_2::token::base_asset::{ASSET_MEMO, ASSET_SUPPLY_CAP, ASSET_SUPPLY_CONTROL, BaseAsset},
    storage::ContractStorage,
};

macro_rules! gate {
    ($self:ident, $bit:expr) => {
        if !match $self.feature_set() {
            Ok(fs) => fs.has($bit),
            Err(e) => return $self.core.err_result(e),
        } {
            return $self.core.err_result(BasePrecompileError::BaseAsset(
                BaseAssetError::feature_not_enabled($bit),
            ));
        }
    };
}

impl Precompile for BaseAsset {
    fn call(&mut self, calldata: &[u8], msg_sender: Address) -> PrecompileResult {
        if let Some(err) = self.core.charge_input(calldata) {
            return err;
        }

        let initialized = match self.is_initialized() {
            Ok(v) => v,
            Err(e) => return self.core.err_result(e),
        };
        if !initialized {
            return self.core.err_result(BasePrecompileError::BaseAsset(
                BaseAssetError::uninitialized(),
            ));
        }

        dispatch_call(calldata, &[], IBaseAssetCalls::abi_decode, |call| match call {
            IBaseAssetCalls::name(_) => metadata::<IBaseAsset::nameCall>(|| self.name()),
            IBaseAssetCalls::symbol(_) => metadata::<IBaseAsset::symbolCall>(|| self.symbol()),
            IBaseAssetCalls::decimals(_) => metadata::<IBaseAsset::decimalsCall>(|| self.decimals()),
            IBaseAssetCalls::totalSupply(_) => metadata::<IBaseAsset::totalSupplyCall>(|| self.total_supply()),
            IBaseAssetCalls::features(_) => metadata::<IBaseAsset::featuresCall>(|| self.features_raw()),
            IBaseAssetCalls::assetClass(_) => metadata::<IBaseAsset::assetClassCall>(|| self.asset_class()),
            IBaseAssetCalls::ISSUER_ROLE(c) => view(c, |_| Ok(Self::issuer_role())),
            IBaseAssetCalls::PAUSER_ROLE(c) => view(c, |_| Ok(Self::pauser_role())),
            IBaseAssetCalls::balanceOf(c) => view(c, |c| self.balance_of(c)),
            IBaseAssetCalls::allowance(c) => view(c, |c| self.allowance(c)),
            IBaseAssetCalls::approve(c) => mutate(c, msg_sender, |s, c| self.approve(s, c)),
            IBaseAssetCalls::transfer(c) => mutate(c, msg_sender, |s, c| self.transfer(s, c)),
            IBaseAssetCalls::transferFrom(c) => mutate(c, msg_sender, |s, c| self.transfer_from(s, c)),
            IBaseAssetCalls::mint(c) => {
                gate!(self, ASSET_SUPPLY_CONTROL);
                mutate_void(c, msg_sender, |s, c| self.mint(s, c))
            }
            IBaseAssetCalls::burn(c) => {
                gate!(self, ASSET_SUPPLY_CONTROL);
                mutate_void(c, msg_sender, |s, c| self.burn(s, c))
            }
            IBaseAssetCalls::supplyCap(_) => {
                gate!(self, ASSET_SUPPLY_CAP);
                metadata::<IBaseAsset::supplyCapCall>(|| self.supply_cap())
            }
            IBaseAssetCalls::setSupplyCap(c) => {
                gate!(self, ASSET_SUPPLY_CAP);
                mutate_void(c, msg_sender, |s, c| self.set_supply_cap(s, c))
            }
            IBaseAssetCalls::paused(_) => metadata::<IBaseAsset::pausedCall>(|| self.paused()),
            IBaseAssetCalls::pause(c) => mutate_void(c, msg_sender, |s, c| self.pause(s, c)),
            IBaseAssetCalls::unpause(c) => mutate_void(c, msg_sender, |s, c| self.unpause(s, c)),
            IBaseAssetCalls::transferWithMemo(c) => {
                gate!(self, ASSET_MEMO);
                mutate(c, msg_sender, |s, c| self.transfer_with_memo(s, c))
            }
            IBaseAssetCalls::mintWithMemo(c) => {
                gate!(self, ASSET_SUPPLY_CONTROL);
                gate!(self, ASSET_MEMO);
                mutate_void(c, msg_sender, |s, c| self.mint_with_memo(s, c))
            }
            IBaseAssetCalls::burnWithMemo(c) => {
                gate!(self, ASSET_SUPPLY_CONTROL);
                gate!(self, ASSET_MEMO);
                mutate_void(c, msg_sender, |s, c| self.burn_with_memo(s, c))
            }
            IBaseAssetCalls::permit(c) => mutate_void(c, msg_sender, |_s, c| self.permit(c)),
            IBaseAssetCalls::nonces(c) => view(c, |c| self.nonces(c)),
            IBaseAssetCalls::DOMAIN_SEPARATOR(c) => view(c, |_| self.domain_separator()),
            IBaseAssetCalls::hasRole(c) => view(c, |c| self.has_role(c)),
            IBaseAssetCalls::grantRole(c) => mutate_void(c, msg_sender, |s, c| self.grant_role(s, c)),
            IBaseAssetCalls::revokeRole(c) => mutate_void(c, msg_sender, |s, c| self.revoke_role(s, c)),
            IBaseAssetCalls::renounceRole(c) => mutate_void(c, msg_sender, |s, c| self.renounce_role(s, c)),
        })
    }
}
