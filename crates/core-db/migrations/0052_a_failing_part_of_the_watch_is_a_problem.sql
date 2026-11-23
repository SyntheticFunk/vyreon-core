-- C-069 (audit 2026-11-23, Codex 8, Gemini 7): a part of the watch that fails (one of the check's
-- parts, the check itself, the digest or the weekly check) is a problem on the owner's channel, one
-- per part, its summary the part's error; before, the error went to the service's log alone.
ALTER TABLE core.problem DROP CONSTRAINT problem_kind_check;
ALTER TABLE core.problem ADD CONSTRAINT problem_kind_check
    CHECK (kind IN ('SESSION_LATE', 'REPORT_AT_RISK', 'QUEUE_PAUSED', 'JOB_FAILED', 'PROVIDER_AUTH',
                    'PROVIDER_ERRORS', 'DATABASE_FULL', 'CALENDAR_END', 'NOT_DERIVED', 'DAILY_STOPPED',
                    'ARCHIVE_MISSING', 'REFCHECK_DISAGREES', 'REFCHECK_NOT_RUN', 'STALLED', 'CLAIMS_EMPTY',
                    'WAL_ARCHIVING', 'BACKUP_STALE', 'WATCH_FAILING'));
