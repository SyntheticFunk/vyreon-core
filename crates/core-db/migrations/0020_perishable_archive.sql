-- C-052: the perishable archive. A published day's perishable inputs (the cohort prints the
-- provider keeps about five sessions, and the ACT01 interval pulls) go from the worker straight to
-- the Spaces bucket through a pre-signed upload URL core issues for one named object; core alone
-- holds the storage key. One row per object: (session, category, the archive's SHA-256). The
-- object key is content-addressed (`perishables/<category>/<session>/<sha256>.tar.zst`), so a
-- later archive of the same session is another object beside the first, never in its place, and
-- a KEPT object is never issued a URL again. The URL itself is a credential and is never stored:
-- only when it expires. KEPT means core read the object back from the bucket (its size, its MD5
-- as the store's ETag, and the SHA-256 the upload was signed with) and found what was requested.
CREATE TABLE core.perishable_archive (
    archive_id     bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    session        date        NOT NULL REFERENCES core.session,
    category       text        NOT NULL CHECK (category IN ('exq01-events', 'act01-auto')),
    sha256         text        NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    md5            text        NOT NULL CHECK (md5 ~ '^[0-9a-f]{32}$'),
    bytes          bigint      NOT NULL CHECK (bytes > 0),
    bucket         text        NOT NULL,
    object_key     text        NOT NULL,
    state          text        NOT NULL CHECK (state IN ('REQUESTED', 'KEPT', 'FAILED')),
    -- how many upload URLs were issued for it (a request sent again issues a new one)
    urls_issued    integer     NOT NULL DEFAULT 1 CHECK (urls_issued > 0),
    requested_at   timestamptz NOT NULL,
    url_expires_at timestamptz NOT NULL,
    kept_at        timestamptz,
    -- why it is not kept: what the worker reported, or what core found reading it back
    failure        text,
    job_id         bigint      NOT NULL REFERENCES core.job,
    engine_build   text        NOT NULL REFERENCES core.engine_build,
    worker         text        NOT NULL,
    UNIQUE (session, category, sha256),
    UNIQUE (bucket, object_key),
    CHECK ((state = 'KEPT') = (kept_at IS NOT NULL)),
    CHECK (state <> 'FAILED' OR failure IS NOT NULL)
);
CREATE INDEX perishable_archive_kept ON core.perishable_archive (session, category) WHERE state = 'KEPT';

-- Perishable archives missing past the configured age are a problem (C-050's watch).
ALTER TABLE core.problem DROP CONSTRAINT problem_kind_check;
ALTER TABLE core.problem ADD CONSTRAINT problem_kind_check
    CHECK (kind IN ('SESSION_LATE', 'REPORT_AT_RISK', 'QUEUE_PAUSED', 'JOB_FAILED', 'PROVIDER_AUTH',
                    'PROVIDER_ERRORS', 'DATABASE_FULL', 'CALENDAR_END', 'NOT_DERIVED', 'DAILY_STOPPED',
                    'ARCHIVE_MISSING'));
