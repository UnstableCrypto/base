//! Discovery v5 constants for the Base network.

/// The ENR key used to identify Base network peers.
pub const BASE_ENR_KEY: &[u8] = b"base";

/// discv5 protocol identity for the Base discovery subnetwork.
///
/// Nodes using this identity silently drop packets from standard `discv5`
/// nodes (and vice-versa), creating a dedicated discovery namespace for Base.
pub const BASE_PROTOCOL_ID: [u8; 6] = *b"basev0";
