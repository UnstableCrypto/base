#![doc = include_str!("../README.md")]

mod enode;
pub use enode::enode_to_multiaddr;

mod discv4_prober;

mod discoverer;
pub use discoverer::{DiscoveredPeer, DiscoveryUpdate, run_discovery_service};

mod fork_id;
pub use fork_id::{
    ALL_DISTANCES, BASE_MAINNET_FORK_HASH_AZUL, BASE_MAINNET_FORK_HASH_JOVIAN,
    BASE_SEPOLIA_FORK_HASH, BASE_SEPOLIA_FORK_HASH_JOVIAN,
    BASE_ZERONET_FORK_HASH_AZUL, BASE_ZERONET_FORK_HASH_JOVIAN,
    fork_hash_for_chain, network_tag, target_prefix_for_network,
};

mod prober;
pub use prober::{BootnodeProber, BootnodeResult, BootnodeSnapshot, PeerEntry};

mod poller;
pub use poller::run_bootnode_poller;
