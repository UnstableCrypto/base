//! Plan-2 hybrid token precompile family.
//!
//! Architecture:
//! - `shared/` — `TokenCore` base class with all common ERC-20/RBAC/pause/permit behavior
//! - `token/` — "token" recipe: BaseAsset + BaseSecurity (no stablecoin dependency)
//! - `stablecoin/` — "stablecoin" recipe: BaseStablecoin (no token dependency)
//! - `policy_registry/` — Base2PolicyRegistry (shared by Security and Stablecoin)
//! - Factories — one per class

pub mod shared;
pub mod token;
pub mod stablecoin;
pub mod policy_registry;
pub mod base_asset_factory;
pub mod base_security_factory;
pub mod base_stablecoin_factory;

pub use token::{BaseAsset, BaseSecurity};
pub use stablecoin::BaseStablecoin;
pub use policy_registry::Base2PolicyRegistry;
pub use base_asset_factory::BaseAssetFactory;
pub use base_security_factory::BaseSecurityFactory;
pub use base_stablecoin_factory::BaseStablecoinFactory;
