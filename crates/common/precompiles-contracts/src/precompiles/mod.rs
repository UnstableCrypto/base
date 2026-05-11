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
