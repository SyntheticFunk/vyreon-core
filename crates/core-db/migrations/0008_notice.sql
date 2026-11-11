-- Notices for the owner's Discord channel, as an outbox: a notice is written in the same
-- transaction as the event it reports, and the service sends it afterwards, so an event is never
-- reported that did not happen and none that happened is lost (the transactional outbox pattern).
CREATE TABLE core.notice (
    notice_id  bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    created_at timestamptz NOT NULL DEFAULT now(),
    message    text        NOT NULL CHECK (message <> ''),
    once       text        UNIQUE,              -- a notice with a key is written only once
    sent_at    timestamptz,
    attempts   integer     NOT NULL DEFAULT 0,
    last_error text
);
CREATE INDEX notice_unsent ON core.notice (notice_id) WHERE sent_at IS NULL;
