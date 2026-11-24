-- C-074 / engine D-462: the first scoped recompute's check (reads only). Run with
--   kubectl -n vyreon exec -i vyreon-pg-1 -c postgres -- psql -d vyreon -v ON_ERROR_STOP=1 \
--     -v day="'2026-11-20'" -v scoped_base=<id> -v scoped_local=<id> [-v full_base=<id> -v full_local=<id>] < scoped-check.sql
-- scoped_*: the copy-forward revisions the REPUBLISH job published (their republish_scope names the job);
-- full_*: the revisions a full recompute of the same day published afterwards (part 3; skip it before).

\echo '1. the copy-forward revisions: their scope, what they replaced, the carried slots'' cutoff'
SELECT revision_id, state, day_local, republish_scope->>'job' AS job, republish_scope->>'replaces' AS replaces,
       republish_scope->'recomputed' AS recomputed, republish_scope->'value_changed' AS value_changed,
       republish_scope->>'carried_cutoff' AS carried_cutoff, snapshot_cutoff
FROM vyreon.day_revision WHERE revision_id IN (:scoped_base, :scoped_local);

\echo '2. every carried slot against the revision it replaced: no row may differ (byte for byte)'
WITH pairs AS (SELECT revision_id AS new, (republish_scope->>'replaces')::bigint AS old FROM vyreon.day_revision
               WHERE revision_id IN (:scoped_base, :scoped_local) AND republish_scope IS NOT NULL)
SELECT n.revision_id, n.measurement_id, n.object_id,
       n.body_text IS DISTINCT FROM o.body_text AS body, n.receipt IS DISTINCT FROM o.receipt AS receipt,
       n.coordinates IS DISTINCT FROM o.coordinates AS coordinates,
       (n.science_version, n.method_version, n.slot_state) IS DISTINCT FROM (o.science_version, o.method_version, o.slot_state) AS versions
FROM pairs p JOIN vyreon.measurement n ON n.revision_id = p.new AND n.carried_from_revision IS NOT NULL
LEFT JOIN vyreon.measurement o ON o.revision_id = p.old AND (o.measurement_id, o.object_id) = (n.measurement_id, n.object_id)
WHERE o.revision_id IS NULL OR n.body_text IS DISTINCT FROM o.body_text OR n.receipt IS DISTINCT FROM o.receipt
   OR n.coordinates IS DISTINCT FROM o.coordinates
   OR (n.science_version, n.method_version, n.slot_state) IS DISTINCT FROM (o.science_version, o.method_version, o.slot_state);
\echo '   streams and reader scalars of the carried slots (two counts each; equal)'
WITH pairs AS (SELECT revision_id AS new, (republish_scope->>'replaces')::bigint AS old FROM vyreon.day_revision
               WHERE revision_id IN (:scoped_base, :scoped_local) AND republish_scope IS NOT NULL)
SELECT p.new,
       (SELECT count(*) FROM vyreon.measurement_stream s JOIN vyreon.measurement m USING (revision_id, measurement_id, object_id, represented_date)
        WHERE s.revision_id = p.new AND m.carried_from_revision IS NOT NULL) AS carried_streams,
       (SELECT count(*) FROM vyreon.measurement_stream s JOIN vyreon.measurement_stream o
          ON o.revision_id = p.old AND (o.measurement_id, o.object_id) = (s.measurement_id, s.object_id)
             AND o.events_sha256 = s.events_sha256 AND o.root_sha256 = s.root_sha256
        WHERE s.revision_id = p.new) - (SELECT count(*) FROM vyreon.measurement_stream s JOIN vyreon.measurement m USING (revision_id, measurement_id, object_id, represented_date)
        WHERE s.revision_id = p.new AND m.carried_from_revision IS NULL) AS equal_to_replaced,
       (SELECT count(*) FROM vyreon.reader_scalar r JOIN vyreon.measurement m USING (revision_id, measurement_id, object_id, represented_date)
        WHERE r.revision_id = p.new AND m.carried_from_revision IS NOT NULL) AS carried_scalars,
       (SELECT count(*) FROM vyreon.reader_scalar r JOIN vyreon.reader_scalar o
          ON o.revision_id = p.old AND (o.measurement_id, o.object_id, o.role_id) = (r.measurement_id, r.object_id, r.role_id) AND o.value = r.value
        JOIN vyreon.measurement m ON (m.revision_id, m.measurement_id, m.object_id, m.represented_date) = (r.revision_id, r.measurement_id, r.object_id, r.represented_date)
        WHERE r.revision_id = p.new AND m.carried_from_revision IS NOT NULL) AS equal_to_replaced_scalars
FROM pairs p;

\if :{?full_base}
\echo '3. the recomputed slots against the full recompute of the same day: every body digest equal, or listed here'
SELECT s.measurement_id, s.object_id, s.body_sha256 AS scoped, f.body_sha256 AS full,
       (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(s.body) k WHERE s.body->k IS DISTINCT FROM f.body->k) AS differing_top_keys
FROM vyreon.measurement s
JOIN vyreon.measurement f ON f.revision_id IN (:full_base, :full_local) AND (f.measurement_id, f.object_id) = (s.measurement_id, s.object_id)
WHERE s.revision_id IN (:scoped_base, :scoped_local) AND s.carried_from_revision IS NULL AND s.body_sha256 <> f.body_sha256
ORDER BY 1, 2;
\echo '   and the carried slots against the full recompute (they are what the full day computes again only where its inputs are those of the day)'
SELECT count(*) FILTER (WHERE s.body_sha256 = f.body_sha256) AS equal, count(*) FILTER (WHERE s.body_sha256 <> f.body_sha256) AS differ
FROM vyreon.measurement s
JOIN vyreon.measurement f ON f.revision_id IN (:full_base, :full_local) AND (f.measurement_id, f.object_id) = (s.measurement_id, s.object_id)
WHERE s.revision_id IN (:scoped_base, :scoped_local) AND s.carried_from_revision IS NOT NULL;
\endif
