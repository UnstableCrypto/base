//! Tests for Base2PolicyRegistry — both raw API and dispatch path.

use alloy::{
    primitives::{Address, U256},
    sol_types::{SolCall, SolInterface},
};
use base_precompiles_contracts::{IBase2PolicyRegistry, IBase2PolicyRegistry::PolicyKind};

use crate::{
    BaseBSpec, Precompile,
    plan_2::policy_registry::{
        ALLOW_ALL_POLICY_ID, REJECT_ALL_POLICY_ID, Base2PolicyRegistry,
    },
    storage::{StorageCtx, hashmap::HashMapStorageProvider},
};

fn admin() -> Address { Address::with_last_byte(0xAA) }
fn user() -> Address { Address::with_last_byte(0x01) }

fn setup() -> HashMapStorageProvider {
    HashMapStorageProvider::new_with_spec(1, BaseBSpec::Beryl)
}

// ─────────────────────────────────────────── raw API tests

#[test]
fn built_in_policies_always_exist() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let registry = Base2PolicyRegistry::new();
        assert!(registry.policy_exists_internal(ALLOW_ALL_POLICY_ID).unwrap());
        assert!(registry.policy_exists_internal(REJECT_ALL_POLICY_ID).unwrap());
        // Any user-created id >= 2 should not exist yet.
        assert!(!registry.policy_exists_internal(2).unwrap());
        assert!(!registry.policy_exists_internal(99).unwrap());
    });
}

#[test]
fn built_in_allow_all_always_authorizes() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let registry = Base2PolicyRegistry::new();
        assert!(registry.is_authorized_internal(ALLOW_ALL_POLICY_ID, user(), admin()).unwrap());
    });
}

#[test]
fn built_in_reject_all_always_rejects() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let registry = Base2PolicyRegistry::new();
        assert!(!registry.is_authorized_internal(REJECT_ALL_POLICY_ID, user(), admin()).unwrap());
    });
}

#[test]
fn create_policy_returns_id_gte_2() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();

        let id = registry.create_policy(
            admin(),
            IBase2PolicyRegistry::createPolicyCall {
                admin: admin(),
                kind: PolicyKind::BLACKLIST,
            },
        ).unwrap();

        assert_eq!(id, 2, "first user policy must get ID 2");
    });
}

#[test]
fn created_policy_is_discoverable() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();

        let id = registry.create_policy(
            admin(),
            IBase2PolicyRegistry::createPolicyCall { admin: admin(), kind: PolicyKind::BLACKLIST },
        ).unwrap();

        assert!(registry.policy_exists_internal(id).unwrap(), "newly created policy must exist");
        assert_eq!(registry.policy_id_counter().unwrap(), 3);
    });
}

#[test]
fn blacklist_policy_authorization() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();

        let id = registry.create_policy(
            admin(),
            IBase2PolicyRegistry::createPolicyCall { admin: admin(), kind: PolicyKind::BLACKLIST },
        ).unwrap();

        // Neither address on the blacklist — transfer is authorized.
        assert!(registry.is_authorized_internal(id, user(), admin()).unwrap());
        assert!(registry.is_authorized_internal(id, admin(), user()).unwrap());

        // Blacklist user.
        registry.add_to_list(
            admin(),
            IBase2PolicyRegistry::addToListCall { policyId: id, account: user() },
        ).unwrap();

        // user as FROM — blocked.
        assert!(!registry.is_authorized_internal(id, user(), admin()).unwrap());
        // user as TO — also blocked (both sides checked).
        assert!(!registry.is_authorized_internal(id, admin(), user()).unwrap());
        // two non-blacklisted addresses — still authorized.
        let other = Address::with_last_byte(0x99);
        assert!(registry.is_authorized_internal(id, admin(), other).unwrap());
    });
}

