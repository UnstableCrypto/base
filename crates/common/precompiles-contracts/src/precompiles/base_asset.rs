pub use IBaseAsset::{IBaseAssetErrors as BaseAssetError, IBaseAssetEvents as BaseAssetEvent};
use alloy_primitives::U256;

crate::sol! {
    /// BaseAsset — permissionless onchain-native token class.
    ///
    /// Structurally cannot have PolicyHook, BurnBlocked, ForceTransfer, HolderLimit, or Currency.
    /// Optional features (SupplyControl, Memo, SupplyCap) are selected via an immutable
    /// per-token bitmap at creation time.
    #[derive(Debug, PartialEq, Eq)]
    #[sol(abi)]
    #[allow(clippy::too_many_arguments)]
    interface IBaseAsset {
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

        // Supply — gated by ASSET_SUPPLY_CONTROL bit
        function mint(address to, uint256 amount) external;
        function burn(uint256 amount) external;

        // Supply cap — gated by ASSET_SUPPLY_CAP bit
        function supplyCap() external view returns (uint256);
        function setSupplyCap(uint256 newCap) external;

        // Pause
        function paused() external view returns (bool);
        function pause() external;
        function unpause() external;

        // Memo overloads — gated by ASSET_MEMO bit
        function transferWithMemo(address to, uint256 amount, bytes32 memo) external returns (bool);
        function mintWithMemo(address to, uint256 amount, bytes32 memo) external;
        function burnWithMemo(uint256 amount, bytes32 memo) external;

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

        // Events
        event Transfer(address indexed from, address indexed to, uint256 amount);
        event Approval(address indexed owner, address indexed spender, uint256 amount);
        event Mint(address indexed to, uint256 amount);
        event Burn(address indexed from, uint256 amount);
        event TransferWithMemo(address indexed from, address indexed to, uint256 amount, bytes32 indexed memo);
        event PauseStateUpdate(address indexed updater, bool isPaused);
        event SupplyCapUpdate(address indexed updater, uint256 newSupplyCap);
        event RoleGranted(bytes32 indexed role, address indexed account, address indexed sender);
        event RoleRevoked(bytes32 indexed role, address indexed account, address indexed sender);

        // Errors
        error Uninitialized();
        error InvalidToken();
        error InvalidRecipient();
        error InsufficientBalance(uint256 available, uint256 required);
        error InsufficientAllowance();
        error ContractPaused();
        error SupplyCapExceeded();
        error PermitExpired();
        error InvalidSignature();
        error FeatureNotEnabled(uint8 feature);
        error Unauthorized();
    }
}

impl BaseAssetError {
    pub const fn uninitialized() -> Self {
        Self::Uninitialized(IBaseAsset::Uninitialized {})
    }
    pub const fn invalid_token() -> Self {
        Self::InvalidToken(IBaseAsset::InvalidToken {})
    }
    pub const fn invalid_recipient() -> Self {
        Self::InvalidRecipient(IBaseAsset::InvalidRecipient {})
    }
    pub const fn insufficient_balance(available: U256, required: U256) -> Self {
        Self::InsufficientBalance(IBaseAsset::InsufficientBalance { available, required })
    }
    pub const fn insufficient_allowance() -> Self {
        Self::InsufficientAllowance(IBaseAsset::InsufficientAllowance {})
    }
    pub const fn contract_paused() -> Self {
        Self::ContractPaused(IBaseAsset::ContractPaused {})
    }
    pub const fn supply_cap_exceeded() -> Self {
        Self::SupplyCapExceeded(IBaseAsset::SupplyCapExceeded {})
    }
    pub const fn permit_expired() -> Self {
        Self::PermitExpired(IBaseAsset::PermitExpired {})
    }
    pub const fn invalid_signature() -> Self {
        Self::InvalidSignature(IBaseAsset::InvalidSignature {})
    }
    pub const fn feature_not_enabled(feature: u8) -> Self {
        Self::FeatureNotEnabled(IBaseAsset::FeatureNotEnabled { feature })
    }
    pub const fn unauthorized() -> Self {
        Self::Unauthorized(IBaseAsset::Unauthorized {})
    }
}
