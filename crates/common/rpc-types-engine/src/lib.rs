#![doc = include_str!("../README.md")]
#![doc(
    html_logo_url = "https://avatars.githubusercontent.com/u/16627100?s=200&v=4",
    html_favicon_url = "https://avatars.githubusercontent.com/u/16627100?s=200&v=4",
    issue_tracker_base_url = "https://github.com/base/base/issues/"
)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod attributes;
pub use attributes::UnstablePayloadAttributes;

mod envelope;
pub use envelope::{
    UnstableExecutionPayloadEnvelope, ExecutionData, MAX_DECOMPRESSED_ENVELOPE_BYTES,
    NetworkPayloadEnvelope, PayloadEnvelopeEncodeError, PayloadEnvelopeError, PayloadHash,
};

mod sidecar;
pub use sidecar::UnstableExecutionPayloadSidecar;

mod payload;
pub use payload::{
    UnstableExecutionPayload, UnstableExecutionPayloadEnvelopeV3, UnstableExecutionPayloadEnvelopeV4,
    UnstableExecutionPayloadEnvelopeV5, UnstableExecutionPayloadV4, UnstablePayloadError,
};

#[cfg(feature = "reth")]
mod reth;
