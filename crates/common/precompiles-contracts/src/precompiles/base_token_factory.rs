pub use IBaseTokenFactory::{
    IBaseTokenFactoryErrors as BaseTokenFactoryError,
    IBaseTokenFactoryEvents as BaseTokenFactoryEvent,
};
use alloy_primitives::Address;

crate::sol! {
    /// Factory for the BaseToken precompile family. Deploys per-token instances at
    /// `0xBA5E_PREFIX || keccak256(deployer, salt)[..8]` and binds the issuer-chosen
    /// `FeatureSet` bitmap immutably at creation time.
    #[derive(Debug, PartialEq, Eq)]
    #[sol(abi)]
    interface IBaseTokenFactory {
        error AddressReserved();
        error TokenAlreadyExists(address token);
        error InvalidFeatures(uint64 features);

        event TokenCreated(
            address indexed token,
            address indexed admin,
            string name,
            string symbol,
            uint8 decimals,
            uint64 features,
            bytes32 salt
        );

        function createToken(
            string memory name,
            string memory symbol,
            uint8 decimals,
            address admin,
            uint64 features,
            bytes32 salt
        ) external returns (address);

        function getTokenAddress(address sender, bytes32 salt) external view returns (address);
        function isBaseToken(address token) external view returns (bool);
    }
}

impl BaseTokenFactoryError {
    pub const fn address_reserved() -> Self {
        Self::AddressReserved(IBaseTokenFactory::AddressReserved {})
    }
    pub const fn token_already_exists(token: Address) -> Self {
        Self::TokenAlreadyExists(IBaseTokenFactory::TokenAlreadyExists { token })
    }
    pub const fn invalid_features(features: u64) -> Self {
        Self::InvalidFeatures(IBaseTokenFactory::InvalidFeatures { features })
    }
}
