Copied from vyreon-engine `docs/PRODUCT_INTENT.md` at the `engine-v1` tag (2026-11-10). The engine copy is canonical; references to `docs/DECISIONS.md` mean the engine's.

This document is the ultimate guide (owner, 2026-10-18): the standing rules at the top of
`docs/DECISIONS.md` and every dated decision yield to it.

# Central thesis (owner, 2026-09-30)

Vyreon's edge — and the reason the business can work — is **correct, explained,
continuous measurements with honest context, across one consistent 64-asset
universe**. Not forecasts, not signals. Every engineering and product decision
should lean into it:
- **Correctness, visibly.** Withheld values say why in plain language; never a
  blank or a fake zero. A public methodology-notes page (readable DECISIONS)
  shows how each number is kept right.
- **Every number explained:** what it means and what it does not.
- **Every number in context:** against the asset's own history (percentiles over
  backfilled history — M9 is product work, not plumbing).
- **Honest highlights:** each paid item states its statistic (robust z and
  own-history percentile), and each issue states its flag count beside the
  measured baseline of ordinary weeks (D-065, D-067).
- **Breadth with one method:** identical measurements across 64 assets make
  cross-asset rankings meaningful.
- **Continuous by construction (owner, 2026-10-30):** each sensor publishes on
  at least 80% of asset-days, reached through numerical stability, never by
  filling; a miss is diagnosed (engine → rework, provider → stated absence,
  structural → not applicable). Sensors that read their own history
  (percentiles, unusualness, changes over time) can only be judged on the
  built database: their coverage is measured after the rebuild.
- **Standard and reasonable (owner, 2026-10-30):** intent decides what is
  measured and why; known, standardized formulae decide how (most sensors
  measure market structure), cited. Every reading should be one a practitioner
  would find reasonable; a standard that would make a sensor unstable or
  routinely withheld comes to the owner with the evidence.
Success is not guaranteed; this is the bet. Profitable, not invulnerable.
The free/paid report is the first product, meant to reach sustainable revenue
and fund further experiments; the engine, history and API are the reusable
platform. Watch which pages and questions draw engaged readers to choose the next.
Expansions (not pivots) once the weekly newsletter answers "will anyone pay",
none assuming trading edge: advisor/RIA market context (compliance-safe,
strongest fit), options-volatility education, derived-data API (licence
permitting), event-focused context (earnings/expiry), white-label widgets,
fund/family-office monitoring (after a correctness track record), data-
validation services (also a career bridge), open-source engine + hosted version.
The signup "use" question shows which has pull.
Broker/app licensing (suggested by Hunter Hub, 2026-09-30): once retail
readership proves engagement, license the explained measurements to brokers and
apps as a differentiator for their users. Gates: Intrinio redistribution terms
(Startup covers display to our own users; third-party supply likely needs a
data-partner/enterprise agreement), reliability record, API security review,
methodology documentation (DECISIONS.md). Models: flat licence, per-active-user,
revenue share; months-long sales cycles.
The business lives on search reach and subscription capture/retention; the
email list is the owned channel (search visitors are chances to capture it).
Trading on the observatory is optional private upside, never base load: the
prior model validated only ~+0.05 Sharpe over volatility-aware buy-and-hold.
Any trading research is pre-registered, strictly out-of-sample, cost-realistic,
and kept separate from the public "no signals" product.

## Competitive landscape (sourced scan, 2026-09-30)
- Raw per-ticker numbers (IV, IV rank/percentile, term structure, skew, OI by
  strike, put/call) are a commodity: free at Barchart, Market Chameleon,
  OptionCharts; explained per-ticker SEO pages exist (OptiView, projectoption,
  VolRadar, FlashAlpha). "Explained" alone is not unique.
- Uncommon: one page with every measure explained, percentile vs the asset's
  own history across measures (put/call OI, relative volume, intraday, not just
  IV), fundamentals, strict no-signal stance, honest chance rates. Several
  competitors present folk interpretation as fact (put/call "bullish/bearish",
  "premium-selling signal", walls/flip).
- No direct equivalent to the paid weekly cross-asset synthesis with chance
  rates; narrative services (SpotGamma $89–299/mo, Tier1 Alpha $49.95/mo,
  MenthorQ $129+/mo) are index-centric and sell levels/flows; Unusual Whales
  ($50–120/mo) is flow/signals. Closest threat: Moontower.ai ($99/mo,
  cross-sectional vol, 500+ names, strong credibility).
