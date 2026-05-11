pub use IBaseTokenPolicyRegistry::{
    IBaseTokenPolicyRegistryErrors as BaseTokenPolicyRegistryError,
    IBaseTokenPolicyRegistryEvents as BaseTokenPolicyRegistryEvent,
};

crate::sol! {
    /// Policy registry for the BaseToken family. Singleton precompile holding allowlist /
    /// blocklist policies that BaseTokens reference via `policyId`. Built-in policy id `1`
    /// is the universal "allow all" sentinel — lookups against id `1` short-circuit.
    #[derive(Debug, PartialEq, Eq)]
    #[sol(abi)]
    interface IBaseTokenPolicyRegistry {
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

impl BaseTokenPolicyRegistryError {
    pub const fn policy_not_found() -> Self {
        Self::PolicyNotFound(IBaseTokenPolicyRegistry::PolicyNotFound {})
    }
    pub const fn invalid_policy_kind() -> Self {
        Self::InvalidPolicyKind(IBaseTokenPolicyRegistry::InvalidPolicyKind {})
    }
    pub const fn unauthorized() -> Self {
        Self::Unauthorized(IBaseTokenPolicyRegistry::Unauthorized {})
    }
}
