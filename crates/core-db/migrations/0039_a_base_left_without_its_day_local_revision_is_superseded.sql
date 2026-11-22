-- C-063 amended (audit 2026-11-22 prebuild round 2, core-r2.md 1, above the cut line): a day block
-- whose day ended for good (set aside FAILED, or cancelled) after its base was published and before
-- its day-local revision left the base PUBLISHED. 0035 made such a day missing for the claim, but the
-- engine's ordered pass reads every PUBLISHED base directly (vyreon-cli `derive/mod.rs` `load_with`),
-- so the "missing" day still fed later days' history (DLM-OI-CHANGE `raw_change_1` from its VS-O02 and
-- VS-O04). Core now supersedes such a base when the block ends (`publish::drop_base_without_day_local`,
-- with C-037's rederive of what read it), and records it here; the day is missing everywhere, and a
-- later attempt that publishes the base and its day-local revision publishes it as any day.
CREATE TABLE core.dropped_base (
    revision_id      bigint      NOT NULL PRIMARY KEY REFERENCES vyreon.day_revision,
    job_id           bigint      NOT NULL REFERENCES core.job,
    represented_date date        NOT NULL,
    profile_id       text        NOT NULL,
    dropped_at       timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX dropped_base_by_job ON core.dropped_base (job_id);
CREATE INDEX dropped_base_by_day ON core.dropped_base (represented_date, profile_id);

-- A failed day's cause (0031), its detail opening with the base its job left and core superseded.
CREATE OR REPLACE FUNCTION core.job_failure_cause(p_job bigint) RETURNS jsonb LANGUAGE sql STABLE AS $$
    SELECT CASE WHEN d.revision_id IS NULL THEN c.cause
                ELSE jsonb_set(c.cause, '{detail}', to_jsonb('base revision ' || d.revision_id
                                                             || ' superseded, no day-local revision was published on it; ' || (c.cause->>'detail')))
           END
    FROM (SELECT core.failure_cause(COALESCE(
              (SELECT COALESCE(e.message, l.failure) FROM core.lease l LEFT JOIN core.job_evidence e ON e.lease_id = l.lease_id
               WHERE l.job_id = p_job AND l.outcome IN ('FAILED', 'EXPIRED') ORDER BY l.lease_id DESC LIMIT 1),
              'no reason recorded')) AS cause) c
    LEFT JOIN LATERAL (SELECT b.revision_id FROM core.dropped_base b WHERE b.job_id = p_job ORDER BY b.revision_id DESC LIMIT 1) d ON true
$$;

-- Why a session has no published day (0037), with one more cause before NOT_SCHEDULED: a day whose
-- latest day job was cancelled after its base was published, the base superseded by core. A FAILED
-- day job states its base through `core.job_failure_cause`.
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
        (SELECT jsonb_build_object('code', 'NOT_SCHEDULED', 'detail', 'its day job ' || b.job_id || ' was cancelled; base revision ' || b.revision_id
                                                                      || ' superseded, no day-local revision was published on it')
         FROM core.dropped_base b JOIN core.job j ON j.job_id = b.job_id
         WHERE b.represented_date = p_session AND (p_profile IS NULL OR b.profile_id = p_profile) AND j.state = 'CANCELLED'
           AND b.job_id = (SELECT max(x.job_id) FROM core.job x
                           WHERE x.kind = 'DAY_BLOCK' AND x.profile_id = b.profile_id AND b.represented_date BETWEEN x.first_session AND x.last_session)
         ORDER BY b.revision_id DESC LIMIT 1),
        jsonb_build_object('code', 'NOT_SCHEDULED', 'detail', 'no day job is queued, running or set aside for this session'))
$$;
