//! End-to-end smoke tests for the BaseToken stack.
//!
//! These tests exercise the full lifecycle through the typed Rust API (factory →
//! initialize → mint/transfer/policy/pause → balanceOf), inside a `HashMapStorageProvider`
//! to keep them fast and dependency-free.

use alloy::primitives::{Address, B256, U256};
use base_precompiles_contracts::{IBaseToken, IBaseTokenFactory, IBaseTokenPolicyRegistry};

use crate::{
    BaseBSpec,
    base_token::{BaseToken, Feature, FeatureSet, authz::ISSUER_ROLE},
    base_token_factory::{BaseTokenFactory, compute_base_token_address},
    base_token_policy_registry::{ALLOW_ALL_POLICY_ID, BaseTokenPolicyRegistry},
    storage::{ContractStorage, StorageCtx, hashmap::HashMapStorageProvider},
};

/// Salt that derives an address outside the reserved range. `keccak256(sender || salt)`'s
/// first 8 bytes must encode a value `>= 1024`.
fn unreserved_salt() -> B256 {
    B256::repeat_byte(0xab)
}

fn admin() -> Address {
    Address::with_last_byte(0xAA)
}

/// Sets up a fresh storage context with the registry and factory pre-initialized.
fn setup() -> HashMapStorageProvider {
    HashMapStorageProvider::new_with_spec(1, BaseBSpec::Beryl)
}

#[test]
fn factory_creates_token_with_features() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut factory = BaseTokenFactory::new();
        factory.initialize().unwrap();

        let mut registry = BaseTokenPolicyRegistry::new();
        registry.initialize().unwrap();

        let features =
            (Feature::Mint as u64) | (Feature::Burn as u64) | (Feature::Pause as u64);

        let token_address = factory
            .create_token(
                admin(),
                IBaseTokenFactory::createTokenCall {
                    name: "TestCoin".into(),
                    symbol: "TC".into(),
                    decimals: 18,
                    admin: admin(),
                    features,
                    salt: unreserved_salt(),
                },
            )
            .unwrap();

        let (expected, lower) = compute_base_token_address(admin(), unreserved_salt());
        assert_eq!(token_address, expected);
        assert!(lower >= 1024, "salt must derive outside reserved range");

        let token = BaseToken::from_address(token_address).unwrap();
        assert!(token.is_initialized().unwrap());
        assert_eq!(token.name().unwrap(), "TestCoin");
        assert_eq!(token.symbol().unwrap(), "TC");
        assert_eq!(token.decimals().unwrap(), 18);
        assert_eq!(token.features().unwrap(), features);
        assert_eq!(FeatureSet::new(token.features().unwrap()).contains(Feature::Mint), true);
        assert_eq!(token.policy_id().unwrap(), ALLOW_ALL_POLICY_ID);
    });
}

#[test]
fn factory_rejects_unknown_feature_bit() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut factory = BaseTokenFactory::new();
        factory.initialize().unwrap();

        let bad_features = 1u64 << 63; // bit not defined in Feature
        let result = factory.create_token(
            admin(),
            IBaseTokenFactory::createTokenCall {
                name: "Bad".into(),
                symbol: "B".into(),
                decimals: 6,
                admin: admin(),
                features: bad_features,
                salt: unreserved_salt(),
            },
        );

        assert!(result.is_err(), "unknown feature bit must be rejected");
    });
}

#[test]
fn mint_then_transfer_updates_balances() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut factory = BaseTokenFactory::new();
        factory.initialize().unwrap();
        BaseTokenPolicyRegistry::new().initialize().unwrap();

        let features = (Feature::Mint as u64) | (Feature::Burn as u64);
        let token_address = factory
            .create_token(
                admin(),
                IBaseTokenFactory::createTokenCall {
                    name: "TC".into(),
                    symbol: "TC".into(),
                    decimals: 6,
                    admin: admin(),
                    features,
                    salt: unreserved_salt(),
                },
            )
            .unwrap();

        let mut token = BaseToken::from_address(token_address).unwrap();

        // Admin grants ISSUER_ROLE to themselves so they can mint.
        token
            .grant_role(
                admin(),
                base_precompiles_contracts::IRolesAuth::grantRoleCall {
                    role: *ISSUER_ROLE,
                    account: admin(),
                },
            )
            .unwrap();

        let alice = Address::with_last_byte(0x01);
        let bob = Address::with_last_byte(0x02);

        token
            .mint(admin(), IBaseToken::mintCall { to: alice, amount: U256::from(1_000u64) })
            .unwrap();

        assert_eq!(
            token.balance_of(IBaseToken::balanceOfCall { account: alice }).unwrap(),
            U256::from(1_000u64),
        );
        assert_eq!(token.total_supply().unwrap(), U256::from(1_000u64));

        token
            .transfer(alice, IBaseToken::transferCall { to: bob, amount: U256::from(400u64) })
            .unwrap();

        assert_eq!(
            token.balance_of(IBaseToken::balanceOfCall { account: alice }).unwrap(),
            U256::from(600u64),
        );
        assert_eq!(
            token.balance_of(IBaseToken::balanceOfCall { account: bob }).unwrap(),
            U256::from(400u64),
        );
    });
}

