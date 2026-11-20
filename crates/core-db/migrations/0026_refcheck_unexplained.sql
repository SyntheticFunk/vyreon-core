-- C-056 (engine D-370 amended 2026-11-20): the worker's account carries, beside `failed`, the
-- failures its rulings table does not explain (`failed_unexplained`: unruled, or ruled for an engine
-- fix). A failure the rulings table matches to a convention the owner ruled as the definition is
-- still stored in the report, but it is not a disagreement to look at. NULL: a report written
-- before the field existed (every failure counts).
ALTER TABLE core.refcheck_report
    ADD COLUMN failed_unexplained integer CHECK (failed_unexplained >= 0 AND failed_unexplained <= failed);
