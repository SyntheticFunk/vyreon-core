-- C-092 (owner, 2026-11-26): the backfill's provider work backs off smoothly. The scaler's count (C-073, 0055;
-- C-086, 0064) counted nothing for the backfill's provider work while a budget draw waited in the last 5 minutes,
-- and everything claimable once that passed: tapes, bursty by nature, all stopped starting, then all started at
-- once, saturated the budget again, and stopped again. Now, after the 5 minutes, a kind's backfill work starts
-- slowly for 30 minutes from the last wait: at most one new pod of the kind a minute (0 while one of that kind and
-- build was leased in the last minute); with no wait in 30 minutes, the count is as before. A wait within the
-- slow start stops new starts again (the 5 minutes). Running pods are never touched. The day blocks keep their own
-- ramp (one in 20 minutes, C-086), which is slower still; the daily session's work and the ordered passes always
-- count.
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '60s';

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
        WHEN p_kind = 'ORDERED_PASS' THEN c.n
        WHEN EXISTS (SELECT FROM core.budget_wait w WHERE w.last_at > now() - interval '5 minutes') THEN 0
        WHEN p_kind = 'DAY_BLOCK' THEN LEAST(c.n, core.backfill_block_room(p_build))
        WHEN EXISTS (SELECT FROM core.budget_wait w WHERE w.last_at > now() - interval '30 minutes')
            THEN LEAST(c.n, CASE WHEN EXISTS (SELECT FROM core.lease l JOIN core.job j USING (job_id)
                                              WHERE j.kind = p_kind AND j.engine_build = p_build AND j.priority <> 1
                                                AND l.granted_at > now() - interval '1 minute') THEN 0 ELSE 1 END)
        ELSE c.n END), 0)::bigint
    FROM c
$$;
