-- FIELD bodies for the Tiingo recompute from the published base revisions. FIELD-DEP01 is slimmed:
-- its coordinates, its masks' session axis and, per pair mask, the 0-based rows that are not true.
-- psql -X -A -t -F $'\t' -d vyreon -v from=2026-11-02 -v to=2026-11-13 -f tiingo-field.sql
SELECT m.represented_date, m.measurement_id, d.engine_build,
       CASE WHEN m.measurement_id = 'FIELD-DEP01' THEN jsonb_build_object(
         'owner', m.body->'owner', 'represented_time', m.body->'represented_time',
         'coordinates', m.body->'coordinates',
         'mask_axis', (SELECT a->'axes'->0->'labels' FROM jsonb_array_elements(m.body->'arrays') a
                       WHERE a->>'role' LIKE 'pair.%.common_mask' LIMIT 1),
         'mask_count', (SELECT count(*) FROM jsonb_array_elements(m.body->'arrays') a
                        WHERE a->>'role' LIKE 'pair.%.common_mask'),
         'mask_not_true', (SELECT COALESCE(jsonb_object_agg(a->>'role', x.rows), '{}'::jsonb)
                           FROM jsonb_array_elements(m.body->'arrays') a
                           CROSS JOIN LATERAL (SELECT jsonb_agg(c.i - 1) AS rows
                                               FROM jsonb_array_elements(a->'cells') WITH ORDINALITY c(cell, i)
                                               WHERE c.cell->>'value' IS DISTINCT FROM 'true') x
                           WHERE a->>'role' LIKE 'pair.%.common_mask' AND x.rows IS NOT NULL)
       )::text ELSE m.body::text END
FROM vyreon.measurement m
JOIN vyreon.day_revision d USING (revision_id, represented_date)
WHERE d.state = 'PUBLISHED'
  AND m.measurement_id IN ('FIELD-BR01', 'FIELD-DIST01', 'FIELD-XCONC01', 'FIELD-SYNC01', 'FIELD-DEP01',
                           'FIELD-GRP01', 'FIELD-RMT01', 'FIELD-DIM01', 'FIELD-SUB01')
  AND m.represented_date BETWEEN :'from' AND :'to'
ORDER BY 1, 2;
