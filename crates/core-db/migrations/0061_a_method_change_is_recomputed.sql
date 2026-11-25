-- C-079 (owner, 2026-11-24: automation debt 5): after a release, core compares the method versions the
-- new build publishes with each published day's, plans the republish of the days that differ (the C-074
-- planner's choice, per set of measurements), schedules it when its cost keeps the spend under the owner's
-- ceiling, and says what it did. One row per decision: a build's day layers once both are published, its
-- history layer once its first pass has run; days being computed when it was decided (`busy`) are decided
-- again once they are free; a HELD decision again when the ceiling or the spend's figures change
-- (release-3 audit, Codex 5 and 11, Gemini 8 and 11).
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '60s';
CREATE TABLE core.recompute_plan (
    plan_id      bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    engine_build text        NOT NULL REFERENCES core.engine_build,
    profile_id   text        NOT NULL,
    -- the layers compared, comma-separated, of base, day-local and history
    layers       text        NOT NULL,
    planned_at   timestamptz NOT NULL DEFAULT clock_timestamp(),
    -- SCHEDULED: queued (some groups may be the owner's: said); HELD: not queued, the owner told why (the
    -- ceiling, the spend not known); NOTHING: no published day differs; FAILED: scheduling stopped part-way
    -- (what was queued is in `plan`)
    outcome      text        NOT NULL CHECK (outcome IN ('SCHEDULED', 'HELD', 'NOTHING', 'FAILED')),
    why          text,
    plan         jsonb       NOT NULL,
    -- the differing days this decision planned, and those it left because they were being computed
    days         text[]      NOT NULL DEFAULT '{}',
    busy         text[]      NOT NULL DEFAULT '{}',
    -- its estimated cost (USD), reserved against the ceiling while its jobs run or are not yet billed
    cost_usd     double precision NOT NULL DEFAULT 0,
    -- the ceiling it was decided under
    ceiling_usd  double precision
);
CREATE INDEX recompute_plan_of ON core.recompute_plan (engine_build, profile_id, layers, plan_id);
-- the jobs a plan queued name it (`scope->>'recompute_plan'`)
CREATE INDEX job_recompute_plan ON core.job (((scope->>'recompute_plan')::bigint)) WHERE scope ? 'recompute_plan';
