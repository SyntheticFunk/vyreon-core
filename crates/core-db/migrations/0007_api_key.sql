-- Keys for the reader API (C-009). A key is shown once, when it is made; only its SHA-256 is
-- kept. Scopes: 'read' (measurements) and 'admin' (the queue and budget status).
CREATE TABLE core.api_key (
    api_key_id bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name       text        NOT NULL UNIQUE,          -- who holds it: 'reporter', 'owner', ...
    key_sha256 text        NOT NULL UNIQUE CHECK (key_sha256 ~ '^[0-9a-f]{64}$'),
    scopes     text[]      NOT NULL CHECK (scopes <@ ARRAY['read', 'admin'] AND cardinality(scopes) > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz
);
