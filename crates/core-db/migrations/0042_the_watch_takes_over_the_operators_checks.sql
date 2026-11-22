-- C-065: core's watch takes over the operator's health checks (`deploy/k8s/ops/health.sh`), and a
-- second, optional channel carries the run's progress.

-- A claim that found nothing while work of its kind was claimable for its build (and, for a daily
-- worker, of priority 1): counted from the claim path, so the watch can tell pods leaving while
-- work waits (the race of 2026-11-22) from pods that found the queue empty. Only such claims are
-- kept (an empty queue's claims are not), at most a week (the watch prunes older rows).
CREATE TABLE core.claim_miss (
    claim_miss_id bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    at            timestamptz NOT NULL DEFAULT clock_timestamp(),
    kind          text        NOT NULL CHECK (kind IN ('TAPE', 'DAY_BLOCK', 'ORDERED_PASS')),
    engine_build  text        NOT NULL,
    worker        text        NOT NULL,
    daily         boolean     NOT NULL,
    claimable     bigint      NOT NULL CHECK (claimable > 0)
);
CREATE INDEX claim_miss_at ON core.claim_miss (at);

-- The stall's clock: when the watch first saw claimable work with no live lease, until it sees
-- either end (one row at most; the watch writes it inside its check).
CREATE TABLE core.stall_seen (
    one   boolean     PRIMARY KEY DEFAULT true CHECK (one),
    since timestamptz NOT NULL
);

ALTER TABLE core.problem DROP CONSTRAINT problem_kind_check;
ALTER TABLE core.problem ADD CONSTRAINT problem_kind_check
    CHECK (kind IN ('SESSION_LATE', 'REPORT_AT_RISK', 'QUEUE_PAUSED', 'JOB_FAILED', 'PROVIDER_AUTH',
                    'PROVIDER_ERRORS', 'DATABASE_FULL', 'CALENDAR_END', 'NOT_DERIVED', 'DAILY_STOPPED',
                    'ARCHIVE_MISSING', 'REFCHECK_DISAGREES', 'REFCHECK_NOT_RUN', 'STALLED', 'CLAIMS_EMPTY'));

-- The progress channel: a post is for the owner's alerts channel (as every post so far) or for the
-- progress channel; each has its own sender and order, so a progress post that fails never holds
-- back or repeats an alert.
ALTER TABLE core.notice ADD COLUMN channel text NOT NULL DEFAULT 'alerts' CHECK (channel IN ('alerts', 'progress'));
ALTER TABLE core.notice ADD CONSTRAINT notice_progress_is_posted CHECK (channel = 'alerts' OR post);
DROP INDEX core.notice_to_post;
CREATE INDEX notice_to_post ON core.notice (channel, notice_id) WHERE post AND sent_at IS NULL;

-- Each progress window composed (every 30 minutes while the progress channel is on), with what it
-- held; the next window starts where the last ended. A window with nothing in it posts nothing.
CREATE TABLE core.progress_window (
    window_end   timestamptz PRIMARY KEY,
    window_start timestamptz NOT NULL CHECK (window_start < window_end),
    lines        integer     NOT NULL CHECK (lines >= 0),
    posts        integer     NOT NULL CHECK (posts >= 0),
    composed_at  timestamptz NOT NULL DEFAULT clock_timestamp()
);
