-- C-070's checks before deploying a core image that carries migration 0053 (DECISIONS C-070,
-- "Rollout"). Reads only; prints revision ids, dates and names, no measurement. Run:
--   kubectl -n vyreon exec -i vyreon-pg-1 -c postgres -- psql -d vyreon -v ON_ERROR_STOP=1 < deploy/k8s/ops/precheck-c070.sql
--
-- 1. Days with two PUBLISHED revisions of one layer. Migration 0053 stops on any (and core then does
--    not start), so this must print no row before the deploy. A row: retire the day
--    (`vyreon-core days retire --preview`, then `--expect <token>`) and compute it again first.
SELECT 'two published of one layer' AS check, represented_date, profile_id,
       CASE WHEN base_revision IS NULL THEN 'base' WHEN day_local THEN 'day-local' ELSE 'history' END AS layer,
       array_agg(revision_id ORDER BY revision_id) AS revisions,
       array_agg(profile_revision ORDER BY revision_id) AS profile_revisions,
       array_agg(engine_build ORDER BY revision_id) AS builds
  FROM vyreon.day_revision
 WHERE state = 'PUBLISHED'
 GROUP BY 2, 3, 4
HAVING count(*) > 1
 ORDER BY 2, 3, 4;

-- 2. PUBLISHED bases with no PUBLISHED day-local revision on them. After the deploy a day block's
--    day is done only with both. Expected: only days a leased block is on now (its worker between
--    its two uploads: `block_on_it` true). A row with `block_on_it` false is a day already done on
--    its base alone (the latent gap C-070 closes): later passes read a bare base there; retire it
--    and compute it again.
SELECT 'base without day-local' AS check, b.represented_date, b.profile_id, b.revision_id, b.engine_build,
       EXISTS (SELECT FROM core.job j
                WHERE j.kind = 'DAY_BLOCK' AND j.state IN ('LEASED','QUEUED') AND j.profile_id = b.profile_id
                  AND j.next_session = b.represented_date) AS block_on_it
  FROM vyreon.day_revision b
 WHERE b.state = 'PUBLISHED' AND b.base_revision IS NULL
   AND NOT EXISTS (SELECT FROM vyreon.day_revision l
                    WHERE l.base_revision = b.revision_id AND l.represented_date = b.represented_date
                      AND l.state = 'PUBLISHED' AND l.day_local)
 ORDER BY 2;
