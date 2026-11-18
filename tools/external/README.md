# External checks

Published values against a source that is neither the engine nor Intrinio. A check here is a band
or a convergence statement **declared and committed before the values are compared**, never fitted
to them. A band that fails is reported as failed; changing a band after seeing values needs a
written reason and the owner.

## Cboe volatility indices (`cboe.py`)

Cboe's daily closes, read 2026-11-18 from
`https://cdn.cboe.com/api/global/us_indices/daily_prices/<INDEX>_History.csv` and frozen in
`cboe/` (`SHA256SUMS`). Each index is Cboe's model-free 30-day implied volatility (the VIX method:
the whole out-of-the-money strip, i.e. a 30-day variance-swap rate). Compared with the engine's
base-layer `VS-IVS01` role `iv30` (the fitted surface's at-the-money 30-day IV) for:

| Index | Cboe's underlying | Our security | Same options? |
|---|---|---|---|
| VIX | S&P 500 index (SPX) options | SPY | no (SPX vs SPY) |
| VXN | Nasdaq-100 index (NDX) options | QQQ | no (NDX vs QQQ) |
| RVX | Russell 2000 index (RUT) options | IWM | no (RUT vs IWM) |
| GVZ | GLD options | GLD | yes |
| OVX | USO options | USO | yes |

Why the index sits above our value: a variance-swap rate integrates the whole smile, so with a
convex smile it exceeds at-the-money IV (Carr and Wu, "Variance Risk Premiums", 2009; Cboe VIX
white paper). The gap widens with skew and with the level of volatility.

**Bands (declared 2026-11-18, before any sample value was compared):**

1. Per session: `-1 <= index - 100*iv30 <= 8` points for every pair. The upper 8 is the VIX band
   declared 2026-10-21 in the engine's `refcheck.outside`, kept unchanged; the same for VXN and RVX
   (equity-index smiles of the same shape) and for GVZ and OVX (same options, milder smile for gold,
   strongly convex for oil). The lower bound is -1, not the engine's 0: at-the-money conventions
   (spot vs forward), close times and interpolation can put a flat-smile pair slightly below.
2. Across the panel: the index and `100*iv30` correlate at >= 0.9 (the 2026-10-21 statement),
   judged only when the index's range over the panel is at least 3 points (a narrower range makes
   correlation mostly noise); otherwise reported, not judged.
3. A withheld `iv30` is reported with its state and reason, never compared.

Pass = every compared session inside band 1, and band 2 where judged.

### Nine more pairs (declared 2026-11-18, before any stored value was compared with them)

The five pairs above keep their rules and their committed files unchanged. Nine more, read
2026-11-18 from the same URL pattern (it now redirects to `cdn-api.cboe.com`), every one published
through 2026-11-17:

| Index | Cboe's underlying | Our security | Same options? |
|---|---|---|---|
| VXAPL | AAPL options | AAPL | yes |
| VXAZN | AMZN options | AMZN | yes |
| VXGOG | Alphabet options ("Cboe Equity VIX on Google"; the documents read do not say GOOG or GOOGL) | GOOGL | not confirmed |
| VXEEM | EEM options | EEM | yes |
| VXEWZ | EWZ options | EWZ | yes |
| VXTLT | TLT options | TLT | yes |
| VXD | DJIA index (DJX) options | DIA | no (DJX vs DIA) |
| VXSLV | SLV options | SLV | yes; no close before 2025-05-15 in the window |
| VXFXI | FXI options | FXI | yes; no close before 2025-09-16 in the window |

