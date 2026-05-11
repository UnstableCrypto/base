//! Singleton transfer-policy registry for the BaseToken family.
//!
//! Holds whitelist / blacklist policies that BaseTokens reference by id. The single
//! built-in policy id [`ALLOW_ALL_POLICY_ID`] is always authorized — this keeps the
//! gas cost of "permissionless" tokens near zero by short-circuiting the registry call
//! entirely.

pub mod dispatch;

use alloy::primitives::Address;
pub use base_precompiles_contracts::{
    BaseTokenPolicyRegistryError, BaseTokenPolicyRegistryEvent,
    IBaseTokenPolicyRegistry::{self, PolicyKind},
};
use base_precompiles_macros::{Storable, contract};

use crate::{
    BASE_TOKEN_POLICY_REGISTRY_ADDRESS,
    error::{BasePrecompileError, Result},
    storage::{Handler, Mapping},
};

/// Reserved policy id that always authorizes any (from, to) pair. Default for new tokens.
pub const ALLOW_ALL_POLICY_ID: u64 = 1;

/// Reserved policy id that always rejects.
pub const REJECT_ALL_POLICY_ID: u64 = 0;

/// Registry singleton precompile. Created policy ids start at `2` because `0` and `1`
/// are reserved built-in sentinels.
#[contract(addr = BASE_TOKEN_POLICY_REGISTRY_ADDRESS)]
pub struct BaseTokenPolicyRegistry {
    /// Monotonically increasing policy id counter (>= 2).
    policy_id_counter: u64,
    /// Per-policy metadata: kind + admin.
    policy_records: Mapping<u64, PolicyRecord>,
    /// Per-policy address membership set.
    policy_set: Mapping<u64, Mapping<Address, bool>>,
}

/// Stored per-policy metadata.
#[derive(Debug, Clone, Default, Storable)]
pub struct PolicyRecord {
    /// `0 = WHITELIST`, `1 = BLACKLIST`. Stored as `u8` for slot packing.
    pub kind: u8,
    /// Admin address authorized to mutate this policy.
    pub admin: Address,
}

impl PolicyRecord {
    fn is_default(&self) -> bool {
        self.kind == 0 && self.admin == Address::ZERO
    }

    fn kind(&self) -> Result<PolicyKind> {
        self.kind.try_into().map_err(|_| {
            BasePrecompileError::BaseTokenPolicyRegistry(
                BaseTokenPolicyRegistryError::invalid_policy_kind(),
            )
        })
    }
}

impl BaseTokenPolicyRegistry {
    /// One-shot init.
    pub fn initialize(&mut self) -> Result<()> {
        self.__initialize()
    }

    /// Returns the next-to-assign policy id, ensuring `>= 2`.
    pub fn policy_id_counter(&self) -> Result<u64> {
        Ok(self.policy_id_counter.read()?.max(2))
    }

    /// Returns whether `policy_id` is a known policy (built-in or user-created).
    pub fn policy_exists(&self, call: IBaseTokenPolicyRegistry::policyExistsCall) -> Result<bool> {
        self.policy_exists_internal(call.policyId)
    }

    /// Internal version of `policy_exists`, callable from sibling precompiles.
    pub fn policy_exists_internal(&self, policy_id: u64) -> Result<bool> {
        if matches!(policy_id, REJECT_ALL_POLICY_ID | ALLOW_ALL_POLICY_ID) {
            return Ok(true);
        }
        Ok(policy_id < self.policy_id_counter()?)
    }

    /// Returns the admin of `policy_id`. Built-in policies have no admin (zero).
    pub fn policy_admin(
        &self,
        call: IBaseTokenPolicyRegistry::policyAdminCall,
    ) -> Result<Address> {
        if self.builtin(call.policyId).is_some() {
            return Ok(Address::ZERO);
        }
        Ok(self.get_policy_record(call.policyId)?.admin)
    }

    /// Returns the kind of `policy_id`.
    pub fn policy_kind(
        &self,
        call: IBaseTokenPolicyRegistry::policyKindCall,
    ) -> Result<PolicyKind> {
        match call.policyId {
            REJECT_ALL_POLICY_ID => Ok(PolicyKind::WHITELIST),
            ALLOW_ALL_POLICY_ID => Ok(PolicyKind::BLACKLIST),
            id => self.get_policy_record(id)?.kind(),
        }
    }

    /// External `isAuthorized(policyId, from, to)`. The caller (a BaseToken) typically
    /// reaches this through [`Self::is_authorized_internal`] instead.
    pub fn is_authorized(
        &self,
        call: IBaseTokenPolicyRegistry::isAuthorizedCall,
    ) -> Result<bool> {
        self.is_authorized_internal(call.policyId, call.from, call.to)
    }

