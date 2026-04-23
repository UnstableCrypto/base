//! Bundle lifecycle events used by the audit pipeline.
//!
//! `BundleEvent` describes a transition in a bundle's lifetime — from initial
//! receipt through builder inclusion, mempool forwarding, cancellation, or
//! drop. Events are published to Kafka and persisted to S3 by the audit
//! archiver under the key layout returned by [`BundleEvent::s3_event_key`].

use alloy_consensus::transaction::{SignerRecoverable, Transaction as ConsensusTransaction};
use alloy_primitives::{Address, B256, TxHash, U256, keccak256};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AcceptedBundle, BundleExtensions};

/// Unique identifier for a bundle.
pub type BundleId = Uuid;

/// Reason a bundle was dropped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DropReason {
    /// Bundle timed out.
    TimedOut,
    /// Bundle transaction reverted.
    Reverted,
}

/// Identifier for a single transaction within a bundle event.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransactionId {
    /// The sender address.
    pub sender: Address,
    /// The transaction nonce.
    pub nonce: U256,
    /// The transaction hash.
    pub hash: TxHash,
}

/// Bundle lifecycle event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", content = "data")]
pub enum BundleEvent {
    /// Bundle was received.
    Received {
        /// Bundle identifier.
        bundle_id: BundleId,
        /// The accepted bundle.
        bundle: Box<AcceptedBundle>,
    },
    /// Bundle was cancelled.
    Cancelled {
        /// Bundle identifier.
        bundle_id: BundleId,
    },
    /// Bundle was included by a builder.
    BuilderIncluded {
        /// Bundle identifier.
        bundle_id: BundleId,
        /// Builder identifier.
        builder: String,
        /// Block number.
        block_number: u64,
        /// Flashblock index.
        flashblock_index: u64,
    },
    /// Bundle was included in a block.
    BlockIncluded {
        /// Bundle identifier.
        bundle_id: BundleId,
        /// Block number.
        block_number: u64,
        /// Block hash.
        block_hash: TxHash,
    },
    /// Bundle was dropped.
    Dropped {
        /// Bundle identifier.
        bundle_id: BundleId,
        /// Drop reason.
        reason: DropReason,
    },
    /// Single transaction was forwarded from a mempool node to a builder.
    ///
    /// Synthetic single-tx "bundle" — the audit server derives the
    /// `bundle_hash` from `tx_hash` using the same algorithm as
    /// [`BundleExtensions::bundle_hash`]: `keccak256(tx_hash.as_slice())`.
    MempoolForwarded {
        /// Hash of the forwarded transaction.
        tx_hash: TxHash,
    },
}

impl BundleEvent {
    /// Returns the bundle ID for this event, if it has one.
    ///
    /// `MempoolForwarded` events are derived from a single transaction hash
    /// and have no bundle UUID, so this returns `None` for that variant.
    pub const fn bundle_id(&self) -> Option<BundleId> {
        match self {
            Self::Received { bundle_id, .. }
            | Self::Cancelled { bundle_id, .. }
            | Self::BuilderIncluded { bundle_id, .. }
            | Self::BlockIncluded { bundle_id, .. }
            | Self::Dropped { bundle_id, .. } => Some(*bundle_id),
            Self::MempoolForwarded { .. } => None,
        }
    }

    /// Returns transaction IDs from this event.
    ///
    /// `Received` events recover signers from the embedded bundle's
    /// transactions. `MempoolForwarded` events expose only the tx hash
    /// (sender/nonce are not carried on the wire), so an empty vec is
    /// returned for that variant.
    pub fn transaction_ids(&self) -> Vec<TransactionId> {
        match self {
            Self::Received { bundle, .. } => bundle
                .txs
                .iter()
                .filter_map(|envelope| {
                    envelope.recover_signer().ok().map(|sender| TransactionId {
                        sender,
                        nonce: U256::from(envelope.nonce()),
                        hash: *envelope.hash(),
                    })
                })
                .collect(),
            Self::Cancelled { .. }
            | Self::BuilderIncluded { .. }
            | Self::BlockIncluded { .. }
            | Self::Dropped { .. }
            | Self::MempoolForwarded { .. } => vec![],
        }
    }

