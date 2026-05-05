-- Migration 009: Add durable submission stages for outbox-dispatched work.

ALTER TABLE proof_sessions
ALTER COLUMN backend_session_id DROP NOT NULL;

DROP INDEX IF EXISTS idx_proof_sessions_active_stage;

CREATE UNIQUE INDEX idx_proof_sessions_active_stage
ON proof_sessions (proof_request_id, session_type)
WHERE status IN ('SUBMITTING', 'RUNNING');
