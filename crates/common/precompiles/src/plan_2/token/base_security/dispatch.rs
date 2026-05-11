//! ABI dispatch for [`BaseSecurity`].

use alloy::{primitives::Address, sol_types::SolInterface};
use base_precompiles_contracts::{
    BaseSecurityError,
    IBaseSecurity::{self, IBaseSecurityCalls},
};
use revm::precompile::PrecompileResult;

use crate::{
    Precompile, dispatch_call, metadata, mutate, mutate_void, view,
    error::BasePrecompileError,
    plan_2::token::base_security::{
        BaseSecurity, SECURITY_FORCE_TRANSFER, SECURITY_HOLDER_LIMIT, SECURITY_MEMO,
    },
    storage::ContractStorage,
};

macro_rules! gate {
    ($self:ident, $bit:expr) => {
        if !match $self.feature_set() {
            Ok(fs) => fs.has($bit),
            Err(e) => return $self.core.err_result(e),
        } {
            return $self.core.err_result(BasePrecompileError::BaseSecurity(
                BaseSecurityError::feature_not_enabled($bit),
            ));
        }
    };
}

impl Precompile for BaseSecurity {
    fn call(&mut self, calldata: &[u8], msg_sender: Address) -> PrecompileResult {
        if let Some(err) = self.core.charge_input(calldata) {
            return err;
        }
        let initialized = match self.is_initialized() {
            Ok(v) => v,
            Err(e) => return self.core.err_result(e),
        };
        if !initialized {
            return self.core.err_result(BasePrecompileError::BaseSecurity(
                BaseSecurityError::uninitialized(),
            ));
        }

        dispatch_call(calldata, &[], IBaseSecurityCalls::abi_decode, |call| match call {
            IBaseSecurityCalls::name(_) => metadata::<IBaseSecurity::nameCall>(|| self.name()),
            IBaseSecurityCalls::symbol(_) => metadata::<IBaseSecurity::symbolCall>(|| self.symbol()),
            IBaseSecurityCalls::decimals(_) => metadata::<IBaseSecurity::decimalsCall>(|| self.decimals()),
            IBaseSecurityCalls::totalSupply(_) => metadata::<IBaseSecurity::totalSupplyCall>(|| self.total_supply()),
            IBaseSecurityCalls::features(_) => metadata::<IBaseSecurity::featuresCall>(|| self.features_raw()),
            IBaseSecurityCalls::assetClass(_) => metadata::<IBaseSecurity::assetClassCall>(|| self.asset_class()),
            IBaseSecurityCalls::ISSUER_ROLE(c) => view(c, |_| Ok(Self::issuer_role())),
            IBaseSecurityCalls::PAUSER_ROLE(c) => view(c, |_| Ok(Self::pauser_role())),
            IBaseSecurityCalls::BURN_BLOCKED_ROLE(c) => view(c, |_| Ok(Self::burn_blocked_role())),
            IBaseSecurityCalls::FORCE_TRANSFER_ROLE(c) => view(c, |_| Ok(Self::force_transfer_role())),
            IBaseSecurityCalls::POLICY_ADMIN_ROLE(c) => view(c, |_| Ok(Self::policy_admin_role())),
            IBaseSecurityCalls::balanceOf(c) => view(c, |c| self.balance_of(c)),
            IBaseSecurityCalls::allowance(c) => view(c, |c| self.allowance(c)),
            IBaseSecurityCalls::approve(c) => mutate(c, msg_sender, |s, c| self.approve(s, c)),
            IBaseSecurityCalls::transfer(c) => mutate(c, msg_sender, |s, c| self.transfer(s, c)),
            IBaseSecurityCalls::transferFrom(c) => mutate(c, msg_sender, |s, c| self.transfer_from(s, c)),
            IBaseSecurityCalls::mint(c) => mutate_void(c, msg_sender, |s, c| self.mint(s, c)),
            IBaseSecurityCalls::burn(c) => mutate_void(c, msg_sender, |s, c| self.burn(s, c)),
            IBaseSecurityCalls::burnBlocked(c) => mutate_void(c, msg_sender, |s, c| self.burn_blocked(s, c)),
            IBaseSecurityCalls::supplyCap(_) => metadata::<IBaseSecurity::supplyCapCall>(|| self.supply_cap()),
            IBaseSecurityCalls::policyId(_) => metadata::<IBaseSecurity::policyIdCall>(|| self.policy_id()),
            IBaseSecurityCalls::setPolicyId(c) => mutate_void(c, msg_sender, |s, c| self.set_policy_id(s, c)),
            IBaseSecurityCalls::paused(_) => metadata::<IBaseSecurity::pausedCall>(|| self.paused()),
            IBaseSecurityCalls::pause(c) => mutate_void(c, msg_sender, |s, c| self.pause(s, c)),
            IBaseSecurityCalls::unpause(c) => mutate_void(c, msg_sender, |s, c| self.unpause(s, c)),
            IBaseSecurityCalls::transferWithMemo(c) => {
                gate!(self, SECURITY_MEMO);
                mutate(c, msg_sender, |s, c| self.transfer_with_memo(s, c))
            }
            IBaseSecurityCalls::mintWithMemo(c) => {
                gate!(self, SECURITY_MEMO);
                mutate_void(c, msg_sender, |s, c| self.mint_with_memo(s, c))
            }
            IBaseSecurityCalls::burnWithMemo(c) => {
                gate!(self, SECURITY_MEMO);
                mutate_void(c, msg_sender, |s, c| self.burn_with_memo(s, c))
            }
            IBaseSecurityCalls::forceTransfer(c) => {
                gate!(self, SECURITY_FORCE_TRANSFER);
                mutate_void(c, msg_sender, |s, c| self.force_transfer(s, c))
            }
            IBaseSecurityCalls::holderCount(c) => {
                gate!(self, SECURITY_HOLDER_LIMIT);
                view(c, |c| self.holder_count(c))
            }
            IBaseSecurityCalls::holderLimit(c) => {
                gate!(self, SECURITY_HOLDER_LIMIT);
                view(c, |c| self.holder_limit(c))
            }
            IBaseSecurityCalls::permit(c) => mutate_void(c, msg_sender, |_s, c| self.permit(c)),
            IBaseSecurityCalls::nonces(c) => view(c, |c| self.nonces(c)),
            IBaseSecurityCalls::DOMAIN_SEPARATOR(c) => view(c, |_| self.domain_separator()),
            IBaseSecurityCalls::hasRole(c) => view(c, |c| self.has_role(c)),
            IBaseSecurityCalls::grantRole(c) => mutate_void(c, msg_sender, |s, c| self.grant_role(s, c)),
            IBaseSecurityCalls::revokeRole(c) => mutate_void(c, msg_sender, |s, c| self.revoke_role(s, c)),
            IBaseSecurityCalls::renounceRole(c) => mutate_void(c, msg_sender, |s, c| self.renounce_role(s, c)),
        })
    }
}
