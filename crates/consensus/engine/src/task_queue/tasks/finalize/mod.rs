//! Error type for finalizing an L2 block.

mod error;
pub use error::FinalizeTaskError;

#[cfg(test)]
mod task_test;
