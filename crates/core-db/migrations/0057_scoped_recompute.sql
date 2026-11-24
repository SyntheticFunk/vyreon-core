-- C-074 (owner, 2026-11-24: "build scoped recompute now, while the cluster runs"): engine D-104's
-- family republish on the cluster. A REPUBLISH job recomputes one session's recompute set (the
-- families or measurements asked for and every same-day dependant, engine D-462) and publishes a
-- copy-forward revision of each layer it touches: the bundle carries the recomputed measurements'
-- slots only, and core copies every other slot of the layer, byte for byte, from the revision it
-- replaces (`vyreon.measurement.carried_from_revision`, `vyreon.day_revision.republish_scope`, both
-- in the store since 0002). The base revision stays STAGED until its day-local revision arrives; the
-- two are promoted in one transaction, so a day is replaced whole or not at all (`publish`).
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '60s';

ALTER TABLE core.job DROP CONSTRAINT job_kind_check;
ALTER TABLE core.job ADD CONSTRAINT job_kind_check CHECK (kind IN ('TAPE', 'DAY_BLOCK', 'ORDERED_PASS', 'REFCHECK', 'REPUBLISH'));
-- a republish is of one session and states why and what it recomputes (`work::schedule_republish`)
ALTER TABLE core.job ADD CONSTRAINT job_republish_scope_check
    CHECK (kind <> 'REPUBLISH' OR (first_session = last_session
                                   AND COALESCE(length(btrim(scope->>'reason')) > 0, false)
                                   AND COALESCE(jsonb_typeof(scope->'recompute') = 'object', false)));
-- one unfinished republish of a day and profile at a time
CREATE UNIQUE INDEX job_one_unfinished_republish ON core.job (profile_id, first_session)
    WHERE kind = 'REPUBLISH' AND state IN ('QUEUED', 'LEASED', 'FAILED');
ALTER TABLE core.job_evidence DROP CONSTRAINT job_evidence_kind_check;
ALTER TABLE core.job_evidence ADD CONSTRAINT job_evidence_kind_check CHECK (kind IN ('TAPE', 'DAY_BLOCK', 'ORDERED_PASS', 'REFCHECK', 'REPUBLISH'));
ALTER TABLE core.claim_miss DROP CONSTRAINT claim_miss_kind_check;
ALTER TABLE core.claim_miss ADD CONSTRAINT claim_miss_kind_check CHECK (kind IN ('TAPE', 'DAY_BLOCK', 'ORDERED_PASS', 'REFCHECK', 'REPUBLISH'));
ALTER TABLE core.stall_seen DROP CONSTRAINT stall_seen_kind_check;
ALTER TABLE core.stall_seen ADD CONSTRAINT stall_seen_kind_check CHECK (kind IN ('TAPE', 'DAY_BLOCK', 'DAILY_BLOCK', 'ORDERED_PASS', 'REFCHECK', 'REPUBLISH', 'CHAIN'));

-- The engine's republish plans, from the build's inventory (`republish`, engine D-462): each
-- family's and measurement's recompute set and what it costs a worker's day (stages, input
-- categories), with the measured reference timings. The planner (`republish::plan`) unites them for
-- a scope; a build registered without them cannot be republished.
CREATE TABLE core.build_republish (
    engine_build text  PRIMARY KEY REFERENCES core.engine_build,
    plans        jsonb NOT NULL CHECK (jsonb_typeof(plans->'seeds') = 'object')
);

-- The ordered pass waits for the republishes of its day and every earlier day, as it waits for the
-- day blocks: what they publish supersedes what it would read (C-037), so the passes after a range
-- of republishes run once, after them. Otherwise 0040's view.
CREATE OR REPLACE VIEW core.claimable_job AS
    SELECT j.*
    FROM core.job j JOIN core.engine_build b USING (engine_build)
    WHERE j.state = 'QUEUED' AND j.next_session IS NOT NULL
      AND b.admitted_at IS NOT NULL AND b.withdrawn_at IS NULL
      AND NOT (SELECT paused FROM core.queue_control)
      AND (j.kind <> 'DAY_BLOCK' OR NOT EXISTS (
              SELECT FROM core.job t
              WHERE t.kind = 'TAPE' AND t.first_session <= j.last_session AND t.state NOT IN ('DONE', 'CANCELLED')
                AND NOT COALESCE(t.scope @> '{"verify": true}', false)))
      AND (j.kind <> 'ORDERED_PASS' OR (
              core.day_published(j.first_session, j.profile_id, j.engine_build)
              AND NOT EXISTS (SELECT FROM core.job o
                              WHERE o.kind = 'ORDERED_PASS' AND o.profile_id = j.profile_id
                                AND o.first_session < j.first_session AND o.state NOT IN ('DONE', 'CANCELLED')
                                AND core.build_runs(o.engine_build)
                                AND (core.day_published(o.first_session, o.profile_id, o.engine_build)
                                     OR EXISTS (SELECT FROM core.job p
                                                WHERE p.kind = 'DAY_BLOCK' AND p.profile_id = o.profile_id AND p.state IN ('QUEUED', 'LEASED')
                                                  AND o.first_session BETWEEN p.first_session AND p.last_session)
                                     OR NOT (EXISTS (SELECT FROM core.job c
                                                     WHERE c.kind = 'DAY_BLOCK' AND c.profile_id = o.profile_id
                                                       AND o.first_session BETWEEN c.first_session AND c.last_session
                                                       AND core.build_runs(c.engine_build)
                                                       AND (c.state <> 'DONE'
                                                            OR EXISTS (SELECT FROM vyreon.day_revision r
                                                                       WHERE r.represented_date = o.first_session AND r.profile_id = o.profile_id
                                                                         AND r.state = 'PUBLISHED' AND r.base_revision IS NULL)))
                                             OR EXISTS (SELECT FROM core.omitted_session m
                                                        WHERE m.session = o.first_session AND m.profile_id = o.profile_id
                                                          AND core.omission_current(m.omission_id)))))
              AND NOT EXISTS (SELECT FROM core.job d
                              WHERE d.kind = 'DAY_BLOCK' AND d.profile_id = j.profile_id
                                AND d.state NOT IN ('DONE', 'CANCELLED', 'FAILED') AND d.next_session <= j.first_session)
              AND NOT EXISTS (SELECT FROM core.job r
                              WHERE r.kind = 'REPUBLISH' AND r.profile_id = j.profile_id
                                AND r.state IN ('QUEUED', 'LEASED') AND r.first_session <= j.first_session
                                AND core.build_runs(r.engine_build))));
