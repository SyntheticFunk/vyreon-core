-- C-054 (engine D-370): what each published day's reference check found. A worker recomputes the
-- day it published with the independent reference suite (QuantLib, TA-Lib) before it deletes the
-- day's raw inputs, and sends core one document per (lease, session): a gzip whose text is one
-- JSON line, the account (`VYREON-REFCHECK@1`: outcome, reason, times, counts, the report's
-- per-measurement summary), and after it the suite's report as JSON (absent when it did not
-- complete, or when core refused it and the worker sent the account alone). Written once per lease
-- and session: the same again changes nothing. A disagreement never withholds or unpublishes the
-- day; it is a problem (REFCHECK_DISAGREES), and so is a check that did not run
-- (REFCHECK_NOT_RUN).
CREATE TABLE core.refcheck_report (
    refcheck_report_id bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    lease_id           bigint      NOT NULL REFERENCES core.lease,
    job_id             bigint      NOT NULL REFERENCES core.job,
    attempt            integer     NOT NULL CHECK (attempt > 0),
    worker             text        NOT NULL,
    engine_build       text        NOT NULL REFERENCES core.engine_build,
    session            date        NOT NULL REFERENCES core.session,
    -- the day's published base revision of the job's build when the report came (what was
    -- checked); NULL once that revision is deleted (a report outlives the values it judged)
    base_revision      bigint      REFERENCES vyreon.day_revision ON DELETE SET NULL,
    outcome            text        NOT NULL CHECK (outcome IN ('COMPLETED', 'TIMEOUT', 'CRASHED', 'NOT_RUN')),
    why                text,
    -- the report's counts, summed over its measurements; present when it completed
    checked            integer     CHECK (checked >= 0),
    passed             integer     CHECK (passed >= 0),
    failed             integer     CHECK (failed >= 0),
    skipped            integer     CHECK (skipped >= 0),
    reference_errors   integer     CHECK (reference_errors >= 0),
    failure_classes    integer     CHECK (failure_classes >= 0),
    measurements_failing integer   CHECK (measurements_failing >= 0),
    summary            jsonb,                  -- per measurement: checked, passed, failed, skipped, ...
    -- what it cost: the whole phase, the suite's wall and processor time, its processes
    wall_seconds       double precision NOT NULL CHECK (wall_seconds >= 0),
    suite_seconds      double precision CHECK (suite_seconds >= 0),
    cpu_seconds        double precision CHECK (cpu_seconds >= 0),
    cpu_complete       boolean     NOT NULL,
    jobs               integer     NOT NULL CHECK (jobs > 0),
    deadline_seconds   integer     NOT NULL CHECK (deadline_seconds > 0),
    report_trimmed     text,                   -- what the worker left out of the report, if anything
    report_bytes       bigint      NOT NULL CHECK (report_bytes >= 0),  -- the report unpacked (0: none)
    document_sha256    text        NOT NULL CHECK (document_sha256 ~ '^[0-9a-f]{64}$'),
    document           bytea       STORAGE EXTERNAL NOT NULL,           -- as sent, already compressed
    stored_at          timestamptz NOT NULL DEFAULT clock_timestamp(),
    UNIQUE (lease_id, session),
    CHECK ((outcome = 'COMPLETED') = (why IS NULL)),
    CHECK ((outcome = 'COMPLETED') = (checked IS NOT NULL AND passed IS NOT NULL AND failed IS NOT NULL AND skipped IS NOT NULL
                                      AND reference_errors IS NOT NULL AND failure_classes IS NOT NULL
                                      AND measurements_failing IS NOT NULL))
);
CREATE INDEX refcheck_report_by_session ON core.refcheck_report (session, stored_at DESC);
CREATE INDEX refcheck_report_by_revision ON core.refcheck_report (base_revision);

ALTER TABLE core.problem DROP CONSTRAINT problem_kind_check;
ALTER TABLE core.problem ADD CONSTRAINT problem_kind_check
    CHECK (kind IN ('SESSION_LATE', 'REPORT_AT_RISK', 'QUEUE_PAUSED', 'JOB_FAILED', 'PROVIDER_AUTH',
                    'PROVIDER_ERRORS', 'DATABASE_FULL', 'CALENDAR_END', 'NOT_DERIVED', 'DAILY_STOPPED',
                    'ARCHIVE_MISSING', 'REFCHECK_DISAGREES', 'REFCHECK_NOT_RUN'));
