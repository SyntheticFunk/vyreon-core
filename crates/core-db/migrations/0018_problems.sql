-- C-050: the owner's channel carries problems, not events. A notice is now either posted (an
-- alert when a problem opens, one "resolved" when it clears, the 09:00 digest, the Saturday
-- check) or only logged (what used to be posted for every block done, build admitted, queue
-- resumed...). Every notice written before this was of the old, chatty kind: they stay as the log
-- (the column's default) and are never posted.
ALTER TABLE core.notice ADD COLUMN post boolean NOT NULL DEFAULT false;
DROP INDEX core.notice_unsent;
CREATE INDEX notice_to_post ON core.notice (notice_id) WHERE post AND sent_at IS NULL;

-- One row per problem, from the check that opened it to the check that saw it clear. A problem is
-- open while `resolved_at` is NULL; the same (kind, key) is never open twice, so a problem is
-- alerted once however often the check sees it. The summary is one line in plain words; the
-- command is what shows more.
CREATE TABLE core.problem (
    problem_id  bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    kind        text        NOT NULL CHECK (kind IN ('SESSION_LATE', 'REPORT_AT_RISK', 'QUEUE_PAUSED', 'JOB_FAILED', 'PROVIDER_AUTH',
                                                     'PROVIDER_ERRORS', 'DATABASE_FULL', 'CALENDAR_END', 'NOT_DERIVED')),
    key         text        NOT NULL CHECK (key <> ''),
    opened_at   timestamptz NOT NULL,
    summary     text        NOT NULL CHECK (summary <> '' AND strpos(summary, E'\n') = 0),
    command     text        NOT NULL CHECK (command <> ''),
    detail      jsonb,                      -- what the check needs to see it clear (NOT_DERIVED's days)
    resolved_at timestamptz,
    CHECK (resolved_at IS NULL OR resolved_at >= opened_at)
);
CREATE UNIQUE INDEX problem_one_open ON core.problem (kind, key) WHERE resolved_at IS NULL;

-- What a published revision holds, counted once (a published revision never changes): its slots,
-- those withheld whole (the engine's containment body: its own result id ends `/engine-fault`),
-- of those the ones withheld for an engine fault and the ones withheld for the provider, and
-- the slots that state an engine fault anywhere. Read by the digest and the weekly check.
CREATE TABLE core.revision_tally (
    revision_id       bigint      PRIMARY KEY REFERENCES vyreon.day_revision ON DELETE CASCADE,
    slots             integer     NOT NULL CHECK (slots >= 0),
    withheld          integer     NOT NULL CHECK (withheld BETWEEN 0 AND slots),
    provider_withheld integer     NOT NULL CHECK (provider_withheld BETWEEN 0 AND withheld),
    engine_faults     integer     NOT NULL CHECK (engine_faults BETWEEN 0 AND slots),
    counted_at        timestamptz NOT NULL DEFAULT now()
);
