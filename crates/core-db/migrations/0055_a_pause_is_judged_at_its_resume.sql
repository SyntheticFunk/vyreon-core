-- C-072 round 3 (audit 2026-11-24-core-c072-r2, 7): a planned pause is judged against its grace at
-- the instant it was resumed, not at the watch's next check. Set by `work::resume`, kept through
-- later pauses (the watch reads the resume after the pause its problem names).
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '60s';
ALTER TABLE core.queue_control ADD COLUMN resumed_at timestamptz;
