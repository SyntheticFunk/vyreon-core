-- C-031: a based revision that reads only its own day (the engine's day-local derived profile).
-- A base republished on an earlier day supersedes the later days' based revisions because they
-- read history; one that reads none stays.
ALTER TABLE vyreon.day_revision ADD COLUMN day_local boolean NOT NULL DEFAULT false;
