//! OP Succinct proving backends.

mod cluster;
pub use cluster::ClusterBackend;

mod mock;
pub use mock::MockBackend;

mod network;
pub use network::NetworkBackend;

mod provider;
pub use provider::{OpSuccinctProvider, WitnessParams};

mod snark_session;
pub use snark_session::{SnarkSession, SnarkSessionRunOutcome};