    /// Authorization check used by sibling precompiles (BaseToken). Short-circuits to
    /// `true` for [`ALLOW_ALL_POLICY_ID`] and `false` for [`REJECT_ALL_POLICY_ID`]; for
    /// user policies, both `from` and `to` must be authorized (whitelisted / not
    /// blacklisted) for the result to be `true`.
    pub fn is_authorized_internal(
        &self,
        policy_id: u64,
        from: Address,
        to: Address,
    ) -> Result<bool> {
        if let Some(auth) = self.builtin(policy_id) {
            return Ok(auth);
        }
        let record = self.get_policy_record(policy_id)?;
        let from_ok = self.address_passes(policy_id, from, &record)?;
        if !from_ok {
            return Ok(false);
        }
        self.address_passes(policy_id, to, &record)
    }

    /// Creates a new simple (whitelist or blacklist) policy.
    pub fn create_policy(
        &mut self,
        msg_sender: Address,
        call: IBaseTokenPolicyRegistry::createPolicyCall,
    ) -> Result<u64> {
        let kind = match call.kind {
            PolicyKind::WHITELIST | PolicyKind::BLACKLIST => call.kind as u8,
            PolicyKind::__Invalid => {
                return Err(BaseTokenPolicyRegistryError::invalid_policy_kind().into());
            }
        };
        let new_policy_id = self.policy_id_counter()?;
        self.policy_id_counter.write(
            new_policy_id.checked_add(1).ok_or(BasePrecompileError::under_overflow())?,
        )?;
        self.policy_records[new_policy_id].write(PolicyRecord { kind, admin: call.admin })?;
        self.emit_event(BaseTokenPolicyRegistryEvent::PolicyCreated(
            IBaseTokenPolicyRegistry::PolicyCreated {
                policyId: new_policy_id,
                admin: call.admin,
                kind: call.kind,
            },
        ))?;
        let _ = msg_sender;
        Ok(new_policy_id)
    }

    /// Adds `account` to the membership set of `policy_id` (whitelist: allowed,
    /// blacklist: blocked). Caller must be the policy admin.
    pub fn add_to_list(
        &mut self,
        msg_sender: Address,
        call: IBaseTokenPolicyRegistry::addToListCall,
    ) -> Result<()> {
        self.set_membership(msg_sender, call.policyId, call.account, true)
    }

    /// Removes `account` from the membership set of `policy_id`. Caller must be the
    /// policy admin.
    pub fn remove_from_list(
        &mut self,
        msg_sender: Address,
        call: IBaseTokenPolicyRegistry::removeFromListCall,
    ) -> Result<()> {
        self.set_membership(msg_sender, call.policyId, call.account, false)
    }

    /// Transfers admin control of `policy_id`. Only callable by the current admin.
    pub fn set_policy_admin(
        &mut self,
        msg_sender: Address,
        call: IBaseTokenPolicyRegistry::setPolicyAdminCall,
    ) -> Result<()> {
        let mut record = self.get_policy_record(call.policyId)?;
        if record.admin != msg_sender {
            return Err(BaseTokenPolicyRegistryError::unauthorized().into());
        }
        record.admin = call.newAdmin;
        self.policy_records[call.policyId].write(record)?;
        self.emit_event(BaseTokenPolicyRegistryEvent::PolicyAdminUpdated(
            IBaseTokenPolicyRegistry::PolicyAdminUpdated {
                policyId: call.policyId,
                newAdmin: call.newAdmin,
            },
        ))
    }

    // -------------------------------------------------------------- internals

    /// Resolves the built-in (`0`/`1`) sentinels.
    #[inline]
    fn builtin(&self, policy_id: u64) -> Option<bool> {
        match policy_id {
            ALLOW_ALL_POLICY_ID => Some(true),
            REJECT_ALL_POLICY_ID => Some(false),
            _ => None,
        }
    }

    fn get_policy_record(&self, policy_id: u64) -> Result<PolicyRecord> {
        let record = self.policy_records[policy_id].read()?;
        if record.is_default() && policy_id >= self.policy_id_counter()? {
            return Err(BaseTokenPolicyRegistryError::policy_not_found().into());
        }
        Ok(record)
    }

    fn set_membership(
        &mut self,
        msg_sender: Address,
        policy_id: u64,
        account: Address,
        present: bool,
    ) -> Result<()> {
        let record = self.get_policy_record(policy_id)?;
        if record.admin != msg_sender {
            return Err(BaseTokenPolicyRegistryError::unauthorized().into());
        }
        self.policy_set[policy_id][account].write(present)?;
        self.emit_event(BaseTokenPolicyRegistryEvent::ListUpdated(
            IBaseTokenPolicyRegistry::ListUpdated { policyId: policy_id, account, present },
        ))
    }

    /// Returns whether `address` is allowed by the (already-loaded) `record`.
    fn address_passes(
        &self,
        policy_id: u64,
        address: Address,
        record: &PolicyRecord,
    ) -> Result<bool> {
        let in_set = self.policy_set[policy_id][address].read()?;
        Ok(match record.kind()? {
            PolicyKind::WHITELIST => in_set,
            PolicyKind::BLACKLIST => !in_set,
            PolicyKind::__Invalid => {
                return Err(BaseTokenPolicyRegistryError::invalid_policy_kind().into());
            }
        })
    }
}
