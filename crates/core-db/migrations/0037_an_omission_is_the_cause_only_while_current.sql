-- C-061 amended (audit 2026-11-22 prebuild, changes-core.md 11): an omission stated the session's
-- cause for ever. Omitted, then scheduled again and failed on an engine hold, the session read OMITTED
-- with the old reason, hiding the engine fault; published by hand later and then retired, it read
-- OMITTED as well. An omission is now the cause only while it is current: its day job is still the
-- latest day job of the profile covering the session, and no retirement of the session came after it.
-- Otherwise the causes after it (FAILED, RETIRED, NOT_SCHEDULED) state the session, as 0034 orders them.
CREATE OR REPLACE FUNCTION core.session_cause(p_session date, p_profile text) RETURNS jsonb LANGUAGE sql STABLE AS $$
    SELECT COALESCE(
        (SELECT jsonb_build_object('code', 'NOT_COMPUTED_YET', 'detail', CASE j.state WHEN 'LEASED' THEN 'its day is being computed' ELSE 'its day is queued' END)
         FROM core.job j
         WHERE j.kind = 'DAY_BLOCK' AND j.state IN ('QUEUED', 'LEASED') AND (p_profile IS NULL OR j.profile_id = p_profile)
           AND p_session BETWEEN j.first_session AND j.last_session
         ORDER BY j.state = 'LEASED' DESC, j.job_id DESC LIMIT 1),
        (SELECT jsonb_build_object('code', 'OMITTED', 'detail', o.reason)
         FROM core.omitted_session o
         WHERE o.session = p_session AND (p_profile IS NULL OR o.profile_id = p_profile)
           AND o.job_id = (SELECT max(j.job_id) FROM core.job j
                           WHERE j.kind = 'DAY_BLOCK' AND j.profile_id = o.profile_id AND o.session BETWEEN j.first_session AND j.last_session)
           AND NOT EXISTS (SELECT FROM core.retired_revision r JOIN core.retirement t USING (retirement_id)
                           WHERE r.represented_date = o.session AND r.profile_id = o.profile_id AND t.retired_at > o.omitted_at)
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
        jsonb_build_object('code', 'NOT_SCHEDULED', 'detail', 'no day job is queued, running or set aside for this session'))
$$;
