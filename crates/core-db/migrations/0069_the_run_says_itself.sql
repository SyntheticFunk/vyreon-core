-- C-097 (owner, 2026-11-26: "In general, you shouldn't have to go looking"): the run states its own timings and
-- withholdings. What each published revision withholds is counted once (a published revision never changes), by
-- measurement, grain and stated reason, with the assets behind it, so a whole-run census and a day's "unusual
-- withholding" are sums over this table, not a read of every body. Additive: nothing existing changes.
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '60s';

CREATE TABLE core.withheld_tally (
    revision_id    bigint  NOT NULL REFERENCES vyreon.day_revision ON DELETE CASCADE,
    measurement_id text    NOT NULL,
    grain          text    NOT NULL CHECK (grain IN ('slot', 'role', 'coordinate')),
    reason         text    NOT NULL,
    n              integer NOT NULL CHECK (n > 0),
    -- the assets (object ids) it is stated for, each once, in order
    objects        text[]  NOT NULL CHECK (cardinality(objects) > 0),
    PRIMARY KEY (revision_id, measurement_id, grain, reason)
);
-- A revision counted (one with nothing withheld has no row above).
CREATE TABLE core.withheld_counted (
    revision_id bigint      PRIMARY KEY REFERENCES vyreon.day_revision ON DELETE CASCADE,
    counted_at  timestamptz NOT NULL DEFAULT now()
);

-- The progress channel states a day's flags (slow, idle, refused, unusual withholding) once: the base revision.
ALTER TABLE core.progress_shown DROP CONSTRAINT progress_shown_source_check;
ALTER TABLE core.progress_shown ADD CONSTRAINT progress_shown_source_check
    CHECK (source IN ('revision', 'refcheck', 'lease', 'notice', 'hand_over', 'job', 'day'));
