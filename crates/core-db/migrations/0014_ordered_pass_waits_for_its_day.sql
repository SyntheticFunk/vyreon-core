-- C-039: an ordered pass waits until every day block is past its day, not only past the days
-- before it. A day block publishes its day's base, then its day-local revision, then says the day
-- is done (C-031); the pass reads both, so while a block's next session is the pass's own day the
-- pass is not claimable (0005 compared with `<`, which let the pass run between the two publishes
-- and fail for want of the day-local revision). A job with no session left is never claimable.
CREATE OR REPLACE VIEW core.claimable_job AS
    SELECT j.*
    FROM core.job j JOIN core.engine_build b USING (engine_build)
    WHERE j.state = 'QUEUED' AND j.next_session IS NOT NULL
      AND b.admitted_at IS NOT NULL AND b.withdrawn_at IS NULL
      AND NOT (SELECT paused FROM core.queue_control)
      AND (j.kind <> 'DAY_BLOCK' OR NOT EXISTS (
              SELECT FROM core.job t
              WHERE t.kind = 'TAPE' AND t.first_session <= j.last_session AND t.state NOT IN ('DONE', 'CANCELLED')))
      AND (j.kind <> 'ORDERED_PASS' OR (
              EXISTS (SELECT FROM vyreon.day_revision d
                      WHERE d.represented_date = j.first_session AND d.profile_id = j.profile_id
                        AND d.state = 'PUBLISHED' AND d.base_revision IS NULL)
              AND NOT EXISTS (SELECT FROM core.job o
                              WHERE o.kind = 'ORDERED_PASS' AND o.profile_id = j.profile_id
                                AND o.first_session < j.first_session AND o.state NOT IN ('DONE', 'CANCELLED'))
              AND NOT EXISTS (SELECT FROM core.job d
                              WHERE d.kind = 'DAY_BLOCK' AND d.profile_id = j.profile_id
                                AND d.state NOT IN ('DONE', 'CANCELLED') AND d.next_session <= j.first_session)));

-- C-038: a job with every session done is done. Before it, a lost `finish` could leave one queued
-- (or failed) with no next session; such a job is DONE, and none can be queued again.
UPDATE core.job SET state = 'DONE', finished_at = COALESCE(finished_at, now()) WHERE state IN ('QUEUED', 'FAILED') AND next_session IS NULL;
ALTER TABLE core.job ADD CONSTRAINT job_queued_has_a_session CHECK (state <> 'QUEUED' OR next_session IS NOT NULL);
