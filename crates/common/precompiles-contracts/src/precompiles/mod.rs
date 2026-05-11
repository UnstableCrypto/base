//! ABI bindings and constants for Base B precompiles.

use alloy_primitives::{Address, address};

mod common_errors;
pub use common_errors::*;
mod b20;
pub use b20::*;
mod b20_factory;
pub use b20_factory::*;
mod b403_registry;
pub use b403_registry::*;
mod base_token;
pub use base_token::*;
mod base_token_factory;
pub use base_token_factory::*;
mod base_token_policy_registry;
pub use base_token_policy_registry::*;

// plan_2: BaseAsset, BaseSecurity, BaseStablecoin, Base2PolicyRegistry
mod base_asset;
pub use base_asset::*;
mod base_asset_factory;
pub use base_asset_factory::*;
mod base_security;
pub use base_security::*;
mod base_security_factory;
pub use base_security_factory::*;
mod base_stablecoin;
pub use base_stablecoin::*;
mod base_stablecoin_factory;
pub use base_stablecoin_factory::*;
mod base2_policy_registry;
pub use base2_policy_registry::*;

/// Base POC B403 registry precompile address.
pub const B403_REGISTRY_ADDRESS: Address = address!("0x8453000000000000000000000000000000000403");
/// Base POC B20 factory precompile address.
pub const B20_FACTORY_ADDRESS: Address = address!("0x8453000000000000000000000000000000000001");
/// BaseToken factory precompile address (sibling to [`B20_FACTORY_ADDRESS`]).
pub const BASE_TOKEN_FACTORY_ADDRESS: Address =
    address!("0xBA5E000000000000000000000000000000000001");
/// BaseToken policy registry precompile address (sibling to [`B403_REGISTRY_ADDRESS`]).
pub const BASE_TOKEN_POLICY_REGISTRY_ADDRESS: Address =
    address!("0xBA5E000000000000000000000000000000000403");

// plan_2 factory addresses — one per token class.
/// BaseAsset factory precompile address.
pub const BASE_ASSET_FACTORY_ADDRESS: Address =
    address!("0xBA5E000A00000000000000000000000000000001");
/// BaseSecurity factory precompile address.
pub const BASE_SECURITY_FACTORY_ADDRESS: Address =
    address!("0xBA5E000B00000000000000000000000000000001");
/// BaseStablecoin factory precompile address.
pub const BASE_STABLECOIN_FACTORY_ADDRESS: Address =
    address!("0xBA5E000C00000000000000000000000000000001");
/// plan-2 policy registry — shared by BaseSecurity and BaseStablecoin, fully isolated from plan-1.
/// Uses the 0xBA5E0020 prefix (byte 4 = 0x20) to ensure no overlap with plan-1 (0xBA5E0000) or
/// plan-2 token prefixes (0xBA5E000A/B/C).
pub const BASE2_POLICY_REGISTRY_ADDRESS: Address =
    address!("0xBA5E002000000000000000000000000000000403");
