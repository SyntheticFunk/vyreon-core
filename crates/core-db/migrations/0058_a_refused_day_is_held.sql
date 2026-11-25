-- C-075 (coordinator, 2026-11-24, an observability gap: live, 2024-07-23 was refused by the engine's
-- input plan, ETHA's ACT01 gap, and core said nothing: the job was queued again and would have alerted
-- only after its last attempt, some 3 x 2-3 hours of node time later). A day the input plan refuses is
-- recorded here when its worker reports it (`work::fail`): held at once (the job FAILED, its day set
-- aside, the build going on) unless every refused input is a capture that failed here retryably, which is
-- retried once; the watch's DAY_REFUSED problem names the day and the refused inputs until the day is
-- published. A new build's hand-over or `queue retry-failed` tries the day again.
CREATE TABLE core.day_refusal (
    refusal_id   bigserial PRIMARY KEY,
    job_id       bigint NOT NULL REFERENCES core.job (job_id),
    lease_id     bigint NOT NULL REFERENCES core.lease (lease_id),
    profile_id   text NOT NULL,
    engine_build text NOT NULL,
    session      date NOT NULL,
    inputs       text NOT NULL,          -- the refused inputs, readable (one line)
    transient    boolean NOT NULL,       -- every refused input a capture that failed here retryably
    held         boolean NOT NULL,       -- the job was held (FAILED) for it, not queued again
    refused_at   timestamptz NOT NULL DEFAULT clock_timestamp(),
    UNIQUE (lease_id)
);
CREATE INDEX day_refusal_session ON core.day_refusal (profile_id, session);

ALTER TABLE core.problem DROP CONSTRAINT problem_kind_check;
ALTER TABLE core.problem ADD CONSTRAINT problem_kind_check
    CHECK (kind IN ('SESSION_LATE', 'REPORT_AT_RISK', 'QUEUE_PAUSED', 'JOB_FAILED', 'PROVIDER_AUTH',
                    'PROVIDER_ERRORS', 'DATABASE_FULL', 'CALENDAR_END', 'NOT_DERIVED', 'DAILY_STOPPED',
                    'ARCHIVE_MISSING', 'REFCHECK_DISAGREES', 'REFCHECK_NOT_RUN', 'STALLED', 'CLAIMS_EMPTY',
                    'WAL_ARCHIVING', 'BACKUP_STALE', 'WATCH_FAILING', 'JOB_THRASHING', 'NO_PROGRESS', 'DAY_REFUSED'));
