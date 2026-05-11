pub use IBase2PolicyRegistry::{
    IBase2PolicyRegistryErrors as Base2PolicyRegistryError,
    IBase2PolicyRegistryEvents as Base2PolicyRegistryEvent,
};

crate::sol! {
    /// Transfer-policy registry for the plan-2 token family (BaseAsset, BaseSecurity, BaseStablecoin).
    /// Singleton precompile. Built-in policy id `1` always authorizes; `0` always rejects.
    #[derive(Debug, PartialEq, Eq)]
    #[sol(abi)]
    interface IBase2PolicyRegistry {
        // PolicyKind: 0 = WHITELIST, 1 = BLACKLIST
        enum PolicyKind {
            WHITELIST,
            BLACKLIST
        }

        function policyIdCounter() external view returns (uint64);
        function policyExists(uint64 policyId) external view returns (bool);
        function isAuthorized(uint64 policyId, address from, address to) external view returns (bool);
        function policyAdmin(uint64 policyId) external view returns (address);
        function policyKind(uint64 policyId) external view returns (PolicyKind);

        function createPolicy(address admin, PolicyKind kind) external returns (uint64);
        function addToList(uint64 policyId, address account) external;
        function removeFromList(uint64 policyId, address account) external;
        function setPolicyAdmin(uint64 policyId, address newAdmin) external;

        event PolicyCreated(uint64 indexed policyId, address indexed admin, PolicyKind kind);
        event ListUpdated(uint64 indexed policyId, address indexed account, bool present);
        event PolicyAdminUpdated(uint64 indexed policyId, address indexed newAdmin);

        error PolicyNotFound();
        error InvalidPolicyKind();
        error Unauthorized();
    }
}

impl Base2PolicyRegistryError {
    pub const fn policy_not_found() -> Self {
        Self::PolicyNotFound(IBase2PolicyRegistry::PolicyNotFound {})
    }
    pub const fn invalid_policy_kind() -> Self {
        Self::InvalidPolicyKind(IBase2PolicyRegistry::InvalidPolicyKind {})
    }
    pub const fn unauthorized() -> Self {
        Self::Unauthorized(IBase2PolicyRegistry::Unauthorized {})
    }
}
