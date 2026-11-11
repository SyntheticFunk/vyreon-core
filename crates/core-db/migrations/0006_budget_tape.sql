-- The Intrinio request budget and the tape caches.

-- The account's request limit is shared by every worker (2,000 a minute for the whole account;
-- core uses a lower ceiling). A worker draws a lease of some requests, valid for a short time,
-- and reports what it used. A lease counts against the ceiling from its grant until a minute
-- after it can last have been used: at its full size until it is released, then at what was
-- used. So the requests made in any rolling minute never exceed the ceiling, whatever the
-- workers' clocks or pace, as long as each stays within its lease.
CREATE TABLE core.budget_lease (
    budget_lease_id bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    worker          text        NOT NULL,
    granted         integer     NOT NULL CHECK (granted > 0),
    granted_at      timestamptz NOT NULL DEFAULT now(),
    valid_until     timestamptz NOT NULL,
    used            integer     CHECK (used BETWEEN 0 AND granted),
    released_at     timestamptz,
    CHECK ((used IS NULL) = (released_at IS NULL))
);
CREATE INDEX budget_lease_recent ON core.budget_lease (valid_until);

-- Requests made, by worker, category and minute: what the workers report when they release.
CREATE TABLE core.request_usage (
    minute   timestamptz NOT NULL,
    worker   text        NOT NULL,
    category text        NOT NULL,
    requests bigint      NOT NULL CHECK (requests >= 0),
    PRIMARY KEY (minute, worker, category)
);

-- One asset-session's reduced tape (C-010): what EVC01's baseline and the intraday FIELD panel
-- read from every earlier session. The body is the engine's own packing, opaque to core, stored
-- as sent; a session a tape cannot be had for is recorded as an absence with its reason. A cache
-- is written once: the same bytes again are accepted, other bytes for the same key are refused,
-- because every later day reads it.
CREATE TABLE core.tape_cache (
    asset_key      text        NOT NULL,
    session        date        NOT NULL REFERENCES core.session,
    method_version text        NOT NULL,
    body_sha256    text        CHECK (body_sha256 ~ '^[0-9a-f]{64}$'),
    body           bytea       STORAGE EXTERNAL,        -- already compressed by the worker
    absence        text,
    stored_at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (session, asset_key, method_version),
    CHECK ((body IS NULL) = (body_sha256 IS NULL)),
    CHECK ((body IS NULL) <> (absence IS NULL))
);
