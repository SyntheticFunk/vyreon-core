-- Price-based base and derived bodies for the Tiingo recompute from the published base and derived
-- revisions, with each derived receipt's bar-history start (`daily_bars_from`).
-- psql -X -A -t -F $'\t' -d vyreon -v from=2026-11-02 -v to=2026-11-13 -f tiingo-prices.sql
SELECT m.represented_date, m.object_id, m.measurement_id, d.engine_build,
       COALESCE(m.receipt->'original_calculation'->'record'->'inputs'->>'daily_bars_from', ''),
       (m.body->'measurement'->'value'->'roles')::text
FROM vyreon.measurement m
JOIN vyreon.day_revision d USING (revision_id, represented_date)
WHERE d.state = 'PUBLISHED'
  AND m.measurement_id IN ('VS-P01', 'VS-P03', 'PATH-L01', 'PATH-L02', 'DLM-TECHNICAL', 'DLM-PRICE-LEVELS',
                           'DLM-DRAWDOWN', 'DLM-RELATIVE-PERFORMANCE', 'DLM-BETA')
  AND m.represented_date BETWEEN :'from' AND :'to'
ORDER BY 1, 2, 3;
