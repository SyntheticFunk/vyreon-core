-- C-057 (owner decision 2026-11-20; engine D-396 amended): the first session on which the
-- provider's option EOD chain of an asset holds a contract, where that is after the provider's
-- option EOD start (core.source_coverage 'option_eod'): before it the asset had no listed options,
-- and the engine's bulk plan spends no API request on its chain. Keyed by the permanent asset key
-- (C-023), like the member it belongs to in any profile. Each row names its evidence: the
-- provider's answers on the session before (no contract) and on the first session (contracts,
-- every open interest 0), frozen in `crates/test-support/fixtures/intrinio-universe/option-first-chain`.
-- An asset without a row is planned as before.
CREATE TABLE core.option_listing (
    asset_key     text PRIMARY KEY,
    first_session date NOT NULL CHECK (first_session > '2021-09-27'),
    evidence      text NOT NULL,
    observed_at   timestamptz NOT NULL
);
INSERT INTO core.option_listing VALUES
    ('ETHA', '2025-04-11',
     'GET /options/prices/by_ticker/ETHA/eod (include_related_symbols=true): 2025-04-10 no contract (body sha256 b0a6d455adf460245e1117f72f2273860d4e6b234ae7a9df4080a05fdc6cf7ac), 2025-04-11 168 contracts, open interest 0 on every one (body sha256 69f553cc9214adf0dc7563af692ef94ee3c7380a2b8c93e95c9974c3ecbf5852); engine fixture etha-first-chain',
     '2026-11-12T22:58:16Z'),
    ('IBIT', '2024-11-19',
     'GET /options/prices/by_ticker/IBIT/eod (include_related_symbols=true, page_size=2): 2024-11-18 no contract, 2024-11-19 contracts (IBIT241220C00030000, C00031000; open interest 0, the listing session)',
     '2026-11-20T02:27:00Z');
