-- C-096 (owner, 2026-11-26, for the phase-1 run: "recording ALL faults"): what the run leaves behind for the
-- catalog. (1) A day's own logs are kept in the archive (category `run-logs`): until now only a failed attempt
-- left evidence, and the 2026-11-25 nightly's idle hour and its ACT01 refusals were read from a pod that was
-- still alive. (2) A worker's steps are kept as a history: `core.lease_step` holds only the current one.
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '60s';

ALTER TABLE core.perishable_archive DROP CONSTRAINT perishable_archive_category_check;
ALTER TABLE core.perishable_archive ADD CONSTRAINT perishable_archive_category_check
    CHECK (category IN ('exq01-events', 'act01-auto', 'run-logs'));

CREATE TABLE core.lease_step_log (
    lease_step_log_id bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    lease_id          bigint      NOT NULL REFERENCES core.lease ON DELETE CASCADE,
    step              text        NOT NULL,
    waiting_until     timestamptz,
    reason            text,
    since             timestamptz NOT NULL
);
CREATE INDEX lease_step_log_by_lease ON core.lease_step_log (lease_id, since);

-- One row each time a lease's step, or what it waits for, changes (its progress words change often and are not
-- a step). A step lasted until the next row's `since`, the last until the lease ended.
CREATE FUNCTION core.lease_step_logged() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' OR (OLD.step, OLD.waiting_until, OLD.reason) IS DISTINCT FROM (NEW.step, NEW.waiting_until, NEW.reason) THEN
        INSERT INTO core.lease_step_log (lease_id, step, waiting_until, reason, since)
        VALUES (NEW.lease_id, NEW.step, NEW.waiting_until, NEW.reason, NEW.reported_at);
    END IF;
    RETURN NULL;
END $$;
CREATE TRIGGER lease_step_logged AFTER INSERT OR UPDATE ON core.lease_step
    FOR EACH ROW EXECUTE FUNCTION core.lease_step_logged();
