-- VS-IVS01 iv30 for the Cboe pairs (the five and the nine more) from the published base revisions, with state and reason.
-- psql -X -A -P footer=off -F $'\t' -d vyreon -v from=2026-11-02 -v to=2026-11-13 -f values.sql
SELECT m.represented_date AS date, m.object_id,
       r->>'state' AS state,
       CASE WHEN r->>'state' = 'SUPPORTED' THEN r->>'value' END AS iv30,
       COALESCE(r->>'reason', '') AS reason
FROM vyreon.measurement m
JOIN vyreon.day_revision d USING (revision_id, represented_date)
CROSS JOIN LATERAL jsonb_array_elements(m.body->'measurement'->'value'->'roles') r
WHERE d.state = 'PUBLISHED'
  AND m.measurement_id = 'VS-IVS01'
  AND m.object_id IN ('SPY', 'QQQ', 'IWM', 'GLD', 'USO', 'AAPL', 'AMZN', 'GOOGL', 'EEM', 'EWZ',
                      'TLT', 'DIA', 'SLV', 'FXI')
  AND m.represented_date BETWEEN :'from' AND :'to'
  AND r->>'role_id' = 'iv30'
ORDER BY 1, 2;
