-- C-073 hotfix (live, 2026-11-24, release a6bb572 / worker 1ae946f7): NO_PROGRESS fired on every
-- block's download step: Intrinio's bulk option files are no request in core's ledger, and a block's
-- first download runs about three hours. A download step is now an implicit wait (`watch::
-- DOWNLOAD_STEP_MAX_MINUTES`), and a step report may carry a `progress` text: any report whose step,
-- wait, reason or progress differs from the last is progress (`step_since`).
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '60s';
ALTER TABLE core.lease_step ADD COLUMN progress text CHECK (length(progress) BETWEEN 1 AND 200);
