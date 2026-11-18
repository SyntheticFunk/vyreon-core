-- CDD-MACRO01 bodies from the published base revisions (one per security; each is compared).
-- psql -X -A -t -F $'\t' -d vyreon -v from=2026-11-02 -v to=2026-11-13 -f fred.sql
SELECT m.represented_date, m.object_id, d.engine_build, m.body::text
FROM vyreon.measurement m
JOIN vyreon.day_revision d USING (revision_id, represented_date)
WHERE d.state = 'PUBLISHED'
  AND m.measurement_id = 'CDD-MACRO01'
  AND m.represented_date BETWEEN :'from' AND :'to'
ORDER BY 1, 2;
