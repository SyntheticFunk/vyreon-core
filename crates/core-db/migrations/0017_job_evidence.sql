-- C-049: what a worker whose attempt failed or was refused leaves before it exits, so that no
-- failure depends on someone reading the pod's log before the pod is gone. One record per lease
-- (an attempt): the stage and stop message, the input plan's refusals, and an excerpt of the
-- worker's logs, redacted by the worker (credentials removed), stored as sent: a gzip of one JSON
-- line (the account) and the excerpt's text after it. Written once: the same lease again changes
-- nothing. The lease's own `failed_stage`, `failure` and `failure_log` (0005) stay as they are.
CREATE TABLE core.job_evidence (
    job_evidence_id bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    lease_id        bigint      NOT NULL UNIQUE REFERENCES core.lease,
    job_id          bigint      NOT NULL REFERENCES core.job,
    -- the lease's place among the job's leases, from 1: the attempt (it keeps counting after a
    -- `retry-failed`, which sets the job's own count back to 0)
    attempt         integer     NOT NULL CHECK (attempt > 0),
    kind            text        NOT NULL CHECK (kind IN ('TAPE', 'DAY_BLOCK', 'ORDERED_PASS')),
    -- the session the attempt stopped on, as the worker names it (within the job's sessions);
    -- NULL when it stopped before its first session (preparing the block)
    session         date        REFERENCES core.session,
    first_session   date        NOT NULL,           -- the job's sessions when the evidence came
    last_session    date        NOT NULL,
    engine_build    text        NOT NULL REFERENCES core.engine_build,
    worker          text        NOT NULL,
    stage           text        NOT NULL CHECK (stage <> ''),
    message         text        NOT NULL CHECK (message <> ''),
    refusals        jsonb,                          -- the input plan's refusing entries, if any
    document_sha256 text        NOT NULL CHECK (document_sha256 ~ '^[0-9a-f]{64}$'),
    document        bytea       STORAGE EXTERNAL NOT NULL,  -- already compressed by the worker
    excerpt_bytes   bigint      NOT NULL CHECK (excerpt_bytes >= 0),  -- the excerpt unpacked
    stored_at       timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX job_evidence_by_job ON core.job_evidence (job_id, attempt);
