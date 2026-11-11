-- The measurement store. Started from the engine's db/schema.sql at engine-v1 (C-019) and changed
-- in three ways:
--   * the four large tables carry the represented date in their keys and are partitioned by its
--     month (C-014a, E15; PostgreSQL declarative partitioning);
--   * a child row's date is tied to its revision's by a composite foreign key, so a row can never
--     sit in another day's partition;
--   * the engine's upgrade steps (ADD COLUMN IF NOT EXISTS, the D-080 backfill) are folded in.
-- Not here: the slot inventory and the promotion of a staged revision (they belong to the admitted
-- engine build and the publish path), and the engine's republish_job (a recompute is a job in
-- core's queue).

CREATE TABLE vyreon.day_revision (
    revision_id      bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    represented_date date        NOT NULL,
    profile_id       text        NOT NULL,
    profile_revision text        NOT NULL,
    engine_build     text        NOT NULL,      -- exact binary identity
    snapshot_cutoff  timestamptz NOT NULL,
    state            text        NOT NULL DEFAULT 'STAGED'
                     CHECK (state IN ('STAGED', 'PUBLISHED', 'SUPERSEDED', 'REJECTED')),
    created_at       timestamptz NOT NULL DEFAULT now(),
    published_at     timestamptz,
    rejection        text,
    -- A derived revision names the base revision of the same day it was computed from (engine
    -- D-080); NULL on a base revision.
    base_revision    bigint,
    -- What a family republish recomputed and why (engine R9c); NULL on a day's ordinary revision.
    republish_scope  jsonb,
    UNIQUE (revision_id, represented_date),
    FOREIGN KEY (base_revision, represented_date) REFERENCES vyreon.day_revision (revision_id, represented_date)
);
-- At most one PUBLISHED revision per day and profile.
CREATE UNIQUE INDEX day_revision_one_published
    ON vyreon.day_revision (represented_date, profile_revision) WHERE state = 'PUBLISHED';
CREATE INDEX day_revision_by_base ON vyreon.day_revision (base_revision) WHERE base_revision IS NOT NULL;

-- One row per governed slot. `body` is the complete accepted measurement as JSONB for query;
-- `body_text` is the exact canonical JSON the engine produced, kept so the body digest can be
-- verified byte for byte on readback.
CREATE TABLE vyreon.measurement (
    revision_id           bigint NOT NULL,
    measurement_id        text   NOT NULL,
    object_id             text   NOT NULL,
    represented_date      date   NOT NULL,
    slot_state            text   NOT NULL,      -- owner-level support summary
    science_version       text   NOT NULL,
    method_version        text   NOT NULL,
    body_sha256           text   NOT NULL CHECK (body_sha256 ~ '^[0-9a-f]{64}$'),
    body_text             text   NOT NULL,
    body                  jsonb  NOT NULL,
    receipt               jsonb  NOT NULL,      -- concise calculation provenance
    -- A row a family republish copied from the day's previous revision names the revision it was
    -- first computed in (engine R9c); NULL on a computed row.
    carried_from_revision bigint,
    PRIMARY KEY (revision_id, measurement_id, object_id, represented_date),
    FOREIGN KEY (revision_id, represented_date)
        REFERENCES vyreon.day_revision (revision_id, represented_date) ON DELETE CASCADE
) PARTITION BY RANGE (represented_date);
CREATE INDEX measurement_by_sensor ON vyreon.measurement (measurement_id, object_id);

-- Product event streams (M0 owners). A body that commits to a stream through
-- product_stream_root_sha256 is complete only with the stream itself: root.json exactly as
-- written, and the events.jsonl bytes gzip-compressed (the SHA-256 of the uncompressed bytes is
-- events_sha256). Already compressed, so stored without a second compression attempt.
CREATE TABLE vyreon.measurement_stream (
    revision_id      bigint NOT NULL,
    measurement_id   text   NOT NULL,
    object_id        text   NOT NULL,
    represented_date date   NOT NULL,
    root_sha256      text   NOT NULL,
    root_bytes       bytea  NOT NULL,
    events_sha256    text   NOT NULL,
    event_count      bigint NOT NULL,
    byte_length      bigint NOT NULL,
    events_gzip      bytea  STORAGE EXTERNAL NOT NULL,
    PRIMARY KEY (revision_id, measurement_id, object_id, represented_date),
    FOREIGN KEY (revision_id, measurement_id, object_id, represented_date)
        REFERENCES vyreon.measurement ON DELETE CASCADE
) PARTITION BY RANGE (represented_date);

-- Reader-facing numeric values of each revision (engine D-067): supported scalars only, indexed
-- once per revision so the 63-session unusualness windows are cheap. Derived from the bodies.
CREATE TABLE vyreon.reader_scalar (
    revision_id      bigint           NOT NULL,
    object_id        text             NOT NULL,
    measurement_id   text             NOT NULL,
    role_id          text             NOT NULL,
    represented_date date             NOT NULL,
    value            double precision NOT NULL,
    PRIMARY KEY (revision_id, object_id, measurement_id, role_id, represented_date),
    FOREIGN KEY (revision_id, represented_date)
        REFERENCES vyreon.day_revision (revision_id, represented_date) ON DELETE CASCADE
) PARTITION BY RANGE (represented_date);
CREATE INDEX reader_scalar_by_series ON vyreon.reader_scalar (object_id, measurement_id, role_id);

