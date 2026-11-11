-- The point-in-time universe (C-017). Shaped by what Intrinio actually returns (fetched for the
-- 64 on 2026-11-11):
--   * a provider "security" is one id with ONE ticker. A rename is a second record: Meta is
--     sec_gQ8B8y (FB, to 2022-06-08, merged into sec_XO82JO) and sec_XO82JO (META), both dated
--     from the 2012 listing. So an asset's ticker on a date comes from its chain of records;
--   * a ticker is reused by unrelated securities (META: a Roundhill ETF to 2022-01-28; IBIT: a
--     Defiance ETF 2022-09-08 to 2023-05-01; FB: a ProShares ETF from 2025-06-26);
--   * the history listings are incomplete: empty for DBA, SLV and UUP, and some old records carry
--     no dates (EEM's CFEM). The security record itself is then the only evidence.
CREATE EXTENSION IF NOT EXISTS btree_gist;     -- equality inside the exclusion constraint below

-- One row per provider record: the members' own, their earlier tickers, and the other securities
-- that have used a member's ticker.
CREATE TABLE core.security (
    security_id      text        PRIMARY KEY,           -- Intrinio security id
    ticker           text        NOT NULL,
    name             text,
    code             text,                              -- EQS, ETF, ETC, DR, ...
    company_id       text,
    cik              text,
    figi             text,
    composite_figi   text,
    share_class_figi text,
    first_price_date date,
    last_price_date  date,                              -- as of fetched_at for a live security
    merged_into      text,                              -- provider's merged_security_id, as given
    delisted         boolean,                           -- NULL: known only from a history listing
    -- the member security whose history listing returned this record as one of its earlier tickers
    chain_of         text        REFERENCES core.security,
    fetched_at       timestamptz NOT NULL
);
CREATE INDEX security_by_ticker ON core.security (ticker);
CREATE INDEX security_by_chain ON core.security (chain_of) WHERE chain_of IS NOT NULL;

-- A profile's members under their permanent Vyreon asset keys, in the engine's order.
CREATE TABLE core.member (
    profile_id  text    NOT NULL,
    asset_key   text    NOT NULL,
    ordinal     integer NOT NULL,
    security_id text    NOT NULL REFERENCES core.security,   -- the security as the provider knows it today
    group_name  text    NOT NULL,
    company     boolean NOT NULL,                             -- carries the company sensors
    PRIMARY KEY (profile_id, asset_key),
    UNIQUE (profile_id, ordinal),
    UNIQUE (profile_id, security_id)
);

-- The ticker a member security traded under, by date. Derived from core.security: each dated
-- earlier record keeps its own range, and today's record has every other day from its first price
-- (to its last, once delisted). No two spans of a security overlap.
CREATE TABLE core.ticker_span (
    security_id text      NOT NULL REFERENCES core.security,
    ticker      text      NOT NULL,
    record_id   text      NOT NULL REFERENCES core.security,  -- the provider record the span comes from
    during      daterange NOT NULL,
    EXCLUDE USING gist (security_id WITH =, during WITH &&)
);

-- The first date each provider source serves (AGENTS.md; prints are our own capture).
CREATE TABLE core.source_coverage (
    source     text PRIMARY KEY,
    first_date date NOT NULL,
    note       text NOT NULL
);
INSERT INTO core.source_coverage VALUES
    ('option_eod',       '2021-09-27', 'Intrinio option EOD prices, keyed by the ticker in use on the date'),
    ('option_snapshots', '2022-05-27', 'Intrinio option snapshot files'),
    ('replay_tapes',     '2023-05-24', 'Intrinio replay files'),
    ('option_prints',    '2026-10-23', 'option trade prints: served for about five sessions, kept from our first capture');

-- A profile's members on a date, in order, with the ticker each traded under; `ticker` is NULL
-- for a member not listed on that date.
CREATE FUNCTION core.members_on(profile text, day date)
RETURNS TABLE (ordinal integer, asset_key text, security_id text, ticker text, group_name text, company boolean)
LANGUAGE sql STABLE AS $$
    SELECT m.ordinal, m.asset_key, m.security_id, t.ticker, m.group_name, m.company
    FROM core.member m LEFT JOIN core.ticker_span t ON t.security_id = m.security_id AND t.during @> day
    WHERE m.profile_id = profile
    ORDER BY m.ordinal
$$;
