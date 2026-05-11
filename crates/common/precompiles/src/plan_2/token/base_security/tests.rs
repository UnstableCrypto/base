//! End-to-end smoke tests for BaseSecurity creation and policy enforcement.

use alloy::primitives::{Address, B256, U256};
use base_precompiles_contracts::{IBaseSecurity, IBaseSecurityFactory, IBase2PolicyRegistry};

use crate::{
    BaseBSpec,
    plan_2::{
        base_security_factory::{BaseSecurityFactory, compute_base_security_address},
        policy_registry::{ALLOW_ALL_POLICY_ID, REJECT_ALL_POLICY_ID, Base2PolicyRegistry},
        token::base_security::{BaseSecurity, ISSUER_ROLE, PAUSER_ROLE},
    },
    storage::{StorageCtx, hashmap::HashMapStorageProvider},
};

fn unreserved_salt() -> B256 {
    B256::repeat_byte(0xab)
}

fn admin() -> Address {
    Address::with_last_byte(0xAA)
}

fn alice() -> Address {
    Address::with_last_byte(0x01)
}

fn bob() -> Address {
    Address::with_last_byte(0x02)
}

fn setup() -> HashMapStorageProvider {
    HashMapStorageProvider::new_with_spec(1, BaseBSpec::Beryl)
}

/// Creates a BLACKLIST policy in the registry and returns its ID (>= 2).
fn create_blacklist_policy(registry: &mut Base2PolicyRegistry) -> u64 {
    let id = registry
        .create_policy(
            admin(),
            IBase2PolicyRegistry::createPolicyCall {
                admin: admin(),
                kind: IBase2PolicyRegistry::PolicyKind::BLACKLIST,
            },
        )
        .unwrap();
    assert!(id >= 2, "user-created policy IDs start at 2");
    id
}

/// Helper: create a BaseSecurity with the given policy and supply_cap.
fn create_security(
    factory: &mut BaseSecurityFactory,
    policy_id: u64,
    supply_cap: U256,
) -> Address {
    factory
        .create_base_security(
            admin(),
            IBaseSecurityFactory::createBaseSecurityCall {
                name: "Test RWA".into(),
                symbol: "TRWA".into(),
                decimals: 6,
                admin: admin(),
                policyId: policy_id,
                supplyCap: supply_cap,
                features: 0,
                salt: unreserved_salt(),
            },
        )
        .unwrap()
}

// ─────────────────────────────────────── factory validation

#[test]
fn factory_rejects_allow_all_policy_id() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut factory = BaseSecurityFactory::new();
        let result = factory.create_base_security(
            admin(),
            IBaseSecurityFactory::createBaseSecurityCall {
                name: "Bad".into(),
                symbol: "B".into(),
                decimals: 6,
                admin: admin(),
                policyId: ALLOW_ALL_POLICY_ID, // must be rejected
                supplyCap: U256::from(1_000_000u64),
                features: 0,
                salt: unreserved_salt(),
            },
        );
        assert!(result.is_err(), "ALLOW_ALL policy must be rejected for securities");
    });
}

#[test]
fn factory_rejects_reject_all_policy_id() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut factory = BaseSecurityFactory::new();
        let result = factory.create_base_security(
            admin(),
            IBaseSecurityFactory::createBaseSecurityCall {
                name: "Bad".into(),
                symbol: "B".into(),
                decimals: 6,
                admin: admin(),
                policyId: REJECT_ALL_POLICY_ID, // must be rejected
                supplyCap: U256::from(1_000_000u64),
                features: 0,
                salt: unreserved_salt(),
            },
        );
        assert!(result.is_err(), "REJECT_ALL policy must be rejected for securities");
    });
}

#[test]
fn factory_rejects_nonexistent_policy_id() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut factory = BaseSecurityFactory::new();
        // Policy ID 99 has never been created — the registry counter starts at 2.
        let result = factory.create_base_security(
            admin(),
            IBaseSecurityFactory::createBaseSecurityCall {
                name: "Bad".into(),
                symbol: "B".into(),
                decimals: 6,
                admin: admin(),
                policyId: 99,
                supplyCap: U256::from(1_000_000u64),
                features: 0,
                salt: unreserved_salt(),
            },
        );
        assert!(result.is_err(), "non-existent policy ID must be rejected");
    });
}

