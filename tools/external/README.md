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
