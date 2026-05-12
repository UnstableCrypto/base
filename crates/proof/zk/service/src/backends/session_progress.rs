//! Derives `ProofProcessingResult` from stored `proof_sessions` rows for any proving backend.
//!
//! Success criteria depend on `ProofType` from `base_zk_db`: today that covers the Succinct SP1
//! cluster pipeline. When new `ProofType` variants are added, give them explicit match arms here
//! so every proving backend shares one definition of done vs still running.

use base_zk_db::{
    ProofSession, ProofStatus, ProofType, SessionStatus as DbSessionStatus, SessionType,
};

use crate::backends::traits::ProofProcessingResult;

/// Maps DB session state to a `ProofProcessingResult` for the given proof type.
#[derive(Debug, Clone, Copy)]
pub struct ProofSessionProgress;

impl ProofSessionProgress {
    /// Derive the proof request processing status from session rows.
    pub fn processing_result(
        proof_type: ProofType,
        sessions: &[ProofSession],
    ) -> ProofProcessingResult {
        if sessions.is_empty() {
            return ProofProcessingResult { status: ProofStatus::Pending, error_message: None };
        }

        for session in sessions {
            if session.status == DbSessionStatus::Failed {
                return ProofProcessingResult {
                    status: ProofStatus::Failed,
                    error_message: session.error_message.clone(),
                };
            }
        }

        match proof_type {
            ProofType::OpSuccinctSp1ClusterCompressed => {
                let all_completed = sessions.iter().all(|s| s.status == DbSessionStatus::Completed);
                if all_completed {
                    ProofProcessingResult { status: ProofStatus::Succeeded, error_message: None }
                } else {
                    ProofProcessingResult { status: ProofStatus::Running, error_message: None }
                }
            }
            ProofType::OpSuccinctSp1ClusterSnarkGroth16 => {
                let stark_done = sessions.iter().any(|s| {
                    s.session_type == SessionType::Stark && s.status == DbSessionStatus::Completed
                });
                let snark_done = sessions.iter().any(|s| {
                    s.session_type == SessionType::Snark && s.status == DbSessionStatus::Completed
                });
                if stark_done && snark_done {
                    ProofProcessingResult { status: ProofStatus::Succeeded, error_message: None }
                } else {
                    ProofProcessingResult { status: ProofStatus::Running, error_message: None }
                }
            }
        }
    }
}
