-- C-062 (audit 2026-11-22 prebuild, surface.md 6): no ordered pass is left under a build that does
-- not run, and never two unfinished passes for one session and profile. A publish that supersedes a
-- later day's history revision queues its pass under a build that runs (`publish::rederive`), and
-- `schedule` keeps a pass of its own build and replaces one of a build that does not run.

-- A build runs while it is admitted and not withdrawn: core's only notion of whose workers claim.
CREATE FUNCTION core.build_runs(p_build text) RETURNS boolean LANGUAGE sql STABLE AS $$
    SELECT EXISTS (SELECT FROM core.engine_build WHERE engine_build = p_build AND admitted_at IS NOT NULL AND withdrawn_at IS NULL)
$$;

-- Never two unfinished passes for one session and profile. Under the code before it two could arise
-- only from one publish requeuing the DONE passes of two builds on one day; any such pair is settled
-- first: the LEASED one, else one whose build runs, else the newest is kept, and the others are
-- CANCELLED with a logged notice.
WITH ranked AS (
    SELECT j.job_id, row_number() OVER (PARTITION BY j.profile_id, j.first_session
                                        ORDER BY j.state = 'LEASED' DESC, core.build_runs(j.engine_build) DESC, j.job_id DESC) AS n
    FROM core.job j WHERE j.kind = 'ORDERED_PASS' AND j.state IN ('QUEUED', 'LEASED', 'FAILED')),
cancelled AS (
    UPDATE core.job SET state = 'CANCELLED', finished_at = now()
    WHERE job_id IN (SELECT job_id FROM ranked WHERE n > 1)
    RETURNING job_id, first_session, profile_id)
INSERT INTO core.notice (message)
SELECT format('Migration 0032 (C-062): ordered pass %s of %s %s cancelled: another unfinished pass of that session is kept.', job_id, profile_id, first_session)
FROM cancelled;
CREATE UNIQUE INDEX job_one_unfinished_pass ON core.job (profile_id, first_session)
    WHERE kind = 'ORDERED_PASS' AND state IN ('QUEUED', 'LEASED', 'FAILED');

-- The ordered pass's claim, as 0031 left it, with one change: an earlier pass holds the later ones
-- only while its build runs. A pass no worker claims (its build withdrawn, or never admitted) holds
-- nothing; `schedule` replaces it with one of the build scheduled.
CREATE OR REPLACE VIEW core.claimable_job AS
    SELECT j.*
    FROM core.job j JOIN core.engine_build b USING (engine_build)
    WHERE j.state = 'QUEUED' AND j.next_session IS NOT NULL
      AND b.admitted_at IS NOT NULL AND b.withdrawn_at IS NULL
      AND NOT (SELECT paused FROM core.queue_control)
      AND (j.kind <> 'DAY_BLOCK' OR NOT EXISTS (
              SELECT FROM core.job t
              WHERE t.kind = 'TAPE' AND t.first_session <= j.last_session AND t.state NOT IN ('DONE', 'CANCELLED')
                AND NOT COALESCE(t.scope @> '{"verify": true}', false)))
      AND (j.kind <> 'ORDERED_PASS' OR (
              EXISTS (SELECT FROM vyreon.day_revision d
                      WHERE d.represented_date = j.first_session AND d.profile_id = j.profile_id
                        AND d.state = 'PUBLISHED' AND d.base_revision IS NULL)
              AND NOT EXISTS (SELECT FROM core.job o
                              WHERE o.kind = 'ORDERED_PASS' AND o.profile_id = j.profile_id
                                AND o.first_session < j.first_session AND o.state NOT IN ('DONE', 'CANCELLED')
                                AND core.build_runs(o.engine_build)
                                AND NOT (
                                    NOT EXISTS (SELECT FROM vyreon.day_revision d
                                                WHERE d.represented_date = o.first_session AND d.profile_id = o.profile_id
                                                  AND d.state = 'PUBLISHED' AND d.base_revision IS NULL)
                                    AND (EXISTS (SELECT FROM core.job f
                                                 WHERE f.kind = 'DAY_BLOCK' AND f.profile_id = o.profile_id AND f.state = 'FAILED'
                                                   AND o.first_session BETWEEN f.first_session AND f.last_session)
                                         OR EXISTS (SELECT FROM core.omitted_session m
                                                    WHERE m.session = o.first_session AND m.profile_id = o.profile_id))
                                    AND NOT EXISTS (SELECT FROM core.job p
                                                    WHERE p.kind = 'DAY_BLOCK' AND p.profile_id = o.profile_id AND p.state IN ('QUEUED', 'LEASED')
                                                      AND o.first_session BETWEEN p.first_session AND p.last_session)))
              AND NOT EXISTS (SELECT FROM core.job d
                              WHERE d.kind = 'DAY_BLOCK' AND d.profile_id = j.profile_id
                                AND d.state NOT IN ('DONE', 'CANCELLED', 'FAILED') AND d.next_session <= j.first_session)));
