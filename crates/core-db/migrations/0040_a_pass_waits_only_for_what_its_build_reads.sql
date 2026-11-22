-- Audit 2026-11-22 prebuild round 2 (core-r2.md) 2 and 3, the ordered pass's claim.
--
-- 2 (above the cut line): `core.day_published` (0035) took any PUBLISHED day-local revision on the
-- day's base. The engine's pass reads exactly its own build's profile revisions (the base profile and
-- `VYREON-DLM-DAY-PROFILE-…`, vyreon-cli `derive/ordered.rs`), so on a day whose day-local revision is
-- of another profile revision it failed three times, and its FAILED pass, on a day the claim still
-- called published, held every later pass. A day is now published for a pass only when its base and
-- its day-local revision are of profile revisions the pass's build registered (its inventory, the
-- profiles it publishes and reads); else the day is missing for that pass, never a FAILED pass that
-- holds the later ones.
--
-- 3: the claim took any omission of a day as making it tried (missing), the reader only a current one
-- (0037: its day job is still the latest covering the session, and no retirement came later). Omitted,
-- published by hand and retired, a day read RETIRED but let the later passes run before its new block.
-- Both now ask `core.omission_current`.

-- A day of the profile is published for an ordered pass of `p_build`: its PUBLISHED base and a
-- PUBLISHED day-local revision on that base, both of profile revisions in the build's inventory.
CREATE FUNCTION core.day_published(p_day date, p_profile text, p_build text) RETURNS boolean LANGUAGE sql STABLE AS $$
    SELECT EXISTS (SELECT FROM vyreon.day_revision b JOIN vyreon.day_revision l
                       ON l.base_revision = b.revision_id AND l.represented_date = b.represented_date
                   WHERE b.represented_date = p_day AND b.profile_id = p_profile AND b.state = 'PUBLISHED' AND b.base_revision IS NULL
                     AND l.profile_id = p_profile AND l.state = 'PUBLISHED' AND l.day_local
                     AND EXISTS (SELECT FROM core.expected_slot e WHERE e.engine_build = p_build AND e.profile_revision = b.profile_revision)
                     AND EXISTS (SELECT FROM core.expected_slot e WHERE e.engine_build = p_build AND e.profile_revision = l.profile_revision))
$$;

-- An omission states its session while it is current (0037's rule): its day job is still the latest
-- day job of the profile covering the session, and no retirement of the session came after it.
CREATE FUNCTION core.omission_current(p_omission bigint) RETURNS boolean LANGUAGE sql STABLE AS $$
    SELECT EXISTS (SELECT FROM core.omitted_session o
                   WHERE o.omission_id = p_omission
                     AND o.job_id = (SELECT max(j.job_id) FROM core.job j
                                     WHERE j.kind = 'DAY_BLOCK' AND j.profile_id = o.profile_id AND o.session BETWEEN j.first_session AND j.last_session)
                     AND NOT EXISTS (SELECT FROM core.retired_revision r JOIN core.retirement t USING (retirement_id)
                                     WHERE r.represented_date = o.session AND r.profile_id = o.profile_id AND t.retired_at > o.omitted_at))
$$;

-- Why a session has no published day (0039), its omission through `core.omission_current`.
CREATE OR REPLACE FUNCTION core.session_cause(p_session date, p_profile text) RETURNS jsonb LANGUAGE sql STABLE AS $$
    SELECT COALESCE(
        (SELECT jsonb_build_object('code', 'NOT_COMPUTED_YET', 'detail', CASE j.state WHEN 'LEASED' THEN 'its day is being computed' ELSE 'its day is queued' END)
         FROM core.job j
         WHERE j.kind = 'DAY_BLOCK' AND j.state IN ('QUEUED', 'LEASED') AND (p_profile IS NULL OR j.profile_id = p_profile)
           AND p_session BETWEEN j.first_session AND j.last_session
         ORDER BY j.state = 'LEASED' DESC, j.job_id DESC LIMIT 1),
        (SELECT jsonb_build_object('code', 'OMITTED', 'detail', o.reason)
         FROM core.omitted_session o
         WHERE o.session = p_session AND (p_profile IS NULL OR o.profile_id = p_profile) AND core.omission_current(o.omission_id)
         ORDER BY o.omission_id DESC LIMIT 1),
        (SELECT core.job_failure_cause(j.job_id)
         FROM core.job j
         WHERE j.kind = 'DAY_BLOCK' AND j.state = 'FAILED' AND (p_profile IS NULL OR j.profile_id = p_profile)
           AND p_session BETWEEN j.first_session AND j.last_session
         ORDER BY j.job_id DESC LIMIT 1),
        (SELECT jsonb_build_object('code', 'RETIRED', 'detail', t.reason)
         FROM core.retired_revision r JOIN core.retirement t USING (retirement_id)
         WHERE r.represented_date = p_session AND (p_profile IS NULL OR r.profile_id = p_profile)
         ORDER BY r.retirement_id DESC LIMIT 1),
        (SELECT jsonb_build_object('code', 'NOT_SCHEDULED', 'detail', 'its day job ' || b.job_id || ' was cancelled; base revision ' || b.revision_id
                                                                      || ' superseded, no day-local revision was published on it')
         FROM core.dropped_base b JOIN core.job j ON j.job_id = b.job_id
         WHERE b.represented_date = p_session AND (p_profile IS NULL OR b.profile_id = p_profile) AND j.state = 'CANCELLED'
           AND b.job_id = (SELECT max(x.job_id) FROM core.job x
                           WHERE x.kind = 'DAY_BLOCK' AND x.profile_id = b.profile_id AND b.represented_date BETWEEN x.first_session AND x.last_session)
         ORDER BY b.revision_id DESC LIMIT 1),
        jsonb_build_object('code', 'NOT_SCHEDULED', 'detail', 'no day job is queued, running or set aside for this session'))
$$;

-- The ordered pass's claim, as 0038 left it, with "published" read for the pass's own build (the
-- pass's day, and an earlier pass's day for that earlier pass's build) and a current omission only.
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
                                AND d.state NOT IN ('DONE', 'CANCELLED', 'FAILED') AND d.next_session <= j.first_session)));