#[test]
fn whitelist_policy_authorization() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();

        let id = registry.create_policy(
            admin(),
            IBase2PolicyRegistry::createPolicyCall { admin: admin(), kind: PolicyKind::WHITELIST },
        ).unwrap();

        // No one whitelisted — nobody can transfer.
        assert!(!registry.is_authorized_internal(id, user(), admin()).unwrap());

        // Whitelist only user.
        registry.add_to_list(
            admin(),
            IBase2PolicyRegistry::addToListCall { policyId: id, account: user() },
        ).unwrap();

        // user→admin still blocked because admin is not on the whitelist (TO check fails).
        assert!(!registry.is_authorized_internal(id, user(), admin()).unwrap());

        // Whitelist admin too.
        registry.add_to_list(
            admin(),
            IBase2PolicyRegistry::addToListCall { policyId: id, account: admin() },
        ).unwrap();

        // Both whitelisted — transfer is now authorized in both directions.
        assert!(registry.is_authorized_internal(id, user(), admin()).unwrap());
        assert!(registry.is_authorized_internal(id, admin(), user()).unwrap());
    });
}

#[test]
fn multiple_policies_get_sequential_ids() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();

        let id1 = registry.create_policy(
            admin(), IBase2PolicyRegistry::createPolicyCall { admin: admin(), kind: PolicyKind::BLACKLIST },
        ).unwrap();
        let id2 = registry.create_policy(
            admin(), IBase2PolicyRegistry::createPolicyCall { admin: admin(), kind: PolicyKind::WHITELIST },
        ).unwrap();
        let id3 = registry.create_policy(
            admin(), IBase2PolicyRegistry::createPolicyCall { admin: admin(), kind: PolicyKind::BLACKLIST },
        ).unwrap();

        assert_eq!(id1, 2);
        assert_eq!(id2, 3);
        assert_eq!(id3, 4);

        for id in [id1, id2, id3] {
            assert!(registry.policy_exists_internal(id).unwrap());
        }
    });
}

// ─────────────────────────────────────────── cross-call tests (create in one call, read in another)

/// Simulates the real EVM scenario: createPolicy in one call, then view in a separate call
/// using the same underlying storage (as a node would across two transactions).
#[test]
fn cross_call_create_then_policy_exists() {
    let mut storage = setup();

    let policy_id = StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();
        registry.create_policy(
            admin(),
            IBase2PolicyRegistry::createPolicyCall { admin: admin(), kind: PolicyKind::BLACKLIST },
        )
    })
    .unwrap();

    let exists =
        StorageCtx::enter(&mut storage, || {
            Base2PolicyRegistry::new().policy_exists_internal(policy_id)
        })
        .unwrap();
    assert!(exists, "policy created in call 1 must be visible in call 2");
}

#[test]
fn cross_call_counter_persists() {
    let mut storage = setup();

    StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();
        registry.create_policy(
            admin(),
            IBase2PolicyRegistry::createPolicyCall { admin: admin(), kind: PolicyKind::BLACKLIST },
        )
    })
    .unwrap();

    let counter =
        StorageCtx::enter(&mut storage, || Base2PolicyRegistry::new().policy_id_counter())
            .unwrap();
    assert_eq!(counter, 3, "counter must reflect the created policy across calls");
}

#[test]
fn cross_call_policy_admin_readable() {
    let mut storage = setup();

    let policy_id = StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();
        registry.create_policy(
            admin(),
            IBase2PolicyRegistry::createPolicyCall { admin: admin(), kind: PolicyKind::BLACKLIST },
        )
    })
    .unwrap();

    let fetched_admin = StorageCtx::enter(&mut storage, || {
        Base2PolicyRegistry::new()
            .policy_admin(IBase2PolicyRegistry::policyAdminCall { policyId: policy_id })
    })
    .unwrap();
    assert_eq!(fetched_admin, admin(), "policy admin must be readable after creation");
}

