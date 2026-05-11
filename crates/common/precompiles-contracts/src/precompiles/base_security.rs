pub use IBaseSecurity::{
    IBaseSecurityErrors as BaseSecurityError, IBaseSecurityEvents as BaseSecurityEvent,
};
use alloy_primitives::U256;

crate::sol! {
    /// BaseSecurity — regulated real-world asset token class.
    ///
    /// Structural-mandatory: PolicyHook, SupplyCap, SupplyControl, BurnBlocked.
    /// Structural-forbidden: Currency, Yield, VirtualAddress.
    /// Bitmap-optional: Memo, ForceTransfer, HolderLimit.
    #[derive(Debug, PartialEq, Eq)]
    #[sol(abi)]
    #[allow(clippy::too_many_arguments)]
    interface IBaseSecurity {
        // ERC-20
        function name() external view returns (string memory);
        function symbol() external view returns (string memory);
        function decimals() external view returns (uint8);
        function totalSupply() external view returns (uint256);
        function balanceOf(address account) external view returns (uint256);
        function transfer(address to, uint256 amount) external returns (bool);
        function approve(address spender, uint256 amount) external returns (bool);
        function allowance(address owner, address spender) external view returns (uint256);
        function transferFrom(address from, address to, uint256 amount) external returns (bool);

        // Supply — always present (structural-mandatory)
        function mint(address to, uint256 amount) external;
        function burn(uint256 amount) external;
        function burnBlocked(address from, uint256 amount) external;

        // Supply cap — always present (structural-mandatory)
        function supplyCap() external view returns (uint256);

        // Policy — always present (structural-mandatory)
        function policyId() external view returns (uint64);
        function setPolicyId(uint64 newPolicyId) external;

        // Pause
        function paused() external view returns (bool);
        function pause() external;
        function unpause() external;

        // Memo overloads — gated by SECURITY_MEMO bit
        function transferWithMemo(address to, uint256 amount, bytes32 memo) external returns (bool);
        function mintWithMemo(address to, uint256 amount, bytes32 memo) external;
        function burnWithMemo(uint256 amount, bytes32 memo) external;

        // ForceTransfer — gated by SECURITY_FORCE_TRANSFER bit
        function forceTransfer(address from, address to, uint256 amount, bytes32 reason) external;

        // HolderLimit — gated by SECURITY_HOLDER_LIMIT bit
        function holderCount() external view returns (uint64);
        function holderLimit() external view returns (uint64);

        // EIP-2612 Permit
        function permit(address owner, address spender, uint256 value, uint256 deadline, uint8 v, bytes32 r, bytes32 s) external;
        function nonces(address owner) external view returns (uint256);
        function DOMAIN_SEPARATOR() external view returns (bytes32);

        // RBAC
        function hasRole(bytes32 role, address account) external view returns (bool);
        function grantRole(bytes32 role, address account) external;
        function revokeRole(bytes32 role, address account) external;
        function renounceRole(bytes32 role) external;

        // Discovery
        function features() external view returns (uint8);
        function assetClass() external view returns (uint8);

        // Role identifiers
        function ISSUER_ROLE() external view returns (bytes32);
        function PAUSER_ROLE() external view returns (bytes32);
        function BURN_BLOCKED_ROLE() external view returns (bytes32);
        function FORCE_TRANSFER_ROLE() external view returns (bytes32);
        function POLICY_ADMIN_ROLE() external view returns (bytes32);

        // Events
        event Transfer(address indexed from, address indexed to, uint256 amount);
        event Approval(address indexed owner, address indexed spender, uint256 amount);
        event Mint(address indexed to, uint256 amount);
        event Burn(address indexed from, uint256 amount);
        event BurnBlocked(address indexed from, uint256 amount);
        event ForceTransfer(address indexed from, address indexed to, uint256 amount, bytes32 indexed reason);
        event TransferWithMemo(address indexed from, address indexed to, uint256 amount, bytes32 indexed memo);
        event PauseStateUpdate(address indexed updater, bool isPaused);
        event PolicyIdUpdate(address indexed updater, uint64 indexed newPolicyId);
        event RoleGranted(bytes32 indexed role, address indexed account, address indexed sender);
        event RoleRevoked(bytes32 indexed role, address indexed account, address indexed sender);

        // Errors
        error Uninitialized();
        error InvalidToken();
        error InvalidRecipient();
        error InsufficientBalance(uint256 available, uint256 required);
        error InsufficientAllowance();
        error ContractPaused();
        error PolicyForbids();
        error InvalidPolicyId();
        error SupplyCapExceeded();
        error HolderLimitReached();
        error PermitExpired();
        error InvalidSignature();
        error FeatureNotEnabled(uint8 feature);
        error Unauthorized();
    }
}

impl BaseSecurityError {
    pub const fn uninitialized() -> Self {
        Self::Uninitialized(IBaseSecurity::Uninitialized {})
    }
    pub const fn invalid_token() -> Self {
        Self::InvalidToken(IBaseSecurity::InvalidToken {})
    }
    pub const fn invalid_recipient() -> Self {
        Self::InvalidRecipient(IBaseSecurity::InvalidRecipient {})
    }
    pub const fn insufficient_balance(available: U256, required: U256) -> Self {
        Self::InsufficientBalance(IBaseSecurity::InsufficientBalance { available, required })
    }
    pub const fn insufficient_allowance() -> Self {
        Self::InsufficientAllowance(IBaseSecurity::InsufficientAllowance {})
    }
    pub const fn contract_paused() -> Self {
        Self::ContractPaused(IBaseSecurity::ContractPaused {})
    }
    pub const fn policy_forbids() -> Self {
        Self::PolicyForbids(IBaseSecurity::PolicyForbids {})
    }
    pub const fn invalid_policy_id() -> Self {
        Self::InvalidPolicyId(IBaseSecurity::InvalidPolicyId {})
    }
    pub const fn supply_cap_exceeded() -> Self {
        Self::SupplyCapExceeded(IBaseSecurity::SupplyCapExceeded {})
    }
    pub const fn holder_limit_reached() -> Self {
        Self::HolderLimitReached(IBaseSecurity::HolderLimitReached {})
    }
    pub const fn permit_expired() -> Self {
        Self::PermitExpired(IBaseSecurity::PermitExpired {})
    }
    pub const fn invalid_signature() -> Self {
        Self::InvalidSignature(IBaseSecurity::InvalidSignature {})
    }
    pub const fn feature_not_enabled(feature: u8) -> Self {
        Self::FeatureNotEnabled(IBaseSecurity::FeatureNotEnabled { feature })
    }
    pub const fn unauthorized() -> Self {
        Self::Unauthorized(IBaseSecurity::Unauthorized {})
    }
}
