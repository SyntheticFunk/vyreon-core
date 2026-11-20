-- C-059 (owner decision 2026-11-20): the tape caches stored before the tape verdicts (C-058) never
-- get one, their raw tapes being gone; a sample of their sessions is verified instead. A verify job
-- is a TAPE job whose scope is {"verify": true}: its worker downloads the session's replay files
-- again, recomputes each member's cache with its build and runs the tape reader on it, and reports a
-- check per member. Core compares the recomputed cache's digest with the cache it holds: equal, the
-- verdict is kept beside the stored cache (C-058); different, the check is a problem to look at.
-- A verify job stores no cache: the stored one is never replaced.
CREATE TABLE core.tape_check (
    tape_check_id     bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    job_id            bigint      NOT NULL REFERENCES core.job,
    lease_id          bigint      NOT NULL REFERENCES core.lease,
    session           date        NOT NULL REFERENCES core.session,
    asset_key         text        NOT NULL,
    method_version    text        NOT NULL,
    -- the cache core held when the check came (NULL: none, or a stated absence)
    stored_sha256     text        CHECK (stored_sha256 ~ '^[0-9a-f]{64}$'),
    -- the worker's recomputed cache (NULL: it recomputed none, `why`)
    recomputed_sha256 text        CHECK (recomputed_sha256 ~ '^[0-9a-f]{64}$'),
    outcome           text        NOT NULL CHECK (outcome IN ('MATCH', 'DIFFER', 'NOT_COMPARED')),
    -- the tape reader's verdict on the recomputed cache, and whether it was kept beside the stored one
    verdict_outcome   text        CHECK (verdict_outcome IN ('PASS', 'FAIL', 'WITHHELD', 'NOT_RUN')),
    verdict_kept      boolean     NOT NULL DEFAULT false,
    why               text,
    tape_bytes        bigint      CHECK (tape_bytes >= 0),
    document          jsonb       NOT NULL,
    checked_at        timestamptz NOT NULL DEFAULT now(),
    UNIQUE (job_id, session, asset_key),
    CHECK ((outcome = 'NOT_COMPARED') = (stored_sha256 IS NULL OR recomputed_sha256 IS NULL)),
    CHECK ((outcome = 'MATCH') = (stored_sha256 IS NOT NULL AND stored_sha256 = recomputed_sha256)),
    CHECK (outcome <> 'NOT_COMPARED' OR why IS NOT NULL),
    CHECK (NOT verdict_kept OR outcome = 'MATCH')
);
CREATE INDEX tape_check_by_session ON core.tape_check (session, asset_key, checked_at);

-- A verify job holds no session's caches back: the day blocks never wait on one (0014 otherwise).
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
                                AND o.first_session < j.first_session AND o.state NOT IN ('DONE', 'CANCELLED'))
              AND NOT EXISTS (SELECT FROM core.job d
                              WHERE d.kind = 'DAY_BLOCK' AND d.profile_id = j.profile_id
                                AND d.state NOT IN ('DONE', 'CANCELLED') AND d.next_session <= j.first_session)));
