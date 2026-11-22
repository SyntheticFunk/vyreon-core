-- C-068: the operational failure-surface audit of 2026-11-22 (opsurface/ops.md), 1 and 4.

-- 1 and 4: a build's unfinished work handed to its admitted successor (`build::hand_over`), once,
-- with what moved: one row per hand-over, and each carried job with the job that replaced it.
CREATE TABLE core.hand_over (
    hand_over_id bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    from_build   text        NOT NULL REFERENCES core.engine_build,
    to_build     text        NOT NULL REFERENCES core.engine_build CHECK (to_build <> from_build),
    reason       text        NOT NULL CHECK (reason <> ''),
    handed_at    timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE core.hand_over_job (
    hand_over_id bigint NOT NULL REFERENCES core.hand_over,
    old_job      bigint NOT NULL REFERENCES core.job,
    -- NULL: a tape verify job of the old build, cancelled and not carried (a sample, round 2, 11)
    new_job      bigint REFERENCES core.job,
    PRIMARY KEY (hand_over_id, old_job)
);
