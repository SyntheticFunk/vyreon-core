-- C-073 (owner, 2026-11-24: the automation debts ride the next release). Three additions:
--
-- 1. REFCHECK, a job kind of its own: a published day whose reference check never reported (its
--    worker stopped before the check ended: a hand-over, a lost node) or did not complete is checked
--    again by a worker of its own, against the revisions core holds published (the dump core serves,
--    `refcheck::dump`), once per day's current revisions. One session; claimed like any job; queued by
--    the watch (`refcheck::queue_reruns`), never by hand.
-- 2. JOB_THRASHING: a job that lost three leases or more without moving on (MILESTONES' automation
--    debt of the 2026-11-23 tape OOM loop).
-- 3. The request budget's waits, by minute: a draw core answered "wait" (the ceiling was reached).
--    The scaler's count (`core.scaler_count`, 50-keda.yaml) counts work that calls the provider only
--    while no draw waited in the last `budget::PRESSURE_MINUTES` minutes, so KEDA starts no pod the
--    budget cannot feed (owner, 2026-11-23: approved).
--
-- A lock wait fails this migration in 10 s instead of holding the core pod in `migrate` (as 0054).
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '60s';

ALTER TABLE core.job DROP CONSTRAINT job_kind_check;
ALTER TABLE core.job ADD CONSTRAINT job_kind_check CHECK (kind IN ('TAPE', 'DAY_BLOCK', 'ORDERED_PASS', 'REFCHECK'));
-- a re-run names the revisions it checks: the day's base and its day-local revision (NULL: none)
ALTER TABLE core.job ADD CONSTRAINT job_refcheck_scope_check
    CHECK (kind <> 'REFCHECK' OR COALESCE(jsonb_typeof(scope->'base_revision') = 'number', false));
-- once per (base, day-local) pair: a re-run that ran is never queued again (its outcome is the
-- day's); one cancelled (a hand-over cancels and carries it) leaves room for its successor
CREATE UNIQUE INDEX job_one_refcheck_per_revisions ON core.job ((scope->>'base_revision'), COALESCE(scope->>'day_local_revision', ''))
    WHERE kind = 'REFCHECK' AND state <> 'CANCELLED';
ALTER TABLE core.job_evidence DROP CONSTRAINT job_evidence_kind_check;
ALTER TABLE core.job_evidence ADD CONSTRAINT job_evidence_kind_check CHECK (kind IN ('TAPE', 'DAY_BLOCK', 'ORDERED_PASS', 'REFCHECK'));
ALTER TABLE core.claim_miss DROP CONSTRAINT claim_miss_kind_check;
ALTER TABLE core.claim_miss ADD CONSTRAINT claim_miss_kind_check CHECK (kind IN ('TAPE', 'DAY_BLOCK', 'ORDERED_PASS', 'REFCHECK'));
ALTER TABLE core.stall_seen DROP CONSTRAINT stall_seen_kind_check;
ALTER TABLE core.stall_seen ADD CONSTRAINT stall_seen_kind_check CHECK (kind IN ('TAPE', 'DAY_BLOCK', 'DAILY_BLOCK', 'ORDERED_PASS', 'REFCHECK', 'CHAIN'));

ALTER TABLE core.problem DROP CONSTRAINT problem_kind_check;
ALTER TABLE core.problem ADD CONSTRAINT problem_kind_check
    CHECK (kind IN ('SESSION_LATE', 'REPORT_AT_RISK', 'QUEUE_PAUSED', 'JOB_FAILED', 'PROVIDER_AUTH',
                    'PROVIDER_ERRORS', 'DATABASE_FULL', 'CALENDAR_END', 'NOT_DERIVED', 'DAILY_STOPPED',
                    'ARCHIVE_MISSING', 'REFCHECK_DISAGREES', 'REFCHECK_NOT_RUN', 'STALLED', 'CLAIMS_EMPTY',
                    'WAL_ARCHIVING', 'BACKUP_STALE', 'WATCH_FAILING', 'JOB_THRASHING'));

-- One row per minute in which a draw waited: how many waited, and the last.
CREATE TABLE core.budget_wait (
    minute  timestamptz PRIMARY KEY,
    waits   integer     NOT NULL CHECK (waits > 0),
    last_at timestamptz NOT NULL
);

-- The scaler's count (C-073): `core.claimable_count`'s, but the work that calls the provider (tapes,
-- backfill day blocks, reference-check re-runs) counts nothing while a draw waited in the last five
-- minutes (`budget::PRESSURE_MINUTES`). A daily session's work (priority 1) and the ordered passes
-- (no provider call) always count. Running pods are never touched: KEDA only starts fewer. SECURITY
-- DEFINER for the reasons 0046 gives; every name schema-qualified.
CREATE FUNCTION core.scaler_count(p_kind text, p_build text, p_daily boolean DEFAULT NULL) RETURNS bigint
    LANGUAGE sql STABLE SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
    SELECT count(*) FROM core.claimable_job j
    WHERE j.kind = p_kind AND j.engine_build = p_build AND j.next_session IS NOT NULL
      AND (p_daily IS NULL OR (j.priority = 1) = p_daily)
      AND (j.kind = 'ORDERED_PASS' OR j.priority = 1
           OR NOT EXISTS (SELECT FROM core.budget_wait w WHERE w.last_at > now() - interval '5 minutes'))
$$;
REVOKE ALL ON FUNCTION core.scaler_count(text, text, boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION core.scaler_count(text, text, boolean) TO vyreon_scaler;
