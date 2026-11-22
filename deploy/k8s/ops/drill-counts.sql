-- The restore drill's comparison (README "Backups and the volume"): run on the source and on the
-- restored cluster and diff the outputs; identical output passes. Per month of the represented
-- date: day revisions by state, and measurement rows with an order-free digest of their body
-- hashes (XOR of each body_sha256's first 64 bits). Reads only; prints no measurement.
SELECT 'day_revision' AS t, to_char(represented_date, 'YYYY-MM') AS month, state AS what,
       count(*)::text AS n
  FROM vyreon.day_revision GROUP BY 2, 3
UNION ALL
SELECT 'measurement', to_char(represented_date, 'YYYY-MM'), 'rows xor64',
       count(*)::text || ' ' || coalesce(bit_xor(('x' || left(body_sha256, 16))::bit(64)::bigint), 0)::text
  FROM vyreon.measurement GROUP BY 2
ORDER BY 1, 2, 3;
