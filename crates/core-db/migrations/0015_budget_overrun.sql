-- C-025, amended 2026-11-15: a worker that reports more requests than its budget lease granted is
-- counted at what it reported, so the requests of a rolling minute stay accounted for. The column
-- held at most the grant (0006), so an overrun was counted at the grant and the excess forgotten.
ALTER TABLE core.budget_lease DROP CONSTRAINT budget_lease_check;
ALTER TABLE core.budget_lease ADD CONSTRAINT budget_lease_used_check CHECK (used >= 0);
