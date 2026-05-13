//! Proof storage pruner for removing stale trie data.

mod error;
pub use error::{UnstableProofStoragePrunerResult, PrunerError, PrunerOutput};

mod pruner;
pub use pruner::UnstableProofStoragePruner;

mod metrics;

mod task;
pub use task::UnstableProofStoragePrunerTask;
