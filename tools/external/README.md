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
