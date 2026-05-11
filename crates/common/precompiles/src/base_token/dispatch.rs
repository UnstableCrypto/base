//! ABI dispatch for [`BaseToken`].
//!
//! This file is the authoritative call table — one match arm per ABI selector. Each
//! arm makes its **feature gate and role gate visible at the call site**: a future
//! reader greps `dispatch.rs` to learn the entire feature/role surface in one pass
//! without chasing into handler bodies.
//!
//! Handler bodies live in sibling files (`erc20.rs`, `mint_burn.rs`, `pause.rs`,
//! `permit.rs`, `policy.rs`, `memo.rs`); this file only routes.

use alloy::{primitives::Address, sol_types::SolInterface};
use base_precompiles_contracts::{
    BaseTokenError,
    IBaseToken::{self, IBaseTokenCalls},
    IRolesAuth::IRolesAuthCalls,
};
use revm::precompile::PrecompileResult;

use crate::{
    Precompile,
    base_token::{BaseToken, Feature},
    charge_input_cost, dispatch_call, metadata, mutate, mutate_void,
    storage::ContractStorage,
    view,
};

/// Decoded call variant — either a BaseToken ABI call or a RolesAuth call. The two
/// share the precompile address; we discriminate by selector.
enum Call {
    Token(IBaseTokenCalls),
    Roles(IRolesAuthCalls),
}

impl Call {
    fn decode(calldata: &[u8]) -> Result<Self, alloy::sol_types::Error> {
        let selector: [u8; 4] = calldata[..4].try_into().expect("calldata len >= 4 (pre-checked)");
        if IRolesAuthCalls::valid_selector(selector) {
            IRolesAuthCalls::abi_decode(calldata).map(Self::Roles)
        } else {
            IBaseTokenCalls::abi_decode(calldata).map(Self::Token)
        }
    }
}

/// Inline gate: short-circuit a dispatch arm with a reverted `FeatureNotEnabled` if
/// any flag in `required` is missing on `$self`.
macro_rules! gate {
    ($self:ident, $($feature:expr),+) => {
        if let Some(err) = $self.ensure_features(&[$($feature),+]) {
            return err;
        }
    };
}

