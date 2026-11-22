-- Audit 2026-11-22 prebuild round 2 (core-r2.md) 4 and 10: C-063 #10 queued a day block again, its
-- attempt not counted, when the attempt's evidence named a day before the block's next session. Core
-- took the progress on trust: a worker naming a stale day on each new lease was refunded without end,
-- and the supplied worker names the claim's first day for any stop without a date of its own (a disk
-- guard before the next day began, say), refunding a failure of the day it was on. A lease now records
-- the session its attempt began at (the job's next session when the lease was granted); an attempt is
-- refunded only when the day its stop message names (`stop: <date> ...`, `stop: prune after <date>:`)
-- is one this very lease did, so each refund moved the block on by a day of its own (`work::requeue`).
-- Leases granted before this have none: their attempts are counted, as before C-063.
ALTER TABLE core.lease ADD COLUMN start_session date;

-- Set by the database when a lease is granted, from its job's next session (a claim moves the job to
-- LEASED without changing it), so every path that grants a lease records it.
CREATE FUNCTION core.lease_start_session() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    NEW.start_session := (SELECT next_session FROM core.job WHERE job_id = NEW.job_id);
    RETURN NEW;
END
$$;
CREATE TRIGGER lease_start_session BEFORE INSERT ON core.lease FOR EACH ROW EXECUTE FUNCTION core.lease_start_session();