#[test]
fn cross_call_add_to_list_then_check_auth() {
    let mut storage = setup();

    let policy_id = StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();
        registry.create_policy(
            admin(),
            IBase2PolicyRegistry::createPolicyCall { admin: admin(), kind: PolicyKind::BLACKLIST },
        )
    })
    .unwrap();

    // Separate call: add user to blacklist.
    StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();
        registry.add_to_list(
            admin(),
            IBase2PolicyRegistry::addToListCall { policyId: policy_id, account: user() },
        )
    })
    .unwrap();

    // Separate call: check authorization.
    let is_blocked = StorageCtx::enter(&mut storage, || {
        Base2PolicyRegistry::new().is_authorized_internal(policy_id, user(), admin())
    })
    .unwrap();
    assert!(!is_blocked, "blacklisted user must be blocked across separate calls");
}

// ─────────────────────────────────────────── dispatch (EVM call) path tests

/// Call `createPolicy` through the full `Precompile::call` dispatch.
#[test]
fn dispatch_create_policy_returns_encoded_id() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();

        let calldata = IBase2PolicyRegistry::createPolicyCall {
            admin: admin(),
            kind: PolicyKind::BLACKLIST,
        }
        .abi_encode();

        let result = registry.call(&calldata, admin());
        let output = result.expect("createPolicy dispatch must not error");
        assert!(!output.reverted, "createPolicy must not revert: {:?}", output.bytes);

        // Decode the returned policy ID.
        let policy_id = IBase2PolicyRegistry::createPolicyCall::abi_decode_returns(&output.bytes)
            .expect("return must decode");
        assert_eq!(policy_id, 2u64, "first policy ID must be 2");
    });
}

#[test]
fn dispatch_policy_exists_after_creation() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();

        // Create through dispatch.
        let create_data = IBase2PolicyRegistry::createPolicyCall {
            admin: admin(),
            kind: PolicyKind::BLACKLIST,
        }
        .abi_encode();
        registry.call(&create_data, admin()).unwrap();

        // Query policyExists(2) through dispatch.
        let exists_data = IBase2PolicyRegistry::policyExistsCall { policyId: 2 }.abi_encode();
        let result = registry.call(&exists_data, admin()).unwrap();
        assert!(!result.reverted);
        let exists =
            IBase2PolicyRegistry::policyExistsCall::abi_decode_returns(&result.bytes).unwrap();
        assert!(exists, "policy 2 must exist after creation");
    });
}

#[test]
fn dispatch_add_to_list_and_check_authorization() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut registry = Base2PolicyRegistry::new();

        // Create a blacklist policy.
        let create_data = IBase2PolicyRegistry::createPolicyCall {
            admin: admin(),
            kind: PolicyKind::BLACKLIST,
        }
        .abi_encode();
        registry.call(&create_data, admin()).unwrap();
        let policy_id = 2u64;

        // user is authorized before being blacklisted.
        let auth_data = IBase2PolicyRegistry::isAuthorizedCall {
            policyId: policy_id,
            from: user(),
            to: admin(),
        }
        .abi_encode();
        let auth_result = registry.call(&auth_data, admin()).unwrap();
        let is_auth =
            IBase2PolicyRegistry::isAuthorizedCall::abi_decode_returns(&auth_result.bytes)
                .unwrap();
        assert!(is_auth, "user must be authorized before blacklisting");

        // Blacklist user through dispatch.
        let add_data = IBase2PolicyRegistry::addToListCall { policyId: policy_id, account: user() }
            .abi_encode();
        let add_result = registry.call(&add_data, admin()).unwrap();
        assert!(!add_result.reverted, "addToList must not revert");

        // Now user should be rejected.
        let auth_result2 = registry.call(&auth_data, admin()).unwrap();
        let is_auth2 =
            IBase2PolicyRegistry::isAuthorizedCall::abi_decode_returns(&auth_result2.bytes)
                .unwrap();
        assert!(!is_auth2, "user must be rejected after blacklisting");
    });
}
