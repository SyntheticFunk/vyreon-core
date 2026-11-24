-- C-072: the progress channel states a hand-over once, in one line (its builds, what it moved, the
-- running jobs it restarted, its reason, and while its successor is not admitted the instruction
-- to admit it). (Part 2 of C-072, planned steps as progress, was retired before any deploy; this
-- migration was rewritten before any deploy: it holds only what parts 1 and 3 need.)
--
-- A lock wait fails this migration in 10 s instead of holding the core pod in `migrate` past the
-- workers' 10-minute leases; the runner rolls it back whole with its ledger row, `vyreon-core
-- migrate` exits non-zero and Kubernetes retries the init container with its back-off.
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '60s';
ALTER TABLE core.progress_shown DROP CONSTRAINT progress_shown_source_check;
ALTER TABLE core.progress_shown ADD CONSTRAINT progress_shown_source_check CHECK (source IN ('revision', 'refcheck', 'lease', 'notice', 'hand_over'));
