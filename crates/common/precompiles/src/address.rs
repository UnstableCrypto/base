//! Address helpers for Base B precompiles.

use alloy::primitives::Address;

/// B20 token address prefix.
pub const B20_PREFIX_BYTES: [u8; 12] =
    [0x84, 0x53, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

/// BaseToken (plan-2) address prefix. Sibling to [`B20_PREFIX_BYTES`]; the two stacks
/// occupy disjoint address ranges so static mempool classification can route on prefix.
pub const BASE_TOKEN_PREFIX_BYTES: [u8; 12] =
    [0xBA, 0x5E, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

/// Returns `true` when an address has the B20 prefix.
pub fn is_b20_prefix(address: &Address) -> bool {
    address.as_slice()[..B20_PREFIX_BYTES.len()] == B20_PREFIX_BYTES
}

/// Returns `true` when an address has the BaseToken prefix.
pub fn is_base_token_prefix(address: &Address) -> bool {
    address.as_slice()[..BASE_TOKEN_PREFIX_BYTES.len()] == BASE_TOKEN_PREFIX_BYTES
}

/// Local address extensions needed by the imported B precompile code.
pub trait BaseBAddressExt {
    /// Returns `true` when an address has the B20 prefix.
    fn is_b20(&self) -> bool;

    /// Returns `true` when an address has the BaseToken prefix.
    fn is_base_token(&self) -> bool;

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

    fn is_virtual(&self) -> bool {
        false
    }
}
