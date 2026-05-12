//! SNARK session reservation helpers.

use std::future::Future;

use base_zk_db::{
    CreateProofSession, ProofRequest, ProofRequestRepo, ProofSession, ProofType,
    SessionStatus as DbSessionStatus, SessionType,
};
use tracing::warn;

/// Result of [`SnarkSession::run_if_needed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnarkSessionRunOutcome {
    /// No SNARK stage to start for this proof request and session list.
    NotNeeded,
    /// Another worker holds the active reservation or session; this caller did not insert a row.
    ReservationNotAcquired,
    /// Reserved row was updated to `RUNNING` with the backend session id from `submit`.
    Activated,
    /// `submit` succeeded but the reservation row was no longer eligible to activate (reaper or race).
    ActivationDidNotApply,
}

impl SnarkSessionRunOutcome {
    /// Whether the caller should re-load sessions from the database before inferring status.
    pub const fn should_refresh_sessions(self) -> bool {
        !matches!(self, Self::NotNeeded)
    }
}

/// Helper for atomically claiming and activating the SNARK aggregation stage.
#[derive(Debug)]
pub struct SnarkSession;

impl SnarkSession {
    /// Return true when a proof request has completed STARK work and needs a SNARK session.
    ///
    /// Any existing SNARK session (including `Failed`) is treated as "already done": a failed
    /// SNARK is intentionally terminal, and retries happen by creating a new proof request.
    pub fn should_start(proof_request: &ProofRequest, sessions: &[ProofSession]) -> bool {
        if proof_request.proof_type != ProofType::OpSuccinctSp1ClusterSnarkGroth16 {
            return false;
        }

        let has_stark_completed = sessions.iter().any(|s| {
            s.session_type == SessionType::Stark && s.status == DbSessionStatus::Completed
        });
        let has_snark_session = sessions.iter().any(|s| s.session_type == SessionType::Snark);

        has_stark_completed && !has_snark_session
    }

    /// Reserve the SNARK slot, run `submit`, and activate (or fail) the row atomically.
    ///
    /// `submit` is only invoked when this caller wins the reservation race, which is what
    /// prevents duplicate Groth16 jobs from being enqueued.
    pub async fn run_if_needed<F, Fut>(
        repo: &ProofRequestRepo,
        proof_request: &ProofRequest,
        sessions: &[ProofSession],
        submit: F,
    ) -> anyhow::Result<SnarkSessionRunOutcome>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = anyhow::Result<CreateProofSession>>,
    {
        if !Self::should_start(proof_request, sessions) {
            return Ok(SnarkSessionRunOutcome::NotNeeded);
        }

        let Some(reservation_id) =
            repo.reserve_proof_session(proof_request.id, SessionType::Snark).await?
        else {
            return Ok(SnarkSessionRunOutcome::ReservationNotAcquired);
        };

        match submit().await {
            Ok(session) => {
                let backend_session_id = session.backend_session_id.clone();
                let activated = match repo
                    .activate_reserved_proof_session(&reservation_id, session)
                    .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        let error_message = format!(
                            "failed to activate reserved SNARK session after submission: {e}"
                        );
                        if let Err(fail_err) = repo
                            .fail_reserved_proof_session(
                                proof_request.id,
                                SessionType::Snark,
                                &reservation_id,
                                &error_message,
                            )
                            .await
                        {
                            warn!(
                                proof_request_id = %proof_request.id,
                                reservation_id = %reservation_id,
                                error = %fail_err,
                                "failed to mark reserved SNARK proof session as failed after activation error"
                            );
                        }
                        return Err(e.into());
                    }
                };

                if !activated {
                    warn!(
                        proof_request_id = %proof_request.id,
                        reservation_id = %reservation_id,
                        backend_session_id = %backend_session_id,
                        "SNARK reservation was cleaned up before activation; backend job may still run"
                    );
                    return Ok(SnarkSessionRunOutcome::ActivationDidNotApply);
                }
                Ok(SnarkSessionRunOutcome::Activated)
            }
            Err(e) => {
                let error_message = format!("failed to submit aggregation proof: {e}");
                if let Err(fail_err) = repo
                    .fail_reserved_proof_session(
                        proof_request.id,
                        SessionType::Snark,
                        &reservation_id,
                        &error_message,
                    )
                    .await
                {
                    warn!(
                        proof_request_id = %proof_request.id,
                        reservation_id = %reservation_id,
                        error = %fail_err,
                        "failed to mark reserved SNARK proof session as failed"
                    );
                }
                Err(e)
            }
        }
    }
}
