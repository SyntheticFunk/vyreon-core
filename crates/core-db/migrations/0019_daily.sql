-- C-051: daily runs. Core queues each new trading session by itself (`core_db::daily`), once the
-- provider has published its inputs, for the admitted build its settings name.

-- A session's daily jobs are claimed before any other work of their kind (the backfill's older
-- blocks would otherwise come first: the claim takes the earliest session, and a day's cohort
-- prints are kept by the provider for five sessions only). 0: ordinary work; 1: a daily run.
ALTER TABLE core.job ADD COLUMN priority smallint NOT NULL DEFAULT 0 CHECK (priority IN (0, 1));

-- One row per session the daily scheduler has handled, written in the transaction that queued its
-- jobs: a session is never queued twice, whatever happens to its jobs afterwards (a cancelled job
-- is the operator's word and stays cancelled). `jobs` is empty when everything the session needed
-- was already queued or published by hand.
CREATE TABLE core.daily_session (
    session      date        NOT NULL REFERENCES core.session,
    profile_id   text        NOT NULL,
    engine_build text        NOT NULL REFERENCES core.engine_build,
    queued_at    timestamptz NOT NULL,
    jobs         bigint[]    NOT NULL,
    PRIMARY KEY (session, profile_id)
);

-- The daily scheduler cannot queue (its build is not admitted, or was withdrawn): a problem until
-- it can, so a stopped schedule is never silent.
ALTER TABLE core.problem DROP CONSTRAINT problem_kind_check;
ALTER TABLE core.problem ADD CONSTRAINT problem_kind_check
    CHECK (kind IN ('SESSION_LATE', 'REPORT_AT_RISK', 'QUEUE_PAUSED', 'JOB_FAILED', 'PROVIDER_AUTH',
                    'PROVIDER_ERRORS', 'DATABASE_FULL', 'CALENDAR_END', 'NOT_DERIVED', 'DAILY_STOPPED'));
