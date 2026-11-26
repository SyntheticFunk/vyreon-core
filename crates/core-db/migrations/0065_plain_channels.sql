-- C-091 (owner, 2026-11-26: the progress post misled for a day and the alerts channel "looks like a disaster"):
-- the progress channel says blocks started and finished and the database's progress; the alerts channel says what
-- needs the owner, what broke (with its cause) and what resolved.
--
-- A lock wait fails this migration in 10 s instead of holding the core pod in `migrate` (as 0054).
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '60s';

-- A problem opened quietly (a stall that may heal itself, NO_PROGRESS): not alerted until it has held
-- `watch::QUIET_MINUTES`; one that resolves before is never said. Every problem before this was alerted.
ALTER TABLE core.problem ADD COLUMN quiet boolean NOT NULL DEFAULT false;

-- A worker's stop said on the alerts channel, once per lease, with its cause: the pod's own status where
-- core could read it (killed for memory, evicted, gone, still running), else what the lease says.
CREATE TABLE core.lease_death (
    lease_id bigint      PRIMARY KEY REFERENCES core.lease,
    cause    text        NOT NULL CHECK (cause <> ''),
    said_at  timestamptz NOT NULL DEFAULT now()
);
-- the leases that ended before this are not said again
INSERT INTO core.lease_death (lease_id, cause)
SELECT lease_id, 'before migration 0065' FROM core.lease WHERE ended_at IS NOT NULL AND outcome IN ('EXPIRED', 'FAILED');

-- The progress channel states a block's finish once: the job it was.
ALTER TABLE core.progress_shown DROP CONSTRAINT progress_shown_source_check;
ALTER TABLE core.progress_shown ADD CONSTRAINT progress_shown_source_check CHECK (source IN ('revision', 'refcheck', 'lease', 'notice', 'hand_over', 'job'));
