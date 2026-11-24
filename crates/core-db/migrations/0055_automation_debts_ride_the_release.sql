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

-- C-073, the C-072 audit's remaining items (r4):
-- A hand-over records which of the jobs it moved its own withdrawal stopped (the progress line's
-- "restarted"); NULL on a hand-over before this (counted as before).
ALTER TABLE core.hand_over_job ADD COLUMN restarted boolean;
-- A lease records its job's sessions when it was granted: a block set aside narrows its job to the
-- day it failed on, and the progress line of a start must still name the block it started (and
-- where it resumed). Set with `start_session`, by the database; NULL on a lease before this.
ALTER TABLE core.lease ADD COLUMN job_first date, ADD COLUMN job_last date;
CREATE OR REPLACE FUNCTION core.lease_start_session() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    SELECT next_session, first_session, last_session INTO NEW.start_session, NEW.job_first, NEW.job_last
    FROM core.job WHERE job_id = NEW.job_id;
    RETURN NEW;
END
$$;

-- C-073 (live investigation, 2026-11-24: the daily run sat idle about an hour in a deliberate EXQ01
-- cutoff wait, its worker heartbeating, and nobody could tell it from a stall). A worker reports the
-- step it is on and any deliberate wait (`POST /worker/v1/lease/step`); the latest per lease is kept.
-- `step_since`: when this step (or this wait) began, the worker's own progress mark.
CREATE TABLE core.lease_step (
    lease_id      bigint      PRIMARY KEY REFERENCES core.lease,
    step          text        NOT NULL CHECK (length(step) BETWEEN 1 AND 200),
    waiting_until timestamptz,
    reason        text        CHECK (length(reason) BETWEEN 1 AND 300),
    step_since    timestamptz NOT NULL,
    reported_at   timestamptz NOT NULL,
    CHECK ((waiting_until IS NULL) = (reason IS NULL))
);
ALTER TABLE core.problem DROP CONSTRAINT problem_kind_check;
ALTER TABLE core.problem ADD CONSTRAINT problem_kind_check
    CHECK (kind IN ('SESSION_LATE', 'REPORT_AT_RISK', 'QUEUE_PAUSED', 'JOB_FAILED', 'PROVIDER_AUTH',
                    'PROVIDER_ERRORS', 'DATABASE_FULL', 'CALENDAR_END', 'NOT_DERIVED', 'DAILY_STOPPED',
                    'ARCHIVE_MISSING', 'REFCHECK_DISAGREES', 'REFCHECK_NOT_RUN', 'STALLED', 'CLAIMS_EMPTY',
                    'WAL_ARCHIVING', 'BACKUP_STALE', 'WATCH_FAILING', 'JOB_THRASHING', 'NO_PROGRESS'));

-- C-073 (release audit, Gemini 6): when `queue retry-failed` last queued a job again; JOB_THRASHING
-- counts only the leases granted after it (a retried job starts clean).
ALTER TABLE core.job ADD COLUMN retried_at timestamptz;

-- C-073 (owner, after the release audit's C13): a bundle's layer is what its registered profile is,
-- never inferred from rows stored before. The engine's inventory declares each profile revision's
-- layer (`profiles`: base | day-local | history); registered with the build. A build registered before
-- the declaration has its profiles' layers from what it published (each profile's first revision).
CREATE TABLE core.build_profile (
    engine_build     text NOT NULL REFERENCES core.engine_build,
    profile_revision text NOT NULL,
    layer            text NOT NULL CHECK (layer IN ('base', 'day-local', 'history')),
    PRIMARY KEY (engine_build, profile_revision)
);
INSERT INTO core.build_profile (engine_build, profile_revision, layer)
SELECT DISTINCT ON (engine_build, profile_revision) engine_build, profile_revision,
       CASE WHEN base_revision IS NULL THEN 'base' WHEN day_local THEN 'day-local' ELSE 'history' END
FROM vyreon.day_revision WHERE state <> 'REJECTED'
ORDER BY engine_build, profile_revision, revision_id;
