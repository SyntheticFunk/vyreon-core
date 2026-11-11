-- Work: the trading sessions, the job queue and its leases. The job table is the source of truth
-- for work; Kubernetes only runs containers (ARCHITECTURE 1). The queue is a plain PostgreSQL
-- table claimed with FOR UPDATE SKIP LOCKED (PostgreSQL documentation, SELECT, "The Locking
-- Clause": the documented way to run a queue with several consumers).

-- The engine's frozen trading calendar, as it lists it. Jobs are made of these sessions.
CREATE TABLE core.session (
    session         date PRIMARY KEY,
    calendar_sha256 text NOT NULL
);

-- One row: whether work may be claimed. A failed check pauses the queue until someone looks.
CREATE TABLE core.queue_control (
    only_row   boolean     PRIMARY KEY DEFAULT true CHECK (only_row),
    paused     boolean     NOT NULL DEFAULT false,
    reason     text,
    changed_at timestamptz NOT NULL DEFAULT now(),
    CHECK (paused = (reason IS NOT NULL))
);
INSERT INTO core.queue_control DEFAULT VALUES;

-- TAPE: one session's replay tapes reduced to the per-asset caches.
-- DAY_BLOCK: consecutive sessions, each computed and published by one worker.
-- ORDERED_PASS: one session's history-reading sensors, strictly in date order.
CREATE TABLE core.job (
    job_id        bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    kind          text        NOT NULL CHECK (kind IN ('TAPE', 'DAY_BLOCK', 'ORDERED_PASS')),
    profile_id    text        NOT NULL,
    engine_build  text        NOT NULL REFERENCES core.engine_build,
    first_session date        NOT NULL REFERENCES core.session,
    last_session  date        NOT NULL REFERENCES core.session,
    -- the first session not done yet: where a reclaimed job resumes. NULL once all are done.
    next_session  date        REFERENCES core.session,
    scope         jsonb,                    -- a recompute's families and measurements; NULL: everything
    state         text        NOT NULL DEFAULT 'QUEUED'
                  CHECK (state IN ('QUEUED', 'LEASED', 'DONE', 'FAILED', 'CANCELLED')),
    attempts      integer     NOT NULL DEFAULT 0,
    max_attempts  integer     NOT NULL DEFAULT 3 CHECK (max_attempts > 0),
    created_at    timestamptz NOT NULL DEFAULT now(),
    finished_at   timestamptz,
    CHECK (last_session >= first_session),
    CHECK (next_session IS NULL OR next_session BETWEEN first_session AND last_session),
    CHECK (kind = 'DAY_BLOCK' OR first_session = last_session)
);
CREATE INDEX job_by_kind ON core.job (kind, state, first_session);

-- A worker's hold on a job. The worker proves it with the token it was given at the claim (only
-- the token's digest is kept). A lease that is not renewed expires and the job is claimed again.
CREATE TABLE core.lease (
    lease_id     bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    job_id       bigint      NOT NULL REFERENCES core.job,
    worker       text        NOT NULL,
    token_sha256 text        NOT NULL UNIQUE,
    granted_at   timestamptz NOT NULL DEFAULT now(),
    expires_at   timestamptz NOT NULL,
    ended_at     timestamptz,
    outcome      text        CHECK (outcome IN ('DONE', 'FAILED', 'EXPIRED')),
    failed_stage text,                      -- where a failing worker says it failed
    failure      text,                      -- its message
    failure_log  text,                      -- the day log it sent
    CHECK ((ended_at IS NULL) = (outcome IS NULL))
);
CREATE UNIQUE INDEX lease_one_active ON core.lease (job_id) WHERE ended_at IS NULL;

-- What a worker may claim now. Used by the claim itself and by the autoscaler's count, so the
-- two cannot disagree.
--   * the queue is not paused and the job's build is admitted and not withdrawn;
--   * a day block waits for every tape job up to its last session (a day reads the tape caches
--     of all earlier sessions);
--   * an ordered pass waits for its day's base revision, for every earlier ordered pass, and for
--     every earlier session of an unfinished day block: it runs behind the contiguous run of
--     published days, one at a time.
CREATE VIEW core.claimable_job AS
    SELECT j.*
    FROM core.job j JOIN core.engine_build b USING (engine_build)
    WHERE j.state = 'QUEUED'
      AND b.admitted_at IS NOT NULL AND b.withdrawn_at IS NULL
      AND NOT (SELECT paused FROM core.queue_control)
      AND (j.kind <> 'DAY_BLOCK' OR NOT EXISTS (
              SELECT FROM core.job t
              WHERE t.kind = 'TAPE' AND t.first_session <= j.last_session AND t.state NOT IN ('DONE', 'CANCELLED')))
      AND (j.kind <> 'ORDERED_PASS' OR (
              EXISTS (SELECT FROM vyreon.day_revision d
                      WHERE d.represented_date = j.first_session AND d.profile_id = j.profile_id
                        AND d.state = 'PUBLISHED' AND d.base_revision IS NULL)
              AND NOT EXISTS (SELECT FROM core.job o
                              WHERE o.kind = 'ORDERED_PASS' AND o.profile_id = j.profile_id
                                AND o.first_session < j.first_session AND o.state NOT IN ('DONE', 'CANCELLED'))
              AND NOT EXISTS (SELECT FROM core.job d
                              WHERE d.kind = 'DAY_BLOCK' AND d.profile_id = j.profile_id
                                AND d.state NOT IN ('DONE', 'CANCELLED') AND d.next_session < j.first_session)));
