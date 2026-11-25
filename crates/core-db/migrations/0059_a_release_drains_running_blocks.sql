-- C-077 (owner, 2026-11-24: automation debt 1, "a release deletes running Jobs"): a worker release no
-- longer ends the running day blocks of the old build. The hand-over withdraws the old build as before
-- (nothing more of it is claimed; every queued job is carried), but a lease of a day block that is
-- computing a day keeps running for that one day: its job is cut to that day, the days after it are
-- carried to the successor at once, and the lease ends when its day is done and its reference check is
-- with core (or the worker finishes, or `work::DRAIN_MINUTES` pass: then it is stopped as before and its
-- day carried). One row per draining lease.
--
-- A lock wait fails this migration in 10 s instead of holding the core pod in `migrate` past the
-- workers' 10-minute leases (0054's rule).
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '60s';
CREATE TABLE core.drain (
    lease_id     bigint      PRIMARY KEY REFERENCES core.lease,
    job_id       bigint      NOT NULL REFERENCES core.job,
    hand_over_id bigint      NOT NULL REFERENCES core.hand_over,
    -- the day the lease finishes (its job's next session when the hand-over ran)
    session      date        NOT NULL,
    -- the drain's bound: past it the lease is stopped as a withdrawal stops one
    until        timestamptz NOT NULL,
    -- when the sweep settled it after its lease ended: the day done, or carried to the successor
    settled_at   timestamptz,
    -- the successor's job for the day, when the drain ended without it
    carried_job  bigint      REFERENCES core.job
);
CREATE INDEX drain_unsettled ON core.drain (lease_id) WHERE settled_at IS NULL;
