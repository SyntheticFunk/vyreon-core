-- C-086 (owner, 2026-11-26: approved): the backfill's day blocks start one at a time. On 2026-11-26 seven
-- blocks started at once and shared the 1,900-a-minute request budget from their first minute: their first
-- days downloaded in 3.5 h instead of about 1.75 h. And a build that has not yet published one real day
-- clean runs one backfill block, not eight (the one-real-day gate, kept by hand until now).
-- C-087 (owner, 2026-11-26: approved): the backfill's stop is one flag in the database, which the scaler and
-- the re-run watch read (`vyreon-core backfill stop|resume|status`), not a setting that needs a restart.
--
-- A lock wait fails this migration in 10 s instead of holding the core pod in `migrate` (as 0054).
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '60s';

-- C-090 (owner, 2026-11-26: the stop rule stops only for systematic breakage): each published revision's
-- count also names the measurements an engine fault withheld on every object of the revision (`watch::COUNTED`;
-- M0-ANALYTICS and INT-IVSC01 on all ~62 assets). The kept counts are dropped and counted again by the watch
-- (`tally_untallied`, in batches, every minute since C-094; before it only with the 09:00 digest), as 0062 did; until a build's revisions are counted
-- again it is not proven (C-086: one backfill block at a time).
DELETE FROM core.revision_tally;
ALTER TABLE core.revision_tally ADD COLUMN whole_faults text[] NOT NULL DEFAULT '{}';

-- C-087: whether the backfill is stopped, why and since when (`backfill::stop`, `resume`).
ALTER TABLE core.queue_control
    ADD COLUMN backfill_stopped    boolean NOT NULL DEFAULT false,
    ADD COLUMN backfill_reason     text,
    ADD COLUMN backfill_changed_at timestamptz,
    ADD CONSTRAINT queue_control_backfill_check CHECK (backfill_stopped = (backfill_reason IS NOT NULL));

-- C-086: whether `p_build` has published one real day clean: a base revision of its own computation (not
-- a republish's copy-forward), still published, holding slots, with a day-local revision published on it,
-- and neither revision holding a measurement an engine fault withheld on every asset (`whole_faults` of
-- `core.revision_tally`, counted by the watch, C-090/C-094). Owner, 2026-11-26: the test is "can the engine
-- execute a small block", to avoid scaling to N nodes and having all N fail; a local engine fault (one asset)
-- is a catalog line, not a reason to hold the ramp. A revision not counted yet does not prove the build.
CREATE FUNCTION core.build_proven(p_build text) RETURNS boolean
    LANGUAGE sql STABLE SET search_path = pg_catalog, pg_temp AS $$
    SELECT EXISTS (
        SELECT FROM vyreon.day_revision d
        JOIN core.revision_tally td ON td.revision_id = d.revision_id
        JOIN vyreon.day_revision l ON l.base_revision = d.revision_id AND l.day_local AND l.state = 'PUBLISHED'
        JOIN core.revision_tally tl ON tl.revision_id = l.revision_id
        WHERE d.engine_build = p_build AND d.state = 'PUBLISHED' AND d.base_revision IS NULL AND d.republish_scope IS NULL
          AND td.slots > 0 AND cardinality(td.whole_faults) = 0 AND cardinality(tl.whole_faults) = 0)
$$;

-- C-086: how many more backfill day blocks (DAY_BLOCK, priority 0) of `p_build` may start at `p_at`:
-- none while one was leased in the last 20 minutes (`work::RAMP_MINUTES`); else, until the build is proven
-- (`core.build_proven`), one less each live lease of its backfill blocks (one block at a time); else one.
CREATE FUNCTION core.backfill_block_room(p_build text, p_at timestamptz DEFAULT now()) RETURNS bigint
    LANGUAGE sql STABLE SET search_path = pg_catalog, pg_temp AS $$
    SELECT CASE
        WHEN EXISTS (SELECT FROM core.lease l JOIN core.job j USING (job_id)
                     WHERE j.kind = 'DAY_BLOCK' AND j.priority <> 1 AND j.engine_build = p_build
                       AND l.granted_at > p_at - interval '20 minutes' AND l.granted_at <= p_at) THEN 0
        WHEN NOT core.build_proven(p_build)
            THEN GREATEST(0, 1 - (SELECT count(*) FROM core.lease l JOIN core.job j USING (job_id)
                                  WHERE j.kind = 'DAY_BLOCK' AND j.priority <> 1 AND j.engine_build = p_build AND l.ended_at IS NULL))
        ELSE 1 END
$$;

-- C-086, C-087: whether the scaler holds back a backfill job (priority 0) of `p_kind` and `p_build` at
-- `p_at`: the backfill stopped (its day blocks, ordered passes and reference-check re-runs), or a day block
-- with no room (`core.backfill_block_room`). The watch reads it so that held work is no stall.
CREATE FUNCTION core.backfill_held(p_kind text, p_build text, p_at timestamptz DEFAULT now()) RETURNS boolean
    LANGUAGE sql STABLE SET search_path = pg_catalog, pg_temp AS $$
    SELECT ((SELECT backfill_stopped FROM core.queue_control) AND p_kind IN ('DAY_BLOCK', 'ORDERED_PASS', 'REFCHECK'))
        OR (p_kind = 'DAY_BLOCK' AND core.backfill_block_room(p_build, p_at) = 0)
$$;

-- The scaler's count (C-073, 0055), now with C-086 and C-087: a daily session's work (priority 1) always
-- counts; the backfill's (priority 0) counts nothing while it is stopped (day blocks, ordered passes,
-- re-runs) or, for the work that calls the provider (all but the ordered passes), while a draw waited in
-- the last five minutes; its day blocks count at most `core.backfill_block_room`. KEDA's `accurate`
-- strategy deducts the pending Jobs (a pod not yet Running), so a 1 starts one pod, which claims; the
-- claim's lease then makes the count 0 for 20 minutes. SECURITY DEFINER for the reasons 0046 gives.
CREATE OR REPLACE FUNCTION core.scaler_count(p_kind text, p_build text, p_daily boolean DEFAULT NULL) RETURNS bigint
    LANGUAGE sql STABLE SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
    WITH c AS (
        SELECT j.priority = 1 AS daily, count(*) AS n FROM core.claimable_job j
        WHERE j.kind = p_kind AND j.engine_build = p_build AND j.next_session IS NOT NULL
          AND (p_daily IS NULL OR (j.priority = 1) = p_daily)
        GROUP BY 1)
    SELECT COALESCE(sum(CASE
        WHEN c.daily THEN c.n
        WHEN (SELECT backfill_stopped FROM core.queue_control) AND p_kind IN ('DAY_BLOCK', 'ORDERED_PASS', 'REFCHECK') THEN 0
        WHEN p_kind <> 'ORDERED_PASS' AND EXISTS (SELECT FROM core.budget_wait w WHERE w.last_at > now() - interval '5 minutes') THEN 0
        WHEN p_kind = 'DAY_BLOCK' THEN LEAST(c.n, core.backfill_block_room(p_build))
        ELSE c.n END), 0)::bigint
    FROM c
$$;
