-- C-054 amended 2026-11-20 (audit round cc, 5): a report is bound to the revisions the worker
-- checked, as its account states them (`revisions`: the base, then the day-local revision), never
-- to whatever base is published when it arrives. `base_revision` keeps its meaning (the base
-- checked); `day_local_revision` is the day-local revision checked (NULL: not stated, or none).
-- A report is current while both are still published.
ALTER TABLE core.refcheck_report
    ADD COLUMN day_local_revision bigint REFERENCES vyreon.day_revision ON DELETE SET NULL;
