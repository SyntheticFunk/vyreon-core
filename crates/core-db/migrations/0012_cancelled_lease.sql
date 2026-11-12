-- A lease can end because its job was cancelled (`vyreon-core queue cancel`).
ALTER TABLE core.lease DROP CONSTRAINT lease_outcome_check;
ALTER TABLE core.lease ADD CONSTRAINT lease_outcome_check CHECK (outcome IN ('DONE', 'FAILED', 'EXPIRED', 'CANCELLED'));
