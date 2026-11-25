-- C-081 (engine D-512 contract a, 2026-11-25): a session counts toward the option prints' retention once it
-- has opened, as the engine's `fill::prints_retained` counts it: `p_at` is an instant, a session's open its
-- 09:30 New York time (every XNYS session opens then; the engine's calendar `open_at`). The date form
-- (`prints_perished(date, date)`, the planners') stays: every session before that UTC date.
CREATE FUNCTION core.prints_perished_at(p_session date, p_at timestamptz, p_keep integer DEFAULT 5) RETURNS boolean LANGUAGE sql STABLE AS $$
    SELECT p_session >= COALESCE((SELECT first_date FROM core.source_coverage WHERE source = 'option_prints'), 'infinity'::date)
       AND p_session < COALESCE((SELECT session FROM core.session
                                 WHERE (session + time '09:30') AT TIME ZONE 'America/New_York' <= p_at
                                 ORDER BY session DESC OFFSET p_keep - 1 LIMIT 1), '-infinity'::date)
$$;

-- C-084: the owner's phase-1 stop rule (MILESTONES "PHASE-1 RUN RULE"): over half of the newest build's
-- judged days look bad by core's count.
ALTER TABLE core.problem DROP CONSTRAINT problem_kind_check;
ALTER TABLE core.problem ADD CONSTRAINT problem_kind_check
    CHECK (kind IN ('SESSION_LATE', 'REPORT_AT_RISK', 'QUEUE_PAUSED', 'JOB_FAILED', 'PROVIDER_AUTH',
                    'PROVIDER_ERRORS', 'DATABASE_FULL', 'CALENDAR_END', 'NOT_DERIVED', 'DAILY_STOPPED',
                    'ARCHIVE_MISSING', 'REFCHECK_DISAGREES', 'REFCHECK_NOT_RUN', 'STALLED', 'CLAIMS_EMPTY',
                    'WAL_ARCHIVING', 'BACKUP_STALE', 'WATCH_FAILING', 'JOB_THRASHING', 'NO_PROGRESS', 'DAY_REFUSED',
                    'SPEND', 'STOP_RULE'));