#[test]
fn factory_rejects_zero_supply_cap() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();
        let policy_id = create_blacklist_policy(&mut registry);

        let mut factory = BaseSecurityFactory::new();
        let result = factory.create_base_security(
            admin(),
            IBaseSecurityFactory::createBaseSecurityCall {
                name: "Bad".into(),
                symbol: "B".into(),
                decimals: 6,
                admin: admin(),
                policyId: policy_id,
                supplyCap: U256::ZERO, // must be rejected
                features: 0,
                salt: unreserved_salt(),
            },
        );
        assert!(result.is_err(), "zero supply cap must be rejected");
    });
}

#[test]
fn factory_creates_security_with_valid_policy() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        // Step 1: create a policy in the registry.
        let mut registry = Base2PolicyRegistry::new();
        let policy_id = create_blacklist_policy(&mut registry);

        // Step 2: create the security using that policy.
        let mut factory = BaseSecurityFactory::new();
        let token_addr = create_security(&mut factory, policy_id, U256::from(1_000_000u64));

        // Verify deterministic address.
        let (expected, lower_bytes) = compute_base_security_address(admin(), unreserved_salt());
        assert_eq!(token_addr, expected);
        assert!(lower_bytes >= 1024, "salt must be outside reserved range");

        // Verify storage was written correctly.
        let token = BaseSecurity::from_address(token_addr).unwrap();
        assert!(token.is_initialized().unwrap());
        assert_eq!(token.name().unwrap(), "Test RWA");
        assert_eq!(token.symbol().unwrap(), "TRWA");
        assert_eq!(token.decimals().unwrap(), 6);
        assert_eq!(token.supply_cap().unwrap(), U256::from(1_000_000u64));
        assert_eq!(token.policy_id().unwrap(), policy_id);
        assert_eq!(token.asset_class().unwrap(), 2);
    });
}

// ─────────────────────────────────────── policy enforcement

#[test]
fn transfer_blocked_by_blacklist_policy() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();
        let policy_id = create_blacklist_policy(&mut registry);

        let mut factory = BaseSecurityFactory::new();
        let token_addr = create_security(&mut factory, policy_id, U256::from(1_000_000u64));
        let mut token = BaseSecurity::from_address(token_addr).unwrap();

        // Grant ISSUER_ROLE so admin can mint.
        token.grant_role(admin(), IBaseSecurity::grantRoleCall { role: *ISSUER_ROLE, account: admin() }).unwrap();

        // Mint to alice.
        token.mint(admin(), IBaseSecurity::mintCall { to: alice(), amount: U256::from(500u64) }).unwrap();
        assert_eq!(token.balance_of(IBaseSecurity::balanceOfCall { account: alice() }).unwrap(), U256::from(500u64));

        // Blacklist bob — transfers TO bob should now be rejected.
        registry.add_to_list(
            admin(),
            IBase2PolicyRegistry::addToListCall { policyId: policy_id, account: bob() },
        ).unwrap();

        let blocked = token.transfer(alice(), IBaseSecurity::transferCall { to: bob(), amount: U256::from(100u64) });
        assert!(blocked.is_err(), "transfer to blacklisted recipient must be rejected");

        // alice→alice (non-blacklisted) should still work.
        let ok = token.transfer(alice(), IBaseSecurity::transferCall { to: alice(), amount: U256::from(0u64) });
        assert!(ok.is_ok());
    });
}

#[test]
fn mint_blocked_by_blacklist_policy() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();
        let policy_id = create_blacklist_policy(&mut registry);

        let mut factory = BaseSecurityFactory::new();
        let token_addr = create_security(&mut factory, policy_id, U256::from(1_000_000u64));
        let mut token = BaseSecurity::from_address(token_addr).unwrap();
        token.grant_role(admin(), IBaseSecurity::grantRoleCall { role: *ISSUER_ROLE, account: admin() }).unwrap();

        // Blacklist alice before minting to her.
        registry.add_to_list(
            admin(),
            IBase2PolicyRegistry::addToListCall { policyId: policy_id, account: alice() },
        ).unwrap();

        let blocked = token.mint(admin(), IBaseSecurity::mintCall { to: alice(), amount: U256::from(100u64) });
        assert!(blocked.is_err(), "mint to blacklisted recipient must be rejected");
    });
}

