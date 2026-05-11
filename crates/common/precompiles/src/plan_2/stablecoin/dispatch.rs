//! ABI dispatch for [`BaseStablecoin`].
//! Structurally absent: supplyCap, setSupplyCap, forceTransfer, holderCount, holderLimit.

use alloy::{primitives::Address, sol_types::SolInterface};
use base_precompiles_contracts::{
    BaseStablecoinError,
    IBaseStablecoin::{self, IBaseStablecoinCalls},
};
use revm::precompile::PrecompileResult;

use crate::{
    Precompile, dispatch_call, metadata, mutate, mutate_void, view,
    error::BasePrecompileError,
    plan_2::stablecoin::{BaseStablecoin, STABLECOIN_MEMO},
    storage::ContractStorage,
};

macro_rules! gate {
    ($self:ident, $bit:expr) => {
        if !match $self.feature_set() {
            Ok(fs) => fs.has($bit),
            Err(e) => return $self.core.err_result(e),
        } {
            return $self.core.err_result(BasePrecompileError::BaseStablecoin(
                BaseStablecoinError::feature_not_enabled($bit),
            ));
        }
    };
}

impl Precompile for BaseStablecoin {
    fn call(&mut self, calldata: &[u8], msg_sender: Address) -> PrecompileResult {
        if let Some(err) = self.core.charge_input(calldata) {
            return err;
        }
        let initialized = match self.is_initialized() {
            Ok(v) => v,
            Err(e) => return self.core.err_result(e),
        };
        if !initialized {
            return self.core.err_result(BasePrecompileError::BaseStablecoin(
                BaseStablecoinError::uninitialized(),
            ));
        }

        dispatch_call(calldata, &[], IBaseStablecoinCalls::abi_decode, |call| match call {
            IBaseStablecoinCalls::name(_) => metadata::<IBaseStablecoin::nameCall>(|| self.name()),
            IBaseStablecoinCalls::symbol(_) => metadata::<IBaseStablecoin::symbolCall>(|| self.symbol()),
            IBaseStablecoinCalls::decimals(_) => metadata::<IBaseStablecoin::decimalsCall>(|| self.decimals()),
            IBaseStablecoinCalls::totalSupply(_) => metadata::<IBaseStablecoin::totalSupplyCall>(|| self.total_supply()),
            IBaseStablecoinCalls::features(_) => metadata::<IBaseStablecoin::featuresCall>(|| self.features_raw()),
            IBaseStablecoinCalls::assetClass(_) => metadata::<IBaseStablecoin::assetClassCall>(|| self.asset_class()),
            IBaseStablecoinCalls::currency(_) => metadata::<IBaseStablecoin::currencyCall>(|| self.currency()),
            IBaseStablecoinCalls::ISSUER_ROLE(c) => view(c, |_| Ok(Self::issuer_role())),
            IBaseStablecoinCalls::PAUSER_ROLE(c) => view(c, |_| Ok(Self::pauser_role())),
            IBaseStablecoinCalls::BURN_BLOCKED_ROLE(c) => view(c, |_| Ok(Self::burn_blocked_role())),
            IBaseStablecoinCalls::POLICY_ADMIN_ROLE(c) => view(c, |_| Ok(Self::policy_admin_role())),
            IBaseStablecoinCalls::balanceOf(c) => view(c, |c| self.balance_of(c)),
            IBaseStablecoinCalls::allowance(c) => view(c, |c| self.allowance(c)),
            IBaseStablecoinCalls::approve(c) => mutate(c, msg_sender, |s, c| self.approve(s, c)),
            IBaseStablecoinCalls::transfer(c) => mutate(c, msg_sender, |s, c| self.transfer(s, c)),
            IBaseStablecoinCalls::transferFrom(c) => mutate(c, msg_sender, |s, c| self.transfer_from(s, c)),
            IBaseStablecoinCalls::mint(c) => mutate_void(c, msg_sender, |s, c| self.mint(s, c)),
            IBaseStablecoinCalls::burn(c) => mutate_void(c, msg_sender, |s, c| self.burn(s, c)),
            IBaseStablecoinCalls::burnBlocked(c) => mutate_void(c, msg_sender, |s, c| self.burn_blocked(s, c)),
            IBaseStablecoinCalls::policyId(_) => metadata::<IBaseStablecoin::policyIdCall>(|| self.policy_id()),
            IBaseStablecoinCalls::setPolicyId(c) => mutate_void(c, msg_sender, |s, c| self.set_policy_id(s, c)),
            IBaseStablecoinCalls::paused(_) => metadata::<IBaseStablecoin::pausedCall>(|| self.paused()),
            IBaseStablecoinCalls::pause(c) => mutate_void(c, msg_sender, |s, c| self.pause(s, c)),
            IBaseStablecoinCalls::unpause(c) => mutate_void(c, msg_sender, |s, c| self.unpause(s, c)),
            IBaseStablecoinCalls::transferWithMemo(c) => {
                gate!(self, STABLECOIN_MEMO);
                mutate(c, msg_sender, |s, c| self.transfer_with_memo(s, c))
            }
            IBaseStablecoinCalls::mintWithMemo(c) => {
                gate!(self, STABLECOIN_MEMO);
                mutate_void(c, msg_sender, |s, c| self.mint_with_memo(s, c))
            }
            IBaseStablecoinCalls::burnWithMemo(c) => {
                gate!(self, STABLECOIN_MEMO);
                mutate_void(c, msg_sender, |s, c| self.burn_with_memo(s, c))
            }
            IBaseStablecoinCalls::permit(c) => mutate_void(c, msg_sender, |_s, c| self.permit(c)),
            IBaseStablecoinCalls::nonces(c) => view(c, |c| self.nonces(c)),
            IBaseStablecoinCalls::DOMAIN_SEPARATOR(c) => view(c, |_| self.domain_separator()),
            IBaseStablecoinCalls::hasRole(c) => view(c, |c| self.has_role(c)),
            IBaseStablecoinCalls::grantRole(c) => mutate_void(c, msg_sender, |s, c| self.grant_role(s, c)),
            IBaseStablecoinCalls::revokeRole(c) => mutate_void(c, msg_sender, |s, c| self.revoke_role(s, c)),
            IBaseStablecoinCalls::renounceRole(c) => mutate_void(c, msg_sender, |s, c| self.renounce_role(s, c)),
        })
    }
}
