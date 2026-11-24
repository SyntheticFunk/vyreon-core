-- C-072: a planned operator step (a queue paused by the operator, the daily runs stopped by a
-- hand-over) is progress, not an alert, unless it stays open longer than its grace
-- (`watch::PLANNED_GRACE_MINUTES`): a forgotten pause is a problem.

-- Who paused the queue: the operator (`vyreon-core queue pause <reason>`), or core itself (a job
-- failed for the last time, a bundle refused). A pause from before this migration is core's (it
-- alerts, as before).
ALTER TABLE core.queue_control ADD COLUMN by_operator boolean NOT NULL DEFAULT false;
ALTER TABLE core.queue_control ADD CONSTRAINT queue_control_operator_pauses CHECK (paused OR NOT by_operator);

-- A planned problem not alerted yet: its alert is posted at this instant if it is still open then
-- (and cleared to NULL once posted). NULL: alerted when it opened, as every problem before this.
ALTER TABLE core.problem ADD COLUMN quiet_until timestamptz;

-- The progress channel states a hand-over once, in one line, its admit, withdrawal, pause and
-- resume folded into it (C-072).
ALTER TABLE core.progress_shown DROP CONSTRAINT progress_shown_source_check;
ALTER TABLE core.progress_shown ADD CONSTRAINT progress_shown_source_check CHECK (source IN ('revision', 'refcheck', 'lease', 'notice', 'hand_over'));
