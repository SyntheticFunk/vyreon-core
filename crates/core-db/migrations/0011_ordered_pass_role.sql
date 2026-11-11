-- C-020: the ordered pass reads history straight from the database with a read-only role. It reads
-- the store's tables (every published revision of a window, not only the reader views), so it has
-- a group role of its own; the deployment creates the login and puts it in this group.
DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'vyreon_ordered_pass') THEN
        CREATE ROLE vyreon_ordered_pass NOLOGIN;
    END IF;
END
$$;
GRANT USAGE ON SCHEMA vyreon TO vyreon_ordered_pass;
GRANT SELECT ON vyreon.day_revision, vyreon.measurement, vyreon.measurement_stream, vyreon.reader_scalar, vyreon.unusual_score,
                vyreon.published_measurement, vyreon.accepted_measurement, vyreon.accepted_stream, vyreon.accepted_reader_scalar
    TO vyreon_ordered_pass;
