-- C-061 (owner, 2026-11-21): a failed day is a missing date, stated with its cause, never a block on
-- later history. The engine's history windows already walk the calendar and state a session with no
-- published day as NOT_PUBLISHED (h5-derived), and a day published later re-derives every later
-- pass (C-037), so holding the later passes for a FAILED day buys nothing.

-- A date the operator omitted: its day failed for the provider (never for the engine: an engine fault
-- is fixed and recomputed) and is not to be computed. `vyreon-core omit` writes one row and cancels
-- the FAILED day job (job_id) and the session's queued ordered pass in the same transaction.
CREATE TABLE core.omitted_session (
    omission_id bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    session     date        NOT NULL REFERENCES core.session,
    profile_id  text        NOT NULL,
    reason      text        NOT NULL CHECK (btrim(reason) <> ''),
    job_id      bigint      NOT NULL REFERENCES core.job,
    omitted_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX omitted_session_by_session ON core.omitted_session (session, profile_id);

-- The cause of a failed day, from its last stop message (the worker's `fail`, or its evidence,
-- C-049). The engine stops a day it holds with `stop: <date> not published: held: ...`
-- (vyreon-cli days.rs `why`, exports.rs `hold_final`): "... lack a provider input (more than X)"
-- is the provider-input hold, "... withheld for an engine fault (more than X)" the fault hold; the
-- detail is the hold's counts, without its list of slots. Anything else is FAILED with the
-- message's first line.
CREATE FUNCTION core.failure_cause(message text) RETURNS jsonb LANGUAGE sql IMMUTABLE AS $$
    SELECT CASE
        WHEN h LIKE '%lack a provider input%' THEN jsonb_build_object('code', 'PROVIDER_INPUT_MISSING', 'detail', regexp_replace(h, '\): .*$', ')'))
        WHEN h LIKE '%withheld for an engine fault%' THEN jsonb_build_object('code', 'ENGINE_FAULT', 'detail', regexp_replace(h, '\): .*$', ')'))
        ELSE jsonb_build_object('code', 'FAILED', 'detail', COALESCE(NULLIF(left(split_part(message, E'\n', 1), 300), ''), 'no reason recorded'))
    END
    FROM (SELECT (regexp_match(message, '(?:^|: )held: ([^\r\n]*)'))[1] AS h) x
$$;

-- The cause of a FAILED job, from its last attempt: that lease's evidence, else its failure.
CREATE FUNCTION core.job_failure_cause(p_job bigint) RETURNS jsonb LANGUAGE sql STABLE AS $$
    SELECT core.failure_cause(COALESCE(
        (SELECT COALESCE(e.message, l.failure) FROM core.lease l LEFT JOIN core.job_evidence e ON e.lease_id = l.lease_id
         WHERE l.job_id = p_job AND l.outcome IN ('FAILED', 'EXPIRED') ORDER BY l.lease_id DESC LIMIT 1),
        'no reason recorded'))
$$;

-- Why a session has no published day, for a profile (NULL: any): {code, detail}. In this order:
--   NOT_COMPUTED_YET  a day job QUEUED or LEASED covers it (a retried or re-scheduled day as well);
--   OMITTED           the operator omitted it, with the reason;
--   PROVIDER_INPUT_MISSING / ENGINE_FAULT / FAILED   its day job is FAILED (`core.job_failure_cause`);
--   NOT_SCHEDULED     no day job is pending or set aside for it.
-- The caller asks only for a session with nothing published: the function does not look.
CREATE FUNCTION core.session_cause(p_session date, p_profile text) RETURNS jsonb LANGUAGE sql STABLE AS $$
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
        jsonb_build_object('code', 'NOT_SCHEDULED', 'detail', 'no day job is queued, running or set aside for this session'))
$$;

-- The ordered pass's claim, as 0029 left it, with two changes (DAY_BLOCK and TAPE claims unchanged):
--   * a FAILED day block no longer holds back the passes of later days (it is a missing date);
--   * an earlier pass does not hold back later ones while its own day has no published base and
--     that day's block is FAILED or the day is omitted, with no day job pending for it. A day queued
--     again (`queue retry-failed`, or scheduled again) holds the passes again, as any pending day.
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