-- Each revision's supported robust z values (engine D-069): the pooled history from which each
-- measure's 95% flag line is calibrated.
CREATE TABLE vyreon.unusual_score (
    revision_id      bigint           NOT NULL,
    object_id        text             NOT NULL,
    measurement_id   text             NOT NULL,
    role_id          text             NOT NULL,
    represented_date date             NOT NULL,
    z                double precision NOT NULL,
    PRIMARY KEY (revision_id, object_id, measurement_id, role_id, represented_date),
    FOREIGN KEY (revision_id, represented_date)
        REFERENCES vyreon.day_revision (revision_id, represented_date) ON DELETE CASCADE
) PARTITION BY RANGE (represented_date);
CREATE INDEX unusual_score_by_measure ON vyreon.unusual_score (measurement_id, role_id);

-- Creates the monthly partitions of the four tables for every month from `first` to `last`, where
-- they do not exist yet, and returns how many it created. There is no default partition: a row
-- for a month without partitions is refused.
--
-- Each partition is created as a plain table and then attached, which locks the parent only in
-- SHARE UPDATE EXCLUSIVE mode, so readers and writers of other months carry on (PostgreSQL
-- documentation, "Table Partitioning", 5.12.2.2). Attaching also takes a short SHARE ROW EXCLUSIVE
-- lock on vyreon.day_revision for the foreign key. Call this when work is scheduled, in its own
-- transaction, and never inside a transaction that has written to the store: two such
-- transactions would deadlock on that lock.
CREATE FUNCTION vyreon.ensure_months(first date, last date) RETURNS integer
LANGUAGE plpgsql AS $$
DECLARE
    month   date;
    parent  text;
    part    text;
    created integer := 0;
BEGIN
    IF first IS NULL OR last IS NULL OR last < first THEN
        RAISE EXCEPTION 'ensure_months: bad range % to %', first, last;
    END IF;
    IF last > first + 7305 THEN
        RAISE EXCEPTION 'ensure_months: % to % is more than 20 years', first, last;
    END IF;
    PERFORM pg_advisory_xact_lock(8751186113873113966);  -- one caller at a time
    FOR month IN SELECT generate_series(date_trunc('month', first::timestamp), last::timestamp, interval '1 month')::date LOOP
        FOREACH parent IN ARRAY ARRAY['measurement', 'measurement_stream', 'reader_scalar', 'unusual_score'] LOOP
            part := parent || to_char(month, '"_y"YYYY"m"MM');
            IF to_regclass(format('vyreon.%I', part)) IS NULL THEN
                EXECUTE format('CREATE TABLE vyreon.%I (LIKE vyreon.%I INCLUDING DEFAULTS INCLUDING CONSTRAINTS INCLUDING STORAGE)', part, parent);
                EXECUTE format('ALTER TABLE vyreon.%I ATTACH PARTITION vyreon.%I FOR VALUES FROM (%L) TO (%L)',
                               parent, part, month, (month + interval '1 month')::date);
                created := created + 1;
            END IF;
        END LOOP;
    END LOOP;
    RETURN created;
END $$;

-- Reader surface: only published, coherent revisions.
CREATE VIEW vyreon.published_measurement AS
    SELECT d.represented_date, d.profile_revision, m.revision_id, m.measurement_id, m.object_id, m.slot_state,
           m.science_version, m.method_version, m.body_sha256, m.body_text, m.body, m.receipt, m.carried_from_revision
    FROM vyreon.day_revision d JOIN vyreon.measurement m USING (revision_id, represented_date)
    WHERE d.state = 'PUBLISHED';

-- Accepted revisions: PUBLISHED, or SUPERSEDED by a later publication and still readable by
-- explicit id. Staged and rejected revisions are never visible.
CREATE VIEW vyreon.accepted_measurement AS
    SELECT d.represented_date, d.profile_revision, d.state AS revision_state, d.published_at, d.snapshot_cutoff,
           d.engine_build, m.revision_id, m.measurement_id, m.object_id, m.body_sha256, m.body_text,
           m.body->>'schema' AS body_schema
    FROM vyreon.day_revision d JOIN vyreon.measurement m USING (revision_id, represented_date)
    WHERE d.state IN ('PUBLISHED', 'SUPERSEDED');

CREATE VIEW vyreon.accepted_stream AS
    SELECT d.state AS revision_state, s.revision_id, s.measurement_id, s.object_id, s.represented_date, s.root_sha256,
           s.root_bytes, s.events_sha256, s.event_count, s.byte_length, s.events_gzip
    FROM vyreon.day_revision d JOIN vyreon.measurement_stream s USING (revision_id, represented_date)
    WHERE d.state IN ('PUBLISHED', 'SUPERSEDED');

CREATE VIEW vyreon.accepted_reader_scalar AS
    SELECT d.represented_date, d.profile_revision, s.revision_id, s.object_id, s.measurement_id, s.role_id, s.value
    FROM vyreon.day_revision d JOIN vyreon.reader_scalar s USING (revision_id, represented_date)
    WHERE d.state = 'PUBLISHED';

-- The read-only role (the ordered pass, C-020; later the reader API's login). Roles belong to the
-- server, not the database: where the deployment declares it first, this only grants. The handler
-- covers another database on the same server creating it at the same moment.
DO $$ BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'vyreon_reader') THEN
        CREATE ROLE vyreon_reader NOLOGIN;
    END IF;
EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL;
END $$;
GRANT USAGE ON SCHEMA vyreon TO vyreon_reader;
GRANT SELECT ON vyreon.published_measurement, vyreon.accepted_measurement, vyreon.accepted_stream,
                vyreon.accepted_reader_scalar TO vyreon_reader;
