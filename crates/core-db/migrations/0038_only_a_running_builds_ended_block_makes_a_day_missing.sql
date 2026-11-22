-- C-062 amended (audit 2026-11-22 prebuild, changes-core.md 6): 0033 took a day as tried and ended
-- (a missing date, holding no later pass) when ANY day block of the profile, of any build and in any
-- state, had ever covered it. Under a newest-first build whose passes are scheduled for the whole
-- range first (C-046), a day whose only block was an earlier build's (cancelled, or DONE and since
-- retired) was then missing, though the new build's block for it was still to come in a later
-- tranche: the later passes ran without it and ran again when it published (C-037). A block now
-- makes its day tried only when its build runs (admitted, not withdrawn), and a DONE block only while
-- something of its day is still published; a day covered by no such block, and not omitted, is still
-- to come and holds the later passes, as a day never scheduled did. A FAILED or CANCELLED block of a
-- build that runs still makes its day missing (C-061, C-062).
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
              core.day_published(j.first_session, j.profile_id)
              AND NOT EXISTS (SELECT FROM core.job o
                              WHERE o.kind = 'ORDERED_PASS' AND o.profile_id = j.profile_id
                                AND o.first_session < j.first_session AND o.state NOT IN ('DONE', 'CANCELLED')
                                AND core.build_runs(o.engine_build)
                                AND (core.day_published(o.first_session, o.profile_id)
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
                                                        WHERE m.session = o.first_session AND m.profile_id = o.profile_id))))
              AND NOT EXISTS (SELECT FROM core.job d
                              WHERE d.kind = 'DAY_BLOCK' AND d.profile_id = j.profile_id
                                AND d.state NOT IN ('DONE', 'CANCELLED', 'FAILED') AND d.next_session <= j.first_session)));
