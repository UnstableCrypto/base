//! **Authorization domain** — RBAC for [`BaseToken`].
//!
//! Mirrors B20's `AccessControl`-style RBAC: each role has an admin role that can
//! grant/revoke it. [`DEFAULT_ADMIN_ROLE`] is the root admin granted to `admin` at
//! `createToken` time. [`UNGRANTABLE_ROLE`] is self-administered and can never be
//! granted externally — used as a sentinel for invariants.
//!
//! Role *constants* and the `check_role` helper live here. Role *requirements* (e.g.
//! "mint requires `ISSUER_ROLE`") live in the domain that owns the operation — this is
//! a deliberate DDD choice: dispatch routes selectors, the domain owns its rules.

use std::sync::LazyLock;

use alloy::primitives::{Address, B256, keccak256};
use base_precompiles_contracts::{IRolesAuth, RolesAuthError, RolesAuthEvent};

use crate::{base_token::BaseToken, error::Result, storage::Handler};

/// Root admin role (zero hash). Holders can grant/revoke any role.
pub const DEFAULT_ADMIN_ROLE: B256 = B256::ZERO;

/// Self-administered role that can never be granted externally. Used as a sentinel
/// for invariants (e.g. detecting whether the role tree was initialized).
pub const UNGRANTABLE_ROLE: B256 = B256::new([0xff; 32]);

/// Role identifier for minting new tokens.
pub static ISSUER_ROLE: LazyLock<B256> = LazyLock::new(|| keccak256(b"BASE_TOKEN_ISSUER_ROLE"));
/// Role identifier for burning tokens.
pub static BURNER_ROLE: LazyLock<B256> = LazyLock::new(|| keccak256(b"BASE_TOKEN_BURNER_ROLE"));
/// Role identifier for pausing / unpausing the token.
pub static PAUSER_ROLE: LazyLock<B256> = LazyLock::new(|| keccak256(b"BASE_TOKEN_PAUSER_ROLE"));
/// Role identifier for changing the active transfer policy id.
pub static POLICY_ADMIN_ROLE: LazyLock<B256> =
    LazyLock::new(|| keccak256(b"BASE_TOKEN_POLICY_ADMIN_ROLE"));

impl BaseToken {
    /// Returns the `ISSUER_ROLE` constant.
    pub fn issuer_role() -> B256 {
        *ISSUER_ROLE
    }

    /// Returns the `BURNER_ROLE` constant.
    pub fn burner_role() -> B256 {
        *BURNER_ROLE
    }

    /// Returns the `PAUSER_ROLE` constant.
    pub fn pauser_role() -> B256 {
        *PAUSER_ROLE
    }

    /// Returns the `POLICY_ADMIN_ROLE` constant.
    pub fn policy_admin_role() -> B256 {
        *POLICY_ADMIN_ROLE
    }

    /// Initializes the roles tree by marking [`UNGRANTABLE_ROLE`] as self-administered.
    pub fn initialize_roles(&mut self) -> Result<()> {
        self.set_role_admin_internal(UNGRANTABLE_ROLE, UNGRANTABLE_ROLE)
    }

    /// Grants [`DEFAULT_ADMIN_ROLE`] to `admin` during initialization.
    pub fn grant_default_admin(&mut self, msg_sender: Address, admin: Address) -> Result<()> {
        self.grant_role_internal(admin, DEFAULT_ADMIN_ROLE)?;
        self.emit_event(RolesAuthEvent::RoleMembershipUpdated(IRolesAuth::RoleMembershipUpdated {
            role: DEFAULT_ADMIN_ROLE,
            account: admin,
            sender: msg_sender,
            hasRole: true,
        }))
    }

    /// Returns whether `account` holds the given `role`.
    pub fn has_role(&self, call: IRolesAuth::hasRoleCall) -> Result<bool> {
        self.has_role_internal(call.account, call.role)
    }

    /// Returns the admin role that governs `role`. Unset reads as zero ([`DEFAULT_ADMIN_ROLE`]).
    pub fn get_role_admin(&self, call: IRolesAuth::getRoleAdminCall) -> Result<B256> {
        self.get_role_admin_internal(call.role)
    }

    /// Grants `role` to `account`.
    pub fn grant_role(
        &mut self,
        msg_sender: Address,
        call: IRolesAuth::grantRoleCall,
    ) -> Result<()> {
        let admin_role = self.get_role_admin_internal(call.role)?;
        self.check_role_internal(msg_sender, admin_role)?;
        self.grant_role_internal(call.account, call.role)?;
        self.emit_event(RolesAuthEvent::RoleMembershipUpdated(IRolesAuth::RoleMembershipUpdated {
            role: call.role,
            account: call.account,
            sender: msg_sender,
            hasRole: true,
        }))
    }

    /// Revokes `role` from `account`.
    pub fn revoke_role(
        &mut self,
        msg_sender: Address,
        call: IRolesAuth::revokeRoleCall,
    ) -> Result<()> {
        let admin_role = self.get_role_admin_internal(call.role)?;
        self.check_role_internal(msg_sender, admin_role)?;
        self.revoke_role_internal(call.account, call.role)?;
        self.emit_event(RolesAuthEvent::RoleMembershipUpdated(IRolesAuth::RoleMembershipUpdated {
            role: call.role,
            account: call.account,
            sender: msg_sender,
            hasRole: false,
        }))
    }

    /// Allows the caller to voluntarily give up their own `role`.
    pub fn renounce_role(
        &mut self,
        msg_sender: Address,
        call: IRolesAuth::renounceRoleCall,
    ) -> Result<()> {
        self.check_role_internal(msg_sender, call.role)?;
        self.revoke_role_internal(msg_sender, call.role)?;
        self.emit_event(RolesAuthEvent::RoleMembershipUpdated(IRolesAuth::RoleMembershipUpdated {
            role: call.role,
            account: msg_sender,
            sender: msg_sender,
            hasRole: false,
        }))
    }

    /// Changes the admin role that governs `role`.
    pub fn set_role_admin(
        &mut self,
        msg_sender: Address,
        call: IRolesAuth::setRoleAdminCall,
    ) -> Result<()> {
        let current_admin_role = self.get_role_admin_internal(call.role)?;
        self.check_role_internal(msg_sender, current_admin_role)?;
        self.set_role_admin_internal(call.role, call.adminRole)?;
        self.emit_event(RolesAuthEvent::RoleAdminUpdated(IRolesAuth::RoleAdminUpdated {
            role: call.role,
            newAdminRole: call.adminRole,
            sender: msg_sender,
        }))
    }

    /// Reverts if `account` does not hold `role`.
    pub fn check_role(&self, account: Address, role: B256) -> Result<()> {
        self.check_role_internal(account, role)
    }

    pub(crate) fn has_role_internal(&self, account: Address, role: B256) -> Result<bool> {
        self.roles[account][role].read()
    }

    pub(crate) fn grant_role_internal(&mut self, account: Address, role: B256) -> Result<()> {
        self.roles[account][role].write(true)
    }

    fn revoke_role_internal(&mut self, account: Address, role: B256) -> Result<()> {
        self.roles[account][role].write(false)
    }

    fn get_role_admin_internal(&self, role: B256) -> Result<B256> {
        self.role_admins[role].read()
    }

    fn set_role_admin_internal(&mut self, role: B256, admin_role: B256) -> Result<()> {
        self.role_admins[role].write(admin_role)
    }

    fn check_role_internal(&self, account: Address, role: B256) -> Result<()> {
        if !self.has_role_internal(account, role)? {
            return Err(RolesAuthError::unauthorized().into());
        }
        Ok(())
    }
}
