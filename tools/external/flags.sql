-- Flag agreement (README "Flag agreement"): VS-IVS01 iv30 of the 14 Cboe pairs with state and reason,
-- and beside it the engine's own unusualness of the same reading (DLM-UNUSUALNESS
-- `DLM-IV30/iv30/robust_z_63`, `percentile_63`, `line_95`), from the published revisions.
-- `from` must reach 63 sessions before the first session judged (about 95 calendar days).
-- psql -X -A -F $'\t' -d vyreon -v from=2024-07-09 -v to=2026-11-20 -f flags.sql > flags.tsv
WITH pairs(object_id) AS (
    VALUES ('SPY'), ('QQQ'), ('IWM'), ('GLD'), ('USO'), ('AAPL'), ('AMZN'), ('GOOGL'), ('EEM'),
           ('EWZ'), ('TLT'), ('DIA'), ('SLV'), ('FXI')
),
iv AS (
    SELECT m.represented_date AS date, m.object_id,
           r->>'state' AS state,
           CASE WHEN r->>'state' = 'SUPPORTED'
                THEN COALESCE(r->'value'->>'data', r->'value'->>'Number', r->>'value') END AS iv30,
           COALESCE(r->>'reason', '') AS reason
    FROM vyreon.measurement m
    JOIN vyreon.day_revision d USING (revision_id, represented_date)
    CROSS JOIN LATERAL jsonb_array_elements(m.body->'measurement'->'value'->'roles') r
    WHERE d.state = 'PUBLISHED'
      AND m.measurement_id = 'VS-IVS01'
      AND m.object_id IN (SELECT object_id FROM pairs)
      AND m.represented_date BETWEEN :'from' AND :'to'
      AND r->>'role_id' = 'iv30'
),
unusual AS (
    SELECT m.represented_date AS date, m.object_id,
           max(CASE WHEN r->>'role_id' = 'DLM-IV30/iv30/robust_z_63' AND r->>'state' = 'SUPPORTED'
                    THEN COALESCE(r->'value'->>'data', r->'value'->>'Number', r->>'value') END) AS z,
           max(CASE WHEN r->>'role_id' = 'DLM-IV30/iv30/robust_z_63' THEN r->>'state' END) AS z_state,
           max(CASE WHEN r->>'role_id' = 'DLM-IV30/iv30/robust_z_63' THEN r->>'reason' END) AS z_reason,
           max(CASE WHEN r->>'role_id' = 'DLM-IV30/iv30/percentile_63' AND r->>'state' = 'SUPPORTED'
                    THEN COALESCE(r->'value'->>'data', r->'value'->>'Number', r->>'value') END) AS percentile,
           max(CASE WHEN r->>'role_id' = 'DLM-IV30/iv30/line_95' AND r->>'state' = 'SUPPORTED'
                    THEN COALESCE(r->'value'->>'data', r->'value'->>'Number', r->>'value') END) AS line
    FROM vyreon.measurement m
    JOIN vyreon.day_revision d USING (revision_id, represented_date)
    CROSS JOIN LATERAL jsonb_array_elements(m.body->'measurement'->'value'->'roles') r
    WHERE d.state = 'PUBLISHED'
      AND m.measurement_id = 'DLM-UNUSUALNESS'
      AND m.object_id IN (SELECT object_id FROM pairs)
      AND m.represented_date BETWEEN :'from' AND :'to'
      AND r->>'role_id' LIKE 'DLM-IV30/iv30/%'
    GROUP BY 1, 2
)
SELECT iv.date, iv.object_id, iv.state, iv.iv30, iv.reason,
       COALESCE(u.z_state, '') AS z_state, COALESCE(u.z_reason, '') AS z_reason,
       u.z, u.percentile, u.line
FROM iv LEFT JOIN unusual u USING (date, object_id)
ORDER BY 1, 2;
