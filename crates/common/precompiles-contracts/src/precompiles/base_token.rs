pub use IBaseToken::{IBaseTokenErrors as BaseTokenError, IBaseTokenEvents as BaseTokenEvent};
use alloy_primitives::U256;

crate::sol! {
    /// BaseToken — sibling per-token precompile to B20.
    ///
    /// One precompile instance per token, addressed at `0xBA5E…`. ERC-20 + EIP-2612 +
    /// per-token RBAC + mint/burn + pause + transfer policy. Per-token feature opt-in via
    /// a `FeatureSet` bitmap chosen at `createToken` time and immutable thereafter.
    #[derive(Debug, PartialEq, Eq)]
    #[sol(abi)]
    #[allow(clippy::too_many_arguments)]
    interface IBaseToken {
        // Standard ERC-20
        function name() external view returns (string memory);
        function symbol() external view returns (string memory);
        function decimals() external view returns (uint8);
        function totalSupply() external view returns (uint256);
        function balanceOf(address account) external view returns (uint256);
        function transfer(address to, uint256 amount) external returns (bool);
        function approve(address spender, uint256 amount) external returns (bool);
        function allowance(address owner, address spender) external view returns (uint256);
        function transferFrom(address from, address to, uint256 amount) external returns (bool);

        // Supply (gated Feature::Mint / Feature::Burn)
        function mint(address to, uint256 amount) external;
        function burn(address from, uint256 amount) external;

        // Pause (gated Feature::Pause)
        function paused() external view returns (bool);
        function pause() external;
        function unpause() external;

        // Policy (gated Feature::Policy)
        function policyId() external view returns (uint64);
        function setPolicyId(uint64 newPolicyId) external;

        // Memo overloads (gated Feature::Memo)
        function transferWithMemo(address to, uint256 amount, bytes32 memo) external returns (bool);
        function mintWithMemo(address to, uint256 amount, bytes32 memo) external;
        function burnWithMemo(address from, uint256 amount, bytes32 memo) external;

        // EIP-2612 Permit (gated Feature::Permit)
        function permit(address owner, address spender, uint256 value, uint256 deadline, uint8 v, bytes32 r, bytes32 s) external;
        function nonces(address owner) external view returns (uint256);
        function DOMAIN_SEPARATOR() external view returns (bytes32);

        // Feature introspection
        function features() external view returns (uint64);

        // Role identifiers (constants)
        function ISSUER_ROLE() external view returns (bytes32);
        function BURNER_ROLE() external view returns (bytes32);
        function PAUSER_ROLE() external view returns (bytes32);
        function POLICY_ADMIN_ROLE() external view returns (bytes32);

        // Events
        event Transfer(address indexed from, address indexed to, uint256 amount);
        event Approval(address indexed owner, address indexed spender, uint256 amount);
        event Mint(address indexed to, uint256 amount);
        event Burn(address indexed from, uint256 amount);
        event TransferWithMemo(address indexed from, address indexed to, uint256 amount, bytes32 indexed memo);
        event PauseStateUpdate(address indexed updater, bool isPaused);
        event PolicyIdUpdate(address indexed updater, uint64 indexed newPolicyId);

        // Errors
        error Uninitialized();
        error InvalidToken();
        error InvalidRecipient();
        error InsufficientBalance(uint256 available, uint256 required);
        error InsufficientAllowance();
        error ContractPaused();
        error PolicyForbids();
        error InvalidPolicyId();
        error PermitExpired();
        error InvalidSignature();
        error FeatureNotEnabled(uint64 feature);
        error Unauthorized();
    }
}

impl BaseTokenError {
    pub const fn uninitialized() -> Self {
        Self::Uninitialized(IBaseToken::Uninitialized {})
    }
    pub const fn invalid_token() -> Self {
        Self::InvalidToken(IBaseToken::InvalidToken {})
    }
    pub const fn invalid_recipient() -> Self {
        Self::InvalidRecipient(IBaseToken::InvalidRecipient {})
    }
    pub const fn insufficient_balance(available: U256, required: U256) -> Self {
        Self::InsufficientBalance(IBaseToken::InsufficientBalance { available, required })
    }
    pub const fn insufficient_allowance() -> Self {
        Self::InsufficientAllowance(IBaseToken::InsufficientAllowance {})
    }
    pub const fn contract_paused() -> Self {
        Self::ContractPaused(IBaseToken::ContractPaused {})
    }
    pub const fn policy_forbids() -> Self {
        Self::PolicyForbids(IBaseToken::PolicyForbids {})
    }
    pub const fn invalid_policy_id() -> Self {
        Self::InvalidPolicyId(IBaseToken::InvalidPolicyId {})
    }
    pub const fn permit_expired() -> Self {
        Self::PermitExpired(IBaseToken::PermitExpired {})
    }
    pub const fn invalid_signature() -> Self {
        Self::InvalidSignature(IBaseToken::InvalidSignature {})
    }
    pub const fn feature_not_enabled(feature: u64) -> Self {
        Self::FeatureNotEnabled(IBaseToken::FeatureNotEnabled { feature })
    }
    pub const fn unauthorized() -> Self {
        Self::Unauthorized(IBaseToken::Unauthorized {})
    }
}
