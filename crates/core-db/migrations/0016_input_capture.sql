-- C-043: the small inputs the provider restates in place (the company, macro and carry captures),
-- kept as a published day read them, so a sensor can be computed again on the same inputs and a
-- later fetch can be compared with the kept one. The archive is the engine's own packing (a gzip
-- tar whose first entry is the capture's manifest.json), stored as sent. Its identity is the
-- SHA-256 of that manifest, which the engine's receipts already bind. A capture is written once:
-- the same identity again changes nothing, and another identity for the same session and kind is
-- another version, kept beside the first with its own times, never in its place. A capture the
-- worker could not keep is recorded with its reason (an absence), as many times as it is reported.
CREATE TABLE core.input_capture (
    input_capture_id bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    session          date        NOT NULL REFERENCES core.session,
    kind             text        NOT NULL CHECK (kind IN ('company', 'macro', 'carry')),
    identity         text        CHECK (identity ~ '^[0-9a-f]{64}$'),
    archive_sha256   text        CHECK (archive_sha256 ~ '^[0-9a-f]{64}$'),
    archive          bytea       STORAGE EXTERNAL,      -- already compressed by the worker
    absence          text        CHECK (absence <> ''),
    -- when the worker's capture wrote its manifest (as the worker states it); the provider's own
    -- retrieval clocks are inside the manifest
    captured_at      timestamptz,
    stored_at        timestamptz NOT NULL DEFAULT clock_timestamp(),
    job_id           bigint      NOT NULL REFERENCES core.job,
    engine_build     text        NOT NULL REFERENCES core.engine_build,
    worker           text        NOT NULL,
    UNIQUE (session, kind, identity),
    CHECK ((identity IS NULL) = (archive IS NULL) AND (archive IS NULL) = (archive_sha256 IS NULL)
           AND (archive IS NULL) = (captured_at IS NULL)),
    CHECK ((archive IS NULL) <> (absence IS NULL))
);
