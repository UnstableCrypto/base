-- Migration 009: Enforce at most one ACTIVE session per (proof_request, session_type)
-- Prevents concurrent status pollers from creating duplicate SNARK aggregation jobs.
--
-- The index is partial on SUBMITTING/RUNNING so terminal sessions (FAILED/COMPLETED)
-- remain as audit history without blocking a retried request from creating a fresh
-- session for the same (proof_request_id, session_type) pair.

-- Resolve any pre-existing duplicates among ACTIVE sessions before adding the constraint.
-- Terminal-state duplicates are left alone since the partial index does not see them.
WITH ranked_active AS (
    SELECT
        id,
        ROW_NUMBER() OVER (
            PARTITION BY proof_request_id, session_type
            ORDER BY
                CASE status
                    WHEN 'RUNNING' THEN 0
                    WHEN 'SUBMITTING' THEN 1
                    ELSE 2
                END,
                created_at ASC,
                id ASC
        ) AS row_num
    FROM proof_sessions
    WHERE status IN ('SUBMITTING', 'RUNNING')
)
DELETE FROM proof_sessions
USING ranked_active
WHERE proof_sessions.id = ranked_active.id
  AND ranked_active.row_num > 1;

CREATE UNIQUE INDEX IF NOT EXISTS idx_proof_sessions_request_type_active_unique
ON proof_sessions(proof_request_id, session_type)
WHERE status IN ('SUBMITTING', 'RUNNING');

COMMENT ON COLUMN proof_sessions.status IS 'Session status: SUBMITTING, RUNNING, COMPLETED, FAILED';
