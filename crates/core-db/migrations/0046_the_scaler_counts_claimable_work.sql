-- C-066: KEDA starts the workers from the queue (deploy/k8s/50-keda.yaml). Its `postgresql` trigger
-- counts the claimable jobs of one kind and one engine build, exactly as a worker of that build
-- claims them (`work::claim_as`: `core.claimable_job` by kind and build, a daily worker only
-- priority 1). It logs in with a login of its own in the group `vyreon_scaler`, which can run this
-- count and nothing else: no table, no view, no other function.
--
-- Why a function and not SELECT on the view: the view calls `core.day_published`,
-- `core.build_runs` and `core.omission_current`, and PostgreSQL checks functions called in a view as
-- if the query using the view called them (CREATE VIEW, Notes); those are SECURITY INVOKER, so the
-- scaler's login would need SELECT on every table they read (the store's revisions among them).
-- SECURITY DEFINER runs the count with the rights of the migrations' owner, with a fixed
-- search_path (PostgreSQL, "Writing SECURITY DEFINER Functions Safely"); every name below and in
-- what it calls is schema-qualified. EXECUTE is revoked from PUBLIC first.
--
-- `p_daily`: NULL counts every claimable job of the kind and build (what an ordinary worker can
-- take); true only the daily runs' (priority 1: what a `VYREON_WORKER_DAILY` worker takes); false
-- only the rest, so the backfill's ScaledJob does not start a pod for a daily job its own
-- ScaledJob has started one for.
--
-- The group role is the server's, not the database's. The migrations' owner (`vyreon`, the
-- database's owner under CloudNativePG) cannot create roles: the cluster's bootstrap creates it
-- (10-postgres.yaml `postInitSQL`), and on a cluster bootstrapped before this migration a superuser
-- creates it once before the core image carrying it starts (deploy/k8s/README.md, "Workers from the
-- queue"). A missing role the migration cannot create stops the migration with that instruction.
DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'vyreon_scaler') THEN
        BEGIN
            CREATE ROLE vyreon_scaler NOLOGIN;
        EXCEPTION
            WHEN duplicate_object OR unique_violation THEN NULL;
            WHEN insufficient_privilege THEN
                RAISE EXCEPTION 'role vyreon_scaler is missing and % cannot create it: as a superuser run CREATE ROLE vyreon_scaler NOLOGIN, then start core again (deploy/k8s/README.md, "Workers from the queue")', current_user;
        END;
    END IF;
END
$$;

CREATE FUNCTION core.claimable_count(p_kind text, p_build text, p_daily boolean DEFAULT NULL) RETURNS bigint
    LANGUAGE sql STABLE SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
    SELECT count(*) FROM core.claimable_job j
    WHERE j.kind = p_kind AND j.engine_build = p_build AND j.next_session IS NOT NULL
      AND (p_daily IS NULL OR (j.priority = 1) = p_daily)
$$;
REVOKE ALL ON FUNCTION core.claimable_count(text, text, boolean) FROM PUBLIC;
GRANT USAGE ON SCHEMA core TO vyreon_scaler;
GRANT EXECUTE ON FUNCTION core.claimable_count(text, text, boolean) TO vyreon_scaler;
