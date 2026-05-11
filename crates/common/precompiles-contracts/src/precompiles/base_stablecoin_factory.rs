pub use IBaseStablecoinFactory::{
    IBaseStablecoinFactoryErrors as BaseStablecoinFactoryError,
    IBaseStablecoinFactoryEvents as BaseStablecoinFactoryEvent,
};
use alloy_primitives::Address;

crate::sol! {
    /// Factory for BaseStablecoin tokens. Enforces class invariants at creation:
    /// valid 3-letter ISO 4217 uppercase currency code, non-default policyId.
    #[derive(Debug, PartialEq, Eq)]
    #[sol(abi)]
    #[allow(clippy::too_many_arguments)]
    interface IBaseStablecoinFactory {
        error AddressReserved();
        error TokenAlreadyExists(address token);
        error InvalidFeatures(uint8 features);
        error InvalidPolicyId();
        error InvalidCurrency();

        event TokenCreated(
            address indexed token,
            address indexed admin,
            string name,
            string symbol,
            uint8 decimals,
            string currency,
            uint64 policyId,
            uint8 features,
            bytes32 salt
        );

        function createBaseStablecoin(
            string memory name,
            string memory symbol,
            uint8 decimals,
            address admin,
            string memory currency,
            uint64 policyId,
            uint8 features,
            bytes32 salt
        ) external returns (address);

        function getBaseStablecoinAddress(address sender, bytes32 salt) external view returns (address);
        function isBaseStablecoin(address token) external view returns (bool);
    }
}

impl BaseStablecoinFactoryError {
    pub const fn address_reserved() -> Self {
        Self::AddressReserved(IBaseStablecoinFactory::AddressReserved {})
    }
    pub const fn token_already_exists(token: Address) -> Self {
        Self::TokenAlreadyExists(IBaseStablecoinFactory::TokenAlreadyExists { token })
    }
    pub const fn invalid_features(features: u8) -> Self {
        Self::InvalidFeatures(IBaseStablecoinFactory::InvalidFeatures { features })
    }
    pub const fn invalid_policy_id() -> Self {
        Self::InvalidPolicyId(IBaseStablecoinFactory::InvalidPolicyId {})
    }
    pub const fn invalid_currency() -> Self {
        Self::InvalidCurrency(IBaseStablecoinFactory::InvalidCurrency {})
    }
}
