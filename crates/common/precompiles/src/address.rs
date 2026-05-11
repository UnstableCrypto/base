//! Address helpers for Base B precompiles.

use alloy::primitives::Address;

/// B20 token address prefix.
pub const B20_PREFIX_BYTES: [u8; 12] =
    [0x84, 0x53, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

/// BaseToken (plan-1) address prefix. Sibling to [`B20_PREFIX_BYTES`]; the two stacks
/// occupy disjoint address ranges so static mempool classification can route on prefix.
pub const BASE_TOKEN_PREFIX_BYTES: [u8; 12] =
    [0xBA, 0x5E, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

// plan_2 per-class address prefixes. Each class has its own 12-byte prefix so block builders
// and mempool classifiers can identify the class from the address without reading state.

/// BaseAsset (plan-2) address prefix — `0xBA5E000A`.
pub const BASE_ASSET_PREFIX_BYTES: [u8; 12] =
    [0xBA, 0x5E, 0x00, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

/// BaseSecurity (plan-2) address prefix — `0xBA5E000B`.
pub const BASE_SECURITY_PREFIX_BYTES: [u8; 12] =
    [0xBA, 0x5E, 0x00, 0x0B, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

/// BaseStablecoin (plan-2) address prefix — `0xBA5E000C`.
pub const BASE_STABLECOIN_PREFIX_BYTES: [u8; 12] =
    [0xBA, 0x5E, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

/// Returns `true` when an address has the B20 prefix.
pub fn is_b20_prefix(address: &Address) -> bool {
    address.as_slice()[..B20_PREFIX_BYTES.len()] == B20_PREFIX_BYTES
}

/// Returns `true` when an address has the BaseToken (plan-1) prefix.
pub fn is_base_token_prefix(address: &Address) -> bool {
    address.as_slice()[..BASE_TOKEN_PREFIX_BYTES.len()] == BASE_TOKEN_PREFIX_BYTES
}

/// Returns `true` when an address has the BaseAsset (plan-2) prefix.
pub fn is_base_asset_prefix(address: &Address) -> bool {
    address.as_slice()[..BASE_ASSET_PREFIX_BYTES.len()] == BASE_ASSET_PREFIX_BYTES
}

/// Returns `true` when an address has the BaseSecurity (plan-2) prefix.
pub fn is_base_security_prefix(address: &Address) -> bool {
    address.as_slice()[..BASE_SECURITY_PREFIX_BYTES.len()] == BASE_SECURITY_PREFIX_BYTES
}

/// Returns `true` when an address has the BaseStablecoin (plan-2) prefix.
pub fn is_base_stablecoin_prefix(address: &Address) -> bool {
    address.as_slice()[..BASE_STABLECOIN_PREFIX_BYTES.len()] == BASE_STABLECOIN_PREFIX_BYTES
}

/// Local address extensions needed by the imported B precompile code.
pub trait BaseBAddressExt {
    /// Returns `true` when an address has the B20 prefix.
    fn is_b20(&self) -> bool;

    /// Returns `true` when an address has the BaseToken (plan-1) prefix.
    fn is_base_token(&self) -> bool;

    /// Returns `true` when an address has the BaseAsset (plan-2) prefix.
    fn is_base_asset(&self) -> bool;

    /// Returns `true` when an address has the BaseSecurity (plan-2) prefix.
    fn is_base_security(&self) -> bool;

    /// Returns `true` when an address has the BaseStablecoin (plan-2) prefix.
    fn is_base_stablecoin(&self) -> bool;

    /// Returns `true` when an address is a virtual account address.
    fn is_virtual(&self) -> bool;
}

impl BaseBAddressExt for Address {
    fn is_b20(&self) -> bool {
        is_b20_prefix(self)
    }

    fn is_base_token(&self) -> bool {
        is_base_token_prefix(self)
    }

    fn is_base_asset(&self) -> bool {
        is_base_asset_prefix(self)
    }

    fn is_base_security(&self) -> bool {
        is_base_security_prefix(self)
    }

    fn is_base_stablecoin(&self) -> bool {
        is_base_stablecoin_prefix(self)
    }

    fn is_virtual(&self) -> bool {
        false
    }
}