#[test]
fn policy_registry_blacklist_blocks_transfer() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut factory = BaseTokenFactory::new();
        factory.initialize().unwrap();
        let mut registry = BaseTokenPolicyRegistry::new();
        registry.initialize().unwrap();

        // Create a blacklist policy administered by admin.
        let policy_id = registry
            .create_policy(
                admin(),
                IBaseTokenPolicyRegistry::createPolicyCall {
                    admin: admin(),
                    kind: IBaseTokenPolicyRegistry::PolicyKind::BLACKLIST,
                },
            )
            .unwrap();
        assert!(policy_id >= 2);

        let features =
            (Feature::Mint as u64) | (Feature::Policy as u64);
        let token_address = factory
            .create_token(
                admin(),
                IBaseTokenFactory::createTokenCall {
                    name: "Gated".into(),
                    symbol: "G".into(),
                    decimals: 6,
                    admin: admin(),
                    features,
                    salt: unreserved_salt(),
                },
            )
            .unwrap();
        let mut token = BaseToken::from_address(token_address).unwrap();

        // Bind the policy.
        token
            .grant_role(
                admin(),
                base_precompiles_contracts::IRolesAuth::grantRoleCall {
                    role: *crate::base_token::authz::POLICY_ADMIN_ROLE,
                    account: admin(),
                },
            )
            .unwrap();
        token
            .set_policy_id(admin(), IBaseToken::setPolicyIdCall { newPolicyId: policy_id })
            .unwrap();

        // Mint to alice (mint goes through policy check too — alice must NOT be on the
        // blacklist yet).
        token
            .grant_role(
                admin(),
                base_precompiles_contracts::IRolesAuth::grantRoleCall {
                    role: *ISSUER_ROLE,
                    account: admin(),
                },
            )
            .unwrap();

        let alice = Address::with_last_byte(0x01);
        let bob = Address::with_last_byte(0x02);

        token
            .mint(admin(), IBaseToken::mintCall { to: alice, amount: U256::from(100u64) })
            .unwrap();

        // Blacklist bob, then attempt transfer alice->bob.
        registry
            .add_to_list(
                admin(),
                IBaseTokenPolicyRegistry::addToListCall { policyId: policy_id, account: bob },
            )
            .unwrap();

        let blocked = token
            .transfer(alice, IBaseToken::transferCall { to: bob, amount: U256::from(10u64) });
        assert!(blocked.is_err(), "blacklisted recipient must reject transfer");
    });
}

#[test]
fn pause_blocks_transfer_when_feature_enabled() {
    let mut storage = setup();
    StorageCtx::enter(&mut storage, || {
        let mut factory = BaseTokenFactory::new();
        factory.initialize().unwrap();
        BaseTokenPolicyRegistry::new().initialize().unwrap();

        let features = (Feature::Mint as u64) | (Feature::Pause as u64);
        let token_address = factory
            .create_token(
                admin(),
                IBaseTokenFactory::createTokenCall {
                    name: "Pausable".into(),
                    symbol: "P".into(),
                    decimals: 6,
                    admin: admin(),
                    features,
                    salt: unreserved_salt(),
                },
            )
            .unwrap();
        let mut token = BaseToken::from_address(token_address).unwrap();

        token
            .grant_role(
                admin(),
                base_precompiles_contracts::IRolesAuth::grantRoleCall {
                    role: *ISSUER_ROLE,
                    account: admin(),
                },
            )
            .unwrap();
        token
            .grant_role(
                admin(),
                base_precompiles_contracts::IRolesAuth::grantRoleCall {
                    role: *crate::base_token::authz::PAUSER_ROLE,
                    account: admin(),
                },
            )
            .unwrap();

        let alice = Address::with_last_byte(0x01);
        let bob = Address::with_last_byte(0x02);

        token
            .mint(admin(), IBaseToken::mintCall { to: alice, amount: U256::from(50u64) })
            .unwrap();

        token.pause(admin(), IBaseToken::pauseCall {}).unwrap();
        let result = token
            .transfer(alice, IBaseToken::transferCall { to: bob, amount: U256::from(1u64) });
        assert!(result.is_err(), "transfer must revert while paused");

        token.unpause(admin(), IBaseToken::unpauseCall {}).unwrap();
        let ok = token
            .transfer(alice, IBaseToken::transferCall { to: bob, amount: U256::from(1u64) })
            .unwrap();
        assert!(ok);
    });
}