- Price anchor: Barchart Premier $29.95/mo, $199.95/yr.
- Search: head terms saturated (Barchart, Market Chameleon); question-form
  context queries ("is X put/call OI unusual vs its history", "is X IV high vs
  its history", "what's unusual about X today") are thin — our target. 64
  tickers limits the long tail (universe expansion is a later lever).
- Moat: data is not one; statistical honesty, correctness, continuity and trust
  are — make them visible on every page.

# Product intent — Vyreon Engine

Status: owner direction for implementation and scoped binding review  
Date: 2026-09-21

Engineering charter: [Implementation freedom](IMPLEMENTATION_FREEDOM.md)

## Product

Vyreon Engine is the production scientific measurement system. It reliably
converts real market data into governed daily sensor measurements, persists
accepted measurements, and makes them queryable by downstream consumers.

The sensor measurements are the product. Reporter is a separate system on
another machine. It consumes accepted measurements and packages them into
reports; it does not recalculate science.

```text
real Intrinio data
  -> independent asset acquisition / preparation / science
  -> sealed per-asset results
  -> FIELD after all required asset inputs are ready
  -> complete governed-day validation
  -> durable accepted sensor measurements
  -> authenticated query/API boundary
  -> transient-state cleanup
```

For one represented trading day `T`, Engine ultimately produces all required
per-asset measurements, all required M0/extension measurements, FIELD/cross-
asset measurements, and the derived reader measurements: today 2,853 base slots
(the 2,785 H5 slots, four intraday FIELD owners and 64 VS-IVS01) plus 1,826
derived slots (30 derived measurements, 28 per security and 2 per company), 4,679
in all. The sensor table generates these counts; a sensor added or retired
changes them.

## Reader products

The overall goal is a reporting product with the maximum possible reader
utility. Engine serves two reader products through Reporter:

- **Per-asset report — free, web.** Every governed asset's sensors, plotted
  over time. A sensor that withholds routinely, or publishes a provider
  artifact as a value, is an empty or misleading panel for readers.
- **Cross-asset report — paid.** A highlight reel of what is historically
  unusual across the population, plus cross-asset (FIELD) synthesis. Its
  value depends on highlights being rare and real, not multiple-comparison
  noise: every highlight rests on a defensible statistic stated against the
  measurement's own history, and each issue shows how many readings flagged
  beside how many a typical week in our history flags (D-067).
  No forecasts, expected returns or regime/state labels — Vyreon exposes the
  measured feature layer, not model output. Concept: `market-research/VYREON_PAID_CROSS_ASSET_PRODUCT_CONCEPT.md`.

Prioritize science and engineering work by reader-facing impact on these two
products.

Presentation decisions (owner, 2026-09-29):
- Free per-asset reports are **web-first** (HTML on the public site) with a PDF
  download; paid synthesis is delivered **as a PDF**.
- Audience is **retail investors**: explain everything in plain language
  (expandable where long), built from market-research
  `public-sensor-documentation` and keeping its stated limits.
- Displayed numbers are **rounded** (2 decimals, or 0 where applicable); exact
  values stay in the data.
- Free reports carry a **summary** up front; there is a lot of data, and
  presentation must keep it from overwhelming.
- Free reports are distribution and SEO/search content that funnels to the
  paid synthesis: optimize for **reader utility first, SEO second**.

## Execution

Assets progress independently wherever the science permits. SPY does not wait
for QQQ merely because both belong to one daily run. Asset ownership is logical,
not permanent worker ownership. Ready acquisition, decoding, canonicalization,
historical preparation, and scientific work use available resources dynamically.

FIELD is the intentional global barrier where cross-asset science genuinely
requires the complete governed population.

## Durable product data

Accepted sensor measurements are durable product data. PostgreSQL, or an
equivalent durable queryable scientific store, is the system of record. A
downstream consumer retrieves the complete accepted result without reconstructing
transient Engine artifacts.

Durable results include, where applicable:

- measurement identity, represented date, and asset/FIELD owner;
- scalar values, arrays/tensors, axes, and governed ordering;
- units and representation;
- support, nonsupport, and withholding state;
- masks and reasons;
- required interpretive context;
- science, profile, and method identity; and
- concise provenance sufficient to defend and interpret the result.

Engine retains enough accepted daily measurement history for Reporter to request
its report window. The current requirement is at least 252 accepted trading
sessions for all governed assets plus FIELD (126/252-session own-history context). This is long-lived scientific
product state, distinct from raw or intermediate data.

## Transient production data

Provider responses, canonical source populations, historical working sets,
caches, solver state, temporary arrays, restart workspaces, and other
computational artifacts are transient. They exist only to compute and validate
the measurements.

After a complete day is validated, atomically committed to the durable
measurement store, and successfully read back without provider access, normal
cleanup deletes transient state. Failed or interrupted runs may retain bounded
restart/debugging state, but transient data does not become a permanent forensic
archive.

Raw provider captures are transient too (owner, 2026-10-18). The provider is
paid for so its data can be fetched again, and network is cheaper than storage on
the droplet: download what day T needs, compute its sensors once, store the final
values and the minimal provenance that proves them, then delete the inputs to free
the disk for T+1. Rules are applied when a capture is read, never baked into it.
Recomputing is rare: a measurement found wrong is recomputed for that sensor alone
over the affected days, re-downloading its inputs (new provider evidence, which may
differ slightly); a better method applies from its date forward.

## Reporter boundary

Reporter is a separate machine and product component. It queries Engine through
an authenticated query/API boundary, reads durable accepted measurements, and
builds report packages without recomputing science.

```text
Reporter
  -> authenticated Engine query/API
  -> durable accepted measurements
  -> report package
```

The Engine query boundary must expose complete accepted scientific content and
concise provenance for an explicit immutable day revision. It must never return
partial, mixed-revision, workspace-dependent, or unaccepted results.

## Primary product milestone

The critical milestone is one complete real production trading day:

```text
cold Engine start
  -> real provider acquisition
  -> all governed science
  -> FIELD
  -> every governed slot accepted (2,785 at H5; 4,679 today)
  -> durable commit
  -> provider-free readback/query
```

The H5 target (achieved 2026-09-29) is completion within 12 hours on the intended
production resource envelope. Everything before H5 exists to make this possible.
Work after H5 improves deployment, reporting, recovery, optimization, and
autonomous operation.

## Design principle

Optimize Engine around validated scientific output per unit time and cost.
Memory and execution cost scale with active scientifically necessary work, not
total universe size multiplied by historical depth.

Preserve the science. Do not preserve legacy Python execution structure,
forensic lifecycle machinery, or old implementation constraints unless the
current authority explicitly adopts their observable behavior.

Stated assumptions (owner, 2026-10-05): where readers search for a
market-convention number that rests on an unobservable assumption (e.g.
conventional net gamma exposure assumes dealers are long calls and short
puts), publish the assumption-free parts as measurements and the convention
figure beside them, with the assumption stated on the page where the number
appears. The reader then owns its interpretation. Never present an assumed
quantity as observed.

## AI-readable source (owner, 2026-09-30)
Agents and answer engines are a new audience; deterministic, explained
measurements are uniquely suited to them. Every public value carries its
definition, unit, as-of time, support state and withheld reason; pages lead
with citable sentences ("As of the 2026-09-18 close, SPY implied volatility …
was higher than 91% of the last 126 sessions"); `llms.txt`, schema.org
Dataset/Observation markup and stable URLs. Later: a Vyreon MCP server / JSON
API as a paid tier (Intrinio redistribution terms apply). Open decision for the
owner: allow AI crawlers (GPTBot, ClaudeBot, PerplexityBot) for citations and
brand at the cost of some zero-click answers.

## Naming (SEO, 2026-09-30)
Public names use the market's vocabulary (implied volatility, IV skew, term
structure, put/call ratio, open interest by strike/expiration, realized
volatility, relative volume), with a qualifier wherever our measurement differs
from the common definition; never borrow a term we do not compute (as of
2026-10-06 we compute GEX and max pain (D-050, D-052, D-055), not 25-delta skew).
Internal names and IDs never appear in public copy.
The prototype's private vocabulary ("cross-horizon coherence", "telemetry") was
invisible to search; the reach of the free reports depends on this.

## Owner outcomes (2026-10-03)

Measured against these, in order of ambition:
1. **Good:** a launched product gives a credible zero-to-one story for senior
   technical interviews (needs public per-asset pages live, the engineering
   case study, and the owner able to explain the core design himself).
2. **Better:** profitable within six months: revenue ≥ running cost
   (~20–30 subscribers at USD 29 against ~USD 550–800/month).
3. **Best:** durable upside to the owner's salary from normal employment
   (revised 2026-10-05: Vyreon replacing a salary by month 12 is not a fair
   goal). Enough recurring surplus to matter, and to give more choice of work.
Month-12 goal (owner, 2026-10-05): **100 paying subscribers** (~USD 2,700/month
net at USD 29), covering running costs (~USD 1,300–1,550/month incl. a possible
Enterprise upgrade) plus the AI subscriptions that build it (~USD 400/month),
with margin. At those costs 100 payers leave roughly USD 500–750/month: upside,
not an income (Codex economics, `commercial-research/2026-10-05/13_*` §6). The Enterprise upgrade itself is a data-driven decision at that point.
No unrealistic growth assumptions; the kill criteria in MILESTONES.md still bind.
Scale rule (owner, 2026-10-04): the goal is eventually the full optionable
market, but the 64-security system must be profitable first. Additions are
chosen by reader demand (coverage requests from signups and pilot
reservations) as well as volume; compute grows ~linearly with names
(~1 h/day for 64 on 16 laptop workers), so expansion is sized in M11.
Data upgrade path (owner, 2026-10-05): Intrinio Enterprise (~USD 1,250/month)
adds Corporate Events (forward ex-dividend dates; ~50k/mo generic and 5k/mo
QQQ ex-dividend searches, covered-call early-assignment checks) and indices
(e.g. SPX expected move, 500/mo). Startup pricing steps from ~USD 333 to
~USD 999/month over 12 months (billed quarterly), so by month 12 Enterprise
costs only ~USD 250/month more (~10 subscribers): the default upgrade if the
business has passed its kill criteria at month 12. Earlier, upgrade only when
revenue covers the difference with margin.

## Forward-information research boundary (owner, 2026-10-05)

Whether any sensor carries forward information is an internal research question
for when multi-year history exists (pre-registered hypotheses, true out-of-sample
tests, multiple-comparison control; negative results recorded). Any signal is
never exposed in the public product (legal risk; "measurements, not forecasts").
Upside paths: personal trading (upside, not base load) or a separately evaluated
institutional data/API product, subject to its own legal and Intrinio licensing
review.
