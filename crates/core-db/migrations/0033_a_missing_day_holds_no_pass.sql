-- C-062 (the gap the 2026-11-22 triage found in C-061, surface.md 5): a day block cancelled by hand,
-- not through `omit`, whose pass was still queued held every later pass. The owner's model: "a failed
-- day just means -1 day". An unfinished earlier pass of the profile now holds the later ones only while
-- its build runs (0032) and its day is still to come:
--   * a base is published on it (its pass will run: it holds as before), or
--   * a QUEUED or LEASED day block covers it (pending: it will publish), or
--   * no day block has ever covered it and it was not omitted: a day of a range whose block is not
--     scheduled yet. A build newest first schedules its blocks in tranches and its passes once,
--     before the first (C-046); were such a day not to hold, each tranche's passes would run before
--     the earlier tranches' days and again after each (measured: 18 pass runs instead of 12 in
--     C-046's test, the quadratic shape).
-- So a day whose blocks all ended FAILED (set aside), CANCELLED (by `omit` or by hand) or DONE with
-- nothing published (its revisions retired or superseded by hand), with none pending, is a missing
-- date: its pass holds nothing, whatever ended it. 0031's FAILED-or-omitted test is this rule's special
-- case. A pass still never runs before a day that will publish: a QUEUED or LEASED block at or before
-- it holds it (the last condition, unchanged).
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
                                AND (EXISTS (SELECT FROM vyreon.day_revision d
                                             WHERE d.represented_date = o.first_session AND d.profile_id = o.profile_id
                                               AND d.state = 'PUBLISHED' AND d.base_revision IS NULL)
                                     OR EXISTS (SELECT FROM core.job p
                                                WHERE p.kind = 'DAY_BLOCK' AND p.profile_id = o.profile_id AND p.state IN ('QUEUED', 'LEASED')
                                                  AND o.first_session BETWEEN p.first_session AND p.last_session)
                                     OR NOT (EXISTS (SELECT FROM core.job c
                                                     WHERE c.kind = 'DAY_BLOCK' AND c.profile_id = o.profile_id
                                                       AND o.first_session BETWEEN c.first_session AND c.last_session)
                                             OR EXISTS (SELECT FROM core.omitted_session m
                                                        WHERE m.session = o.first_session AND m.profile_id = o.profile_id))))
              AND NOT EXISTS (SELECT FROM core.job d
                              WHERE d.kind = 'DAY_BLOCK' AND d.profile_id = j.profile_id
                                AND d.state NOT IN ('DONE', 'CANCELLED', 'FAILED') AND d.next_session <= j.first_session)));
