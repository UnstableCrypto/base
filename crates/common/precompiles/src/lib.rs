#![doc = include_str!("../README.md")]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod provider;
pub use provider::BasePrecompiles;

mod spec;
pub use spec::BasePrecompileSpec;

#[cfg(feature = "native-dex")]
mod dex;
#[cfg(feature = "native-dex")]
pub use dex::{BASE_DEX_ADDRESS, BaseDexPrecompile};

mod bn254_pair;

mod bls12_381;
