# option-first-chain

The evidence behind `core.option_listing` (C-057): for each member whose options were first listed
after the provider's option EOD start (2021-09-27), Intrinio's option EOD answer
(`GET /options/prices/by_ticker/{root}/eod?date=D&include_related_symbols=true`, first page) on the
session before its first chain (no contract) and on its first chain (contracts, every open interest
0: open interest is the prior session's settled count, so a listing session has none). Real data,
unmodified.

| file | what | how fetched |
|---|---|---|
| `ETHA-2025-04-10.json` | `{"prices":[],"next_page":null}` | the engine's capture (`vyreon-inputs fill`, test droplet, 2026-11-12; `page_size=10000`): body `capture/00000000.body` of `vyreon-engine/workspace/fixtures/etha-first-chain/h5-acq/ETHA/options/2025-04-10`, copied byte for byte (SHA-256 `b0a6d455…`) |
| `ETHA-2025-04-11.json` | 168 ETHA contracts, all open interest 0, one page | the same capture of 2025-04-11 (body SHA-256 `69f553cc…`, the capture index's) |
| `IBIT-2024-11-18.json` | no contract | Intrinio MCP `get_options_prices_by_ticker_eod` (`page_size=2`, `include_related_symbols=true`), 2026-11-20T02:2xZ, as returned |
| `IBIT-2024-11-19.json` | first page of 2: IBIT241220C00030000, C00031000, open interest 0 | the same call, as returned (the next page token is the provider's) |

The 2025-04-11 A/B (`docs/MILESTONES.md`, window from 2024-05-21) agrees: its bulk day files held no
row of ETHA on 181 sessions and of IBIT on 126, exactly the sessions 2024-07-23 (ETHA's first trading
session) to 2025-04-10 and 2024-05-21 to 2024-11-18.
