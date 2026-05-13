//! Block type for Unstable chains.

use crate::UnstableTxEnvelope;

/// A block type for Unstable chains.
pub type UnstableBlock = alloy_consensus::Block<UnstableTxEnvelope>;
