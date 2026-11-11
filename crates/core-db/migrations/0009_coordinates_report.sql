-- Bundle contract 2 (C-030). A slot's coordinates view is the engine's projection of its own body
-- (each coordinate's value, state, reason, unit and quality): core cannot compute it, so it is
-- stored as published. NULL for a slot the engine does not project. A revision's report is what
-- its run measured of itself (stage timings, request counts).
ALTER TABLE vyreon.measurement ADD COLUMN coordinates jsonb;
ALTER TABLE vyreon.day_revision ADD COLUMN report jsonb;
