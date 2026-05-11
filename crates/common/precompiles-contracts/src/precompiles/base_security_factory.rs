pub use IBaseSecurityFactory::{
    IBaseSecurityFactoryErrors as BaseSecurityFactoryError,
    IBaseSecurityFactoryEvents as BaseSecurityFactoryEvent,
};
use alloy_primitives::Address;

crate::sol! {
    /// Factory for BaseSecurity tokens. Enforces class invariants at creation:
    /// non-default policyId, non-zero supplyCap, valid feature bitmap.
    #[derive(Debug, PartialEq, Eq)]
    #[sol(abi)]
    #[allow(clippy::too_many_arguments)]
    interface IBaseSecurityFactory {
        error AddressReserved();
        error TokenAlreadyExists(address token);
        error InvalidFeatures(uint8 features);
        error InvalidPolicyId();
        error InvalidSupplyCap();

        event TokenCreated(
            address indexed token,
            address indexed admin,
            string name,
            string symbol,
            uint8 decimals,
            uint64 policyId,
            uint256 supplyCap,
            uint8 features,
            bytes32 salt
        );

        function createBaseSecurity(
            string memory name,
            string memory symbol,
            uint8 decimals,
            address admin,
            uint64 policyId,
            uint256 supplyCap,
            uint8 features,
            bytes32 salt
        ) external returns (address);

        function getBaseSecurityAddress(address sender, bytes32 salt) external view returns (address);
        function isBaseSecurity(address token) external view returns (bool);
    }
}

impl BaseSecurityFactoryError {
    pub const fn address_reserved() -> Self {
        Self::AddressReserved(IBaseSecurityFactory::AddressReserved {})
    }
    pub const fn token_already_exists(token: Address) -> Self {
        Self::TokenAlreadyExists(IBaseSecurityFactory::TokenAlreadyExists { token })
    }
    pub const fn invalid_features(features: u8) -> Self {
        Self::InvalidFeatures(IBaseSecurityFactory::InvalidFeatures { features })
    }
    pub const fn invalid_policy_id() -> Self {
        Self::InvalidPolicyId(IBaseSecurityFactory::InvalidPolicyId {})
    }
    pub const fn invalid_supply_cap() -> Self {
        Self::InvalidSupplyCap(IBaseSecurityFactory::InvalidSupplyCap {})
    }
}
