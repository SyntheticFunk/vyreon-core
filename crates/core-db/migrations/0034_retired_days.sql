-- C-062: `vyreon-core days retire <reason> <date>...` takes a day's published revisions out of what is
-- read (state SUPERSEDED; every row is kept, nothing is deleted) with the operator's reason: for the
-- days core holds from earlier builds, which phase 1 must not read as its own. Each retirement is one
-- row (its reason, the sessions named and the unfinished jobs it cancelled); each revision it took
-- down, one row of `core.retired_revision`.
CREATE TABLE core.retirement (
    retirement_id bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    reason        text        NOT NULL CHECK (btrim(reason) <> ''),
    sessions      date[]      NOT NULL CHECK (cardinality(sessions) > 0),
    cancelled     bigint[]    NOT NULL,
    retired_at    timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE core.retired_revision (
    revision_id      bigint NOT NULL PRIMARY KEY REFERENCES vyreon.day_revision,
    retirement_id    bigint NOT NULL REFERENCES core.retirement,
    represented_date date   NOT NULL,
    profile_id       text   NOT NULL
);
CREATE INDEX retired_revision_by_day ON core.retired_revision (represented_date, profile_id);

-- Why a session has no published day (0031), with RETIRED after a failed day job's cause and before
-- NOT_SCHEDULED: the latest retirement of one of its revisions, with its reason. A day queued again
-- after its retirement reads NOT_COMPUTED_YET, one that failed again its failure, as before.
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
