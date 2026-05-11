pub use IBaseAssetFactory::{
    IBaseAssetFactoryErrors as BaseAssetFactoryError,
    IBaseAssetFactoryEvents as BaseAssetFactoryEvent,
};
use alloy_primitives::Address;

crate::sol! {
    /// Factory for BaseAsset tokens. Creates permissionless per-token instances at
    /// `0xBA5E000A || keccak256(deployer, salt)[..8]`. Feature bitmap is immutable at creation.
    #[derive(Debug, PartialEq, Eq)]
    #[sol(abi)]
    interface IBaseAssetFactory {
        error AddressReserved();
        error TokenAlreadyExists(address token);
        error InvalidFeatures(uint8 features);
        error InvalidSupplyCap();

        event TokenCreated(
            address indexed token,
            address indexed admin,
            string name,
            string symbol,
            uint8 decimals,
            uint8 features,
            bytes32 salt
        );

        function createBaseAsset(
            string memory name,
            string memory symbol,
            uint8 decimals,
            address admin,
            uint8 features,
            bytes32 salt
        ) external returns (address);

        function getBaseAssetAddress(address sender, bytes32 salt) external view returns (address);
        function isBaseAsset(address token) external view returns (bool);
    }
}

impl BaseAssetFactoryError {
    pub const fn address_reserved() -> Self {
        Self::AddressReserved(IBaseAssetFactory::AddressReserved {})
    }
    pub const fn token_already_exists(token: Address) -> Self {
        Self::TokenAlreadyExists(IBaseAssetFactory::TokenAlreadyExists { token })
    }
    pub const fn invalid_features(features: u8) -> Self {
        Self::InvalidFeatures(IBaseAssetFactory::InvalidFeatures { features })
    }
    pub const fn invalid_supply_cap() -> Self {
        Self::InvalidSupplyCap(IBaseAssetFactory::InvalidSupplyCap {})
    }
}
