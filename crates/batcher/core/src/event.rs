//! Internal driver event type produced by the `tokio::select!` I/O phase.

use base_batcher_encoder::SubmissionId;
use base_common_consensus::BaseBlock;
use base_protocol::L2BlockInfo;

use crate::TxOutcome;

/// Result of an asynchronous local safe head lookup for a reset boundary.
#[derive(Debug)]
pub enum ResetSafeHeadResult {
    /// The lookup task returned a reset boundary.
    Resolved {
        /// Safe head to catch up from, when one is available.
        safe_head: Option<u64>,
    },
    /// The lookup task closed before sending a result, leaving only the watch fallback.
    Closed {
        /// Last local safe head observed on the watch channel before the lookup started.
        fallback: Option<u64>,
    },
}

/// Events the driver can receive from external sources during the I/O phase.
#[derive(Debug)]
pub enum DriverEvent {
    /// Cancellation token fired, or L2 source signalled exhausted.
    Shutdown,
    /// New L2 unsafe block from the source.
    Block(Box<BaseBlock>),
    /// Source requested a force-flush of the current channel.
    Flush,
    /// L2 reorganisation; new safe head provided.
    Reorg(L2BlockInfo),
    /// An in-flight L1 transaction settled, carrying one or more packed submissions.
    Receipt(Vec<SubmissionId>, TxOutcome),
    /// L1 chain head advanced.
    L1Head(u64),
    /// Safe L2 head advanced (from watch channel).
    SafeHead(u64),
    /// Fresh local safe head lookup completed for a reset boundary.
    ResetSafeHead(ResetSafeHeadResult),
    /// L1 head source permanently closed (Exhausted or Closed error).
    L1SourceClosed,
}
