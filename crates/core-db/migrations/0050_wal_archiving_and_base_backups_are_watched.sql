-- C-068 (C-067 amended): the operational failure-surface audit of 2026-11-22 (opsurface/ops.md), 2.
-- WAL archiving failing or stuck (or the WAL not readable by the watch), and a base backup older than
-- the watch's setting, are problems on the owner's channel.
ALTER TABLE core.problem DROP CONSTRAINT problem_kind_check;
ALTER TABLE core.problem ADD CONSTRAINT problem_kind_check
    CHECK (kind IN ('SESSION_LATE', 'REPORT_AT_RISK', 'QUEUE_PAUSED', 'JOB_FAILED', 'PROVIDER_AUTH',
                    'PROVIDER_ERRORS', 'DATABASE_FULL', 'CALENDAR_END', 'NOT_DERIVED', 'DAILY_STOPPED',
                    'ARCHIVE_MISSING', 'REFCHECK_DISAGREES', 'REFCHECK_NOT_RUN', 'STALLED', 'CLAIMS_EMPTY',
                    'WAL_ARCHIVING', 'BACKUP_STALE'));