Pairing evidence: Cboe consultation C2022040800 (2022-04-08) names VXAPL, VXAZN, VXGOG, VXEWZ,
VXEEM and VXTLT with their underlyings ("Volatility Index for underlying TLT"); VXSLV is Cboe's
Silver ETF, VXFXI its China ETF (FXI), VXD its DJIA volatility index. Files: Cboe's terms allow one
copy for personal non-commercial use and ask for approval before other use, so these nine are
**not committed**: they are cached outside the repo and `cboe/fetched.json` records url, sha256 and
fetch time; `cboe.py` refuses a cache that differs. (The five committed files carry the same terms
question; the research recommends one email to Cboe. Owner's call.)

**Bands for the nine:**

1. Per session: `-1 <= index - 100*iv30 <= U` with `U = max(8, 0.25 * 100*iv30)`. Reasoning: the
   variance-swap rate integrates the smile, `sigma_VS^2 = integral of sigma(z)^2 phi(z) dz` over
   the standardized moneyness z (Gatheral, *The Volatility Surface*, ch. 11). With
   `sigma(z) = sigma_0 (1 + a z + b z^2)`, `sigma_VS / sigma_0 ~ 1 + a^2/2 + b`: the excess is
   **proportional to the level**, so a fixed 8 points is a 50% excess at a level of 16 and 16% at
   50. For an S&P-like 30-day smile (a ~ -0.3, b ~ 0.1) the ratio is about 1.15. Single stocks
   and these ETFs carry a flatter put skew (smaller a^2) than index options but fatter wings and
   earnings jumps, which the Cboe strip reaches to its last two-zero-bid strike; 1.25 bounds the
   ratio with headroom for close-time and interpolation differences. At levels below 32 the old
   absolute 8 governs, so the new band is never tighter than the old one. The lower -1 is the old
   pairs' (ATM conventions; also an earnings jump inside 30 days can make a single-stock smile
   concave at the money, and a thin strip's truncation lowers the index).
   **Disclosure:** this band was written after the research pilot reported these nine pairs'
   gaps on 12 sessions (2025-04-11, 2025-12-10, 2025-12-11, 2026-11-02..06 and 11-09..12; largest
   AAPL 2025-04-11, 8.2). Those sessions are marked `seen`; their verdicts are not evidence for
   the band, and each session's verdict under the old pairs' fixed `[-1, 8]` is reported beside it.
   Only `blind` sessions test it (2026-11-13 in the sample; then the phase-1 days).
2. Correlation as for the five: >= 0.9 when the index's range over the panel is >= 3 points.
3. A withheld `iv30` is reported with its state and reason; a date without a Cboe close (VXSLV,
   VXFXI above) is NOT_COMPARABLE, never a pass.

Pass = every compared session of the nine inside band 1 and band 2 where judged; reported over
all sessions and over blind sessions alone.

**Dates.** The sample (2026-11-02..06 and 11-09..13, engine build 05837898) is judged as one
panel. The other published days in the phase-1 window (2025-04-11 and 2025-12-10, build 1a532d8b;
2025-12-11, build 0ca127ea) are reported separately with their builds, band 1 only (a panel of
mixed builds is not a correlation panel). This applies to all checks below.

## OCC cleared volume (`occ.py`)

OCC's Volume Query (`marketdata.theocc.com/volume-query`, CSV, no key): cleared option contracts
per underlying, option symbol (the standard root `EWZ` and adjusted roots such as `2EWZ`),
call/put, exchange and account type (customer, firm, market maker). One request per date returns
every underlying (`symbolType=ALL`, about 4 MB), so a day costs one request, not 64. Each cleared
trade is recorded once per side, so **contracts = the quantity summed over exchanges and account
types, divided by 2**; a sum that is odd for one symbol and side is listed as an anomaly. The data
is OCC's clearing record: independent of OPRA's tape, of Intrinio and of the engine. Cached outside
the repo; `occ.py fetch` records url, sha256 and fetch time.

**Terms: unverified.** OCC's terms page refuses automated reads. **The owner must read OCC's terms
in a browser before this check is relied on.** Requests stay modest: one per date, one second
apart, a generic User-Agent.

**Ours:** base-layer `VS-O06`: `reported_subset.reported_oi_total` (the session's reported option
volume; the role keeps the O02 template's name, `h3-o06` maps volume into it) and
`reported_subset.put_share`. `DLM-PCR-VOLUME` and `DLM-OPTIONS-VOLUME` are history-pass sensors not
stored for these dates: not comparable here; they inherit this check's inputs.

**Compared, per security-day:** standard root = OCC option symbol equal to our ticker; adjusted =
every other symbol under the same underlying (listed by name).

1. Volume, `rel = ours / OCC - 1`: inside `[-2%, +0.5%]` when OCC >= 10,000 contracts;
   `[-5%, +1%]` at 1,000-10,000; below 1,000 reported, not judged. Reasoning: two record keepers
   counting the same trades. The asymmetry allows trades OCC clears that never reach the vendor's
   per-contract daily volume (late or corrected prints) but not a vendor total above the clearing
   record beyond busted-print noise. At 1,000-10,000 a single stray print is a larger fraction.
2. Put share, `|ours - OCC|`: <= 0.005 at >= 10,000 contracts, <= 0.02 at 1,000-10,000. Reasoning:
   a missing fraction f of contracts with put share p' moves the share by f |p' - p|; f <= 2% and
   |p' - p| <= 0.25 give 0.005.
3. **Adjusted roots.** Our volume omits adjusted-root contracts today (EWZ 2026-11-12: `2EWZ`, 38%
   of the underlying's cleared total); an engine fix is in progress. Where OCC has adjusted-root
   volume, ours is judged against both totals with bands 1-2:
   - `PASS_ALL_ROOTS`: inside against standard + adjusted (the fix's target);
   - `ADJUSTED_MISSING`: inside against the standard root only. The known gap: listed with the
     adjusted contracts, their share of the total and the all-roots put-share difference;
   - `FAIL`: inside against neither.
   Where OCC has no adjusted root the two totals are one and the verdict is PASS or FAIL.
4. NOT_COMPARABLE: our role withheld (state and reason) or OCC has no rows for the underlying.

Pass (`pass`) = no FAIL. `pass_all_roots` = no FAIL and no ADJUSTED_MISSING. **The gate is `pass`
while the engine's adjusted-root fix is pending and `pass_all_roots` from the first engine build
that carries it.** The adjusted-root gap is reported on every day either way.

Seen: the research pilot compared SPY, NVDA, IWM, TSLA, AAPL, GLD, MSTR, EWZ, XLU and UUP on
2026-11-12 (and MSTR's `2MSTR`); those security-days are `seen`, all others `blind`.

**Found on the first run (2026-11-18), rules unchanged:** rule 3's premise is wrong for most of
the volume it covers. A root with a leading digit (`2EWZ`, `4SPY`) is a **FLEX** root, not a
corporate-action adjustment (Cboe and NYSE FLEX symbology: 1/2 American/European physically
settled, 3/4 cash settled; Cboe `flexsymbology.pdf`, NYSE "Cash-Settled FLEX ETF Options").
Adjusted roots carry a trailing digit (`BABA2`, `ETHA1`). In the sample, non-standard volume was
8.27 M FLEX contracts, 0.09 M adjusted, 7 k unidentified (`B1SPY`-style). The report labels each
root's kind (information only; verdicts follow the rules as declared). Whether FLEX volume belongs
in the reader's option volume is a definition for the owner and the published standard, not for
this check; until then `ADJUSTED_MISSING` mostly means "FLEX not counted".

## FRED/ALFRED (`fred.py`)

`CDD-MACRO01` publishes three FRED series: CPIAUCNS (BLS), FEDFUNDS and INDPRO (Federal Reserve),
each with its latest value, its observation date and a 24-observation history. Reference: ALFRED's
vintage CSV `alfred.stlouisfed.org/graph/alfredgraph.csv?id=<S>&vintage_date=<D>` (no key): the
series as it stood on D, releases made on D included. Cached outside the repo; `fred.py fetch`
records url, sha256 and fetch time. FRED's terms ask for permission only for copyrighted
third-party series; these three are public-domain government data.

**Compared, per date T and series:**

1. Clock: our `observation_date` against the latest observation in the vintage of T.
   MATCH; `RELEASE_DAY` (ours is the latest of the vintage of T-1 and T is the release date:
   a release made on T may come after the 16:00 close, e.g. H.15 at 16:15; listed, not failed);
   LAG (older than the vintage of T-1 allows: stale, listed, not failed, as EDGAR's clock);
   LOOKAHEAD (later than the vintage of T: failed).
2. Value and history: our value and every history cell (observation date and value) equal to
   the reference vintage: the cells' dates are the vintage's 24 latest non-missing observations
   up to our observation date (a month the agency never published, such as October 2025 CPI, is
   absent in both), and each value is exactly equal.
   - CPIAUCNS, FEDFUNDS: the reference vintage is T's (T-1's for RELEASE_DAY). BLS does not
     revise the not-seasonally-adjusted CPI (engine D-305).
   - INDPRO: the body states `vintage_basis = PROVIDER_CURRENT_REVISED_VINTAGE_AS_CAPTURED_NOT_
     FIRST_PRINT` (engine D-305, owner 2026-11-16): the reference vintage is the New York date of
     its `vintage_captured_at`. Its difference from T's vintage is reported per cell as
     `REVISED_AFTER_T` (information the reader did not have on T; the engine states it; not a
     verdict on D-305).

**Tolerance: exact.** FRED and the provider carry the agency's published decimal (for example
`337.511`); both parse to the same IEEE-754 double, and no arithmetic intervenes. Any difference
is a delivery or selection fault.

Not compared: the text roles (`native_unit`, `frequency`, `seasonal_adjustment`).
Pass = no value or history FAIL and no LOOKAHEAD. Seen: the research pilot compared 2026-11-12.

## EDGAR filing times for CDD-EVENTS01 (`edgar.py events`)

From the submissions files `edgar.py fetch` already caches (`events.sql` gives the bodies).
EDGAR's `acceptanceDateTime` is UTC, as its `Z` says. (Corrected 2026-11-18 before any comparison:
the first declaration said it was Eastern time written with a `Z`. Evidence, outside the compared
window: Apple's item-2.02 8-Ks of 2023-2024, all released at 16:30 New York, read 20:30Z under
daylight time and 21:30Z under standard time, and a Form 4 accepted 2015-10-08T01:43Z carries
filing date 2015-10-07, i.e. 21:43 New York before the 22:00 Section 16 cut-off.) T's close is
16:00 New York.

1. `sec_filing.earnings_release_8k_at` and every cell of its `.history`: equal, to the second, to
   the acceptance of an original 8-K (form `8-K`) of the same filer. Each matched 8-K's EDGAR items
   are reported (2.02 = results of operations); the engine's set is the provider's earnings flag,
   which also marks some non-2.02 8-Ks (D-073), so a non-2.02 match is noted, not failed.
   Completeness: every original 8-K with item 2.02 accepted by T's close and inside the history's
   366-day window must be in the history; a missing one is FAIL (a missed earnings event moves the
   event window and IV-crush readings).
2. The latest periodic report (`sec_filing.report_type`, `period_end`, `filing_date`,
   `accepted_at`): an EDGAR filing of that form and period exists with that filing date, accepted
   at that second. An instant at 00:00:00Z where EDGAR's acceptance differs is FAIL (a time the
   provider did not state, published as one). Clock against the latest original 10-K, 10-Q, 20-F
   or 40-F accepted by T's close: MATCH, LAG (listed), LOOKAHEAD (failed).
3. A withheld coordinate or history is NOT_COMPARABLE with its reason. A match that holds only
   when `acceptanceDateTime` is read as New York time is FAIL (`ZONE`).

Not compared: expected earnings (a provider forecast, stated as such), dividends and splits
(Tiingo, not bought), insider filings (their selection rule differs from EDGAR's listing).
Pass = no FAIL and no LOOKAHEAD. Seen: nothing compared before; AAPL's 2026-11-12 body was read
while writing these rules (not compared).

## SEC EDGAR (`edgar.py`)

The filers' own XBRL facts, read 2026-11-18 from SEC's API: `data.sec.gov/api/xbrl/companyfacts/`
(every non-dimensional fact of every XBRL filing, with accession, form and filing date) and
`data.sec.gov/submissions/` (each filing's form, period of report and acceptance time). Cached
outside the repo; `edgar.py fetch` records each file's sha256 and fetch time. The stored bodies
come from core's published revisions (`edgar.sql`).

**Compared:**

1. CDD-FND01: every supported base reading and every cell of its `<base>.history`:
   `asset_scale`, `revenue_scale` (ln of the amount), `net_margin`, `operating_margin`,
   `operating_cash_margin`, `liabilities_to_assets`, `equity_to_assets`, `current_ratio`,
   `cash_to_assets`, `capital_intensity` (|capex| / |revenue|), `deposits_to_assets`,
   `loans_to_assets`, `return_on_assets_state` (quarter net income over the mean of total assets
   at this and the prior position's end).
2. CDD-FND02: each `<base>.as_reported` as in 1. `<base>.standardized` is the provider's
   reclassification, not a filer fact: its difference from EDGAR is reported, not judged.
3. CDD-CAPVAL01: `reported_shares_outstanding` against `dei:EntityCommonStockSharesOutstanding`
   and `public_float_value` against `dei:EntityPublicFloat` at their effective dates;
   `earnings_yield`: each of its four stated quarters, their sum, and the yield recomputed as
   EDGAR's sum over (the stated raw close x EDGAR's cover shares / the stated ordinary shares per
   receipt); `price_to_book`: the stated total equity and preferred against EDGAR, recomputed the
   same way; `float_to_latest_reported_shares`: its denominator only (the provider's float shares
   as stated over EDGAR's cover shares at the denominator date; the cover page states the float in
   dollars, so the float in shares is the provider's derivation), only where no split was applied.
4. Clock: each FND01 base's `latest_filed_period_end` against the period of the latest original
   periodic report (10-Q, 10-K, 20-F) EDGAR accepted at or before T's close (16:00 New York).

Not compared: year-over-year and sequential differences and percentiles (arithmetic on the levels
above over longer histories), FND02's `difference`, the closing price, enterprise value (withheld).

**How the EDGAR figure is formed** (the engine's construction, spec 06 §5.8 as `fnd.rs` has it):

- Element: the tag the body states (`winning_normalized_source_tag`, matched to the us-gaap or
  ifrs-full element whose name normalizes to it), in the unit the body states. History cells
  state no tag; the reading's basis tag is used (the history is its same-basis series). **The
  elements are the engine's, not chosen independently: this check does not test the engine's
  choice of element.** As a side check, the engine's as-reported alias order (`fnd.rs`) is applied
  to EDGAR's facts of the same filing and period; where it picks another tag, the figure carries a
  tag note (Intrinio's delivery and EDGAR differ), not a verdict.
- Period: the fact's `end` is the cell's `fiscal_period_end`. Durations: discrete quarter at most
  120 days, half-year 150-200, nine months 240-300, fiscal year 340-380; balance items are instants.
- Quarters: balance items direct. Income Q1-Q3 the discrete quarter; Q4 = fiscal year - Q1 - Q2 -
  Q3, the quarters being the periods of the three periodic reports before the year's end (EDGAR
  submissions). Cash flow Q1 direct, Q2 = half-year - Q1, Q3 = nine months - half-year, Q4 = year -
  nine months. Annual filers (the engine's FY_DIRECT_ANNUAL_FILER): the fiscal-year fact. ROA's
  prior assets: the instant at the previous periodic report's period (an annual filer's prior year).
- As first filed: among the facts for one element, unit and period, the one in the periodic filing
  (10-Q, 10-K, 20-F, 40-F, or an amendment) with the earliest acceptance. Later filings' values
  for the same period are kept as alternatives.
- Arithmetic as the engine: a / b; |a| / |b|; ln |a|; ROA a / ((b + c) / 2).

**Tolerance (declared 2026-11-18, before any stored value was compared).** The same filed facts
give the same number: XBRL values are exact decimals below 2^53, the quarter derivations are
integer sums, each ratio or logarithm is one IEEE-754 operation. So `|ours - EDGAR| <= 1e-9 *
max(1, |EDGAR|)`, share counts and the float included. No rounding band: the provider is expected
to carry the filed value exactly.

**Verdicts:**

- PASS: within tolerance against the as-first-filed facts.
- RESTATED (explained, not failed): outside against them, within tolerance when one or more
  operands take a later periodic filing's value for the same period accepted by T's close (an
  amendment or a restated comparative); the filings are named.
- LOOKAHEAD (failed): reproduced only with a filing accepted after T's close.
- UNIT: the value matches EDGAR's fact in another unit than the body states. Failed for
  `asset_scale`, `revenue_scale` and the valuation readings (the currency is part of the value);
  noted, not failed, for FND01's dimensionless ratios.
- NOT_COMPARABLE: EDGAR has no fact for the element, unit (or any currency) and period, or a
  quarter's operands cannot be identified; with the reason. Counted and listed, never a pass.
- FAIL: anything else.
- Clock: MATCH; LAG (ours older than EDGAR's latest by T: stale, listed, not failed); LOOKAHEAD
  (newer than EDGAR allows: failed). A foreign filer's 6-K quarters are not periodic XBRL
  reports: only its 20-F periods are judged.

Pass = no FAIL, failed UNIT or LOOKAHEAD anywhere. The sample's criterion (5) ("EDGAR agrees for
every company's latest-quarter figures") is reported separately over the current readings alone.
The report states what was not comparable.

## Tiingo daily prices (`tiingo.py`)

A second vendor's daily bars recomputing what is a deterministic function of daily prices.
**Declared 2026-11-18, before any stored value was compared with Tiingo** (nothing here is
`seen`). Tiingo's EOD endpoint `api.tiingo.com/tiingo/daily/<TICKER>/prices?startDate=2019-12-01&
endDate=2026-11-17`, one request per security (64), Commercial API plan licensed for this internal
validation. The token is read from `tiingo.txt` and sent as the `Authorization` header, never in a
URL. Tiingo's data is licensed: it is cached **outside the repo**; `tiingo.py fetch` records url
(without the token), sha256 and fetch time in the cache's `fetched.json`, and the checks refuse a
cache that differs. Fields used: `date`, `open`, `high`, `low`, `close`, `volume`, `divCash`,
`splitFactor`. Tiingo's own `adj*` fields are **not** used: the engine's adjustment is rebuilt from
the raw fields. Whether Tiingo's close is the primary listing's official close was not established.

Dates: the sample, 2026-11-02..06 and 11-09..13 (engine build 05837898), from core's published
revisions (`tiingo-field.sql`, `tiingo-prices.sql`). Definitions are the engine's at that build
(`h5-field`, `h3-p01`, `h3-p03`, `h3-path`, `h5-derived` `ta.rs`/`technical.rs`).

**The engine's inputs, rebuilt from Tiingo:**

- Sessions: Tiingo's SPY dates. They must equal the axis of the stored FIELD-DEP01 masks where
  the two overlap (otherwise the tool stops: an alignment error, not a verdict).
- A session's adjustment factor (it relates the bars before t to t): `f_t = (1 - divCash_t /
  close_{t-1}) / splitFactor_t`, the dividend term only when `0 < divCash_t < close_{t-1}`. This is
  `ta::back_adjust` (D-063) with Tiingo's dividend and split (Tiingo's `splitFactor` 2 is
  Intrinio's ratio 0.5). P01 uses Intrinio's per-day factor (D-024), taken to be the same
  CRSP-style factor: a convention difference shows as a return difference on an ex-date, which
  the input check below lists per security-day.
- Adjusted log return `r_t = ln(close_t / (close_{t-1} f_t))` (P01 `adjusted_r1`, FIELD's panel
  cell); raw `ln(close_t / close_{t-1})` (P01 `raw_r1`); split-free raw
  `ln(close_t splitFactor_t / close_{t-1})` (FIELD's raw cell, SUB01 `adjusted_raw`);
  `adjusted_r5` the sum of five adjusted log returns. Window-adjusted OHLC (P03, PATH):
  each earlier bar times the product of the later factors.
- Missing data: **the engine's support, not Tiingo's coverage.** FIELD cells are supported
  exactly where the published FIELD-DEP01 common masks say (union over the sample's bodies:
  security i is supported on session s when some mask of a pair holding i is true on s); older
  sessions count as supported where Tiingo has the bar. Each window's engine-withheld cells are
  counted in the report. Tiingo lacking a bar the engine supports is NOT_COMPARABLE for every
  quantity whose window holds it, never a pass. DLM records: Tiingo's bars from the receipt's
  `daily_bars_from` (the engine's history start after any D-063 restart) through T; SPY's from
  SPY's own receipt; a session missing in that span is NOT_COMPARABLE.

**Tolerances and their reasons.** Two vendors carrying the same official closes give the same
returns to rounding, so the bands are sized to detect one materially wrong print, not to absorb
method differences; a failure is reported with its cause, never re-banded.

- Per-return input tolerance `5e-4` (5 bp). A dividend or ex-date convention difference is a few
  bp at most; a wrong close is more.
- Correlation-type quantities `0.01`, as the research suggested. Reason: one return off by about
  1% in a 21-session window moves that security's 63 pair correlations by up to a few tenths and
  the mean of the 2,016 pairs by about 0.01, so 0.01 is the size of one bad print, and identical
  inputs give about 1e-12.
- Leading eigenvalue 1% relative (the research's figure; about 0.2 at a level of 20, the same
  size as 0.003 on the mean correlation, since `lambda_1 ~ 1 + 63 * mean`).
- Derived quantities take the tolerance their operands' tolerances propagate to (stated per item).
- A level percentile (`PCT.MIDRANK` over n references): `|delta| <= m / n`, m = the number of
  Tiingo reference levels within twice the level's tolerance of Tiingo's current level (only
  those can change sides when each level moves by at most its tolerance), and each stored
  reference cell within the level's tolerance.

### Check 1: FIELD (`tiingo.py field`)

Each T's panel is the 376 calendar sessions through T. Compared:

1. FIELD-SYNC01 `W21.*`, `W63.*`: `off_diagonal_mean`, `median`, `q10`, `q90` (Pearson over the
   terminal-inclusive 21 or 63 sessions, all 64 required in every row, else withheld
   INCOMPATIBLE_COMMON_SUPPORT; Type-7 quantiles of the 2,016 upper-triangle cells): `0.01`.
   `pair_count`, `window_start`, `window_end` exact. Both `.level_percentile_cap126` contexts
   and their `.reference` arrays by the percentile rule (level tolerance 0.01). The
   `pearson_matrix` arrays: every cell `0.02` (one pair is noisier than the mean of 2,016).
2. FIELD-RMT01 `leading_eigenvalue` (Pearson spectrum of the last at most 250 all-64 rows ending
   T, minimum 126): 1% relative. `spectrum` ranks 2..64: `|delta| <= 0.02` absolute (their sum is
   about 44, so 0.02 is under 0.05% of the trace per rank; small eigenvalues have no useful
   relative scale). Leading-eigenvalue share `lambda_1 / 64` reported (follows from the above).
3. FIELD-DIM01 from that spectrum: `participation_ratio` and `entropy_effective_rank` 2% relative
   (`PR = 64^2 / sum lambda^2` is dominated by `lambda_1^2`: 1% on `lambda_1` gives about 2%);
   `k95` within 1 (a threshold crossing of the cumulative share); `reconstruction_loss` at k:
   `|delta| <= (0.01 lambda_1 + 0.02 (k - 1)) / 64` (the cumulative sum of the spectrum bands).
4. FIELD-DEP01 every pair (last 250 calendar sessions, each pair on its common support, minimum
   126): `pearson`, `spearman`, `common_scale_pearson` `0.01` (250 sessions: one bad print moves a
   pair by less than in W21); `signed_concordance` `6 / count` and `categorical_sign_agreement`
   `3 / count` (three sign flips: a return within a tick of zero can change sign between vendors);
   `common_count` exact (it is the engine's own mask, a consistency check).
5. FIELD-GRP01 `within_median`, `between_median`, `median_difference`,
   `group_equal_weighted_difference`, each `group.within_median`: `0.01` (a median of values each
   within 0.01 is within 0.01); pair counts exact.
6. FIELD-SUB01, where published (it is withheld unless admitted, which needs seeded null draws):
   `pearson_spearman.*`, `primary_common_scale_removed.*`, `adjusted_raw.*`,
   `trailing.L{63,126,189,249,250}.*`, `rolling126.*`, `leave_group_out.*` and the minima
   `trailing.minimum_overlap`, `rolling126.minimum_adjacent_overlap`,
   `leave_group_out.minimum_overlap`: overlap `0.01`, principal angle 1.5 degrees (overlap =
   cos^2 of the angle: 0.01 is about 1.7 degrees near 10 degrees). `base_projector_k1`: Tiingo's
   leading vector v must give `v' P v >= 0.99`.
7. Cross-section of T: FIELD-BR01 counts within 1, fractions within `1/64`, `signed_prevalence`,
   `directional_synchronization`, `fragmentation` within `2/64` (one security changing sign);
   FIELD-DIST01 quantiles, `cross_sectional_mad`, `iqr`: `5e-4` (an order statistic of values each
   within 5e-4 is within 5e-4; MAD and IQR within `1e-3`); FIELD-XCONC01 `absolute_move_sum`
   `64 * 5e-4`, `absolute_move_hhi`, `top_one_share`, `top_five_share` and `top_five.share`
   `0.005`, `effective_count` 5% relative (shares of a sum of about 0.5 move by about 1e-3 per
   5 bp; 0.005 leaves room for a top-five reordering), `top_five.object_id` reported. Their
   contexts by the percentile rule.
8. Input check: DIST01 `object_return_state` (each security's adjusted log return on T) against
   Tiingo's `r_t`: `5e-4`.

Not compared, with the reason: RMT01 `iid_leading_q99`, `independent_shift_leading_q99`,
`common_scale_shift_leading_q99`, `separated_dimension` and the three null `*_rankwise_q95`
arrays, DIM01 `separated_dimension`, GRP01 `permutation_q95` and `permutation_exceedance`, SUB01
`bootstrap.*`, `minimum_k1_overlap`, `k3_state`, `k5_state` (seeded NumPy PCG64 draws: reproducing
them would re-implement the engine's generator, not test the prices); FIELD-FDSYNC01,
FIELD-FDRMT01, FIELD-FDDIM01, FIELD-MHDEP01 (15-minute quote midpoints, intraday); FIELD-SUP01,
FIELD-OPTBR01, M0-FIELD-ANALYTICS (options).

### Check 2: price-based base and derived sensors (`tiingo.py prices`)

Per security-day, over the 64 securities:

1. **Input: raw close.** VS-P01 `raw_close` and every cell of `raw_close.history` (the 63
   sessions before T; one comparison per security-session, a session stated differently by two
   bodies listed): `|delta| <= max(0.005, 1e-4 * close)` (half a cent, or 1 bp on prices above
   $50).
2. **Input: returns.** VS-P01 `adjusted_r1`, `raw_r1`, `adjusted_r5` and every cell of
   `adjusted_r1.history`: `5e-4`; `adjusted_close` as `raw_close` (it is T's raw close).
3. VS-P03 `CC.W{5,10,21,42}` 0.5% relative; `PARKINSON`, `GARMAN_KLASS`, `ROGERS_SATCHELL`,
   `YANG_ZHANG` at the four windows 2% relative (they read the high and low, which one print sets
   and vendors' print eligibility can differ). PATH-L01 `gap`, `intraday`: `5e-4`; PATH-L02
   `raw_log_high_low_range`: `1e-3`.
4. DLM-TECHNICAL, DLM-PRICE-LEVELS, DLM-DRAWDOWN, DLM-RELATIVE-PERFORMANCE, DLM-BETA, numeric
   roles, `ta.rs` exactly (Wilder RSI/ATR/ADX seeds, EMA seeded by SMA, SAR from the history's
   first bar, all from `daily_bars_from`):
   - prices (unit `usd`: moving averages, bands, channels, pivots, Fibonacci levels, swing and
     52-week highs and lows, SAR, Ichimoku, MACD lines, ATR): `0.01 + 1e-3 |v|`;
   - oscillators (units `index_0_100`, `index_minus_100_0`): 1.0 point; CCI: `2 + 0.02 |v|`;
   - returns and distances (`return_*`, `spy_return_*`, `excess_vs_spy_*`, `distance_sma_*`,
     `distance_from_52_week_high`, `rate_of_change_10`, `gap`, `atr_14_percent`,
     `bollinger_bandwidth`, drawdowns, `worst_*_session_return`): `1e-3 + 1e-3 |v|`;
   - bounded positions (`bollinger_percent_b`, `position_in_52_week_range`): `0.01`;
     `downside_deviation_1y` 1% relative; `regression_channel_50.slope_per_session`
     `1e-5 + 0.01 |v|`;
   - `correlation_63`, `correlation_1y`: `0.01`; `beta_63`, `beta_1y`: `0.01 + 0.01 |v|`
     (`beta_1y` is the recomputed FIELD-DEP01 SPY pair times the volatility ratio, D-087);
   - `rsi_14.percentile_252`: the percentile rule with RSI's 1.0 point.
   - Reported, not judged: the volume-based roles (`obv_change_20`, `chaikin_money_flow_20`,
     `money_flow_index_14`, `volume_price_trend_change_20`, `vwma_20`, `distance_vwma_20`,
     `volume_oscillator_5_20`: the two vendors' daily volume definitions were not established),
     every text and integer role (states, crossings, session counts: threshold functions of the
     numbers above; their agreement rate is reported).

Not compared: PATH-L03-L07 (its W21 path geometry was not transcribed in this round), VS-P07
(volume), DLM-CANDLESTICK (categorical; reported agreement only), DLM-VWAP (consolidated tape),
every `change5_*`, `magnitude_percentile_cap126` and `continuation_ratio` context of the base
sensors (not transcribed in this round), and the option-based sensors.

**Verdicts (both checks):** PASS inside the band; FAIL outside it, or the engine supported where
the recompute is undefined on the same inputs; NOT_COMPARABLE (engine withheld, with its state
and reason; Tiingo lacking a bar). Pass = no FAIL. The report gives, per quantity, the count
compared, the count failed, the largest difference and its security-day, and every failure.

**First run (2026-11-18, the sample, rules unchanged): both checks pass, no FAIL, nothing
NOT_COMPARABLE.** FIELD: 165,030 comparisons over 88 quantities; SYNC01 W21/W63 means within
4e-7, the leading eigenvalue within 6.4e-4 of about 22.7 (3e-5 relative), every DEP01 pair within
8.5e-4 (HYG.XHB), SUB01 overlaps within 3e-6. Prices: 86,656 comparisons over 108 quantities;
Intrinio's raw closes equal Tiingo's exactly on all 4,608 security-sessions compared (the 63
sessions before each T); the largest return difference is 2.7e-5 (TSM 2026-09-16, a dividend
ex-date: the two vendors' dividend factors). Older differences (a 52-week high 0.09 off for XHB,
`beta_1y` 0.0018 for XHB) come from bars before the compared close history, inside the bands. Two
tool faults were fixed after the first run, neither a band: the port lacked the engine's SPY
self-pair (`spy_pairs_of`: SPY's one-year correlation is 1 over the window), and a 2e-16
cross-body difference was filed as a failure where the rule says listed. The two vendors agree
because they carry the same official closes: this check shows the engine computes what it
declares from correct closes; it does not test what neither vendor could get wrong (both may
share an exchange's error), the options, intraday and seeded-null quantities listed above, or
any date outside the sample.