#[test]
fn supply_cap_enforced_at_mint() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();
        let policy_id = create_blacklist_policy(&mut registry);

        let cap = U256::from(100u64);
        let mut factory = BaseSecurityFactory::new();
        let token_addr = create_security(&mut factory, policy_id, cap);
        let mut token = BaseSecurity::from_address(token_addr).unwrap();
        token.grant_role(admin(), IBaseSecurity::grantRoleCall { role: *ISSUER_ROLE, account: admin() }).unwrap();

        // Mint exactly at cap — should succeed.
        token.mint(admin(), IBaseSecurity::mintCall { to: alice(), amount: cap }).unwrap();

        // Minting 1 more should fail.
        let over = token.mint(admin(), IBaseSecurity::mintCall { to: alice(), amount: U256::from(1u64) });
        assert!(over.is_err(), "minting beyond supply cap must be rejected");
    });
}

#[test]
fn pause_blocks_transfer() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();
        let policy_id = create_blacklist_policy(&mut registry);

        let mut factory = BaseSecurityFactory::new();
        let token_addr = create_security(&mut factory, policy_id, U256::from(1_000_000u64));
        let mut token = BaseSecurity::from_address(token_addr).unwrap();

        token.grant_role(admin(), IBaseSecurity::grantRoleCall { role: *ISSUER_ROLE, account: admin() }).unwrap();
        token.grant_role(admin(), IBaseSecurity::grantRoleCall { role: *PAUSER_ROLE, account: admin() }).unwrap();

        token.mint(admin(), IBaseSecurity::mintCall { to: alice(), amount: U256::from(50u64) }).unwrap();

        // Pause then attempt transfer.
        token.pause(admin(), IBaseSecurity::pauseCall {}).unwrap();
        let blocked = token.transfer(alice(), IBaseSecurity::transferCall { to: bob(), amount: U256::from(1u64) });
        assert!(blocked.is_err(), "transfer must revert while paused");

        // Unpause — transfer should work again (bob is not blacklisted).
        token.unpause(admin(), IBaseSecurity::unpauseCall {}).unwrap();
        let ok = token.transfer(alice(), IBaseSecurity::transferCall { to: bob(), amount: U256::from(1u64) });
        assert!(ok.is_ok());
    });
}

#[test]
fn burn_blocked_rejects_authorized_address() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();
        let policy_id = create_blacklist_policy(&mut registry);

        let mut factory = BaseSecurityFactory::new();
        let token_addr = create_security(&mut factory, policy_id, U256::from(1_000_000u64));
        let mut token = BaseSecurity::from_address(token_addr).unwrap();

        token.grant_role(admin(), IBaseSecurity::grantRoleCall { role: *ISSUER_ROLE, account: admin() }).unwrap();
        token.grant_role(
            admin(),
            IBaseSecurity::grantRoleCall {
                role: crate::plan_2::token::base_security::BURN_BLOCKED_ROLE.clone(),
                account: admin(),
            },
        ).unwrap();

        token.mint(admin(), IBaseSecurity::mintCall { to: alice(), amount: U256::from(100u64) }).unwrap();

        // alice is NOT on the blacklist — burn_blocked must reject.
        let not_blocked = token.burn_blocked(admin(), IBaseSecurity::burnBlockedCall { from: alice(), amount: U256::from(10u64) });
        assert!(not_blocked.is_err(), "burn_blocked must reject non-blocked addresses");
    });
}

#[test]
fn burn_blocked_succeeds_for_blacklisted_address() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();
        let policy_id = create_blacklist_policy(&mut registry);

        let mut factory = BaseSecurityFactory::new();
        let token_addr = create_security(&mut factory, policy_id, U256::from(1_000_000u64));
        let mut token = BaseSecurity::from_address(token_addr).unwrap();

        token.grant_role(admin(), IBaseSecurity::grantRoleCall { role: *ISSUER_ROLE, account: admin() }).unwrap();
        token.grant_role(
            admin(),
            IBaseSecurity::grantRoleCall {
                role: crate::plan_2::token::base_security::BURN_BLOCKED_ROLE.clone(),
                account: admin(),
            },
        ).unwrap();

        // Mint to alice while she's not blacklisted.
        token.mint(admin(), IBaseSecurity::mintCall { to: alice(), amount: U256::from(100u64) }).unwrap();

        // Now blacklist alice.
        registry.add_to_list(
            admin(),
            IBase2PolicyRegistry::addToListCall { policyId: policy_id, account: alice() },
        ).unwrap();

        // burn_blocked should now succeed.
        token.burn_blocked(admin(), IBaseSecurity::burnBlockedCall { from: alice(), amount: U256::from(50u64) }).unwrap();
        assert_eq!(
            token.balance_of(IBaseSecurity::balanceOfCall { account: alice() }).unwrap(),
            U256::from(50u64),
        );
        assert_eq!(token.total_supply().unwrap(), U256::from(50u64));
    });
}
