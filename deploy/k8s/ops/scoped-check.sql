-- C-074 / engine D-462: the first scoped recompute's check (reads only). Run with
--   kubectl -n vyreon exec -i vyreon-pg-1 -c postgres -- psql -d vyreon -v ON_ERROR_STOP=1 \
--     -v scoped_base=<id> -v scoped_local=<id> [-v full_base=<id> -v full_local=<id>] < scoped-check.sql
-- scoped_*: the day's base and day-local revisions after the REPUBLISH job (a day-local-only scope keeps the
-- base: give that id); full_*: the revisions a full recompute of the same day published afterwards (part 3).
-- Every part lists what is wrong; an empty list passes. Release-2 audit (Codex 12, Gemini 11): every
-- comparison is driven by both revisions, so a slot, stream or scalar missing on either side is a row.

\echo '1. the copy-forward revisions: their scope and what they replaced'
SELECT revision_id, state, day_local, republish_scope->>'job' AS job, republish_scope->>'replaces' AS replaces,
       republish_scope->'recomputed' AS recomputed, republish_scope->'value_changed' AS value_changed, snapshot_cutoff
FROM vyreon.day_revision WHERE revision_id IN (:scoped_base, :scoped_local);

\echo '2. the carried slots against the revision each replaced, both ways: no row may print'
WITH pairs AS (SELECT revision_id AS new, (republish_scope->>'replaces')::bigint AS old,
                      ARRAY(SELECT jsonb_array_elements_text(republish_scope->'recomputed')) AS recomputed
               FROM vyreon.day_revision WHERE revision_id IN (:scoped_base, :scoped_local) AND republish_scope IS NOT NULL),
     n AS (SELECT p.new, m.* FROM pairs p JOIN vyreon.measurement m ON m.revision_id = p.new AND NOT m.measurement_id = ANY(p.recomputed)),
     o AS (SELECT p.new, m.* FROM pairs p JOIN vyreon.measurement m ON m.revision_id = p.old AND NOT m.measurement_id = ANY(p.recomputed))
SELECT COALESCE(n.new, o.new) AS revision, COALESCE(n.measurement_id, o.measurement_id) AS measurement, COALESCE(n.object_id, o.object_id) AS object,
       CASE WHEN n.revision_id IS NULL THEN 'missing in the copy' WHEN o.revision_id IS NULL THEN 'not in the revision replaced'
            WHEN n.carried_from_revision IS NULL THEN 'not marked carried' ELSE 'differs' END AS what
FROM n FULL JOIN o ON o.new = n.new AND (o.measurement_id, o.object_id) = (n.measurement_id, n.object_id)
WHERE n.revision_id IS NULL OR o.revision_id IS NULL OR n.carried_from_revision IS NULL
   OR n.carried_from_revision <> COALESCE(o.carried_from_revision, o.revision_id)
   OR n.body_text IS DISTINCT FROM o.body_text OR n.receipt IS DISTINCT FROM o.receipt OR n.coordinates IS DISTINCT FROM o.coordinates
   OR (n.science_version, n.method_version, n.slot_state, n.body_sha256) IS DISTINCT FROM (o.science_version, o.method_version, o.slot_state, o.body_sha256);

\echo '   their event streams, both ways: no row may print'
WITH pairs AS (SELECT revision_id AS new, (republish_scope->>'replaces')::bigint AS old,
                      ARRAY(SELECT jsonb_array_elements_text(republish_scope->'recomputed')) AS recomputed
               FROM vyreon.day_revision WHERE revision_id IN (:scoped_base, :scoped_local) AND republish_scope IS NOT NULL),
     n AS (SELECT p.new, s.* FROM pairs p JOIN vyreon.measurement_stream s ON s.revision_id = p.new AND NOT s.measurement_id = ANY(p.recomputed)),
     o AS (SELECT p.new, s.* FROM pairs p JOIN vyreon.measurement_stream s ON s.revision_id = p.old AND NOT s.measurement_id = ANY(p.recomputed))
SELECT COALESCE(n.new, o.new) AS revision, COALESCE(n.measurement_id, o.measurement_id) AS measurement, COALESCE(n.object_id, o.object_id) AS object
FROM n FULL JOIN o ON o.new = n.new AND (o.measurement_id, o.object_id) = (n.measurement_id, n.object_id)
WHERE n.revision_id IS NULL OR o.revision_id IS NULL
   OR (n.root_sha256, n.events_sha256, n.event_count, n.byte_length) IS DISTINCT FROM (o.root_sha256, o.events_sha256, o.event_count, o.byte_length)
   OR md5(n.events_gzip) <> md5(o.events_gzip) OR n.root_bytes <> o.root_bytes;

\echo '   their reader scalars and unusual scores, both ways: no row may print'
WITH pairs AS (SELECT revision_id AS new, (republish_scope->>'replaces')::bigint AS old,
                      ARRAY(SELECT jsonb_array_elements_text(republish_scope->'recomputed')) AS recomputed
               FROM vyreon.day_revision WHERE revision_id IN (:scoped_base, :scoped_local) AND republish_scope IS NOT NULL),
     v AS (SELECT 'scalar' AS kind, revision_id, measurement_id, object_id, role_id, value FROM vyreon.reader_scalar
           UNION ALL SELECT 'unusual', revision_id, measurement_id, object_id, role_id, z FROM vyreon.unusual_score),
     n AS (SELECT p.new, v.* FROM pairs p JOIN v ON v.revision_id = p.new AND NOT v.measurement_id = ANY(p.recomputed)),
     o AS (SELECT p.new, v.* FROM pairs p JOIN v ON v.revision_id = p.old AND NOT v.measurement_id = ANY(p.recomputed))
SELECT COALESCE(n.new, o.new) AS revision, COALESCE(n.kind, o.kind) AS kind, COALESCE(n.measurement_id, o.measurement_id) AS measurement,
       COALESCE(n.object_id, o.object_id) AS object, COALESCE(n.role_id, o.role_id) AS role, n.value AS copied, o.value AS replaced
FROM n FULL JOIN o ON o.new = n.new AND (o.kind, o.measurement_id, o.object_id, o.role_id) = (n.kind, n.measurement_id, n.object_id, n.role_id)
WHERE n.value IS DISTINCT FROM o.value;

\if :{?full_base}
\echo '3. the recomputed slots against the full recompute of the same day, both ways: each slot missing on a side or whose digest differs, with the body keys that differ'
WITH s AS (SELECT m.* FROM vyreon.measurement m WHERE m.revision_id IN (:scoped_base, :scoped_local) AND m.carried_from_revision IS NULL),
     f AS (SELECT m.* FROM vyreon.measurement m WHERE m.revision_id IN (:full_base, :full_local)
             AND m.measurement_id IN (SELECT measurement_id FROM s))
SELECT COALESCE(s.measurement_id, f.measurement_id) AS measurement, COALESCE(s.object_id, f.object_id) AS object,
       s.body_sha256 AS scoped, f.body_sha256 AS full,
       (SELECT array_agg(k ORDER BY k) FROM (SELECT jsonb_object_keys(s.body) UNION SELECT jsonb_object_keys(f.body)) x(k)
        WHERE s.body->k IS DISTINCT FROM f.body->k) AS differing_top_keys
FROM s FULL JOIN f ON (f.measurement_id, f.object_id) = (s.measurement_id, s.object_id)
WHERE s.revision_id IS NULL OR f.revision_id IS NULL OR s.body_sha256 <> f.body_sha256
ORDER BY 1, 2;
\endif
