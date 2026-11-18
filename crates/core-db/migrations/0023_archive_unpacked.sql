-- C-052 amended (audit 2026-11-17 round w, 7 and 17).
-- (17) A row KEPT before 0022 has no endpoint: where it was read back is not recorded, so it is
-- not counted as kept (none are expected: the archive was off on the cluster). From here a KEPT
-- row always names its endpoint.
UPDATE core.perishable_archive
SET state = 'FAILED', kept_at = NULL,
    failure = 'kept before migration 0022, where it was read back is not recorded: not counted as kept (C-052, round w 17)'
WHERE state = 'KEPT' AND endpoint IS NULL;
ALTER TABLE core.perishable_archive ADD CONSTRAINT perishable_archive_kept_where CHECK (state <> 'KEPT' OR endpoint IS NOT NULL);

-- (7) A category a worker could not pack (its deadline passed, a file could not be read, zstd
-- failed) is reported here, so the archive it never requested is still counted missing.
CREATE TABLE core.perishable_unpacked (
    unpacked_id  bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    session      date        NOT NULL REFERENCES core.session,
    category     text        NOT NULL CHECK (category IN ('exq01-events', 'act01-auto')),
    failure      text        NOT NULL,
    reported_at  timestamptz NOT NULL,
    job_id       bigint      NOT NULL REFERENCES core.job,
    engine_build text        NOT NULL REFERENCES core.engine_build,
    worker       text        NOT NULL
);
CREATE INDEX perishable_unpacked_session ON core.perishable_unpacked (session, category);
