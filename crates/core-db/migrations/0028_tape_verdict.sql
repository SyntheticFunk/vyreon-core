-- C-058 (owner decision 2026-11-20; engine D-370 amended 2026-11-20): the tape worker's verdict on
-- each tape cache it stores. A day worker holds only the cache (the manifest and the engine's derived
-- bins), never the raw tape, so the reference suite cannot read the tape there; the tape worker, which
-- still holds it, runs the suite's independent tape reader on it and compares its bins with the cache.
-- Kept once beside the cache it judges (the packed cache's digest must be the stored one's) and served
-- with it; the day check reads the cached bins for the tape owners only under a PASS. A FAIL is a
-- disagreement to look at (REFCHECK_DISAGREES); none, or NOT_RUN, is a cache not verified.
CREATE TABLE core.tape_verdict (
    session            date        NOT NULL,
    asset_key          text        NOT NULL,
    method_version     text        NOT NULL,
    cache_sha256       text        NOT NULL CHECK (cache_sha256 ~ '^[0-9a-f]{64}$'),
    outcome            text        NOT NULL CHECK (outcome IN ('PASS', 'FAIL', 'WITHHELD', 'NOT_RUN')),
    differences        integer     NOT NULL CHECK (differences >= 0),
    reader_bins_sha256 text        CHECK (reader_bins_sha256 ~ '^[0-9a-f]{64}$'),
    why                text,
    document           jsonb       NOT NULL,
    stored_at          timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (session, asset_key, method_version),
    FOREIGN KEY (session, asset_key, method_version) REFERENCES core.tape_cache,
    CHECK ((outcome IN ('PASS', 'FAIL')) = (reader_bins_sha256 IS NOT NULL)),
    CHECK (outcome = 'PASS' OR why IS NOT NULL)
);
-- the problems and the digest read the failures and the newest
CREATE INDEX tape_verdict_failed ON core.tape_verdict (session) WHERE outcome = 'FAIL';
CREATE INDEX tape_verdict_stored ON core.tape_verdict (stored_at);
