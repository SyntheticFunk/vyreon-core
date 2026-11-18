-- The CDD-EVENTS01 bodies `edgar.py events` compares, from the published revisions.
-- psql -X -A -t -F $'\t' -d vyreon -v from=2026-11-02 -v to=2026-11-13 -f events.sql
SELECT m.represented_date, m.measurement_id, m.object_id, m.body::text
FROM vyreon.measurement m
JOIN vyreon.day_revision d USING (revision_id, represented_date)
WHERE d.state = 'PUBLISHED'
  AND m.measurement_id = 'CDD-EVENTS01'
  AND m.represented_date BETWEEN :'from' AND :'to'
ORDER BY 1, 2, 3;
