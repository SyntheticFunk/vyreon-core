-- Audit 2026-11-22 prebuild round 2 (core-watch.md) 11: the progress channel attributed a published
-- day to whichever block's lease overlapped its date and time, so a day-local revision published
-- by hand inside a running block's range was credited to that block's worker and shortened the
-- minutes of its next day. A revision published under a lease now records that lease, in the same
-- transaction; one published by hand has no row. Revisions published before this have none.
CREATE TABLE core.lease_revision (
    revision_id bigint PRIMARY KEY REFERENCES vyreon.day_revision ON DELETE CASCADE,
    lease_id    bigint NOT NULL REFERENCES core.lease
);
CREATE INDEX lease_revision_by_lease ON core.lease_revision (lease_id);
