-- Migration 009: Enforce one session row per proof request and session type
-- Prevents concurrent status pollers from creating duplicate SNARK aggregation jobs.

-- Keep the most useful row if duplicates already exist, then enforce the invariant.
WITH ranked_sessions AS (
    SELECT
        id,
        ROW_NUMBER() OVER (
            PARTITION BY proof_request_id, session_type
            ORDER BY
                CASE status
                    WHEN 'COMPLETED' THEN 0
                    WHEN 'RUNNING' THEN 1
                    WHEN 'SUBMITTING' THEN 2
                    WHEN 'FAILED' THEN 3
                    ELSE 4
                END,
                created_at ASC,
                id ASC
        ) AS row_num
    FROM proof_sessions
)
DELETE FROM proof_sessions
USING ranked_sessions
WHERE proof_sessions.id = ranked_sessions.id
  AND ranked_sessions.row_num > 1;

CREATE UNIQUE INDEX IF NOT EXISTS idx_proof_sessions_request_type_unique
ON proof_sessions(proof_request_id, session_type);

COMMENT ON COLUMN proof_sessions.status IS 'Session status: SUBMITTING, RUNNING, COMPLETED, FAILED';
