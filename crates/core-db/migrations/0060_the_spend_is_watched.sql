-- C-078 (owner, 2026-11-24: automation debt 3): DigitalOcean's own billing figures, read by core every
-- hour with a read-only token (`core_api::spend`), kept here; the watch states the spend since the
-- owner's start against the ceiling each morning on the progress channel and alerts at 80% and 100% of
-- it (`SPEND`). A read that failed is kept with why (a token not given is said, never a failure).
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '60s';
CREATE TABLE core.spend_reading (
    reading_id        bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    read_at           timestamptz NOT NULL DEFAULT clock_timestamp(),
    -- why the figures could not be read (no token, a refusal, an answer without them); NULL when read
    error             text,
    -- the month the figure is of (DigitalOcean's `generated_at`, UTC), 'YYYY-MM'
    month             text        CHECK (month ~ '^[0-9]{4}-[0-9]{2}$'),
    -- `/v2/customers/my/balance` month_to_date_usage, USD
    month_to_date_usd double precision,
    generated_at      timestamptz,
    -- `/v2/customers/my/invoices`: the finished months, [{"period": "YYYY-MM", "usd": n}]
    invoices          jsonb       NOT NULL DEFAULT '[]',
    CHECK ((error IS NULL) = (month IS NOT NULL AND month_to_date_usd IS NOT NULL))
);
CREATE INDEX spend_reading_at ON core.spend_reading (read_at);

ALTER TABLE core.problem DROP CONSTRAINT problem_kind_check;
ALTER TABLE core.problem ADD CONSTRAINT problem_kind_check
    CHECK (kind IN ('SESSION_LATE', 'REPORT_AT_RISK', 'QUEUE_PAUSED', 'JOB_FAILED', 'PROVIDER_AUTH',
                    'PROVIDER_ERRORS', 'DATABASE_FULL', 'CALENDAR_END', 'NOT_DERIVED', 'DAILY_STOPPED',
                    'ARCHIVE_MISSING', 'REFCHECK_DISAGREES', 'REFCHECK_NOT_RUN', 'STALLED', 'CLAIMS_EMPTY',
                    'WAL_ARCHIVING', 'BACKUP_STALE', 'WATCH_FAILING', 'JOB_THRASHING', 'NO_PROGRESS', 'DAY_REFUSED',
                    'SPEND'));
