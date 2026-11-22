-- C-061 amended (audit 2026-11-22 prebuild, changes-core.md 2, above the cut line): a day block
-- publishes a day in two bundles, its base and then its day-local derived revision computed on that
-- base (C-031), and the ordered pass refuses a day without that day-local revision (engine
-- `vyreon-cli/src/derive/ordered.rs`: "no published day-local derived revision on base revision N").
-- A block whose last attempt published the base and failed before the day-local upload stood FAILED
-- with the base published: its pass was claimable, was refused by the engine three times, and its
-- FAILED pass with the published base then held every later pass (0033's first branch). A day now
-- counts as published, for its own pass and for the passes after it, only when its base and a
-- day-local revision on that very base are both PUBLISHED. A FAILED block that left only its base is
-- a missing day, like any other set-aside day; while its block is pending it holds, as before.

-- A day of the profile is published for the ordered pass: its PUBLISHED base, and a PUBLISHED
-- day-local revision of the profile computed on that base.
CREATE FUNCTION core.day_published(p_day date, p_profile text) RETURNS boolean LANGUAGE sql STABLE AS $$
    SELECT EXISTS (SELECT FROM vyreon.day_revision b JOIN vyreon.day_revision l
                       ON l.base_revision = b.revision_id AND l.represented_date = b.represented_date
                   WHERE b.represented_date = p_day AND b.profile_id = p_profile AND b.state = 'PUBLISHED' AND b.base_revision IS NULL
                     AND l.profile_id = p_profile AND l.state = 'PUBLISHED' AND l.day_local)
$$;

-- The day's PUBLISHED base revision when no day-local revision is published on it: what a set-aside
-- day or a refused pass left, for the notices that name it. NULL otherwise.
CREATE FUNCTION core.base_without_day_local(p_day date, p_profile text) RETURNS bigint LANGUAGE sql STABLE AS $$
    SELECT b.revision_id FROM vyreon.day_revision b
    WHERE b.represented_date = p_day AND b.profile_id = p_profile AND b.state = 'PUBLISHED' AND b.base_revision IS NULL
      AND NOT EXISTS (SELECT FROM vyreon.day_revision l
                      WHERE l.base_revision = b.revision_id AND l.represented_date = b.represented_date
                        AND l.profile_id = p_profile AND l.state = 'PUBLISHED' AND l.day_local)
    ORDER BY b.revision_id DESC LIMIT 1
$$;

-- The ordered pass's claim, as 0033 left it, with "a base is published on it" read as
-- `core.day_published` in both places: the pass's own day, and an earlier pass's day that holds it.
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
                                                       AND o.first_session BETWEEN c.first_session AND c.last_session)
                                             OR EXISTS (SELECT FROM core.omitted_session m
                                                        WHERE m.session = o.first_session AND m.profile_id = o.profile_id))))
              AND NOT EXISTS (SELECT FROM core.job d
                              WHERE d.kind = 'DAY_BLOCK' AND d.profile_id = j.profile_id
                                AND d.state NOT IN ('DONE', 'CANCELLED', 'FAILED') AND d.next_session <= j.first_session)));