impl Precompile for BaseToken {
    fn call(&mut self, calldata: &[u8], msg_sender: Address) -> PrecompileResult {
        if let Some(err) = charge_input_cost(&mut self.storage, calldata) {
            return err;
        }

        // Initialization gate. Every selector requires bytecode at this address;
        // `BaseToken` cannot be invoked directly until `BaseTokenFactory` has called
        // `initialize` against this address.
        let initialized = match self.is_initialized() {
            Ok(v) => v,
            Err(e) => return self.storage.error_result(e),
        };
        if !initialized {
            return self.storage.error_result(BaseTokenError::uninitialized());
        }

        dispatch_call(calldata, &[], Call::decode, |call| match call {
            // ---------------------------------------------------------------- Metadata
            Call::Token(IBaseTokenCalls::name(_)) => metadata::<IBaseToken::nameCall>(|| self.name()),
            Call::Token(IBaseTokenCalls::symbol(_)) => {
                metadata::<IBaseToken::symbolCall>(|| self.symbol())
            }
            Call::Token(IBaseTokenCalls::decimals(_)) => {
                metadata::<IBaseToken::decimalsCall>(|| self.decimals())
            }
            Call::Token(IBaseTokenCalls::totalSupply(_)) => {
                metadata::<IBaseToken::totalSupplyCall>(|| self.total_supply())
            }
            Call::Token(IBaseTokenCalls::features(_)) => {
                metadata::<IBaseToken::featuresCall>(|| self.features())
            }

            // Role identifiers (constants — no feature gate)
            Call::Token(IBaseTokenCalls::ISSUER_ROLE(c)) => view(c, |_| Ok(Self::issuer_role())),
            Call::Token(IBaseTokenCalls::BURNER_ROLE(c)) => view(c, |_| Ok(Self::burner_role())),
            Call::Token(IBaseTokenCalls::PAUSER_ROLE(c)) => view(c, |_| Ok(Self::pauser_role())),
            Call::Token(IBaseTokenCalls::POLICY_ADMIN_ROLE(c)) => {
                view(c, |_| Ok(Self::policy_admin_role()))
            }

            // ---------------------------------------------------------------- ERC-20
            Call::Token(IBaseTokenCalls::balanceOf(c)) => view(c, |c| self.balance_of(c)),
            Call::Token(IBaseTokenCalls::allowance(c)) => view(c, |c| self.allowance(c)),
            Call::Token(IBaseTokenCalls::approve(c)) => {
                mutate(c, msg_sender, |s, c| self.approve(s, c))
            }
            Call::Token(IBaseTokenCalls::transfer(c)) => {
                mutate(c, msg_sender, |s, c| self.transfer(s, c))
            }
            Call::Token(IBaseTokenCalls::transferFrom(c)) => {
                mutate(c, msg_sender, |s, c| self.transfer_from(s, c))
            }

            // ---------------------------------------------------------------- Supply (Feature::Mint / Feature::Burn)
            Call::Token(IBaseTokenCalls::mint(c)) => {
                gate!(self, Feature::Mint);
                mutate_void(c, msg_sender, |s, c| self.mint(s, c))
            }
            Call::Token(IBaseTokenCalls::burn(c)) => {
                gate!(self, Feature::Burn);
                mutate_void(c, msg_sender, |s, c| self.burn(s, c))
            }

            // ---------------------------------------------------------------- Pause (Feature::Pause)
            Call::Token(IBaseTokenCalls::paused(_)) => {
                gate!(self, Feature::Pause);
                metadata::<IBaseToken::pausedCall>(|| self.paused())
            }
            Call::Token(IBaseTokenCalls::pause(c)) => {
                gate!(self, Feature::Pause);
                mutate_void(c, msg_sender, |s, c| self.pause(s, c))
            }
            Call::Token(IBaseTokenCalls::unpause(c)) => {
                gate!(self, Feature::Pause);
                mutate_void(c, msg_sender, |s, c| self.unpause(s, c))
            }

            // ---------------------------------------------------------------- Policy (Feature::Policy)
            Call::Token(IBaseTokenCalls::policyId(_)) => {
                gate!(self, Feature::Policy);
                metadata::<IBaseToken::policyIdCall>(|| self.policy_id())
            }
            Call::Token(IBaseTokenCalls::setPolicyId(c)) => {
                gate!(self, Feature::Policy);
                mutate_void(c, msg_sender, |s, c| self.set_policy_id(s, c))
            }

            // ---------------------------------------------------------------- Memo overloads (Feature::Memo)
            Call::Token(IBaseTokenCalls::transferWithMemo(c)) => {
                gate!(self, Feature::Memo);
                mutate(c, msg_sender, |s, c| self.transfer_with_memo(s, c))
            }
            Call::Token(IBaseTokenCalls::mintWithMemo(c)) => {
                gate!(self, Feature::Mint, Feature::Memo);
                mutate_void(c, msg_sender, |s, c| self.mint_with_memo(s, c))
            }
            Call::Token(IBaseTokenCalls::burnWithMemo(c)) => {
                gate!(self, Feature::Burn, Feature::Memo);
                mutate_void(c, msg_sender, |s, c| self.burn_with_memo(s, c))
            }

            // ---------------------------------------------------------------- EIP-2612 (Feature::Permit)
            Call::Token(IBaseTokenCalls::permit(c)) => {
                gate!(self, Feature::Permit);
                mutate_void(c, msg_sender, |_s, c| self.permit(c))
            }
            Call::Token(IBaseTokenCalls::nonces(c)) => {
                gate!(self, Feature::Permit);
                view(c, |c| self.nonces(c))
            }
            Call::Token(IBaseTokenCalls::DOMAIN_SEPARATOR(c)) => {
                gate!(self, Feature::Permit);
                view(c, |_| self.domain_separator())
            }

            // ---------------------------------------------------------------- RolesAuth (always available)
            Call::Roles(IRolesAuthCalls::hasRole(c)) => view(c, |c| self.has_role(c)),
            Call::Roles(IRolesAuthCalls::getRoleAdmin(c)) => view(c, |c| self.get_role_admin(c)),
            Call::Roles(IRolesAuthCalls::grantRole(c)) => {
                mutate_void(c, msg_sender, |s, c| self.grant_role(s, c))
            }
            Call::Roles(IRolesAuthCalls::revokeRole(c)) => {
                mutate_void(c, msg_sender, |s, c| self.revoke_role(s, c))
            }
            Call::Roles(IRolesAuthCalls::renounceRole(c)) => {
                mutate_void(c, msg_sender, |s, c| self.renounce_role(s, c))
            }
            Call::Roles(IRolesAuthCalls::setRoleAdmin(c)) => {
                mutate_void(c, msg_sender, |s, c| self.set_role_admin(s, c))
            }
        })
    }
}
