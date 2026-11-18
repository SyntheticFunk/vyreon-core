-- VS-O06 reported volume and put share from the published base revisions, with state and reason.
-- psql -X -A -F $'\t' -d vyreon -v from=2026-11-02 -v to=2026-11-13 -f occ.sql
SELECT m.represented_date AS date, m.object_id,
       v->>'state' AS volume_state,
       CASE WHEN v->>'state' = 'SUPPORTED' THEN v->'value'->>'data' END AS volume,
       COALESCE(v->>'reason', '') AS volume_reason,
       p->>'state' AS put_share_state,
       CASE WHEN p->>'state' = 'SUPPORTED' THEN p->'value'->>'data' END AS put_share,
       COALESCE(p->>'reason', '') AS put_share_reason,
       d.engine_build
FROM vyreon.measurement m
JOIN vyreon.day_revision d USING (revision_id, represented_date)
CROSS JOIN LATERAL jsonb_path_query_first(m.body,
  '$.measurement.value.roles[*] ? (@.role_id == "reported_subset.reported_oi_total")') v
CROSS JOIN LATERAL jsonb_path_query_first(m.body,
  '$.measurement.value.roles[*] ? (@.role_id == "reported_subset.put_share")') p
WHERE d.state = 'PUBLISHED'
  AND m.measurement_id = 'VS-O06'
  AND m.represented_date BETWEEN :'from' AND :'to'
ORDER BY 1, 2;
