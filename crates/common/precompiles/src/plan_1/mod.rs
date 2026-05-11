//! Plan-1 implementation of the BaseToken precompile family.
//!
//! Container module grouping the three sibling precompiles introduced by plan-1
//! (the second token-standard implementation in this repo, alongside the legacy B20
//! stack):
//!
//! - [`base_token`] — per-token precompile addressed at `0xBA5E…`. ERC-20 + EIP-2612 +
//!   per-token RBAC + immutable `FeatureSet` opt-in.
//! - [`base_token_factory`] — singleton factory at `0xBA5E…0001`. Deploys new tokens
//!   at deterministic addresses and binds the issuer-chosen feature bitmap.
//! - [`base_token_policy_registry`] — singleton at `0xBA5E…0403`. Whitelist /
//!   blacklist policies referenced by `BaseToken::policy_id`.
//!
//! These modules are re-exported from the crate root (`crate::base_token`,
//! `crate::base_token_factory`, `crate::base_token_policy_registry`) so existing
//! call sites and tests continue to resolve without path churn.

pub mod base_token;
pub mod base_token_factory;
pub mod base_token_policy_registry;
