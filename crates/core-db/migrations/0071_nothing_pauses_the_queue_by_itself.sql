-- C-101 (owner, 2026-11-26: "the nightly should never get paused"; 2026-11-27: "the system needs to eventually
-- function without you"). Two paths still paused the whole queue by themselves, the nightly included, until someone
-- ran `queue resume`: a tape job out of attempts, and a bundle core refused. Both are now contained to their job
-- and day.
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '60s';

-- A bundle core refused (a failed check: `publish::Error::Refused`), per lease: the first refusal of a job's day
-- lets the day be tried once more (a body changed on the way is refused too), the second holds it, as C-075
-- holds a day its input plan refused.
CREATE TABLE core.bundle_refusal (
    refusal_id   bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    job_id       bigint      NOT NULL REFERENCES core.job (job_id),
    lease_id     bigint      NOT NULL REFERENCES core.lease (lease_id),
    session      date        NOT NULL,
    why          text        NOT NULL,
    held         boolean     NOT NULL,
    refused_at   timestamptz NOT NULL DEFAULT clock_timestamp(),
    UNIQUE (lease_id)
);
CREATE INDEX bundle_refusal_job ON core.bundle_refusal (job_id, session);

-- A day block (and a republish that reads the tapes) waits for the tape jobs of its sessions and the earlier
-- ones while they are queued or running, not once one has FAILED for the last time. Otherwise 0057's view.
CREATE OR REPLACE VIEW core.claimable_job AS
    SELECT j.*
    FROM core.job j JOIN core.engine_build b USING (engine_build)
    WHERE j.state = 'QUEUED' AND j.next_session IS NOT NULL
      AND b.admitted_at IS NOT NULL AND b.withdrawn_at IS NULL
      AND NOT (SELECT paused FROM core.queue_control)
      -- (C-074, release-2 audit, Codex 9: a republish that reads the tapes waits for them as a day block does)
      -- C-101: a tape job that FAILED for the last time holds nothing back: the block's own fill captures the
      -- tapes core holds no cache of (engine D-358), and the failed job is JOB_FAILED's to say
      AND ((j.kind <> 'DAY_BLOCK' AND NOT (j.kind = 'REPUBLISH' AND COALESCE(j.scope->'categories' ? 'consolidated', false))) OR NOT EXISTS (
              SELECT FROM core.job t
              WHERE t.kind = 'TAPE' AND t.first_session <= j.last_session AND t.state NOT IN ('DONE', 'CANCELLED', 'FAILED')
                AND NOT COALESCE(t.scope @> '{"verify": true}', false)))
      AND (j.kind <> 'ORDERED_PASS' OR (
              core.day_published(j.first_session, j.profile_id, j.engine_build)
              AND NOT EXISTS (SELECT FROM core.job o
                              WHERE o.kind = 'ORDERED_PASS' AND o.profile_id = j.profile_id
                                AND o.first_session < j.first_session AND o.state NOT IN ('DONE', 'CANCELLED')
                                AND core.build_runs(o.engine_build)
                                AND (core.day_published(o.first_session, o.profile_id, o.engine_build)
                                     OR EXISTS (SELECT FROM core.job p
                                                WHERE p.kind = 'DAY_BLOCK' AND p.profile_id = o.profile_id AND p.state IN ('QUEUED', 'LEASED')
                                                  AND o.first_session BETWEEN p.first_session AND p.last_session)
                                     OR NOT (EXISTS (SELECT FROM core.job c
                                                     WHERE c.kind = 'DAY_BLOCK' AND c.profile_id = o.profile_id
                                                       AND o.first_session BETWEEN c.first_session AND c.last_session
                                                       AND core.build_runs(c.engine_build)
                                                       AND (c.state <> 'DONE'
                                                            OR EXISTS (SELECT FROM vyreon.day_revision r
                                                                       WHERE r.represented_date = o.first_session AND r.profile_id = o.profile_id
                                                                         AND r.state = 'PUBLISHED' AND r.base_revision IS NULL)))
                                             OR EXISTS (SELECT FROM core.omitted_session m
                                                        WHERE m.session = o.first_session AND m.profile_id = o.profile_id
                                                          AND core.omission_current(m.omission_id)))))
              AND NOT EXISTS (SELECT FROM core.job d
                              WHERE d.kind = 'DAY_BLOCK' AND d.profile_id = j.profile_id
                                AND d.state NOT IN ('DONE', 'CANCELLED', 'FAILED') AND d.next_session <= j.first_session)
              AND NOT EXISTS (SELECT FROM core.job r
                              WHERE r.kind = 'REPUBLISH' AND r.profile_id = j.profile_id
                                AND r.state IN ('QUEUED', 'LEASED') AND r.first_session <= j.first_session
                                AND core.build_runs(r.engine_build))));
