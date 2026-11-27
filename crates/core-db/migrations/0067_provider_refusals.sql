-- C-095 (owner, 2026-11-26): the provider's refusals are recorded, slow the budget, and are said.
-- The nightly of 2026-11-25 lost 28 assets' ACT01 capture to HTTP 429 while core had counted 1,540 to 1,770
-- requests in the minute, under its 1,900 ceiling and the provider's 2,000: which of the provider's limits was hit
-- is not known. Each refusal a worker meets is now stored with what the provider said (its words, its rate-limit
-- headers, a stated Retry-After) and what core had committed against the ceiling at that moment.
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '60s';

CREATE TABLE core.provider_refusal (
    provider_refusal_id bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    at                  timestamptz NOT NULL DEFAULT clock_timestamp(),
    worker              text        NOT NULL CHECK (worker <> ''),
    category            text        NOT NULL,
    -- the request's path, never its query (which may hold the key)
    endpoint            text        NOT NULL CHECK (strpos(endpoint, '?') = 0),
    status              integer     NOT NULL CHECK (status BETWEEN 100 AND 599),
    retry_after_seconds integer     CHECK (retry_after_seconds >= 0),
    message             text,
    headers             jsonb       NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(headers) = 'object'),
    -- what the open and recently ended budget leases held against the ceiling when the refusal reached core
    committed           integer     NOT NULL CHECK (committed >= 0)
);
CREATE INDEX provider_refusal_at ON core.provider_refusal (at);

-- The refusals are a problem of their own while they come (`watch::provider`).
ALTER TABLE core.problem DROP CONSTRAINT problem_kind_check;
ALTER TABLE core.problem ADD CONSTRAINT problem_kind_check
    CHECK (kind IN ('SESSION_LATE', 'REPORT_AT_RISK', 'QUEUE_PAUSED', 'JOB_FAILED', 'PROVIDER_AUTH',
                    'PROVIDER_ERRORS', 'DATABASE_FULL', 'CALENDAR_END', 'NOT_DERIVED', 'DAILY_STOPPED',
                    'ARCHIVE_MISSING', 'REFCHECK_DISAGREES', 'REFCHECK_NOT_RUN', 'STALLED', 'CLAIMS_EMPTY',
                    'WAL_ARCHIVING', 'BACKUP_STALE', 'WATCH_FAILING', 'JOB_THRASHING', 'NO_PROGRESS', 'DAY_REFUSED',
                    'SPEND', 'STOP_RULE', 'PROVIDER_THROTTLED'));
