-- C-068 (C-065 amended): the operational failure-surface audit of 2026-11-22 (opsurface/ops.md), 8.
-- The stall's clock per kind of work (a daily session's day blocks apart), and one for the chain (nothing claimable, nothing leased, work
-- still queued): one row per kind at most; the watch writes them in its check.
DROP TABLE core.stall_seen;
CREATE TABLE core.stall_seen (
    kind  text        PRIMARY KEY CHECK (kind IN ('TAPE', 'DAY_BLOCK', 'DAILY_BLOCK', 'ORDERED_PASS', 'CHAIN')),
    since timestamptz NOT NULL
);