    /// Returns the `bundle_hash` for events that carry bundle data.
    ///
    /// For `MempoolForwarded`, computes the hash from the single `tx_hash`
    /// using the same algorithm as [`BundleExtensions::bundle_hash`] applied
    /// to a one-tx bundle: `keccak256(tx_hash.as_slice())`.
    pub fn bundle_hash(&self) -> Option<B256> {
        match self {
            Self::Received { bundle, .. } => Some(bundle.bundle_hash()),
            Self::MempoolForwarded { tx_hash } => Some(keccak256(tx_hash.as_slice())),
            Self::Cancelled { .. }
            | Self::BuilderIncluded { .. }
            | Self::BlockIncluded { .. }
            | Self::Dropped { .. } => None,
        }
    }

    /// Generates the event key used as both the Kafka message key and S3
    /// object name.
    ///
    /// For `Received` and `MempoolForwarded` events, the key is derived from
    /// `bundle_hash` so that the same bundle/transaction observed by
    /// different pods produces the same key (cross-pod dedup).
    pub fn generate_event_key(&self) -> String {
        match self {
            Self::Received { bundle, .. } => {
                let hash = bundle.bundle_hash();
                format!("received-{hash}")
            }
            Self::BlockIncluded { bundle_id, block_hash, .. } => {
                format!("block-included-{bundle_id}-{block_hash}")
            }
            Self::MempoolForwarded { tx_hash } => {
                let hash = keccak256(tx_hash.as_slice());
                format!("mpool-fwd-{hash}")
            }
            Self::Cancelled { bundle_id } => format!("cancelled-{bundle_id}"),
            Self::BuilderIncluded { bundle_id, .. } => format!("builder-included-{bundle_id}"),
            Self::Dropped { bundle_id, .. } => format!("dropped-{bundle_id}"),
        }
    }

    /// Returns the full S3 key for this event: `bundles/{prefix}/{event_key}`.
    ///
    /// `Received` and `MempoolForwarded` use the `bundle_hash` as the
    /// prefix; all other variants use the bundle's UUID.
    pub fn s3_event_key(&self) -> String {
        let prefix = match self {
            Self::Received { bundle, .. } => format!("{}", bundle.bundle_hash()),
            Self::MempoolForwarded { tx_hash } => {
                format!("{}", keccak256(tx_hash.as_slice()))
            }
            Self::Cancelled { bundle_id }
            | Self::BuilderIncluded { bundle_id, .. }
            | Self::BlockIncluded { bundle_id, .. }
            | Self::Dropped { bundle_id, .. } => format!("{bundle_id}"),
        };
        format!("bundles/{prefix}/{}", self.generate_event_key())
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{B256, TxHash, keccak256};

    use super::*;

    #[test]
    fn mempool_forwarded_bundle_hash_matches_single_tx_keccak() {
        let tx_hash = TxHash::from([0xab; 32]);
        let event = BundleEvent::MempoolForwarded { tx_hash };

        let expected = keccak256(tx_hash.as_slice());
        assert_eq!(event.bundle_hash(), Some(expected));
    }

    #[test]
    fn mempool_forwarded_event_key_uses_bundle_hash() {
        let tx_hash = TxHash::from([0x11; 32]);
        let event = BundleEvent::MempoolForwarded { tx_hash };

        let bundle_hash = keccak256(tx_hash.as_slice());
        assert_eq!(event.generate_event_key(), format!("mpool-fwd-{bundle_hash}"));
    }

    #[test]
    fn mempool_forwarded_s3_key_layout() {
        let tx_hash = TxHash::from([0x42; 32]);
        let event = BundleEvent::MempoolForwarded { tx_hash };

        let bundle_hash: B256 = keccak256(tx_hash.as_slice());
        let expected = format!("bundles/{bundle_hash}/mpool-fwd-{bundle_hash}");
        assert_eq!(event.s3_event_key(), expected);
    }

    #[test]
    fn mempool_forwarded_has_no_bundle_id_or_tx_ids() {
        let event = BundleEvent::MempoolForwarded { tx_hash: TxHash::from([0x99; 32]) };
        assert!(event.bundle_id().is_none());
        assert!(event.transaction_ids().is_empty());
    }
}
