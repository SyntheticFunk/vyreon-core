-- C-079 (owner, 2026-11-24: automation debt 5): after a release, core compares the method versions the
-- new build publishes with each published day's, plans the republish of the days that differ (the C-074
-- planner's choice, per set of measurements), schedules it when its cost keeps the spend under the owner's
-- ceiling, and says what it did. One row per build, profile and the layers compared (a build's history
-- layer is known only once its first ordered pass has run: compared then, in a row of its own).
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '60s';
CREATE TABLE core.recompute_plan (
    plan_id      bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    engine_build text        NOT NULL REFERENCES core.engine_build,
    profile_id   text        NOT NULL,
    -- the layers compared, comma-separated, of base, day-local and history
    layers       text        NOT NULL,
    planned_at   timestamptz NOT NULL DEFAULT clock_timestamp(),
    -- SCHEDULED: queued; HELD: not queued, the owner told why (the ceiling, the spend not known); NOTHING:
    -- no published day differs; FAILED: scheduling stopped part-way (what was queued is in `plan`)
    outcome      text        NOT NULL CHECK (outcome IN ('SCHEDULED', 'HELD', 'NOTHING', 'FAILED')),
    why          text,
    plan         jsonb       NOT NULL,
    UNIQUE (engine_build, profile_id, layers)
);
