//! Shared base for plan-2 token classes.
//!
//! `TokenCore` is the concrete base type holding all common EVM storage (slots 0-9)
//! and all behavior identical across BaseAsset, BaseSecurity, and BaseStablecoin.
//! Neither `token/` nor `stablecoin/` depends on the other — both depend only on
//! this shared module.

pub mod core;
pub use core::{DEFAULT_ADMIN_ROLE, TokenCore, TransferKind, UNGRANTABLE_ROLE};
