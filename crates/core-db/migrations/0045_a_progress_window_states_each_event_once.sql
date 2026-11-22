-- Audit 2026-11-22 prebuild round 2 (core-watch.md) 9: an event is stamped inside the transaction
-- that writes it (a pause notice with the transaction's start), so it can commit after the window
-- covering its time was composed, and the next window left it out by its time: lost to the
-- progress channel. Each window now records what it stated; the next takes every event not yet
-- stated from `progress::STALE_HOURS` before its start. Kept with their window (`KEEP_DAYS`).
CREATE TABLE core.progress_shown (
    window_end timestamptz NOT NULL REFERENCES core.progress_window ON DELETE CASCADE,
    source     text        NOT NULL CHECK (source IN ('revision', 'refcheck', 'lease', 'notice')),
    id         bigint      NOT NULL,
    PRIMARY KEY (source, id)
);
CREATE INDEX progress_shown_by_window ON core.progress_shown (window_end);
