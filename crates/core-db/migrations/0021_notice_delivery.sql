-- C-050 amended (audit 2026-11-17 round v, coreops 8): a post is claimed before it is sent. The
-- sender sets `sending_since` on the oldest unsent post (row-locked, so two senders never take the
-- same one) and posts it only if no other claim on it is live; `sent_at` is then set under that
-- claim. A claim left by a sender that stopped before recording its outcome lapses after
-- `core_api::notices::CLAIM_SECONDS` and the post is sent again: the one window in which a post
-- can go twice is between Discord accepting it and core recording that (Discord's webhook takes
-- no idempotency key).
ALTER TABLE core.notice ADD COLUMN sending_since timestamptz;
