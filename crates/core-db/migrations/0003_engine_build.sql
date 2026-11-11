-- Engine builds and their slot inventories. A build is registered with the exact slots each of its
-- profiles publishes; core checks every day against that inventory, never against a list the
-- worker sends with the day (ARCHITECTURE 4.1). A build may claim work only once admitted
-- (C-014b): its golden days and block-start test passed. For the first build admission is a
-- manual checklist, recorded in `admission`.
--
-- The inventory is the engine's expected_slot keyed by build as well as profile. It is exact for
-- a fixed member list; when members vary by date (engine E6) it becomes sensors by object class,
-- expanded over the day's members.

CREATE TABLE core.engine_build (
    engine_build  text        PRIMARY KEY CHECK (engine_build <> ''),   -- exact binary identity
    registered_at timestamptz NOT NULL DEFAULT now(),
    admitted_at   timestamptz,
    admission     jsonb,                 -- what it was admitted on: the checks, their results, by whom
    withdrawn_at  timestamptz,           -- an admitted build that may no longer claim work
    withdrawal    text,
    CHECK ((admitted_at IS NULL) = (admission IS NULL)),
    CHECK ((withdrawn_at IS NULL) = (withdrawal IS NULL)),
    CHECK (withdrawn_at IS NULL OR admitted_at IS NOT NULL)
);

CREATE TABLE core.expected_slot (
    engine_build     text    NOT NULL REFERENCES core.engine_build,
    profile_revision text    NOT NULL,
    slot_ordinal     integer NOT NULL,
    measurement_id   text    NOT NULL,
    object_id        text    NOT NULL,
    layer            text    NOT NULL,          -- BASELINE | M0_EXTENSION | DEMAND_LED
    PRIMARY KEY (engine_build, profile_revision, measurement_id, object_id),
    UNIQUE (engine_build, profile_revision, slot_ordinal)
);

-- Every revision names a registered build.
ALTER TABLE vyreon.day_revision
    ADD CONSTRAINT day_revision_engine_build_fkey FOREIGN KEY (engine_build) REFERENCES core.engine_build;
CREATE INDEX day_revision_by_build ON vyreon.day_revision (engine_build);
