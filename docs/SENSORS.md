# Sensors: the map

What each measurement reads, depends on and can withhold, for reading the phase-1 run (owner, 2026-11-26: "a
memory/guide while we follow the plan"). A map, not a rulebook: the engine's code and `docs/DECISIONS.md` are the
authority. Each statement carries a code location and is labelled PROVEN (the code path was read) or INFERRED.

**Status.** Traced by a read-only agent on engine `fd319730`, 2026-11-26: 31 of 94 measurements fully traced, 63
partly (registry, graph edges, method label and reason strings; not every condition). Verified by the coordinator
so far: section 4's PCR-UQTR-I01 finding (`h3-replay/src/official.rs:60`, `QUOTE_UNVERIFIED_FROM = "2026-11-06"`).
Everything else is the agent's reading until checked. What the run teaches about a sensor is added under
"Learned in the run" at the end, dated.

## Sensor trace: every measurement, its inputs, parents and withholding reasons

Written 2026-11-26. Read-only trace of the engine checkout `/home/diggz/vyreon-engine-worker`
(branch `worker`, tip `fd319730`). Nothing was built, run or changed. No network request was made.

### How to read this file

1. Every path is relative to `/home/diggz/vyreon-engine-worker` unless it starts with `/`.
   `DECISIONS:n` means `docs/DECISIONS.md` line n.
2. **PROVEN** means I read the code path at the cited line. **INFERRED** means I rely on a name, a
   comment, a registry title, a string literal found by search, or arithmetic on the census counts.
3. Classes of a withholding reason:
   - **ENGINE**: the engine failed or cannot resolve it (rework).
   - **PROVIDER**: the provider's delivery cannot support it (stated absence).
   - **STRUCTURAL**: not applicable by construction (arithmetic, no such contract, not listed).
   - **HISTORY**: needs more built days or a longer in-day history.
   - **RULE**: a ruling or threshold withholds on otherwise usable input.
   The class is my reading. Where the code or DECISIONS states the class, I say so.
4. The census file is `/home/diggz/claude-artifacts/checks/census-2026-11-25.tsv`. Its 94 `ALL` rows
   are the list of ids traced here.
5. Trace depth. 31 ids are fully traced (I read the compute or record builder and every reason
   site). 63 are partly traced (registry row, graph edges, method label and reason strings found by
   search; not every condition was read). Each block says which. The list is in section 6.

### 0. The machinery every sensor shares

### 0.1 The registry and the graph (PROVEN)

1. One table lists every sensor: `crates/vyreon-sensors/src/lib.rs:151-264`. Base rows first, then
   30 derived rows. The 18 day-local derived ids are `DAY_LOCAL` at `:21-26`. The other 12 derived
   ids are the history layer (`:333-336`).
2. One graph lists every same-day parent edge and every store-history edge:
   `crates/vyreon-graph/src/lib.rs:75-191`. Families are at `:37-60`. A test pins base edges to the
   executor's catalogue (`:7-9`); the executor's edges are `crates/h3-asset/src/catalogue.rs:624-660`.
3. Layer of an id: `crates/vyreon-graph/src/lib.rs:353-359` (base, day-local, history).
4. Base sensors read no store history. Their histories are recomputed from captures inside the
   day's run (`crates/vyreon-graph/src/lib.rs:13-14`). Derived history sensors read earlier
   published days from the database (`crates/vyreon-cli/src/derive/ordered.rs:1-6`).

### 0.2 Stage order (PROVEN)

`crates/vyreon-cli/src/day.rs:1702-1726` stamps: intent A plan, intent A (history nodes), exports,
intent B plan, intent B (T), export index, company, surface, FIELD. Then bundle (`:1659`),
day-local derived (`:1678`), reference check (`:278`). The fill categories and their run order are
`crates/vyreon-cli/src/fill.rs:47-57`: daily, options, quotes, carry, macro, company, act01,
consolidated, exq01.

Which category feeds which stage (PROVEN, `crates/vyreon-graph/src/lib.rs:460-482`):

| Stage | Categories it needs |
|---|---|
| Intent A, intent B (owner routes, views of every asset) | daily, options, quotes, carry, macro, act01, consolidated |
| Surfaces and day-local derive | daily, options, quotes, carry, macro, consolidated |
| Company stage | company, daily, options, quotes |
| EXQ family | exq01 and act01 |
| ACT family | act01 |

### 0.3 Reasons any slot can carry: set C (PROVEN, `crates/h1-contract/src/lib.rs:32-77`)

| Reason | Condition | Class |
|---|---|---|
| ENGINE_FAULT | the slot's own kernel failed (`:33-34`) | ENGINE |
| PARENT_ENGINE_FAULT | a parent was faulted or failed; the kernel did not run (`:35-36`) | ENGINE |
| PROVIDER_INPUT_MISSING | an input the provider should have delivered for the asset is absent: no chain, no daily bar, no tape (`:47-51`) | PROVIDER (stated in the comment) |
| PROVIDER_COVERAGE_START | what it reads lies before a source's first date or the asset's first session (`:52-57`). Dates: option snapshots 2022-05-27, replay tapes 2023-05-24 | STRUCTURAL (the comment says provider-limited, "nothing to deliver") |
| CARRY_NOT_IDENTIFIED | the asset's surface was not built: no expiry has three call/put pairs whose American σ agree at one residual yield (`:58-62`; D-185, DECISIONS "D-185") | PROVIDER by the comment; the three-pair minimum is a RULE (D-185, D-107 §8) |
| EMPTY_DELIVERED_POPULATION | an option population it reads was delivered empty (`:43-46`; D-117) | PROVIDER |

Precedence when two apply: `crates/h3-asset/src/h4_run.rs:1218-1224` (coverage start < carry <
empty < input missing < parent fault < engine fault).

"C" below means these six. "C-surface" means the slot can also state CARRY_NOT_IDENTIFIED because
it reads VS-IVS01.

### 0.4 Tape reasons: set T (PROVEN)

The replay tape of a session is reduced to 15-minute bins. Each bin has three gates: price, volume,
quote (`crates/h3-interval-source/src/consolidated.rs:379-383`, `:454-500`). D111 applies them per
family (`crates/h3-temporal/src/lib.rs:292-300`). Reasons list: `consolidated.rs:105-138` (16) plus
`crates/h3-temporal/src/lib.rs:118-130` (5).

| Reason | Condition | Gate | Class |
|---|---|---|---|
| TAPE_LATE_START | tape's first event after the bin's start (`consolidated.rs:477-478`) | all | PROVIDER |
| TAPE_TRUNCATED | tape's last event before the bin's end; last bin needs close + 30 min (`:475`, `:479-480`) | all | PROVIDER |
| TAPE_GAP | a minute of the bin with no trade message (price, volume) or no quote message (quote); or a dropout minute under 0.35 of its neighbours' median on both streams (`:487-490`, `:391-409`; D-157) | per stream | PROVIDER; 0.35 is a RULE constant |
| QUOTE_BOOK_ONE_SIDED / CROSSED / LOCKED | the bin's closing book is not two-sided and valid (`:491-498`; D-133) | quote | PROVIDER |
| SOURCE_VOLUME_RECONCILIATION_FAILED | counted day volume differs from the official daily volume by more than 2% in either direction (`:447-453`, constant `:97`; D-089 ruling 5) | volume, trade count | RULE (2%) on a PROVIDER comparison |
| OFFICIAL_VOLUME_UNAVAILABLE | no positive official volume to compare with (`:448-449`, `:197-199`) | volume | PROVIDER |
| TAPE_CORRUPT_SEGMENT | skipped messages of the security above 0.1% of its messages (`:463-465`, constant `:103`) | all bins | PROVIDER; 0.1% is a RULE constant |
| TAPE_UNAVAILABLE, NO_TAPE_MESSAGES, MULTI_TAPE | capture withheld whole or no tape object (`:458-462`, `:411-413`) | all bins | PROVIDER |
| QUOTE_TAPE_TRUNCATED / LATE_START / GAP, QUOTE_BOOK_INVALID | session-wide READ@1 quote facts; valid closing books under 90% (`:420-438`, constant `:100`) | kept for comparison (`:503-506`) | PROVIDER |
| TAPE_INCOMPLETE_AGAINST_OFFICIAL_BAR | gate 2: tape OHLC differs from the official bar and the tape cannot hold the bar (range inside, close not on tape, or volume more than 2% short) (`crates/h3-replay/src/official.rs:387-409`) | price | PROVIDER (stated `official.rs:18`); exact-match tolerance is a RULE (D-411, D-412) |
| TAPE_DIFFERS_FROM_OFFICIAL_BAR | gate 2: differs and none of the above (`official.rs:404-405`) | price | ENGINE (classed engine in D-411, DECISIONS:16500) |
| OFFICIAL_BAR_UNAVAILABLE | no official daily bar (`official.rs:389`) | price | PROVIDER |
| QUOTE_COMPLETENESS_UNVERIFIED | gate 3: session date >= 2026-11-06 (`official.rs:60`, `:412-414`) | quote | RULE, date-shaped (D-411, provider fault P-04) |
| TAPE_LISTING_RESTATED | the provider restated the listing exchange after the cache was derived and the raw histories are gone (`crates/h3-temporal/src/lib.rs:125-130`; D-468) | whole session | ENGINE (stated "engine-limited") |

Bin-cell reasons inside D111 itself: MISSING_REQUIRED_INPUT (no row or null field),
INVALID_REQUIRED_INPUT (invalid field), DEGENERATE_INPUT (non-positive or inconsistent OHLC,
negative volume, bad book) (`crates/h3-temporal/src/lib.rs:53-67`, `:150-232`).

### 0.5 Derived-layer shared reasons (PROVEN, `crates/h5-derived/src/science.rs`, `lib.rs`)

| Reason | Condition | Class |
|---|---|---|
| DEPENDENCY_WITHHELD (+ detail) | a parent role is withheld; the detail carries the parent's reason (`lib.rs:400-405`, `:1497-1524`, `science.rs:277-282`) | follows the parent |
| INSUFFICIENT_LEVEL_HISTORY | fewer eligible prior sessions than 75% of the window: 95 of 126, 189 of 252 (`science.rs:116-124`, `:161`; D-404) | HISTORY (floor is a RULE, D-404) |
| INSUFFICIENT_WINDOW_SUPPORT | a mean's window has fewer eligible sessions than ceil(0.75 h) (`science.rs:181-232`; D-404) | HISTORY |
| HISTORY_OF_ANOTHER_METHOD_VERSION | the minimum is met only with sessions published under another science or method label; or a change's earlier endpoint is one (`versions.rs:19`, `science.rs:139-141`, `:228-232`, `:393`; D-273) | RULE (D-273) |
| NUMERICAL_DOMAIN | a result is not finite (`science.rs:56`, `:245`, `:277`) | ENGINE |
| PARENT_COUNT_WITHHELD | an integer role's parent count is withheld (`lib.rs:213-217`) | follows the parent |
| ENGINE_FAULT | the object's chain failed to load or its derive faulted (`crates/vyreon-cli/src/derive/local.rs:416-417`) | ENGINE |
| EMPTY_DELIVERED_POPULATION | the chain was delivered empty (`derive/local.rs:418-420`; D-117) | PROVIDER |

### 1. Families

| Family | Ids | Layer | Crates |
|---|---|---|---|
| VS-Q* | VS-Q01..Q05 | base | h2-q01, h3-q02..q05 |
| Price (VS-P*, PATH-*, SAI-PD01) | VS-P01, VS-P03, VS-P07, PATH-L01, PATH-L02, PATH-L03-L07, SAI-PD01 | base | h3-p01, h3-p03, h3-p07, h3-path, h3-pd01 |
| VS-O*, VS-L01 | VS-O01, O02, O03, O04, O06, OIJ01, VS-L01 | base | h3-o01..o06, h3-oij01, h3-l01 |
| VS-V*, INT-IVSC01, SAI-G01 | VS-V02, VS-V03-CURRENT, VS-V03-MATCHED-NATIVE, INT-IVSC01, SAI-G01 | base | h3-v02, h3-v03, h3-v03-matched, h3-ivsc, h3-g01 |
| VS-IVS01 | VS-IVS01 | base | h3-iv-surface |
| M0-* | M0-ANALYTICS, IV-PROFILE, IV-CHANGE, GAMMA, GAMMA-CHANGE, PRINT-SIZE, FIELD-ANALYTICS | base | h3-m0-analytics, h3-m0-contract, h3-m0-gamma, h3-m0-print-size |
| Replay (PCR-*, D111, INT-TEMP-*) | D111-TEMP01, INT-TEMP-IP01, PCR-UPATH-I01, PCR-UACT-I01, PCR-UQTR-I01, INT-TEMP-EVC01 | base | h3-replay, h3-interval-source, h3-temporal |
| SAI-ACT*, INT-ACT-TV15 | SAI-ACT01, SAI-ACT01-INTRADAY, INT-ACT-TV15 | base | h3-act01 |
| SAI-EXQ* | SAI-EXQ01, SAI-EXQ01-TEMPORAL | base | h3-exq01, h3-asset (exq01_*) |
| CDD-* company | CDD-REF01-02, FND01, FND02, CAPVAL01, EVENTS01 | base | h5-company |
| CDD-MACRO01 | CDD-MACRO01 | base | h3-macro, h3-asset (macro01_production) |
| FIELD-* | 15 ids on FIELD.CORE64 | base | h5-field |
| DLM day-local | 18 ids | day-local | h5-derived, vyreon-cli derive/local.rs |
| DLM history | 12 ids | history | h5-derived, vyreon-cli derive/ordered.rs |

1. **VS-Q\***. Quality checks of the option population the engine reads: counts, quote classes,
   identity, clocks, provider analytic provenance. Q01 is the root of every option sensor
   (`crates/vyreon-graph/src/lib.rs:76-80`). Base layer, intent B. Q01 counts the population after
   the closing-snapshot overlay (`crates/vyreon-sensors/src/lib.rs:140-144`). PROVEN for the edges,
   INFERRED for the plain-words meaning (registry titles `:155-161`).
2. **Price family**. Daily-bar readings: prices and returns (P01), gap and range (PATH-L01, L02),
   21-session path geometry (PATH-L03-L07), realized-variation estimators (P03), volume (P07),
   multi-horizon displacement (PD01). All read VS-P01 (`graph lib.rs:81-87`). Base layer, intent B,
   histories recomputed in the day from daily captures. Titles: `sensors lib.rs:162-168`.
3. **VS-O\*, VS-L01**. Reported open interest and volume laid on the strike and expiry lattice, and
   end-of-day quote width. O02, O04, L01 and OIJ01 are the "endpoint quartet": intent A computes
   their history endpoints and exports them (`crates/vyreon-cli/src/day.rs:23`, `:992`). Base layer.
4. **VS-V\*, INT-IVSC01, SAI-G01**. V03-CURRENT fits the engine's own American σ on one selected
   monthly slice. IVSC01 classifies whether T and T−5 are comparable. V03-MATCHED publishes the
   T vs T−5 changes. V02 and G01 are internal checks of the provider's analytics
   (`sensors lib.rs:176`, `:180`: PERSIST and Internal). Base layer.
5. **VS-IVS01**. The day's American implied-volatility surface per asset: IV30, IV60, IV90, skew,
   term ratio, 30-day smile (`crates/h3-iv-surface/src/body.rs:38-67`). Built by the `h5_surface`
   stage from chains, closing quotes, carry inputs, the rate curve and dividends. Base layer, its own
   "surface" stage. No graph parent (`graph lib.rs:154`).
6. **M0-\***. Own-model analytics per contract (IV, Greeks), IV profiles for every expiry, their
   reconstructed changes, the gamma-weighted OI field and its change, and print sizes of the selected
   contracts. Titles `sensors lib.rs:221-227`. M0-ANALYTICS binds the day's surface
   (`graph lib.rs:141`). Base layer, intent B (the heavy routes, `catalogue.rs:424-426`).
7. **Replay family**. Readings of the consolidated tape in 15-minute bins: the grid and its support
   (D111), the price path (UPATH), volume and trade-count activity (UACT), quote widths (UQTR),
   same-slot events against prior sessions (EVC01), and a registry control (IP01). Base layer.
8. **SAI-ACT\***. Option activity of a certified cohort of 20 contracts (2 expiries x 2 rights x 5
   strikes, `crates/h3-asset/src/exq01_present_result.rs:50-52`) from per-contract option interval
   pulls. Base layer.
9. **SAI-EXQ\***. Execution prices of the cohort's prints against same-row quotes. Reads the
   perishable print capture (`graph lib.rs:368-375`; D-103). Base layer.
10. **CDD company**. Company binding, fundamentals as reported and standardized, capital and
    valuation, corporate events, for the 17 companies (`sensors lib.rs:33`, `:192-196`). Base layer,
    company stage.
11. **CDD-MACRO01**. Three macro series (CPIAUCNS, FEDFUNDS, INDPRO) as known on T, the same body for
    every one of the 64 assets (`crates/h3-macro/src/lib.rs:9-23`). Base layer.
12. **FIELD-\***. Cross-section measurements over the 64 on one object, `FIELD.CORE64`: breadth,
    return distribution, concentration, synchronization, dependence, spectrum, option-field support.
    Daily panel from VS-P01; intraday panel from D111 quote midpoints (`graph lib.rs:128-153`).
13. **DLM day-local**. Reader-facing records derived from T's base bodies and T's input files
    (chain, surface file, daily bars, tape of T). Computed in the day (`derive/local.rs`).
14. **DLM history**. Records that read earlier published days: level percentiles, means, changes,
    IV context, IV crush, unusualness. Computed by the ordered pass from the database
    (`derive/ordered.rs:1-6`).

### 2. Every measurement id

Format of each block: layer; meaning; provider inputs; parents; method label; code; reasons.
"Title" quotes the registry (`crates/vyreon-sensors/src/lib.rs`, line given).

### 2.1 VS-Q family (base; partly traced)

Provider inputs for all five: option EOD chain of T (fill category `options`) with the closing
quote snapshot overlay (category `quotes`). INFERRED from `sensors lib.rs:140-144` and
`graph lib.rs:464`. All can state set C.

1. **VS-Q01** — title "Option-population support of the population the engine reads" (`:155`).
   Parents: none (`graph lib.rs:76`). Code: `crates/h2-q01/src/lib.rs` (reason string `:387`).
   Reasons: EMPTY_DELIVERED_POPULATION when zero rows delivered (`h2-q01/src/lib.rs:387`, PROVIDER,
   INFERRED condition); C.
2. **VS-Q02** — "Returned quote classification" (`:156`). Parents: Q01 (`graph :77`). Method
   `VS-Q02-QUOTE-CLASSIFICATION@1` (`crates/h3-q02/src/candidate_run.rs:371`). Code
   `crates/h3-q02/src/result.rs`. Reasons: EMPTY_DELIVERED_POPULATION (`result.rs:139`); C. INFERRED.
3. **VS-Q03** — "Strict identity and registered challenges" (`:157`). Parents: Q01. Method
   `VS-Q03-STRICT-IDENTITY@1` (`crates/h3-q03/src/result.rs:372`). Reasons:
   EMPTY_DELIVERED_POPULATION (`probe_run.rs:198`); C. Registered challenges are the FB/META 2022
   transition and the NVDA 2024 split (`result.rs:25-26`). INFERRED.
4. **VS-Q04** — "Represented dates and source clocks" (`:158`). Parents: Q01, Q03 (`graph :79`).
   Method `VS-Q04-CLOCK-REDUCTION@1` (`crates/h3-q04/src/candidate_run.rs:237`). Reasons (PROVEN,
   `crates/h3-q04/src/result.rs:360-369`): INVALID_REQUIRED_INPUT on role 3 [PROVIDER];
   CLOCK_INCOMPATIBLE on role 6 [PROVIDER]; EMPTY_DELIVERED_POPULATION on roles 3 and 5 exactly when
   zero rows were delivered (`:371-375`); C.
5. **VS-Q05** — "Provider analytic support and provenance" (`:161`), internal (not served, D-397).
   Parents: Q01, Q03, Q04. Method `VS-Q05-ANALYTIC-PROVENANCE@1`
   (`crates/h3-asset/src/h4_quality.rs:789`). Reasons: EMPTY_DELIVERED_POPULATION
   (`crates/h3-q05/src/result.rs:106`); C. INFERRED.

### 2.2 Price family (base; partly traced)

Provider input for all: daily prices (Intrinio `stock_prices`; category `daily`, and the daily bars
the `options` category captures in T's windows, `crates/vyreon-cli/src/fill.rs:7-9`). The
own-history context is CAP126: the 126 sessions before T, floor 95 eligible
(`crates/h1-numerics/src/lib.rs:87-105`; D-404). All can state set C except CARRY and EMPTY (no
option input; INFERRED).

1. **VS-P01** — "Current prices, returns and histories" (`:162`). Parents: none (`graph :81`).
   Method `VS-P01-RAW-ADJUSTED-HIST63-CAP126@2-D-404` (`crates/h3-asset/src/h4_p01.rs:15`). Code
   `crates/h3-p01/src/lib.rs:449-507`. Reasons (PROVEN at those lines):
   - MISSING_REQUIRED_INPUT: a session's bar is absent (`:449`) [PROVIDER].
   - IDENTITY_CONFLICT: the bar's provider security id differs from T's (`:450`) [PROVIDER].
   - INVALID_REQUIRED_INPUT: an invalid bar field (`:455`) [PROVIDER].
   - NO_PRIOR_HISTORY: no earlier session (`:464`) [HISTORY / STRUCTURAL at listing].
   - WITHHELD_BY_CHALLENGE: a registered challenge covers the return (`:468`) [RULE].
   - INCOMPATIBLE_COMMON_SUPPORT: one endpoint unsupported (`:472`, `:507`) [follows the cause].
   - DEGENERATE_INPUT: non-finite log ratio (`:471`, `:505`) [PROVIDER].
   - INSUFFICIENT_ELIGIBLE: fewer than 95 eligible in CAP126 (`:498`) [HISTORY; floor D-404].
   - ZERO_DENOMINATOR: continuation ratio with a zero prior change (`:506`) [STRUCTURAL].
2. **PATH-L01** — "Gap and intraday decomposition" (`:163`). Parents: P01. Method
   `PATH-L01-RAW-PATH-HCONTEXT5-CAP126@3-D-404` (`crates/h3-asset/src/h4_path.rs:15`). Code
   `crates/h3-path/src/lib.rs`, `result.rs`. Reason strings in the crate: NO_PRIOR_HISTORY,
   ZERO_DENOMINATOR (`lib.rs:148`, `:196`), INSUFFICIENT_ELIGIBLE, DEGENERATE_INPUT,
   INVALID_REQUIRED_INPUT, WITHHELD_BY_CHALLENGE (`result.rs:6`). Conditions INFERRED to mirror P01.
3. **PATH-L02** — "Current raw bar range" (`:164`). Parents: P01. Method
   `PATH-L02-RAW-PATH-HCONTEXT5-CAP126@3-D-404` (`h4_path.rs:27`). Same crate and reasons.
   ZERO_DENOMINATOR for the range coordinate: `crates/h3-path/src/lib.rs:148` (PROVEN line,
   condition not read).
4. **PATH-L03-L07** — "W21 close-path geometry" (`:165`). Seven coordinates: current_drawdown,
   maximum_drawdown, net_to_gross, positive_qv_share, largest_increment_qv_share, qv_time_centroid,
   qv_hhi (`crates/h3-path/src/w21.rs:10-18`). Parents: P01. Method
   `PATH-L03-L07-W21-RAW-PATH-HCONTEXT5-CAP126@3-D-404` (`crates/h3-asset/src/h4_daily_owner.rs:279`).
   Reasons (PROVEN, `w21.rs:258-345`):
   - ZERO_DENOMINATOR: the continuation ratio when the prior 5-session change of the coordinate is
     exactly 0, or a coordinate value is absent with no other reason (`:261-263`, `:326-339`)
     [STRUCTURAL].
   - NO_PRIOR_HISTORY: a reference session is not in the daily history (`:304`) [HISTORY].
   - DEGENERATE_INPUT: non-finite change (`:265`) [PROVIDER].
   - INSUFFICIENT_ELIGIBLE: percentile with fewer than 95 eligible (`:341-343`) [HISTORY].
   - INVALID_REQUIRED_INPUT (`w21.rs:497`, condition not read).
5. **VS-P03** — "Rolling realized-variation estimator family" (`:166`): close-to-close,
   Garman-Klass, Rogers-Satchell, Yang-Zhang over 5, 10, 21, 42 sessions (INFERRED from the method
   label and `crates/h3-p03/src/lib.rs:11-13`). Parents: P01. Method
   `VS-P03-FIVE-ESTIMATOR-W5-W10-W21-W42-NUMPYVAR212@3-D-404` (`h4_daily_owner.rs:122`). Reason
   strings: NO_PRIOR_HISTORY, INVALID_REQUIRED_INPUT, NUMERICAL_DOMAIN, ZERO_DENOMINATOR,
   INSUFFICIENT_ELIGIBLE (`crates/h3-p03/src/production_result.rs:178`, `:354`, `:384-385`).
   Conditions not read. Its `CC.W21` role feeds DLM-HV-CONTEXT and DLM-IV-RV
   (`crates/h5-derived/src/lib.rs:722`, `:996`).
6. **VS-P07** — "Underlying reported activity" (`:167`). Parents: P01. Method
   `VS-P07-RAW-VOLUME-PRIOR63-HIST63@2` (`h4_daily_owner.rs:176`). Reason strings:
   MISSING_REQUIRED_INPUT, INVALID_REQUIRED_INPUT (a present but invalid volume,
   `crates/h3-p01/src/lib.rs:70-71`), NO_PRIOR_HISTORY, ZERO_DENOMINATOR, NUMERICAL_DOMAIN
   (`crates/h3-p07/src/production_result.rs:317-330`). Conditions not read.
7. **SAI-PD01** — "Multi-horizon price displacement" (`:168`). Parents: P01. Method
   `SAI-PD01-ADJUSTED-ROBUST-CAP126@3-D-404` (`h4_daily_owner.rs:224`). Reason strings:
   NO_PRIOR_HISTORY (`crates/h3-pd01/src/production.rs:66`), INVALID_REQUIRED_INPUT (`:76`),
   NUMERICAL_DOMAIN (`:85`), INSUFFICIENT_ELIGIBLE (`:145`), DEGENERATE_INPUT (`:147`),
   MISSING_REQUIRED_INPUT (`:300`). Conditions not read.

### 2.3 VS-O family and VS-L01 (base; partly traced)

Provider inputs: option EOD chain of T after the closing-quote overlay; for the endpoint quartet
(O02, O04, L01, OIJ01) also the chains of the history window, recomputed in the day (intent A).
Open interest dated T is the previous close (`crates/h5-derived/src/chain.rs:13`; D-026). All can
state set C.

1. **VS-O01** — "Delivered lattice topology" (`:169`). Parents: Q01, Q03, Q04 (`graph :88`).
   Method `VS-O01-STRICT-LATTICE-TYPE7@2` (`crates/h3-asset/src/h4_quality.rs:887`). Reasons:
   EMPTY_DELIVERED_POPULATION (`crates/h3-o01/src/result.rs:120`); INSUFFICIENT_ELIGIBLE for a
   strike-gap statistic without enough strikes (`:148-154`, PROVEN line, threshold not read);
   NOT_APPLICABLE cells.
2. **VS-O02** — "Reported OI scale and concentration" (`:170`). Parents: Q01, Q03, Q04. Method
   `VS-O02-CURRENT-NATIVE-CELL-OI@1` (`crates/h3-asset/src/h4_history_endpoint.rs:517`). Code
   `crates/h3-o02/src/history.rs`, `complete_result.rs`. Reasons:
   - PROVIDER_OI_INCOMPLETE: the overlay cleared every row's OI (`crates/h3-o02/src/result.rs:72-73`;
     D-079) [PROVIDER].
   - INSUFFICIENT_ELIGIBLE: a percentile with fewer than 95 eligible of 126 (PROVEN,
     `history.rs:244-247`) [HISTORY; D-404].
   - ZERO_ELIGIBLE_OI_MASS, ZERO_DENOMINATOR (`history.rs:179`, `:536`) [STRUCTURAL, INFERRED].
   - NO_PRIOR_HISTORY (`complete_result.rs:268`), WITHHELD_BY_CHALLENGE (`history.rs:474`),
     SOURCE_CLOCK_AFTER_CUTOFF (`temporal_result.rs:404`), EMPTY_DELIVERED_POPULATION
     (`history.rs:172`). Conditions not read.
3. **VS-O03** — "OI marginal strike and DTE geometry" (`:171`). Parents: O02, P01, Q01, Q03, Q04
   (`graph :90`). Method `VS-O03-WEIGHTED-MARGINALS-CHANGE5@3` (`crates/h3-asset/src/h4_o03.rs:13`).
   Reasons (PROVEN, `crates/h3-o03/src/production.rs:240-272`):
   - EMPTY_DELIVERED_POPULATION: zero rows at T, or at T or T−5 for a change.
   - PROVIDER_OI_INCOMPLETE: the day's OI was cleared (`:243`).
   - INSUFFICIENT_ELIGIBLE: a marginal has no value otherwise (`:245`).
   - WITHHELD_BY_CHALLENGE: a registered challenge covers T−5..T (`:260`, `:265`) [RULE].
   - PROVIDER_INPUT_MISSING: T−5 not delivered (`:262`; D-481).
   - ZERO_DENOMINATOR: a median is not positive at either endpoint (`:268`) [STRUCTURAL].
4. **VS-O04** — "Native-expiry OI allocation" (`:172`). Parents: O02, Q01, Q03, Q04. Method
   `VS-O04-NATIVE-EXPIRY-TV-CAP126@4-D-517` (`crates/h3-asset/src/h4_o04.rs:19`). Reasons (PROVEN
   list, `crates/h3-o04/src/production_result.rs:402-409`): EMPTY_DELIVERED_POPULATION,
   INSUFFICIENT_ELIGIBLE (no native expiry with eligible OI at an endpoint,
   `history_production.rs:73-74`; or percentile under 95 eligible, `:240`), ZERO_DENOMINATOR,
   NO_PRIOR_HISTORY, NO_PERSISTENT_EXPIRY (no expiry common to both endpoints; INFERRED meaning)
   [STRUCTURAL], WITHHELD_BY_CHALLENGE, PROVIDER_OI_INCOMPLETE (`history_production.rs:71`).
5. **VS-OIJ01** — "Joint reported-OI field" (`:173`). Parents: O02, P01, Q01, Q03, Q04. Method
   `VS-OIJ01-JOINT-FIELD@1` (`h4_history_endpoint.rs:959`). Compatibility rule: D-001
   (DECISIONS:120-140). Reasons: INSUFFICIENT_ELIGIBLE on the percentile, five-session TV and path
   efficiency (`crates/h3-oij01/src/history.rs:189`; test at `history_production.rs:687-716`),
   GRID_CHANGED, ENDPOINT_INVALID, NO_PERSISTENT_EXPIRY (`deformation.rs:135-173`),
   NO_ELIGIBLE_REFERENCE (`production_result.rs:552`), PROVIDER_OI_INCOMPLETE,
   ZERO_ELIGIBLE_OI_MASS (`lib.rs:66-68`), NO_PRIOR_HISTORY. Conditions not read.
6. **VS-O06** — "Reported-subset option-volume support" (`:174`), persist-only. Parents: Q01, Q03,
   Q04. Method `VS-O06-REPORTED-VOLUME-SUBSET@4` (`crates/h3-asset/src/h4_option_measure.rs:309`).
   Reasons (PROVEN constants, `crates/h3-o06/src/production_result.rs:86-103`):
   EMPTY_DELIVERED_POPULATION (zero delivered), INSUFFICIENT_ELIGIBLE (delivered but none
   eligible), OPEN_INTEREST_CONTRACTS_NOT_STATED (D-390), PROVIDER_OI_INCOMPLETE,
   PROVIDER_VOLUME_INCOMPLETE (overlay cleared every row's volume, D-079), ZERO_DENOMINATOR
   (reported fraction undefined, `:145-149`).
7. **VS-L01** — "Returned EOD option quote width" (`:175`). Parents: Q01, Q02, Q03, Q04
   (`graph :94`). Method `VS-L01-TYPE7-WIDTH-CONTEXT5@2-D-404` (`crates/h3-asset/src/h4_l01.rs:16`).
   Reasons: INSUFFICIENT_ELIGIBLE on the percentile under 95 eligible (PROVEN,
   `crates/h3-l01/src/history_production.rs:153-155`); NO_PRIOR_HISTORY, ZERO_DENOMINATOR,
   MISSING_REQUIRED_INPUT, EMPTY_DELIVERED_POPULATION, WITHHELD_BY_CHALLENGE
   (`history.rs:169-214`, `history_production.rs:281`). Conditions not read.

### 2.4 VS-V family, INT-IVSC01, SAI-G01

1. **VS-V02** (base; partly traced) — "Provider analytic distributions" (`:176`), persist-only.
   Inputs: the provider's IV and Greeks on the chain. Parents: Q01, Q03, Q04, Q05 (`graph :95`).
   Method `VS-V02-TYPE7-FIELD-DISTRIBUTIONS@1` (`h4_option_measure.rs:405`). Reasons (PROVEN,
   `crates/h3-v02/src/production_result.rs:108-121`): PROVIDER_ANALYTICS_COVERAGE (the field was
   cleared, D-083 B band); ZERO_DENOMINATOR; EMPTY_DELIVERED_POPULATION; INSUFFICIENT_ELIGIBLE. C.
2. **VS-V03-CURRENT** (base; partly traced) — "Selected own-IV geometry" (`:177`). Meaning: on the
   first standard monthly expiry 14 to 365 days out, the median at-the-money σ, the Theil-Sen slope
   in log-moneyness and the residual scale, per right (`crates/h3-v03/src/lib.rs:690-704`;
   `crates/h3-ivsc/src/lib.rs:8`). Inputs: chain plus the day's surface file (its own American σ per
   contract and implied spot; `h3-v03/src/lib.rs:454-460`, `:555-556`). Parents: P01, Q01..Q04,
   VS-IVS01 (`graph :97`). Method `VS-V03-CURRENT-AMERICAN-IV-FIT@3`
   (`crates/h3-asset/src/h4_v03.rs:83`). Reasons:
   - INSUFFICIENT_ELIGIBLE: a role has no value (`crates/h3-v03/src/result.rs:160-167`, PROVEN).
     The fit minimums are: term group at least 3 rows and 1 per side within |k| <= w_t; slope group
     at least 7 rows and 2 per side within |k| <= w_s (`lib.rs:125-126`); windows
     w_t = min(0.20, max(0.025, 2h)), w_s = min(0.60, max(0.10, 5h)) (`lib.rs:111`, constants
     `:45-50`; D-093) [RULE thresholds on PROVIDER quotes].
   - Projection states NO_GLOBAL_MEASURABLE_SLICE and NO_MEASURABLE_RIGHT_SLICE_AT_SELECTED_DTE are
     values of a text role, not reasons (`result.rs:535-539`).
   - NOT_APPLICABLE for σ-not-applicable assets (`result.rs:519`).
   - C-surface: CARRY_NOT_IDENTIFIED when the surface is absent (set C).
3. **INT-IVSC01** (base; fully traced) — "Exact selected-slice comparison classification" (`:178`).
   One role, a 2x3 table of states (right x coordinate) (`crates/h3-ivsc/src/lib.rs:74`, `:241-242`).
   Parents: V03-CURRENT of T and of T−5 (`graph :98`; `lib.rs:65-66`). Method
   `INT-IVSC01-S058-EXACT-FIT-MEMBER@2-D-086` (`crates/h3-asset/src/h4_ivsc.rs:13`). States
   (PROVEN, `lib.rs:110-160`):
   - PRIOR_REFIT_ON_CURRENT_SLICE: T's expiry differs from T−5's, or the windows differ, and the
     T−5 refit on T's slice gave a value and a fit (`:127-137`; D-083, D-093).
   - INSUFFICIENT_SUPPORT: same slice, other windows and no refit (`:139-141`); or a fit membership
     is absent at T or T−5 (`:142-144`); or the bindings are not compatible, or a value, expiry or
     right is absent at either endpoint (`:145-154`).
   - CHANGED_SELECTED_SLICE: expiry or right differs and no refit (`:155-157`).
   - SAME_SLICE_CHANGED_FIT_SUPPORT / SAME_SLICE_SAME_FIT_SUPPORT (`:158-160`).
   - WITHHELD_BY_CHALLENGE: a corporate-action ex-date in [T−5, T] (`:183-192`) [RULE].
   Slot reasons: C-surface.
4. **VS-V03-MATCHED-NATIVE** (base; fully traced) — "Matched selected-IV changes" (`:179`). Six
   roles: per right, log ratio of the term median IV, log ratio of the residual scale, difference of
   the slope (`crates/h3-v03-matched/src/lib.rs:8-13`, `:157-193`). Parents: V03-CURRENT (T and
   T−5), IVSC01 (`graph :99`). Method `VS-V03-MATCHED-NATIVE-S058-TWO-ENDPOINT@3-D-400`
   (`crates/h3-asset/src/h4_v03_matched.rs:14`). Reasons (PROVEN, `lib.rs:70-101`):
   - CHANGED_SELECTED_SLICE: the IVSC cell says so [STRUCTURAL; D-029, D-083].
   - WITHHELD_BY_CHALLENGE: the IVSC cell says so [RULE].
   - INSUFFICIENT_ELIGIBLE: the IVSC cell is INSUFFICIENT_SUPPORT (`:80`) [see IVSC conditions].
   - INVALID_REQUIRED_INPUT: a log-ratio endpoint is not positive (`:90-92`) [PROVIDER / STRUCTURAL].
   - INCOMPATIBLE_COMMON_SUPPORT is accepted by the validator (`:369`); no site that assigns it was
     found in this file (INFERRED: retained bodies only).
   Slot reasons: C-surface.
5. **SAI-G01** (base; partly traced) — "Provider-gamma-weighted OI geometry" (`:180`), internal
   (D-101 ruling 2). Inputs: the provider's gamma and reported OI. Parents: O02, Q01, Q03, Q04, Q05
   (`graph :100`). Method `SAI-G01-NATIVE-GAMMA-OI-FIELD@1` (`crates/h3-asset/src/h4_g01.rs:13`).
   Cell reasons (PROVEN, `crates/h3-g01/src/result.rs:160-188`): INSUFFICIENT_ELIGIBLE (gamma zero,
   underflow, or global support failed), MISSING_REQUIRED_INPUT (gamma missing),
   INVALID_REQUIRED_INPUT (gamma invalid), NOT_APPLICABLE. Also PROVIDER_ANALYTIC_UNRESOLVED
   (`result.rs:119`), PROVIDER_OI_INCOMPLETE, PROVIDER_ANALYTICS_COVERAGE (`lib.rs:66-67`),
   ZERO_DENOMINATOR (`result.rs:320`). C.

### 2.5 VS-IVS01 (base; fully traced for its body, partly for the fit)

- Title "Implied-volatility surface (American)" (`sensors lib.rs:233`).
- Inputs: chains and closing quotes, carry inputs (category `carry`), the rate curve and T−5's
  macro (category `macro`; D-239), daily bars for dividends (`graph lib.rs:466-469`). The Treasury
  curve is the Fed's H.15 for now (Rules in force 10, DECISIONS:42-45).
- Parents: none in the graph (`:154`). It is built by `h5_surface` before intent B binds it.
- Method `H5-SURFACE@26` (`crates/h3-iv-surface/src/stage.rs:587`); body method text
  `IV-SURFACE v0.2 am_full (D-084/D-086/D-095)` (`body.rs:117`).
- Fit window: expiries 7 to 200 days (`crates/h3-iv-surface/src/lib.rs:45-46`).
- 11 roles (`body.rs:55-67`). Reasons (PROVEN, `body.rs:38-67`, `:70-113`):
  - NO_SURFACE_FIT: no fit and the quotes could have supported one [ENGINE, stated `body.rs:104`].
  - INSUFFICIENT_SURFACE_QUOTES: fewer than three two-sided quotes in the 7-200-day window, or none
    on one side of the money (`lib.rs:551-554`, `:575`) [PROVIDER, stated `body.rs:105`; three is a
    RULE minimum].
  - NO_30_DAY_BRACKET: the fit has no 30-day reading (`body.rs:41`, `:52`, `:58`) [PROVIDER].
  - NO_STANDARD_ERROR: IV30 exists but is not identified (`:41`, `:47-51`) [ENGINE / PROVIDER,
    not established which].
  - IV30_NOT_IDENTIFIED: IV60, IV90 and the term ratio when the 30-day level is not identified
    (`:43-45`; D-086) [RULE].
  - NO_60_DAY_BRACKET, NO_90_DAY_BRACKET (`:56-59`) [PROVIDER].
  - EMPTY_DELIVERED_POPULATION on every role (`:72-80`; D-117).
  - PROVIDER_INPUT_MISSING, ENGINE_FAULT, PROVIDER_COVERAGE_START on every role
    (`:84-98`; `crates/vyreon-cli/src/exports.rs:508-511`, `:910-921`).
  - CARRY_NOT_IDENTIFIED on every role: no expiry of the chain identifies a residual yield
    (`crates/h3-iv-surface/src/bin/h5_surface.rs:346-362`; `stage.rs:199-204`; D-185).
- Also inside the stage, per expiry: an expiry whose forward lies outside its quotes' no-arbitrage
  bounds is withheld (no σ, no Greeks) (`stage.rs:211-215`; D-560) [RULE].

### 2.6 M0 family (base; partly traced)

Inputs: chain and closing quotes, the day's surface file (carry, dividends, implied spot). All can
state C-surface.

1. **M0-ANALYTICS** — "Own IV and Greeks engine" (`:221`). Parents: Q01..Q04, VS-IVS01
   (`graph :141`; no macro parent, D-106 item 2.2, `catalogue.rs:626-627`). Method
   `M0-ANALYTICS@1.2.0` (`crates/h3-asset/src/h4_run.rs:4066`). Code `crates/h3-m0-analytics/`,
   `crates/h3-asset/src/h4_m0_analytics.rs`. Per-contract inversion states (INFERRED from literals,
   `h3-m0-analytics/src/american.rs:62-67`, `trace.rs:55-107`, `american_kernel.rs:64-66`):
   NO_TIME_VALUE, BELOW_DOMAIN, ABOVE_DOMAIN, UNIDENTIFIED_STYLE, LOW_VEGA,
   NUMERICAL_NONCONVERGENCE, OUTSIDE_SOLVER_DOMAIN, NO_MID, NOT_INVERTED, INVALID_INPUT,
   INVALID_REQUIRED_INPUT, NUMERICAL_DOMAIN, MODEL_INPUT_UNAVAILABLE, MODEL_INPUT_OUTSIDE_DOMAIN,
   MODEL_SENSITIVITY_INCOMPLETE. Reading a carry that was not identified is an error
   (`american.rs:89`).
2. **M0-IV-PROFILE** — "All-native-expiry IV profiles" (`:222`). Parents: M0-ANALYTICS,
   V03-CURRENT (`graph :142`). Method `M0-IV-PROFILE@1.1.0` (`h4_run.rs:4068`). Reason strings:
   CENTRAL_FIT_UNAVAILABLE, DEGENERATE_COORDINATE, EXPIRES_AT_THE_CLOSE
   (`crates/h3-m0-analytics/src/iv_profile_groups.rs:104`, `:319`, `:405`),
   CHANGED_RECONSTRUCTION_SUPPORT (`iv_profile.rs:577`). Conditions not read.
3. **M0-IV-CHANGE** — "Reconstructed same-member IV changes" (`:223`). Parents: M0-IV-PROFILE
   (T and T−5; `graph :143`). Method `M0-IV-CHANGE@1.1.0` (`h4_run.rs:4069`). Reason strings:
   INSUFFICIENT_ELIGIBLE (`crates/h3-m0-analytics/src/iv_change.rs:654`), DIFFERENT_RATE_BRANCH
   (`:469`). Rule history: D-003, D-043 (DECISIONS:163-172). Conditions not read.
4. **M0-GAMMA** — "Own-model gamma-weighted OI field" (`:224`). Parents: Q01, M0-ANALYTICS
   (`graph :144`). Method `M0-GAMMA@1.3.0` (`h4_run.rs:4070`). Reason strings
   (`crates/h3-m0-gamma/src/store/projection.rs:77-472`): INSUFFICIENT_ELIGIBLE, ZERO_DENOMINATOR
   (zero total weight; read by DLM-GAMMA-EXPOSURE as "no positive cells",
   `crates/h5-derived/src/lib.rs:687`), PROVIDER_OI_INCOMPLETE, MISSING/INVALID_REQUIRED_INPUT,
   NUMERICAL_DOMAIN, EMPTY_DELIVERED_POPULATION. Conditions not read.
5. **M0-GAMMA-CHANGE** — "Reconstructed gamma-field deformation" (`:225`). Parents: M0-GAMMA.
   Method `M0-GAMMA-CHANGE@1.2.0` (`h4_run.rs:4071`). Reason strings
   (`crates/h3-asset/src/h4_m0_gamma_change.rs:265-268`, `:432`): NUMERICAL_DOMAIN,
   WITHHELD_BY_CHALLENGE, INCOMPATIBLE_COMMON_SUPPORT, PROVIDER_OI_INCOMPLETE. Rule: D-003, D-042.
6. **M0-PRINT-SIZE** — "Returned selected-contract print sizes" (`:226`): p10, p25, median, p75,
   p90, IQR, MAD and two ratios of reported print sizes (`crates/h3-m0-print-size/src/lib.rs:14-23`).
   Inputs: the EXQ01 cohort prints. Parents: SAI-EXQ01 (`graph :146`). Method `M0-PRINT-SIZE@1.2.0`
   (`crates/h3-asset/src/h4_m0_print_size.rs:67`). Reasons: C (a fault of SAI-EXQ01 arrives as
   PARENT_ENGINE_FAULT). Own value-level reasons not read.
7. **M0-FIELD-ANALYTICS** — "Homogeneous own-IV field" (`:227`), one slot on FIELD.CORE64. Parents:
   P01, O02, L01, V02, O06, M0-ANALYTICS, M0-IV-PROFILE, M0-GAMMA of the 64 (`graph :63`, `:147`).
   Code `crates/h5-field/src/option_field.rs`, `exports.rs`. Reason list at
   `option_field.rs:36-68` (see FIELD-OPTBR01). Conditions not read.

### 2.7 Replay family

Provider inputs for all: the consolidated replay tapes of T (Intrinio `get_security_replay_file`,
all three SIP tapes; `crates/h3-interval-source/src/consolidated.rs:1`, `:93`; category
`consolidated`) and the official daily bar of T (`stock_prices`, as traded;
`crates/h3-replay/src/official.rs:12-14`). EVC01 also reads up to 252 history sessions of tape
(`crates/vyreon-cli/src/fill.rs:105-106`).

1. **D111-TEMP01** (base; fully traced) — "Intraday grid and family-local support" (`:186`).
   Meaning: for each 15-minute bin, whether OHLC, volume, trade count and quote are supported, and
   the same for adjacent pairs (`crates/h3-temporal/src/lib.rs:20-31`, `:234-338`). Parents: none
   (`graph :106`). Method `D111-TEMP01-FOUR-FAMILY-GRID@12` (`crates/h3-asset/src/h4_d111.rs:30`).
   Read rule `VYREON-REPLAY-READ@10` (`official.rs:56`); tape method
   `VYREON-REPLAY-CONSOLIDATED@14` (`consolidated.rs:70`). Reasons: set T per cell, plus the three
   bin-cell reasons of 0.4; slot: C.
2. **INT-TEMP-IP01** (base; partly traced) — "Temporal preservation registry control" (`:188`).
   Parents: none. Method `INT-TEMP-IP01-FROZEN-REGISTRY@2` (`crates/h3-asset/src/h4_ip01.rs:13`).
   State PASS or CONTROL_FAILED when the registry identity does not match the active one (PROVEN,
   `crates/h3-temporal/src/ip01.rs:244-248`) [ENGINE]. It reads no provider input (INFERRED).
3. **PCR-UPATH-I01** (base; fully traced for gates) — "Sampled 15-minute price path" (`:183`).
   Parents: D111, IP01 (`graph :103`). Method `PCR-UPATH-I01-SAMPLED-PATH@15`
   (`crates/h3-asset/src/h4_interval_measure.rs:100`). Code `crates/h3-temporal/src/upath.rs`,
   `upath_result.rs`. A role states the reason of the first unsupported price cell
   (`upath_result.rs:196-202`). Reasons: every price-gate reason of set T (TAPE_LATE_START,
   TAPE_TRUNCATED, TAPE_GAP, TAPE_CORRUPT_SEGMENT, TAPE_UNAVAILABLE,
   TAPE_INCOMPLETE_AGAINST_OFFICIAL_BAR, TAPE_DIFFERS_FROM_OFFICIAL_BAR, OFFICIAL_BAR_UNAVAILABLE,
   TAPE_LISTING_RESTATED), the bin-cell reasons, and C. D-411 says gate 2 withholds "UPATH (every
   bin and summary)" (DECISIONS:16494).
4. **PCR-UACT-I01** (base; fully traced) — "Separate interval volume and trade-count activity"
   (`:184`). Ten summary roles: for volume and for trade count, the total, opening-hour share,
   closing-hour share, largest-bin share and time centroid (`crates/h3-temporal/src/uact.rs:17-31`).
   Parents: D111, IP01 (`graph :104`). Method `PCR-UACT-I01-ACTIVITY@8`
   (`h4_interval_measure.rs:143`). A family's five summaries need every bin of the session
   supported (`uact.rs:131-144`, `:145-162`). The role states the first unsupported bin's reason
   (`crates/h3-temporal/src/uact_result.rs:232-236`). Reasons:
   - Every volume-gate reason of set T, including SOURCE_VOLUME_RECONCILIATION_FAILED (a day-level
     verdict copied to every bin, `consolidated.rs:470`, `:500`) [RULE 2% + PROVIDER].
   - MISSING_REQUIRED_INPUT when no cell states a reason (`uact_result.rs:236`).
   - ZERO_DENOMINATOR: the shares when the total is zero (`:238-244`; `uact.rs:42-45`) [STRUCTURAL].
   - C.
5. **PCR-UQTR-I01** (base; fully traced for gates) — "Returned interval-close quote widths"
   (`:185`). Parents: D111, IP01 (`graph :105`). Method `PCR-UQTR-I01-QUOTE-WIDTH@9`
   (`h4_interval_measure.rs:183`). Code `crates/h3-temporal/src/uqtr.rs`, `uqtr_result.rs`.
   Reasons: every quote-gate reason of set T (TAPE_LATE_START, TAPE_TRUNCATED, TAPE_GAP,
   QUOTE_BOOK_ONE_SIDED, QUOTE_BOOK_CROSSED, QUOTE_BOOK_LOCKED, QUOTE_COMPLETENESS_UNVERIFIED), the
   bin-cell reasons, and C. Gate 3 withholds "UQTR (bid, ask, width and its summaries)"
   (DECISIONS:16505-16506).
6. **INT-TEMP-EVC01** (base; partly traced) — "Strictly-prior same-slot events" (`:187`). Meaning
   (INFERRED): per 15-minute slot and per measure (three measures, 26 bins,
   `crates/h3-temporal/src/evc01.rs:10-15`), today's value against the same slot of prior sessions.
   Parents: D111, IP01, UPATH, UACT, UQTR (`graph :107`). Method `INT-TEMP-EVC01-TYPE7-Q90@17`
   (`crates/h3-asset/src/h4_evc01.rs:26`). Constants: baseline window 252 sessions, minimum 189
   (`evc01.rs:83-94`). Reasons: INSUFFICIENT_ELIGIBLE when fewer than 189 eligible priors
   (`evc01.rs:93-94`, `:434`) [HISTORY; D-404]; NO_PRIOR_HISTORY (`evc01_result.rs:46`);
   TAPE_UNAVAILABLE (`evc01_result.rs:429`); every set T reason passed through from the current
   bin (real test `crates/h3-temporal/tests/evc01_real.rs:167-171`); C.

### 2.8 SAI-ACT family (base; partly traced)

Provider input: per-contract option interval pulls for the ACT01 cohort (category `act01`; "the
provider's first per-contract option interval", `crates/vyreon-cli/src/plan.rs:74`; trade counts
start later, `plan.rs:104`, D-237). The cohort is selected at T−5 (`crates/vyreon-cli/src/day_inputs.rs:110`).

1. **SAI-ACT01** — "Certified option-activity summary" (`:181`). Parents: P01, IP01 (`graph :101`).
   Method `SAI-ACT01-CERTIFIED-COHORT-30M-SAME-COHORT-TV@3` (`crates/h3-asset/src/h4_act01.rs:18`).
   Reasons:
   - MISSING_REQUIRED_INPUT: no count was delivered for the reading (`crates/h3-act01/src/lib.rs:106`,
     `:245-254`, `:296`, `:315`, `:330`, `:385`; D-237: rows without a trade count are unknown, never
     zero, `lib.rs:122`, `:152`) [PROVIDER].
   - INVALID_REQUIRED_INPUT: an invalid joint ratio (`lib.rs:255-256`) [PROVIDER].
   - ZERO_DENOMINATOR: no qualifying trade (`lib.rs:108`, `:257-258`, `:298`) [STRUCTURAL].
   - NO_GOVERNED_COHORT_CERTIFICATE: no cohort for the asset (`crates/h3-act01/src/result.rs:240`)
     [STRUCTURAL / PROVIDER, not established which].
   - EMPTY_DELIVERED_POPULATION (`result.rs:278`). C.
2. **SAI-ACT01-INTRADAY** — "Certified activity over time" (`:189`). Parents: SAI-ACT01, IP01
   (`graph :109`). Code `crates/h3-act01/src/intraday.rs`. Reasons: MISSING_REQUIRED_INPUT (a count
   not delivered or a session-long outage), ZERO_DENOMINATOR (no qualifying trade),
   INVALID_REQUIRED_INPUT (`intraday.rs:85-138`, `:256-257`, `:338-346`),
   NO_GOVERNED_COHORT_CERTIFICATE (`:564-605`), EMPTY_DELIVERED_POPULATION (`:633-639`). C.
3. **INT-ACT-TV15** — "Internal certified-cadence sensitivity" (`:190`), persist-only. Parents:
   ACT01-INTRADAY, IP01 (`graph :110`). Method `INT-ACT-TV15-COMMON-SUPPORT-TWO-CADENCE@4`
   (`crates/h3-asset/src/h4_activity_view.rs:154`). Reason strings: INSUFFICIENT_ELIGIBLE
   (`crates/h3-act01/src/tv15.rs:438`), MISSING_REQUIRED_INPUT. Conditions not read. C.

### 2.9 SAI-EXQ family (base; partly traced)

Provider input: the option prints of the ACT01 cohort (category `exq01`). The provider keeps a
session's prints for about five sessions (`crates/vyreon-cli/src/fill.rs:59-61`; D-103); later they
exist only in the bucket archive, which no fill reads back (`graph lib.rs:368-372`; D-360). The
family also needs the `act01` category (`graph lib.rs:480-482`).

1. **SAI-EXQ01** — "Execution relative to same-row returned quotes" (`:182`). Parents: none in the
   graph (`:102`). Method `SAI-EXQ01-CROSS-SECTIONAL-ARITHMETIC@3`
   (`crates/h3-asset/src/exq01_present_result.rs:58`). Reasons (PROVEN, `:190-203`, `:55`):
   - EMPTY_DELIVERED_POPULATION: zero delivered events.
   - ZERO_DENOMINATOR: coordinate 3 with eligible events but a zero denominator [STRUCTURAL].
   - INSUFFICIENT_ELIGIBLE: no eligible event otherwise.
   - EVENT_SOURCE_NOT_RETAINED: a cohort exists but its prints were not captured while the provider
     kept them (`:53-55`) [PROVIDER, stated].
   - NO_GOVERNED_EXECUTION_SCOPE_CERTIFICATE: no cohort (`crates/h3-asset/src/h4_exq01_none.rs:388`).
   - C.
2. **SAI-EXQ01-TEMPORAL** — "Private event-order preservation" (`:191`). Parents: SAI-EXQ01, IP01
   (`graph :111`). Method `SAI-EXQ01-TEMPORAL-PRIVATE-PRESERVATION@3`
   (`crates/h3-asset/src/h4_exq01_temporal.rs:10`). Reasons (PROVEN list, `:201-210`):
   NO_GOVERNED_EXECUTION_SCOPE_CERTIFICATE, EVENT_SOURCE_NOT_RETAINED, EMPTY_DELIVERED_POPULATION,
   INSUFFICIENT_ELIGIBLE. C.

### 2.10 CDD company family (base; partly traced; 17 companies)

Provider input: the company capture of T (category `company`: fundamentals as reported and
standardized, daily metrics, public float, expected earnings, SEC filings, insider filings;
`crates/h5-company/src/source.rs:694-716`, `:1061-1062`, `:1385-1466`) plus the companies' daily
bars for splits and closes (`graph lib.rs:471-475`; D-188, D-259). Parents: none
(`graph :112-116`). Company export schema `H5-COMPANY@5` (`crates/vyreon-cli/src/exports.rs:91`).
Core keeps the capture because the provider restates it in place (`graph lib.rs:364-366`; D-306).

1. **CDD-REF01-02** — "Effective company binding" (`:192`). Code
   `crates/h5-company/src/science/ref01.rs`. Strings: EXACT_SINGLE_EFFECTIVE_BINDING (`:168`),
   NO_RETAINED_RECORD (`:98`), IDENTITY_CONFLICT (`science/mod.rs:438`). Conditions not read.
2. **CDD-FND01** — "As-reported fiscal state" (`:193`). Code `crates/h5-company/src/science/fnd.rs`
   (4,124 lines; not read). Reason strings: MISSING_REQUIRED_INPUT (`:3421`),
   INVALID_REQUIRED_INPUT (`:3539`), INSUFFICIENT_ELIGIBLE (`:2264`), REPRESENTATION_INCOMPATIBLE
   (`:3198`), REPRESENTATIONS_DISAGREE (`:2702`), UNKNOWN_CLOCK (`:2209`), ZERO_DENOMINATOR.
3. **CDD-FND02** — "Provider-standardized fiscal companion" (`:194`). Same file. Strings as FND01
   (not separated by sensor in this trace).
4. **CDD-CAPVAL01** — "Provider capital and valuation context" (`:195`). Code
   `crates/h5-company/src/science/capval.rs`. Strings: PROVIDER_VALUE_ABSENT (`:65`),
   RECEIPT_RATIO_NOT_WHOLE (`:68`), DEPOSITARY_RECEIPT_RATIO_NOT_ESTABLISHED (`:26`),
   SECURITY_CURRENCY_NOT_PROVED (`:34`), FUNDAMENTALS_CURRENCY_DIFFERS_FROM_SECURITY_CURRENCY
   (`:38`), COMMON_EQUITY_NOT_POSITIVE (`:39`), SESSION_CLOSE_NOT_READ (`:33`),
   MISSING/INVALID_REQUIRED_INPUT. Conditions not read.
5. **CDD-EVENTS01** — "Typed corporate-event context" (`:196`). Code
   `crates/h5-company/src/science/events.rs`. Strings: CONTROL_FAILED (`:768`),
   MISSING/INVALID_REQUIRED_INPUT, PROVIDER_BINDING_UNRESOLVED and MISSING_REQUIRED_INPUT on the
   expected-earnings date (seen through DLM-EVENT-WINDOW's test,
   `crates/vyreon-cli/src/derive/local.rs:652-673`). The expected date is the provider's forecast as
   captured, not as of T (`events.rs:61`; D-305).

All five can state ENGINE_FAULT, PARENT_ENGINE_FAULT, PROVIDER_INPUT_MISSING,
PROVIDER_COVERAGE_START (set C without the option reasons; INFERRED).

### 2.11 CDD-MACRO01 (base; fully traced)

- Title "Three-series current-vintage macro context" (`:197`). One slot per asset, 64 slots, same
  content (`crates/h3-asset/src/macro01_production.rs:139-246`).
- Inputs: category `macro`: Intrinio economic indices CPIAUCNS (`ind_ogDOny`), FEDFUNDS
  (`ind_pgV15y`), INDPRO (`ind_NgY1Zg`), each with metadata, full history pages and a terminal probe
  (`crates/h3-macro/src/lib.rs:25-38`; `source_capture.rs:160-177`).
- Parents: none (`graph :117`). Method `CDD-MACRO01-REVISED-HISTORY-LATEST24@3`
  (`crates/h3-asset/src/h4_macro.rs:12`; D-508).
- 20 roles: per series value, observation_date, native_unit, frequency, seasonal_adjustment (15),
  a history role per series (3), and INDPRO's vintage_basis and vintage_captured_at (2)
  (`macro01_production.rs:163`, `:190-222`, `:120-133`).
- Point in time: an observation counts only from its published release date (table
  `crates/h3-macro/src/lib.rs:97-99`; D-079, D-081, D-380; FEDFUNDS D-381).
- Reasons (PROVEN, `macro01_production.rs:82-113`, `:153-222`;
  `crates/h3-macro/src/source_capture.rs:536-568`; `lib.rs:313-336`):
  - MISSING_REQUIRED_INPUT: the series' support state is WITHHELD_NO_OBSERVATION: metadata matched
    but no observation is readable on T (`macro01_production.rs:105-106`;
    `source_capture.rs:546-548`) [PROVIDER / STRUCTURAL].
  - INVALID_REQUIRED_INPUT: any other non-supported state (`:107-108`). The states are:
    WITHHELD_NOT_DELIVERED (the capture could not read the series in its documented shape,
    `source_capture.rs:178-183`; `bin/capture_source.rs:479-481`; D-297) and the five metadata
    gates WITHHELD_WRONG_IDENTITY, _UNIT, _FREQUENCY, _SEASONAL_ADJUSTMENT, _INVALID_START
    (`lib.rs:278-284`, `:323-338`; `source_capture.rs:370-374`) [PROVIDER; the exact-match metadata
    gate is a RULE].
  - INDPRO's stale unit label ("Index 2012=100") is overridden only when the twelve 2017 values
    average within 0.5 of 100 (`lib.rs:287-306`; D-044). Since D-508 the override re-checks
    frequency, seasonal adjustment and start (`lib.rs:309-318`; DECISIONS:21198-21200).
  - A whole-body error (not a withholding) for a wrong source identity, wrong series count or
    order, or a selection error other than "no observation" (`macro01_production.rs:146-169`;
    `source_capture.rs:549`). That becomes ENGINE_FAULT for the slot (INFERRED).
  - ENGINE_FAULT, PARENT_ENGINE_FAULT, PROVIDER_INPUT_MISSING, PROVIDER_COVERAGE_START,
    CARRY_NOT_IDENTIFIED on all 20 roles of a faulted slot (`macro01_production.rs:252-292`,
    test `:692-696`).

### 2.12 FIELD family (base; partly traced; one object FIELD.CORE64)

Export schema `H5-FIELD@3` (`crates/vyreon-cli/src/exports.rs:44`; D-509) and `H5-FIELD-OPTIONS@3`
(`exports.rs:60`). Daily panel input: VS-P01's panel of the 64 (daily bars). Intraday panel input:
D111's quote midpoints (`graph lib.rs:149-153`; D-085); a midpoint stays usable under
QUOTE_COMPLETENESS_UNVERIFIED (`crates/h3-replay/src/official.rs:448-452`). Context windows:
CAP126 with floor 95 (`crates/h5-field/src/context.rs:15-16`); FIELD-DEP01's 250-session window
with floor 188 (`crates/h5-derived/src/technical.rs:101`); intraday windows 63 sessions
(`crates/h5-field/src/intraday.rs:33-35`).

Shared reasons seen (conditions not read except where cited): INSUFFICIENT_ELIGIBLE (a population
statistic without enough eligible members, `daily.rs:55`, `:104`, `:183`; a percentile under 95
eligible, `context.rs:65-70`), NO_PRIOR_HISTORY (`context.rs:68`, `:78`; `daily.rs:279`;
`intraday.rs:222-230`, `:327`), MISSING_REQUIRED_INPUT (`panel.rs:39`), TAPE_UNAVAILABLE (every
security's capture withheld whole, `intraday.rs:520-530`), NOT_APPLICABLE (self-pairs,
`intraday.rs:230`), CONTROL_FAILED, DEGENERATE_INPUT, INCOMPATIBLE_COMMON_SUPPORT,
ZERO_DENOMINATOR, ENGINE_FAULT (`daily.rs:12-13`, `intraday.rs:26`).

| Id | Title (`sensors lib.rs` line) | Parents (`graph lib.rs` line) |
|---|---|---|
| FIELD-SUP01 | Coordinate-local field support tensor (`:208`) | P01, O02, L01, V02, O06, M0-ANALYTICS, M0-IV-PROFILE, M0-GAMMA (`:128`) |
| FIELD-BR01 | Cross-security signed breadth (`:209`) | P01 (`:129`) |
| FIELD-DIST01 | Cross-sectional return distribution (`:210`) | P01 (`:130`) |
| FIELD-XCONC01 | Absolute-return contribution concentration (`:211`) | P01 (`:131`) |
| FIELD-SYNC01 | Complete-window return synchronization (`:212`) | P01 (`:132`) |
| FIELD-DEP01 | Pairwise dependence and support (`:213`) | P01 (`:133`); read by DLM-BETA (`:185`) |
| FIELD-GRP01 | Frozen-label dependence comparison (`:214`) | P01 (`:134`) |
| FIELD-RMT01 | Empirical correlation spectrum (`:215`) | P01 (`:135`) |
| FIELD-SUB01 | Qualified leading-projector stability (`:216`) | P01 (`:136`) |
| FIELD-DIM01 | Spectrum-derived dimension summaries (`:217`) | P01 (`:137`) |
| FIELD-OPTBR01 | Native option-field support distributions (`:218`) | same eight as SUP01 (`:138`) |
| FIELD-MHDEP01 | Market-hours dependence, 15-minute midpoint correlations and betas (`:229`) | P01, D111 (`:150`) |
| FIELD-FDSYNC01 | Full-day synchronisation (`:230`) | P01, D111 (`:151`) |
| FIELD-FDRMT01 | Full-day random-matrix spectrum (`:231`) | P01, D111 (`:152`) |
| FIELD-FDDIM01 | Full-day effective dimension (`:232`) | P01, D111 (`:153`) |

FIELD-option owners (SUP01, OPTBR01, M0-FIELD-ANALYTICS) pass through each member's reason from a
fixed list (`crates/h5-field/src/option_field.rs:36-68`): REPRESENTATION_INCOMPATIBLE,
NO_CANONICAL_HEAD, CONTROL_FAILED, PROVIDER_BINDING_UNRESOLVED, NO_CALENDAR_AUTHORITY,
NO_GOVERNED_COHORT_CERTIFICATE, NO_GOVERNED_EXECUTION_SCOPE_CERTIFICATE, IDENTITY_CONFLICT,
INCOMPLETE_PAGINATION, WITHHELD_BY_CHALLENGE, CHANGED_SELECTED_SLICE, REPRESENTATION_CHANGED,
UNKNOWN_CLOCK, CLOCK_INCOMPATIBLE, INCOMPATIBLE_COMMON_SUPPORT, NO_PRIOR_HISTORY,
EMPTY_DELIVERED_POPULATION, MISSING/INVALID_REQUIRED_INPUT, INSUFFICIENT_ELIGIBLE,
DEGENERATE_INPUT, ZERO_DENOMINATOR. Which role states which was not traced.

### 2.13 DLM day-local (18 ids)

All are built in `crates/vyreon-cli/src/derive/local.rs` from the base bodies of T and T's input
files. Whole-record containment (PROVEN, `derive/local.rs:396-440`):

- The chain's population or the spot is withheld: every record that reads the chain is withheld
  DEPENDENCY_WITHHELD with that cause (`:404-411`).
- The surface is withheld: the Greek records are withheld whole; the expected move loses only the
  legs that read the surface (`:437-440`; `crates/h5-derived/src/lib.rs:1473-1495`; D-461).
- The chain failed to load: ENGINE_FAULT (`:416-417`). Delivered empty:
  EMPTY_DELIVERED_POPULATION (`:418-420`).
- A whole-record withholding keeps the declaration roles and withholds every other role
  (`h5-derived/src/lib.rs:1503-1524`).

Chain records read "the admitted population of the published day": the option chain file named by
VS-O02's population root, or VS-O06's (`graph lib.rs:166`; D-403). Provider fields cleared by the
overlay give PROVIDER_VOLUME_INCOMPLETE (ZERO-DTE, UNUSUAL) and PROVIDER_OI_INCOMPLETE (MAX-PAIN,
UNUSUAL, CENTROID) on every numeric role but spot (`h5-derived/src/lib.rs:903-926`; D-079).

1. **DLM-MAX-PAIN** (fully traced) — the strike at which option holders are paid least at expiry,
   for the front and the monthly expiry. Inputs: chain (reported OI), close. Parents: O02, O06, P01
   (`graph :167`). Method `DLM-MAX-PAIN@1.5.0` (`h5-derived/src/lib.rs:825`). Code `lib.rs:809-825`,
   `chain.rs:87`. Reasons: NO_EXPIRY_OPEN_INTEREST (the expiry has no positive OI total, or none
   reported; `:813-818`) [STRUCTURAL / PROVIDER]; NO_LISTED_EXPIRY (`:821`) [STRUCTURAL];
   PROVIDER_OI_INCOMPLETE; containment.
2. **DLM-ZERO-DTE** (fully traced) — share of the session's option volume in contracts expiring
   today. Parents: O02, O06, P01. Method `DLM-ZERO-DTE@1.2.0` (`:830`). Reasons: ZERO_TOTAL_VOLUME
   (`:832`) [STRUCTURAL]; PROVIDER_VOLUME_INCOMPLETE (every volume cleared, or a contract expiring
   on T has unknown volume, `:907-912`) [PROVIDER]; containment.
3. **DLM-UNUSUAL-CONTRACTS** (fully traced) — contracts expiring after today whose volume exceeded
   their start-of-day OI, with at least 100 contracts (`chain.rs:127`, `:160-164`). Parents: O02,
   O06, P01. Method `DLM-UNUSUAL-CONTRACTS@2.2.0` (`:761`). Reasons: NO_FLAGGED_CONTRACTS with state
   NOT_APPLICABLE when nothing is flagged (the shares are 0/0; `:772`, `:785-788`; D-131)
   [STRUCTURAL]; ZERO_POST_SESSION_EXPIRY_VOLUME (`:776`) [STRUCTURAL];
   PROVIDER_VOLUME_INCOMPLETE; PROVIDER_OI_INCOMPLETE; containment.
4. **DLM-QUOTE-FRICTION** (fully traced) — median and p90 relative spread of near-the-money
   (within 5%), 7-45-day standard contracts (`chain.rs:205-208`). Parents: O02, O06, P01. Method
   `DLM-QUOTE-FRICTION@1.1.0` (`:842`). Reasons: FEWER_THAN_3_QUOTED_CONTRACTS (`:843-844`,
   constant `chain.rs:130`) [RULE minimum 3 on PROVIDER quotes]; containment.
5. **DLM-OI-CENTROID** (fully traced) — OI-weighted average log-moneyness for calls, puts, all;
   width; mean DTE. Parents: O02, O06, P01. Method `DLM-OI-CENTROID@1.2.0` (`:853`). Reasons:
   NO_OPEN_INTEREST (`:851-860`) [STRUCTURAL]; PROVIDER_OI_INCOMPLETE; containment.
6. **DLM-PREMIUM-YIELD** (fully traced) — put and call premium yields at the money, 5% and 10% out,
   on the expiry nearest 30 days and at least 7 days out (`chain.rs:457-458`). Parents: O02, O06,
   P01 (`graph :178`). Method `DLM-PREMIUM-YIELD@1.4.0` (`:900`). Code `lib.rs:1720-1752`. Reasons:
   NO_QUOTED_STRIKES_BRACKETING_TARGET: no two quoted strikes bracket the target, no extrapolation
   (`:1741`; `chain.rs:417-421`) [PROVIDER / STRUCTURAL]; NO_LISTED_EXPIRY (`:1730-1741`);
   containment.
7. **DLM-EXPECTED-MOVE** (fully traced) — the at-the-money straddle cost as a share of the close,
   for the front expiry and the expiry nearest 30 days. Inputs: chain quotes, close, surface file.
   Parents: O02, O06, VS-IVS01, P01 (`graph :172`). Method `DLM-EXPECTED-MOVE@2.5.0` (`:902`). Code
   `lib.rs:866-902`, `:1378-1440`. Reasons:
   - NO_QUOTED_STRADDLE_NEAR_SPOT: no quoted straddle at the close, between bracketing strikes, or
     at the nearest quoted strike within 2% (`:1398`; `chain.rs:258-266`) [PROVIDER].
   - WIDE_STRADDLE_QUOTE: the straddle's half-spread exceeds 25% of its cost
     (`chain.rs:387-390`; `lib.rs:1396`; D-111) and the surface's straddle cannot stand in
     (`:1406`; D-122). The detail names the sub-condition (`lib.rs:1416-1439`):
     NOT_A_FITTED_SLICE (the expiry is outside 7-200 days or has no fitted slice or no yield),
     SLICE_AT_FIT_BOUND, CALENDAR_INVALID_AT_STRIKE, SURFACE_STRADDLE_UNPRICED, OUTSIDE_QUOTE_BAND
     [RULE thresholds; D-111, D-122, D-086].
   - QUOTED_STRADDLE_OUTSIDE_QUOTE_BAND: the repriced quoted straddle lies outside its own
     no-arbitrage band and the surface's cannot stand in (`:1399-1404`, `:1411-1413`; D-448) [RULE].
   - NO_LISTED_EXPIRY (`:894-896`).
   - DEPENDENCY_WITHHELD (surface cause) on legs that are not directly quoted (`:1473-1495`).
8. **DLM-GREEK-EXPOSURE** (fully traced) — OI-weighted American delta, gamma, vanna, charm in
   dollars, calls, puts, conventional net; the gamma flip level. Inputs: the surface file's
   per-contract Greeks, chain OI. Parents: O02, O06, VS-IVS01, P01, DLM-IV30 (`graph :176`). Method
   `DLM-GREEK-EXPOSURE@2.2.0` (`:1675`). Code `lib.rs:1548-1619`, `:1659-1675`. Reasons:
   - PROVIDER_OI_INCOMPLETE: every OI cleared (`:1579`, `:1602-1611`) [PROVIDER].
   - NO_SUPPORTED_OPEN_INTEREST_ROWS: no row with positive OI and Greeks (`:1579`) [ENGINE /
     PROVIDER].
   - CHARM_NOT_RESOLVED: a solved row has no charm (`:1583`; D-124) [ENGINE limit].
   - NO_SIGN_CHANGE_WITHIN_20_PERCENT: the gamma profile never changes sign (`:1598`) [STRUCTURAL].
   - GAMMA_REPRICING_FAILED: a point of the gamma profile is unknown (NaN)
     (`crates/h3-iv-surface/src/stage.rs:591-598`; `lib.rs:1528-1530` calls it "an engine limit")
     [ENGINE].
   - NO_POSITIVE_OPEN_INTEREST on the share roles (`:1612`, `:1647`) [STRUCTURAL].
   - DEPENDENCY_WITHHELD whole when chain, spot or surface is withheld.
   - No gate on the unsolved share: nothing is withheld for it (`:1537-1538`; D-228).
9. **DLM-IMPLIED-PROBABILITY** (fully traced) — 30-day risk-neutral probabilities of +/-5% and
   +/-10%, and the one-standard-deviation range from IV30. Parents: O02, O06, VS-IVS01, P01,
   DLM-IV30 (`graph :177`). Method `DLM-IMPLIED-PROBABILITY@2.2.0` (`:1712`). Reasons:
   NO_30_DAY_SMILE: the smile gives no probability in [0, 1] (`:1687-1688`) [PROVIDER / ENGINE];
   DEPENDENCY_WITHHELD with detail IV30 when IV30 is absent (`:1697-1705`); whole-record
   containment.
10. **DLM-GAMMA-EXPOSURE** (fully traced) — dollar gamma per 1% move from M0-GAMMA's central group,
    by right, conventional net, and the call and put walls. Parents: M0-GAMMA, VS-IVS01, P01
    (`graph :164`). Method `DLM-GAMMA-EXPOSURE@2.3.0` (`derive/local.rs:512`). Code
    `h5-derived/src/lib.rs:635-710`. Reasons: DEPENDENCY_WITHHELD with M0-GAMMA's reason or
    SPOT_WITHHELD (`:679-698`); NO_POSITIVE_GAMMA_CELLS for a wall with no cell of that right
    (`:660-664`) [STRUCTURAL].
11. **DLM-TECHNICAL** (partly traced) — standard indicators from daily bars (moving averages,
    crosses, RSI-type states, Ichimoku; INFERRED from `technical.rs:442-536`). Inputs: daily bars
    of the asset. Parents: P01 (`graph :181`). Method `DLM-TECHNICAL@1.13.0` (`technical.rs:191`).
    Reasons: INSUFFICIENT_PRICE_HISTORY (`:410`) [HISTORY of daily bars, not of built days];
    VOLUME_UNREPORTED (`:485`) [PROVIDER]. It also states the recent split and dividend adjustments
    that later scoring reads (`:123-137`; D-169).
12. **DLM-PRICE-LEVELS** (partly traced) — price levels from daily bars. Parents: P01. Method
    `DLM-PRICE-LEVELS@1.1.0` (`technical.rs:609`). Reasons: INSUFFICIENT_PRICE_HISTORY (INFERRED).
13. **DLM-CANDLESTICK** (partly traced) — named candlestick patterns (`ta.rs:426-466`). Parents:
    P01. Method `DLM-CANDLESTICK@1.3.0` (`technical.rs:198`). Reasons: INSUFFICIENT_PRICE_HISTORY
    (test `technical.rs:1132-1152`).
14. **DLM-RELATIVE-PERFORMANCE** (partly traced) — returns against SPY. Parents: P01. Method
    `DLM-RELATIVE-PERFORMANCE@1.2.0` (`technical.rs:713`). Reasons: INSUFFICIENT_PRICE_HISTORY
    (test `:1294`); BENCHMARK_UNAVAILABLE (INFERRED).
15. **DLM-DRAWDOWN** (partly traced) — one-year drawdown figures from adjusted closes
    (`technical.rs:754`). Parents: P01 (`graph :156`). Method `DLM-DRAWDOWN@2.1.0` (`:753`).
    Reasons: INSUFFICIENT_PRICE_HISTORY (INFERRED).
16. **DLM-BETA** (partly traced) — 63-session and one-year beta and correlation to SPY. The
    one-year correlation is FIELD-DEP01's SPY pair (`graph :184-185`; D-087). Method
    `DLM-BETA@2.0.0` (`technical.rs:800`). Reasons (`technical.rs:633-687`): FIELD_DEPENDENCE_ABSENT
    (no pair from FIELD-DEP01), RETURNS_DIFFER_FROM_FIELD_SUPPORT (the asset's returns are not on
    the pair's sessions), ZERO_BENCHMARK_VARIANCE, BENCHMARK_UNAVAILABLE (no SPY bars),
    INSUFFICIENT_COMMON_SUPPORT and FIELD_PAIR_NOT_SUPPORTED (`derive/mod.rs:1085-1092`),
    INSUFFICIENT_PRICE_HISTORY (`:617`).
17. **DLM-VWAP** (fully traced) — two session VWAPs from the tape (all eligible executions;
    price-setting executions), close versus each, their share counts, and the tape's share of the
    official volume. Inputs: T's replay tape and official daily bar. Parents: P01, D111
    (`graph :187-188`; D-462). Method `DLM-VWAP@3.2.9` (`technical.rs:797`). Code
    `derive/archive.rs:264-355`, `technical.rs:756-800`. Reasons:
    - The first bin's price reason, else a degenerate OHLC cell's reason, else the volume reason
      (`archive.rs:323-328`). So every price-gate and volume-gate reason of set T can appear.
    - Gate 1: with a volume reason present, both VWAPs, both close-vs-VWAP values and both share
      counts are withheld (`archive.rs:325-330`; D-411).
    - venue_volume_share states the volume reason (`technical.rs:781-784`).
    - EMPTY_INTRADAY_CAPTURE: a capture with no reason and no value (`:771`).
    - NO_INTRADAY_CAPTURE: no capture record (`:789-790`) [PROVIDER].
    - VOLUME_UNREPORTED / INSUFFICIENT_PRICE_HISTORY for the share when the daily volume is not
      usable (`:784`).
18. **DLM-EVENT-WINDOW** (fully traced; 17 companies) — the next expected earnings date and the
    straddle-priced move to the first expiry after it. Inputs: CDD-EVENTS01's expected date and
    8-K acceptances, chain, close, surface. Parents: O02, O06, VS-IVS01, P01, CDD-EVENTS01,
    DLM-IV30 (`graph :189`). Method `DLM-EVENT-WINDOW@2.4.0` (`h5-derived/src/lib.rs:1139`). Code
    `lib.rs:1075-1140`. Reasons:
    - NO_UPCOMING_EARNINGS_DATE: no expected date at or after T, or an 8-K by T's close already
      confirms it (`:1085-1087`, `:1127`) [PROVIDER / STRUCTURAL].
    - DEPENDENCY_WITHHELD with CDD-EVENTS01's cause when the date could not be established
      (`:1125-1126`).
    - NO_EXPIRY_AFTER_EVENT (`:1120`) [STRUCTURAL].
    - The straddle leg's reasons as in DLM-EXPECTED-MOVE (`:1105-1118`).
    - MISSING_REQUIRED_INPUT on the forecast capture time (`:1135`).
    - A chain cause withholds only the `event_expiry*` roles; a spot or surface cause only the six
      straddle roles (`:1146-1170`; D-406).

### 2.14 DLM history (12 ids; ordered pass)

All are built in `crates/vyreon-cli/src/derive/ordered.rs` from the database: published base days
through T, T's day-local revision, and the history profile's earlier days (`ordered.rs:1-6`,
`:44-55`). A faulted object's records are withheld ENGINE_FAULT (`ordered.rs:233`).

1. **DLM-PCR-OI** (fully traced) — put/call ratio of reported open interest and its percentile in
   the prior 126 and 252 sessions. Parents: O02; history O02 252 sessions (`graph :157`). Method
   `DLM-PCR-OI@1.0.0+DLM-LEVEL-HISTORY@3.0.0` (`ordered.rs:243`). Code
   `h5-derived/src/lib.rs:289-398`. Reasons: ZERO_CALL_DENOMINATOR (call share is 0, or the total
   weight was 0) (`science.rs:49-62`) [STRUCTURAL]; NUMERICAL_DOMAIN; DEPENDENCY_WITHHELD;
   INSUFFICIENT_LEVEL_HISTORY; HISTORY_OF_ANOTHER_METHOD_VERSION; PARENT_COUNT_WITHHELD.
2. **DLM-PCR-VOLUME** (fully traced) — the same on reported session volume. Parents: O06; history
   O06 252 (`graph :158`). Method `DLM-PCR-VOLUME@1.1.0+DLM-LEVEL-HISTORY@3.0.0` (`ordered.rs:245`).
   Extra reason: LOW_VOLUME_UNDER_100_CONTRACTS: the percentiles are withheld when the session's
   volume is under 100 contracts (`lib.rs:297-304`, constant `:1796`; D-073, D-077) [RULE]. The
   ratio itself is still published with a qualifier (`:376`).
3. **DLM-PCR-HISTORY** (fully traced) — per basis, the one-session change and the 5, 10, 30-session
   means of the ratio. Parents: O02, O06; history 252 each (`graph :159`). Method
   `DLM-PCR-HISTORY@1.3.0` (`ordered.rs:248`). Code `lib.rs:408-451`, `science.rs:181-232`,
   `:382-399`. Reasons:
   - CURRENT_RATIO_WITHHELD: T's ratio is not eligible (`science.rs:196`, `:397`).
   - PREVIOUS_RATIO_UNAVAILABLE: T−1's is not eligible or not published (`:395-396`) [HISTORY].
   - INSUFFICIENT_WINDOW_SUPPORT: fewer than 4 of 5, 8 of 10, 23 of 30 eligible, T included
     (`:181-200`; floor `h1-numerics/src/lib.rs:97-99`) [HISTORY].
   - HISTORY_OF_ANOTHER_METHOD_VERSION (`:393`, `:228-232`; D-273) [RULE].
   - A thin-volume session never enters history (`lib.rs:176-179`).
4. **DLM-OI-CHANGE** (fully traced) — one-session OI change split into persistent, expired and
   newly listed expiries. Parents: O02, O04, OIJ01; history 1 session each (`graph :160-162`).
   Method `DLM-OI-CHANGE@2.2.0` (`ordered.rs:250`). Code `lib.rs:468-549`. Reasons:
   PRIOR_SESSION_NOT_LISTED (`:481`; D-422) [STRUCTURAL]; CORPORATE_ACTION_EX_DATE (an OCC
   adjustment on T−1 or T, `:485`, `:508-511`) [RULE D-170]; CORPORATE_ACTION_UNKNOWN (no positive
   evidence that none took effect, `:512-515`) [RULE D-170];
   HISTORY_OF_ANOTHER_METHOD_VERSION (`:487`); DEPENDENCY_WITHHELD (`:506-507`);
   PRIOR_SESSION_NOT_PUBLISHED (`:540`) [HISTORY]; ZERO_PRIOR_OI_DENOMINATOR (`science.rs:366`).
5. **DLM-OPTIONS-VOLUME** (fully traced) — total option volume, its ratio to the prior 21-session
   mean, its 126/252 percentiles, and turnover against start-of-day OI. Parents: O02, O04, O06,
   OIJ01; history 252 (`graph :163`). Method `DLM-OPTIONS-VOLUME@1.4.0+DLM-LEVEL-HISTORY@3.0.0`
   (`ordered.rs:252`). Code `lib.rs:583-629`. Reasons: INSUFFICIENT_WINDOW_SUPPORT (prior mean
   needs 16 of 21, `science.rs:205-217`); ZERO_REFERENCE_MEAN (`lib.rs:594`); DEPENDENCY_WITHHELD
   (the ratio when the mean is withheld); INSUFFICIENT_LEVEL_HISTORY;
   OPEN_INTEREST_CONTRACTS_NOT_STATED (`:618-621`; D-390); ZERO_OPEN_INTEREST (`:622`). Sessions
   before the contract basis is established count as before the series (`:556-562`; D-170).
6. **DLM-HV-CONTEXT** (fully traced) — 21-session realized volatility (VS-P03 `CC.W21`) and its
   126/252 percentiles. Parents: P03; history P03 252 (`graph :165`). Method
   `DLM-HV-CONTEXT@1.0.0+DLM-LEVEL-HISTORY@3.0.0` (`ordered.rs:254`). Code `lib.rs:712-758`.
   Reasons: DEPENDENCY_WITHHELD; INSUFFICIENT_LEVEL_HISTORY; HISTORY_OF_ANOTHER_METHOD_VERSION.
7. **DLM-IV30** (fully traced) — VS-IVS01's 30-day at-the-money IV and its standard error, copied.
   Parents: VS-IVS01, P01 (`graph :173`). Method `DLM-IV30@2.1.0` (`lib.rs:931`). Code
   `lib.rs:939-960`. Reason: DEPENDENCY_WITHHELD with VS-IVS01's reason (`:944-952`). No threshold
   on the standard error (`:957`; D-095).
8. **DLM-IV-CONTEXT** (fully traced) — IV30's percentile and range rank in the prior 126 and 252
   sessions. Parents: VS-IVS01, P01, DLM-IV30; history DLM-IV30 252 (`graph :174`). Method
   `DLM-IV-CONTEXT@2.0.0+DLM-LEVEL-HISTORY@3.0.0` (`lib.rs:992`). Reasons:
   INSUFFICIENT_LEVEL_HISTORY; FLAT_LEVEL_RANGE (`:980`) [STRUCTURAL]; DEPENDENCY_WITHHELD;
   HISTORY_OF_ANOTHER_METHOD_VERSION.
9. **DLM-IV-RV** (fully traced) — IV30 minus 21-session realized volatility, and their ratio.
   Parents: VS-IVS01, P01, P03 (`graph :175`). Method `DLM-IV-RV@2.0.0` (`lib.rs:1007`). Reasons:
   DEPENDENCY_WITHHELD (`:1002-1005`); ZERO_RV_DENOMINATOR (`:1009`) [STRUCTURAL].
10. **DLM-IV-TERM** (fully traced) — at-the-money IV at 30, 60, 90 days, the 90/30 ratio, and each
    tenor's 5-session change. Parents: VS-IVS01, P01; history DLM-IV-TERM 5 sessions
    (`graph :179`). Method `DLM-IV-TERM@2.0.0` (`lib.rs:933`). Code `lib.rs:1016-1054`. Reasons:
    - OUTSIDE_VALIDATED_SURFACE_WINDOW: tenors 7, 14 and 180 are always withheld; their roles are
      PERSIST, not served (`:1020-1029`, `:1041`; D-086) [RULE].
    - DEPENDENCY_WITHHELD: a tenor VS-IVS01 withholds, and the change of any withheld tenor
      (`:1039`).
    - NO_PRIOR_SESSION_VALUE: T−5 has no published value of this method (`:1037`) [HISTORY].
    - PRIOR_VALUE_WITHHELD (`:1034`); HISTORY_OF_ANOTHER_METHOD_VERSION (`:1036`);
      NO_PRIOR_SESSION (`:1048`).
11. **DLM-IV-CRUSH** (fully traced; 17 companies) — IV30 change from the last session before the
    most recent 8-K-confirmed earnings announcement to the first session after it. Parents: O02,
    O06, VS-IVS01, P01, CDD-EVENTS01, DLM-IV30; history CDD-EVENTS01 and DLM-IV30 252
    (`graph :190`). Method `DLM-IV-CRUSH@2.1.0` (`lib.rs:1243`). Code `lib.rs:1173-1244`. Reasons:
    NO_CONFIRMED_EVENT_IN_HISTORY (`:1239`) [HISTORY]; IV30_UNAVAILABLE, _PRE, _POST
    (`:1217`, `:1231-1232`) [HISTORY or the parent's cause]; UNKNOWN_CLOCK for a date-only
    acceptance (`:1227`; D-501) [PROVIDER]; HISTORY_OF_ANOTHER_METHOD_VERSION (`:1214`);
    DEPENDENCY_WITHHELD or EMPTY_DELIVERED_POPULATION only when T is the post session
    (`ordered.rs:262-296`).
12. **DLM-UNUSUALNESS** (partly traced) — robust z of every reader-facing scalar of the security
    against its own prior 63 published sessions, with a calibrated 95% flag line. Parents: every
    scalar; history every scalar 63 sessions and its own 252 (`graph :180`). Method
    `DLM-UNUSUALNESS@1.8.0` (`derive/mod.rs:1929`). Constants: window 63, minimum 48, line quantile
    0.95, at least 500 pooled values over 252 sessions (`crates/h5-derived/src/unusual.rs:273-282`).
    Reasons (`unusual.rs:339`, `:368-403`): TODAY_WITHHELD; INSUFFICIENT_HISTORY (fewer than 48
    eligible) [HISTORY]; FLAT_HISTORY, STEP_HISTORY, DEGENERATE_SCALE [STRUCTURAL];
    LINE_UNCALIBRATED (fewer than 500 pooled values) [HISTORY]; DEPENDENCY_WITHHELD for a reading
    an engine fault took today (`h5-derived/src/lib.rs:1848-1866`; D-244). Conditions between
    `unusual.rs:371-403` were not read line by line.

### 3. Dependency graph

Source for every edge: `crates/vyreon-graph/src/lib.rs:75-191` (PROVEN). Category to stage:
`:460-482`. Read it as "if the line above is withheld whole, what is indented under it follows".
A child reads a parent by role, so a parent withheld at one role withholds only the child roles
that read it.

```
daily prices (categories daily, options)
  VS-P01
    PATH-L01, PATH-L02, PATH-L03-L07, VS-P03, VS-P07, SAI-PD01
    VS-O03, VS-OIJ01 (also need VS-O02)
    VS-V03-CURRENT (also needs Q01..Q04, VS-IVS01)
    SAI-ACT01 (also needs INT-TEMP-IP01)
    FIELD-BR01, DIST01, XCONC01, SYNC01, DEP01, GRP01, RMT01, SUB01, DIM01
      FIELD-DEP01 -> DLM-BETA
    FIELD-MHDEP01, FDSYNC01, FDRMT01, FDDIM01 (also need D111-TEMP01)
    FIELD-SUP01, FIELD-OPTBR01, M0-FIELD-ANALYTICS (also need O02, L01, V02, O06, M0-ANALYTICS, M0-IV-PROFILE, M0-GAMMA)
    day-local: DLM-DRAWDOWN, TECHNICAL, PRICE-LEVELS, CANDLESTICK, RELATIVE-PERFORMANCE, BETA, VWAP
    day-local (spot): DLM-MAX-PAIN, ZERO-DTE, UNUSUAL-CONTRACTS, QUOTE-FRICTION, OI-CENTROID, PREMIUM-YIELD,
                      EXPECTED-MOVE, GREEK-EXPOSURE, IMPLIED-PROBABILITY, GAMMA-EXPOSURE, EVENT-WINDOW
    history: DLM-IV30, IV-CONTEXT, IV-RV, IV-TERM, IV-CRUSH
    VS-P03 -> DLM-HV-CONTEXT, DLM-IV-RV

option EOD chain + closing quote overlay (categories options, quotes)
  VS-Q01
    VS-Q02, VS-Q03
      VS-Q04 (Q01, Q03)
        VS-Q05 (Q01, Q03, Q04)
        VS-O01, VS-O06, VS-O02 (Q01, Q03, Q04)
          VS-O02
            VS-O03, VS-O04, VS-OIJ01, SAI-G01 (G01 also Q05)
            DLM-PCR-OI, DLM-PCR-HISTORY, DLM-OI-CHANGE, DLM-OPTIONS-VOLUME (history)
            every day-local chain record (population root; VS-O06's root where O02 states none, D-403)
          VS-O06
            DLM-PCR-VOLUME, DLM-PCR-HISTORY, DLM-OPTIONS-VOLUME (history)
          VS-O04, VS-OIJ01 -> DLM-OI-CHANGE, DLM-OPTIONS-VOLUME
        VS-V02 (Q01, Q03, Q04, Q05)
      VS-L01 (Q01..Q04)
      M0-ANALYTICS (Q01..Q04, VS-IVS01)
        M0-IV-PROFILE (also VS-V03-CURRENT) -> M0-IV-CHANGE
        M0-GAMMA (also Q01) -> M0-GAMMA-CHANGE; -> DLM-GAMMA-EXPOSURE

chains + closing quotes + carry inputs + rate curve/macro + dividends (categories options, quotes, carry, macro, daily)
  surface stage (h5_surface)
    VS-IVS01
      VS-V03-CURRENT -> INT-IVSC01 -> VS-V03-MATCHED-NATIVE (both also read T-5's V03)
      M0-ANALYTICS -> (as above)
      DLM-EXPECTED-MOVE (surface legs only), DLM-GREEK-EXPOSURE, DLM-IMPLIED-PROBABILITY, DLM-GAMMA-EXPOSURE,
      DLM-EVENT-WINDOW (straddle roles only)
      DLM-IV30 -> DLM-IV-CONTEXT, DLM-IV-CRUSH, DLM-EVENT-WINDOW, DLM-GREEK-EXPOSURE, DLM-IMPLIED-PROBABILITY
      DLM-IV-RV, DLM-IV-TERM

replay tapes of T (category consolidated) + official daily bar (category daily)
  D111-TEMP01
    PCR-UPATH-I01, PCR-UACT-I01, PCR-UQTR-I01 (each also INT-TEMP-IP01)
      INT-TEMP-EVC01 (D111, IP01, UPATH, UACT, UQTR; plus up to 252 tape history sessions)
    FIELD-MHDEP01, FDSYNC01, FDRMT01, FDDIM01 (quote midpoints)
    DLM-VWAP
  INT-TEMP-IP01 (frozen registry control; no provider input)
    SAI-ACT01, SAI-ACT01-INTRADAY, INT-ACT-TV15, SAI-EXQ01-TEMPORAL, UPATH, UACT, UQTR, EVC01

ACT01 option interval pulls (category act01)
  SAI-ACT01 (also P01, IP01)
    SAI-ACT01-INTRADAY
      INT-ACT-TV15

EXQ01 cohort prints (category exq01; needs the act01 cohort)
  SAI-EXQ01
    SAI-EXQ01-TEMPORAL (also IP01)
    M0-PRINT-SIZE

company capture (category company)
  CDD-REF01-02, CDD-FND01, CDD-FND02, CDD-CAPVAL01
  CDD-EVENTS01 -> DLM-EVENT-WINDOW, DLM-IV-CRUSH

macro capture (category macro)
  CDD-MACRO01 (no dependents in the graph)
  the surface's rate inputs (D-239)

everything per security or company
  DLM-UNUSUALNESS (every reader-facing scalar, 63 sessions back)
```

Notes.
1. SAI-EXQ01 has no graph parent, yet it needs the ACT01 cohort (`graph lib.rs:480-482`; D-103
   title, DECISIONS:3876). An ACT01 capture failure therefore shows on SAI-EXQ01 as its own
   ENGINE_FAULT, not as PARENT_ENGINE_FAULT. PROVEN for the category need; INFERRED for the
   mechanism of the fault.
2. CDD-MACRO01 has no dependent sensor. The surface reads macro inputs directly, not through the
   sensor (`graph lib.rs:467-468`).
3. A store-history edge is separate from a same-day edge: `graph lib.rs:156-190`, last argument.

### 4. The 2026-11-25 census read against the map

The census counts are (layer, id, grain, reason, n, assets). What "grain" means in the census
query (slot, role, coordinate) is not defined in the engine code I read; see section 6.

### 4.1 The ACT01 incident lines (given: one capture failure for 28 assets)

| Line | Reading | Label |
|---|---|---|
| SAI-ACT01 role ENGINE_FAULT 756 / 28 | 27 roles per asset. The slot's own kernel failed (set C). | PROVEN meaning; cause given |
| SAI-ACT01-INTRADAY role PARENT_ENGINE_FAULT 812 / 28 | 29 roles per asset. Parent SAI-ACT01 (`graph :109`). | PROVEN edge |
| INT-ACT-TV15 slot PARENT_ENGINE_FAULT 28 | Parent ACT01-INTRADAY (`graph :110`). | PROVEN edge |
| SAI-EXQ01 role ENGINE_FAULT 924 / 28, slot 28 | 33 roles per asset. Its own fault because EXQ reads the ACT01 cohort without a graph edge (`graph :102`, `:480-482`). | INFERRED mechanism |
| SAI-EXQ01-TEMPORAL role 84 / 28, slot 28 | 3 roles per asset. Parent SAI-EXQ01 (`graph :111`). | PROVEN edge |
| M0-PRINT-SIZE coordinate 420 / 28, slot 28 | 15 coordinates per asset. Parent SAI-EXQ01 (`graph :146`). | PROVEN edge |
| DLM-UNUSUALNESS DEPENDENCY_WITHHELD 364 / 28 (history) | 13 readings per asset that an engine fault took from the scores (`h5-derived/src/lib.rs:1848-1866`). | PROVEN path |

Expected on a nightly: no. It is one incident. To publish: the ACT01 pulls of T must be captured.
The provider serves ACT01 pulls for old sessions (`graph lib.rs:370-371`), but the EXQ01 prints
only for about five sessions (`fill.rs:59-61`).

### 4.2 PCR-UACT-I01 and DLM-VWAP / SOURCE_VOLUME_RECONCILIATION_FAILED (55 assets)

1. Condition (PROVEN). `Manifest::trade_gate` returns the reason when
   `|counted_volume − official_volume| / official_volume > 0.02`
   (`crates/h3-interval-source/src/consolidated.rs:197-199`, `:447-453`; constant `:97`). The
   verdict is day-level and is put on the volume gate of every bin (`:470`, `:500`). UACT's ten
   summaries need every bin (`crates/h3-temporal/src/uact.rs:131-162`).
2. Counts check (INFERRED arithmetic). UACT: 55 x 10 roles = 550. Matches. VWAP: a fully gated
   asset withholds 7 roles (`technical.rs:768-785`). 53 x 7 + 2 x 1 = 373. So 53 assets carry the
   volume reason on all seven roles and 2 carry it only on `venue_volume_share`, because a price
   reason outranks it on the other six (`derive/archive.rs:328`). This says 2 of the 4
   price-gated assets (GOOGL, VTV, XHB, XLY) also failed the volume check and 2 did not. The split
   is consistent with the counts but not unique.
3. Which official volume is compared (PROVEN). The official bar of T is the one final daily row
   (`frequency` daily, `intraperiod` false) of the published daily attempt
   (`crates/h3-replay/src/capture.rs:240-257`, `:262-290`). Before every read, the cache's
   reconciliation is restated to the bar the archive holds now (`official.rs:433-446`;
   `restate.rs`; D-268, DECISIONS:10868-10908). On a nightly, T's first daily capture is taken by
   the `options` category the night after T; the `daily` category re-captures only a bar that
   already exists (`crates/vyreon-cli/src/fill.rs:1036-1042`). The re-capture covers the last 3
   sessions before today (`crates/vyreon-cli/src/day_inputs.rs:107-109`).
4. Is it a threshold, a recent-session condition or a defect?
   - The trigger is a threshold: 2%, an owner ruling (D-089 ruling 5, `consolidated.rs:95-97`).
   - 55 of 64 on one recent night points to a data condition, not to 55 separate tape faults.
     INFERRED. DECISIONS records that a next-day daily bar can be preliminary: EMB and MSFT
     2026-10-16 were 3.3% and 4.6% below the API's own prints (DECISIONS:3427-3432). A test encodes
     exactly this case: a bar 4.6% low withholds the volume bins; the final bar restores them
     (`crates/h3-replay/src/restate.rs:483-510`).
   - D-268 says this case was "not verified on real data: a real preliminary bar against a real
     cache on the cluster" (DECISIONS:10908).
   - The opposite direction exists too: tapes before 2025-11-26 hold 33-77% of consolidated volume
     (provider fault P-23; DECISIONS:16465-16466). That is a different era from 2026-11-25.
   - An engine defect is not excluded: for example a counting rule that drifts from the official
     definition. Nothing I read shows one.
5. Evidence that would decide it.
   1. For each of the 55 captures, `reconciliation.counted_volume` and `official_volume` in the
      manifest (`consolidated.rs:185-195`): the sign and the size of the difference. A preliminary
      bar gives counted above official, clustered at a few percent.
   2. The same assets' official volume re-read after a later daily re-capture. If the verdict
      flips with no change to the tape, it was the bar.
   3. Whether the 9 passing assets differ in listing venue or volume (not established).
6. What must be true to publish: counted volume within 2% of the final official volume, and every
   bin free of tape faults. Whether a published nightly is recomputed when the final bar arrives
   is not established (it is core's scheduling; Rules in force 15 says a day is computed once,
   DECISIONS:57-61).

### 4.3 PCR-UQTR-I01 / QUOTE_COMPLETENESS_UNVERIFIED (64 assets, 320 = 5 roles each)

1. Condition (PROVEN). `quote_gate(date)` returns the reason for every session date on or after
   `QUOTE_UNVERIFIED_FROM = "2026-11-06"` (`crates/h3-replay/src/official.rs:60`, `:412-414`). It is
   applied to every bin's quote family after the bin's own reasons (`:424-428`).
2. Class: a date-shaped ruling (D-411 gate 3; provider fault P-04: from that session the files
   carry about three times the quote messages and books about twice as wide,
   DECISIONS:16466-16467). D-411 states why it is a date and not a detector ("no detector from the
   data is honest yet", DECISIONS:16507-16508).
3. Expected on a nightly: yes, on every nightly and every asset, until "a quote-completeness check
   exists" (`official.rs:27-31`, `:58-59`). In the 597-day run it will be 100% withheld by this
   gate on sessions from 2026-11-06 and 0% by this gate before it.
4. Not an engine defect by the code: the rule does what it says. It is the clearest case in the
   census of a rule that withholds a whole sensor on input the bin gates accept.
5. What must be true to publish: a session date before 2026-11-06, or the constant retired.
6. Side effects (PROVEN): D111's quote cells and EVC01's width input are withheld too
   (DECISIONS:16505-16506). The FIELD intraday midpoints are not (`official.rs:448-452`).

### 4.4 CDD-MACRO01 / INVALID_REQUIRED_INPUT (64 assets, 512 = 8 roles each)

1. Which series (INFERRED arithmetic on PROVEN role counts). A withheld CPIAUCNS or FEDFUNDS
   withholds 6 roles (5 fields + history). A withheld INDPRO withholds 8 (5 + history + 2 vintage
   roles) (`crates/h3-asset/src/macro01_production.rs:163`, `:190-222`). 8 per asset means INDPRO
   alone. The other two series published.
2. Which condition (PROVEN). INVALID_REQUIRED_INPUT is stated for any non-supported state other
   than "no observation" (`macro01_production.rs:102-110`). So INDPRO's state is either
   WITHHELD_NOT_DELIVERED (the capture could not read the series in its documented shape, D-297) or
   one of the five metadata gates (`crates/h3-macro/src/lib.rs:323-338`).
3. Why all 64: one macro capture feeds every asset's slot (`macro01_production.rs:139-246`).
4. Candidates, none decided:
   - The provider did not deliver INDPRO in shape that night (data condition).
   - The provider's INDPRO metadata fails a gate. D-508, dated 2026-11-25, made the unit-label
     override re-check frequency, seasonal adjustment and start
     (`crates/h3-macro/src/lib.rs:309-318`). D-508 says the retained metadata passes
     ("no real case", DECISIONS:21206). So a metadata failure would be a change on the provider's
     side, or the D-044 base check failing (the twelve 2017 values no longer average within 0.5 of
     100, `lib.rs:292-306`).
   - An engine defect in D-508's change. Nothing I read shows one.
5. Evidence that would decide it: the published body's `series_evidence[2]` for any asset:
   `metadata_gate`, `support_state` and `not_delivered_reason`
   (`macro01_production.rs:176-190`). One row answers it.
6. Expected on a nightly: no. D-305 and D-297 describe an ordinary day with three supported series.
7. What must be true to publish: INDPRO delivered, identity, frequency ("monthly"), seasonal
   adjustment ("Seasonally Adjusted") and start date valid, and unit either "Index 2017=100" or
   verified by the 2017 values.

### 4.5 VS-V03-MATCHED-NATIVE / INSUFFICIENT_ELIGIBLE (25 assets, 98 roles)

1. Condition (PROVEN). A matched role is INSUFFICIENT_ELIGIBLE exactly when INT-IVSC01's cell for
   that right and coordinate is INSUFFICIENT_SUPPORT (`crates/h3-v03-matched/src/lib.rs:80`). That
   cell arises when (`crates/h3-ivsc/src/lib.rs:139-154`): the slice is the same but T−5's windows
   differ and the refit on T's windows is not supported; or a fit membership is absent at T or
   T−5; or the two endpoints' source bindings are not compatible; or a value, expiry or right is
   absent at either endpoint.
2. Reading of the count (INFERRED). 98 / 25 is about 4 roles per asset of 6. The slope and the
   residual scale share one fit group (`crates/h3-v03/src/lib.rs:200`), so 4 per asset fits
   "slope group withheld for both rights, term group published". The slope group needs at least 7
   rows and 2 per side in its window; the term group 3 and 1 (`lib.rs:125-126`; D-093).
3. The census has no VS-V03-CURRENT withholding line for these 25 assets. If the census counts
   every withheld role, T's own slope readings exist and the missing support is at the T−5
   endpoint (2026-11-18) or in its refit. INFERRED, and it depends on how the census treats
   NOT_APPLICABLE and array roles (section 6).
4. Class: a threshold rule (the D-093 minimums and window formula) acting on thin quotes, plus the
   D-083/D-093 refit rule. A candidate for the overfit review, not a recent-session condition and
   not shown to be a defect.
5. Evidence that would decide it: for one of the 25 assets, INT-IVSC01's 2x3 state table and
   `predecessor5_refit` (`h3-ivsc/src/lib.rs:74-78`, `:266-270`), and the slope roles and
   `window_bits` of VS-V03-CURRENT on 2026-11-18 and 2026-11-25. Whether the 25 assets recur day
   after day decides between "thin market" and "ladder change at T−5".
6. Expected on a nightly: partly. D-003 kept this as "an opportunistic exact-match measurement"
   (DECISIONS:173-176); D-083 and D-093 later added the refit. 25 of 64 on one day is the number
   the 80% test will judge.
7. What must be true to publish: both endpoints carry the coordinate and its fit membership, with
   equal windows or a supported refit.

### 4.6 PCR-UPATH-I01 and DLM-VWAP / TAPE_INCOMPLETE_AGAINST_OFFICIAL_BAR (GOOGL, VTV, XHB) and TAPE_DIFFERS_FROM_OFFICIAL_BAR (XLY)

1. Condition (PROVEN, `crates/h3-replay/src/official.rs:387-409`). The tape's session open, high,
   low, close must equal the official bar within half the tape's price step ($0.0001 below $1,024)
   (`:300-323`). On any difference:
   - TAPE_INCOMPLETE when the tape's high is below or its low above the official one, or the close
     differs and the tape holds none of the listing market's closing statements, or any field
     differs and counted volume is more than 2% short (`:396-403`).
   - TAPE_DIFFERS otherwise (`:404-405`).
2. Counts check. UPATH 3 x 11 = 33 and XLY 11: eleven roles per asset, all withheld (PROVEN that
   gate 2 withholds every bin and summary, DECISIONS:16494). VWAP 3 x 6 = 18 and XLY 6: the six
   price-read roles (`technical.rs:768-778`).
3. Class: an exact-match rule (D-411, D-412) on a provider comparison. D-411 classes
   TAPE_INCOMPLETE as provider and TAPE_DIFFERS as engine (DECISIONS:16498-16501).
4. Recent-session condition? Possible for the same reason as 4.2: the official bar read the night
   after may be preliminary in price as well as volume. Not established. The `short` clause uses
   the same 2% volume comparison (`official.rs:422`).
5. Evidence that would decide it: per asset, the tape OHLC against the official OHLC, the
   manifest's `session_close_source`, `listing_market_center`, `closing_auction_prints_any_market`
   and `listing_closing_auction_prints` counts (`official.rs:366-375`), and the reconciliation's
   sign. For XLY (TAPE_DIFFERS): which of the four fields differs.
6. Expected on a nightly: a few assets, yes, by the history D-411 and D-412 record. Four of 64 is
   6%.
7. What must be true to publish: all four prices equal at the tape's step.

### 4.7 UUP / CARRY_NOT_IDENTIFIED (VS-V03-CURRENT 29, VS-IVS01 11, V03-MATCHED 6, INT-IVSC01 1, M0-ANALYTICS, M0-IV-PROFILE, M0-GAMMA slots)

1. Condition (PROVEN). `h5_surface` removes an asset whose chain is not empty and none of whose
   expiries identifies a residual yield, and writes an absence record with each expiry's pair
   count (`crates/h3-iv-surface/src/bin/h5_surface.rs:346-362`; `stage.rs:199-204`). An expiry
   identifies a yield with at least three two-sided call/put pairs whose American σ agree at one q
   (D-185, quoting D-107 §8).
2. Propagation (PROVEN). The executor withholds every reader of the surface for that reason and
   their dependants (D-185 text; `crates/h3-asset/src/h4_run.rs:727-731`). VS-IVS01 states it on
   all 11 roles (11 matches `body.rs:55-67`). The derived records read DEPENDENCY_WITHHELD: UUP's
   DLM-GREEK-EXPOSURE 22, GAMMA-EXPOSURE 10, EXPECTED-MOVE 6, IMPLIED-PROBABILITY 6, and in the
   history layer IV30 2, IV-CONTEXT 4, IV-RV 3, IV-TERM 7. Each count matches the role lists I read
   (`h5-derived/src/lib.rs:635-710`, `:1473-1495`, `:1548-1619`, `:1685-1706`, `:954-956`,
   `:964-985`, `:1008-1011`, `:1022-1045`).
3. Class: the code calls it provider-limited (`crates/h1-contract/src/lib.rs:58-62`). The
   three-pair minimum is the rule that turns thin quotes into no surface.
4. Known pattern (PROVEN from DECISIONS, D-185): over 64 assets x 361 sessions the one-year book
   had no identified expiry on 35 asset-sessions, UUP 18 of them. In that sample UUP could always
   borrow from an expiry beyond a year; the 13 fully withheld were LQD, DBA, five sector funds and
   ETHA. So a UUP day with no identified expiry at any maturity is not in D-185's sample.
5. Later rule that can remove expiries: D-560 withholds an expiry whose forward lies outside its
   quotes' no-arbitrage bounds (`stage.rs:211-215`). Whether D-560 took UUP's remaining expiry on
   2026-11-25 is not established.
6. Evidence that would decide it: UUP's absence record for 2026-11-25
   (`UUP.absent.json`, `detail.pairs`: per expiry the pair count), and whether any expiry was
   withheld by D-560.
7. Expected on a nightly: for UUP, sometimes. One asset of 64.
8. What must be true to publish: one expiry with three agreeing call/put pairs.

### 4.8 DLM-EXPECTED-MOVE / WIDE_STRADDLE_QUOTE (24 assets, 50 roles)

1. Condition (PROVEN, `crates/h5-derived/src/lib.rs:1378-1408`). A leg has a quoted straddle; its
   half-spread is more than 25% of its cost (`chain.rs:390`; D-111); and the surface's straddle
   cannot replace it (`:1406`; D-122). The replacement fails for one of five named conditions
   (`:1416-1439`): the expiry is not a fitted slice (outside 7-200 days, or no slice, or no
   yield), the slice is at a fit bound, total variance decreases beside the slice at that strike,
   no finite price, or the surface price is outside the quote band.
2. Counts (INFERRED). A withheld leg withholds `straddle_cost` and `expected_move`: 2 roles. 50
   roles is 25 legs over 24 assets.
3. Class: two thresholds meeting. 0.25 is an owner ruling (D-111). The 7-day floor of the fit is
   D-086 (`crates/h3-iv-surface/src/lib.rs:45`).
4. Recent-session condition? Likely the calendar, not data lateness. INFERRED. The front leg is
   the first listed expiry strictly after T (`chain.rs:59-64`). T is Wednesday 2026-11-25. If the
   front expiry is within 6 days, it can never be a fitted slice, so every wide front leg is
   withheld. Short-dated straddles on low-priced ETFs (the census examples are ARKK, EEM, EFA, EWJ,
   EWY) are cheap against a one-cent tick. The near-30 leg is at least 7 days out by construction
   (`chain.rs:457-458`), so it fails only on the other four conditions.
5. Evidence that would decide it: each withheld role's `reason_detail` (it names the condition)
   and the leg's `calendar_dte` and `relative_half_spread` roles (`lib.rs:876`, `:891`). A count
   by leg (front or near_30) and by detail over the 597 days gives the population directly.
6. Expected on a nightly: yes for thin, low-priced ETFs with a front expiry inside 7 days. 24 of
   64 assets with one leg withheld; the other leg of most of them published.
7. What must be true to publish: a half-spread at most 25% of cost, or a sound fitted slice at
   that expiry with a surface price inside the quote band.

### 4.9 DLM-IV-TERM / OUTSIDE_VALIDATED_SURFACE_WINDOW (64 assets, 192 = 3 roles each)

1. Condition (PROVEN). Tenors 7, 14 and 180 days are withheld unconditionally
   (`crates/h5-derived/src/lib.rs:1022-1029`). Their roles are written with visibility PERSIST,
   which is never returned to readers (`:1041`; `crates/vyreon-sensors/src/lib.rs:91-92`).
2. Class: a ruling (D-086: "7/14/180 lie outside its validated 7-200-day fit and stay private
   (withheld) until research re-validates a wider window", `lib.rs:1020-1021`).
3. Expected on a nightly: yes, always, on every asset. It is constant, so it carries no
   information in the run. It should be excluded from any coverage ratio of reader-facing roles.
4. Related lines (PROVEN arithmetic on the same loop):
   - DEPENDENCY_WITHHELD 199 = 64 x 3 (the `change_5` of the three always-withheld tenors, `:1039`)
     + 7 for UUP (three tenors, three changes, the term ratio).
   - NO_PRIOR_SESSION_VALUE 189 = 63 x 3: the 30, 60, 90-day changes. T−5 (2026-11-18) has no
     published DLM-IV-TERM value in the history store (`:1037`;
     `crates/vyreon-cli/src/derive/mod.rs:765-770`). HISTORY.

### 4.10 The remaining base lines

| Line | Condition in code | Expected on a nightly? | To publish | Label |
|---|---|---|---|---|
| PATH-L03-L07 role ZERO_DENOMINATOR 9 / 8 (AAPL, AMZN, LLY, TSLA, UUP...) | A coordinate's continuation ratio when its prior 5-session change is exactly 0, or a coordinate value absent without another reason (`crates/h3-path/src/w21.rs:261-263`, `:326-339`). Coordinates that sit at 0 or do not move (drawdown at a new high, an unchanged maximum drawdown) give it. | Yes. Arithmetic. | A non-zero prior change. | PROVEN condition; which coordinate INFERRED |
| VS-O02 2, VS-L01 1, VS-O04 1, VS-OIJ01 1 role INSUFFICIENT_ELIGIBLE, ETHA | A CAP126 percentile with fewer than 95 eligible sessions (`h3-o02/src/history.rs:244-247`; `h3-l01/src/history_production.rs:153-155`; `h3-o04/src/history_production.rs:240`; `h3-oij01/src/history.rs:189`). ETHA has many sessions delivered empty ("ETHA's 181 empty sessions", `h5-derived/src/lib.rs:1723-1725`). | Yes for ETHA until 95 of its last 126 sessions are eligible. | 95 eligible of 126. | PROVEN floor; ETHA cause INFERRED |
| SAI-ACT01 role MISSING_REQUIRED_INPUT 1, MDY | A count-based reading with no count delivered (`crates/h3-act01/src/lib.rs:245-254`, `:296`, `:315`, `:330`). MDY is not among the 28 faulted assets. | Occasionally (D-237). | The provider's interval rows carry the trade count. | PROVEN sites; which role not established |
| VS-V03-CURRENT 29, VS-IVS01 11, V03-MATCHED 6, INT-IVSC01 1, M0-ANALYTICS, M0-IV-PROFILE, M0-GAMMA, UUP | See 4.7. | | | |

### 4.11 The remaining day-local lines

| Line | Condition in code | Class | Expected? | Label |
|---|---|---|---|---|
| DLM-UNUSUAL-CONTRACTS NO_FLAGGED_CONTRACTS 54 / 6 (DBA, EMB, INDA, TIP, VGK...) | No contract had volume above its start-of-day OI with at least 100 contracts. The nine share roles are 0/0, state NOT_APPLICABLE (`h5-derived/src/lib.rs:772-788`). 6 x 9 = 54. | STRUCTURAL | Yes, on quiet assets. Count, volume and share of all volume still publish as 0. | PROVEN |
| DLM-GREEK-EXPOSURE NO_SIGN_CHANGE_WITHIN_20_PERCENT 6 / 3 (DBA, EMB, UNG) | The gamma profile has no sign change; flip level and distance (`:1597-1600`). | STRUCTURAL | Yes. | PROVEN |
| DLM-GREEK-EXPOSURE GAMMA_REPRICING_FAILED 4 / 2 (MSTR, NVDA) | A point of the stage's gamma profile is NaN: a contract's gamma did not resolve at a shifted spot (`h3-iv-surface/src/stage.rs:591-598`). Flip level and distance only. | ENGINE (the code says "an engine limit", `lib.rs:1528-1530`) | Not by design. Two assets. | PROVEN condition; root cause not traced |
| DLM-PREMIUM-YIELD NO_QUOTED_STRIKES_BRACKETING_TARGET 3 / 1 (TIP) | One target strike (one of six) has no two quoted strikes around it; no extrapolation (`lib.rs:1737-1745`; `chain.rs:417-421`). 3 roles = one put target. | PROVIDER / STRUCTURAL | Yes on narrow chains. | PROVEN |
| DLM-EVENT-WINDOW NO_UPCOMING_EARNINGS_DATE 2 / 2 (BABA, TSM) | No expected date at or after T from CDD-EVENTS01, and no stated cause for its absence (`lib.rs:1085-1087`, `:1125-1128`). | PROVIDER (the forecast) | Yes for companies the provider gives no forecast for. | PROVEN condition; "foreign filers" INFERRED |
| DLM-GREEK-EXPOSURE 22, GAMMA-EXPOSURE 10, EXPECTED-MOVE 6, IMPLIED-PROBABILITY 6 DEPENDENCY_WITHHELD, UUP | Follow 4.7. | | | PROVEN |
| DLM-VWAP lines | See 4.2 and 4.6. | | | |
| DLM-EXPECTED-MOVE WIDE_STRADDLE_QUOTE | See 4.8. | | | |

### 4.12 The history lines (short)

1. DLM-UNUSUALNESS INSUFFICIENT_HISTORY 13,657: fewer than 48 eligible of the prior 63 sessions
   (`unusual.rs:273-276`, `:371`). Expected with about 20 built days. HISTORY.
2. INSUFFICIENT_LEVEL_HISTORY on IV-CONTEXT (252 = 63 x 4), OPTIONS-VOLUME (128), HV-CONTEXT (128),
   PCR-OI (128), PCR-VOLUME (126): the 95-of-126 and 189-of-252 floors (`science.rs:116-124`).
   Expected. HISTORY.
3. DLM-PCR-HISTORY INSUFFICIENT_WINDOW_SUPPORT 381: all six means on both bases for every asset
   but EMB's three volume means (64 x 6 − 3 = 381). The 5-session mean needs 4 eligible of the last
   5 including T (`science.rs:181-200`). It is withheld on all 64 on both bases, so at least two
   of T−4..T−1 are not eligible on each basis. INFERRED: the built days before 2026-11-25 are not
   five consecutive sessions, or their readings are not eligible. Which, not established.
4. DLM-PCR-HISTORY HISTORY_OF_ANOTHER_METHOD_VERSION 63 (one role per asset). The remaining
   supported roles are 64 (512 − 381 − 63 − 4), one per asset. INFERRED reading: one basis's
   `change_1` is supported on all 64 and the other's is withheld on 63 because T−1 is of another
   method version (`science.rs:393`); EMB's is CURRENT_RATIO_WITHHELD instead. The volume basis
   fits, because EMB's volume ratio is thin today. Compatibility compares the science and method
   labels of the measurement and its whole lineage on the two sessions (`versions.rs:60-71`).
   VS-O02 and VS-O06 share Q01, Q03, Q04 (`graph :89`, `:93`), so the label that differs between
   2026-11-24 and 2026-11-25 would be VS-O06's own. Not a HISTORY-length effect: it is D-273's
   rule reacting to a label change during the run. Evidence: the science and method labels of
   VS-O06 rows for SPY on the two sessions (`derive/mod.rs:565-577`).
5. DLM-PCR-HISTORY CURRENT_RATIO_WITHHELD 4 and DLM-PCR-VOLUME LOW_VOLUME_UNDER_100_CONTRACTS 2,
   EMB: the session's option volume is under 100 contracts (`lib.rs:176-179`, `:297-304`; D-073).
   RULE. Expected on EMB.
6. DLM-OPTIONS-VOLUME INSUFFICIENT_WINDOW_SUPPORT 64 and DEPENDENCY_WITHHELD 64: the prior
   21-session mean needs 16 eligible (`science.rs:205-217`), and the ratio to it follows
   (`lib.rs:594`). HISTORY.
7. DLM-IV-CRUSH NO_CONFIRMED_EVENT_IN_HISTORY 17 / 17: no 8-K-confirmed announcement whose post
   session is at or before T was found in the events history (`lib.rs:1181-1196`, `:1239`).
   HISTORY.
8. DLM-IV-TERM lines: see 4.9.

### 5. Rules that can withhold a base-layer sensor on clean input

"Clean input" here means the provider delivered rows that pass identity and shape checks.

| # | Rule | Constant | Where | D-number | Sensors it reaches |
|---|---|---|---|---|---|
| 1 | Day volume reconciliation | 2% either way | `crates/h3-interval-source/src/consolidated.rs:97`, `:447-453` | D-089 ruling 5; D-094; D-411 gate 1 | D111 volume and trade-count cells, PCR-UACT-I01, EVC01 volume, DLM-VWAP |
| 2 | Tape prices equal the official bar | half the tape step ($0.00005 below $1,024) | `crates/h3-replay/src/official.rs:300-323`, `:387-409` | D-411 gate 2; D-412 | D111 OHLC, PCR-UPATH-I01, EVC01 squared return, DLM-VWAP |
| 3 | Replay book unverified from a date | 2026-11-06 | `official.rs:60`, `:412-414` | D-411 gate 3 (P-04) | D111 quote cells, PCR-UQTR-I01, EVC01 width |
| 4 | Dropout minute | under 0.35 of the median of 5 neighbours each side, both streams | `consolidated.rs:398-409` | D-157 | every bin family of the minute's bin |
| 5 | Corrupt segment | lost messages above 0.1% of the security's | `consolidated.rs:103`, `:463-465` | no D-number found for the bound; its scope (the security's own messages) is audit d412 round 4 item 3 (`consolidated.rs:101-102`) | every bin |
| 6 | A silent minute gates its stream | any minute with zero messages | `consolidated.rs:484-490` | D-157 | the bin's price and volume, or quote |
| 7 | Full-session requirement for summaries | every bin supported | `crates/h3-temporal/src/uact.rs:131`, `:145` | D-094 ("day summaries are withheld exactly when a bin they need is", `consolidated.rs:90`) | PCR-UACT-I01 summaries; UPATH and UQTR summaries (INFERRED) |
| 8 | Completeness floor of an own-history window | 75%, rounded up: 95 of 126, 189 of 252, 188 of 250, 48 of 63 | `crates/h1-numerics/src/lib.rs:87-105` | D-404 (40 CFR Part 50 App. K and T) | P01, PATH, P03, PD01, O02, O04, L01, OIJ01 percentiles; FIELD contexts; EVC01 baseline (189 priors, `evc01.rs:83-94`) |
| 9 | Carry identification | 3 call/put pairs agreeing at one residual yield, on at least one expiry | `crates/h3-iv-surface/src/bin/h5_surface.rs:346-362` | D-185; D-107 §8 | VS-IVS01, V03-CURRENT, IVSC01, V03-MATCHED, M0-* and every derived surface reader |
| 10 | Forward outside its quotes' no-arbitrage bounds | the expiry is withheld | `crates/h3-iv-surface/src/stage.rs:211-215` | D-560 | VS-IVS01 expiries, M0 rows of that expiry |
| 11 | Surface fit minimum | 3 two-sided quotes in 7-200 days, both sides of the money | `crates/h3-iv-surface/src/lib.rs:45-46`, `:551-554`, `:575` | D-086; audit 2026-11-14 surface 11 | VS-IVS01 (INSUFFICIENT_SURFACE_QUOTES) |
| 12 | 60/90-day readings need an identified 30-day level | — | `crates/h3-iv-surface/src/body.rs:43-45` | D-086 | VS-IVS01 iv60, iv90, term_90_30 |
| 13 | V03 fit minimums and windows | term: 3 rows, 1 per side; slope: 7 rows, 2 per side; windows from floors 0.025 / 0.10, scales 2 / 5, caps 0.20 / 0.60 | `crates/h3-v03/src/lib.rs:45-50`, `:111`, `:125-126` | D-093; D-029 | VS-V03-CURRENT, then IVSC01 and V03-MATCHED |
| 14 | V03 slice selection | first standard monthly 14 to 365 days out | `crates/h3-v03/src/lib.rs:690-704` | D-029; D-093 audit | VS-V03-CURRENT |
| 15 | Matched change needs like-for-like endpoints | same slice and windows, or a supported T−5 refit | `crates/h3-ivsc/src/lib.rs:110-160` | D-083; D-093; D-121; D-400 | INT-IVSC01, VS-V03-MATCHED-NATIVE |
| 16 | Registered challenges | a corporate-action ex-date in [T−5, T]; FB/META 2022; NVDA 2024 split | `crates/h3-ivsc/src/lib.rs:183-192`; `crates/h3-q03/src/result.rs:25-26` | audit 3b (spec 5.3) | P01, PATH, O02, O03, O04, L01, IVSC01, V03-MATCHED, M0-GAMMA-CHANGE |
| 17 | Macro metadata must match exactly | identity, unit, "monthly", seasonal adjustment, start date; INDPRO unit override within 0.5 of 100 | `crates/h3-macro/src/lib.rs:287-338` | D-044; D-508 | CDD-MACRO01 |
| 18 | Macro point-in-time release table | an observation counts from its release date | `crates/h3-macro/src/lib.rs:53-99` | D-034, D-079, D-081, D-380, D-381 | CDD-MACRO01 (MISSING_REQUIRED_INPUT when none is readable) |
| 19 | Print retention | prints older than about 5 sessions cannot be captured | `crates/vyreon-cli/src/fill.rs:59-61` | D-103; D-360 | SAI-EXQ01, EXQ01-TEMPORAL, M0-PRINT-SIZE (EVENT_SOURCE_NOT_RETAINED) |
| 20 | Rows without a trade count are unknown, never zero | — | `crates/h3-act01/src/lib.rs:122`, `:152` | D-237 | SAI-ACT01, ACT01-INTRADAY, TV15 |
| 21 | Overlay clears a field on every row | OI or volume cleared for the day | `crates/h3-o02/src/result.rs:72-73`; `crates/h3-o06/src/production_result.rs:91-95` | D-079 | O02, O03, O04, O06, OIJ01, G01, M0-GAMMA (PROVIDER_OI_INCOMPLETE / PROVIDER_VOLUME_INCOMPLETE) |

Day-local rules that the population will also be judged against (not base layer, listed for
completeness): the straddle half-spread limit 0.25 (`chain.rs:390`; D-111) with the surface
fallback's five conditions (D-122); the quote-band check (D-448); the friction minimum of 3 quoted
contracts (`chain.rs:130`); the unusual-contract minimum of 100 contracts (`chain.rs:127`); the
put/call volume minimum of 100 contracts (`h5-derived/src/lib.rs:1796`; D-073); the method-version
rule (D-273); the contract-basis rule (D-170).

### 6. Not established

1. **What the census calls grain.** I did not find the query that produced the census. The split
   of slot, role and coordinate is not defined in the engine code I read. Counts that I matched by
   arithmetic assume one row per withheld role.
2. **Whether the census counts NOT_APPLICABLE roles and array cells.** DLM-UNUSUAL-CONTRACTS
   suggests NOT_APPLICABLE roles are counted (54 = 6 x 9). Not confirmed for base bodies.
3. **Why 55 assets failed the 2% volume check.** The sign and size of each difference are in the
   capture manifests, which I did not have. A preliminary next-day official bar is a recorded
   precedent, not a finding for this day.
4. **Whether a nightly is recomputed after the final daily bar arrives.** The engine restates the
   reconciliation at read time. Whether core schedules a re-read of a published day is outside the
   engine.
5. **Which INDPRO state caused CDD-MACRO01's withholding.** Not delivered, or a metadata gate. The
   body's `series_evidence` says which.
6. **Which IVSC condition gave INSUFFICIENT_SUPPORT on the 25 assets.** Four conditions are
   possible.
7. **Why UUP had no identified carry at any maturity on 2026-11-25**, and whether D-560 removed an
   expiry.
8. **Which sub-condition withheld each wide straddle leg.** The role's `reason_detail` names it.
9. **Why M0-IV-CHANGE and M0-GAMMA-CHANGE do not appear for UUP** in the census beside their
   parents M0-IV-PROFILE and M0-GAMMA. D-185 says dependants are withheld with the readers.
10. **Why VS-V03-CURRENT's UUP line is at role grain and M0's at slot grain** for the same reason.
11. **Which built days preceded 2026-11-25**, and why the 5-session PCR means were short on both
    bases for all 64 assets.
12. **Which label changed between 2026-11-24 and 2026-11-25** in VS-O06's lineage. Reasoned to
    VS-O06's own label; not seen.
13. **The provider input of each sensor at the level of the fetch.** I cite the category and the
    comments that tie it to a stage. I did not read `plan.rs` (3,588 lines), `views.rs` (3,154) or
    `fill.rs` in full, so the exact endpoint and window per sensor are INFERRED except where a line
    is cited.
14. **The exact window sizes** "132-session quote window" and "201-session window" named in core's
    notes and `fill.rs:9`: which sensor reads which was not traced.
15. **Conditions of the reason strings marked "conditions not read".** That covers the Q family,
    P03, P07, PD01, PATH-L01, PATH-L02, O01 thresholds, OIJ01, the M0 kernels, ACT01-INTRADAY and
    TV15 details, the five company sensors, the FIELD owners, DLM-TECHNICAL, PRICE-LEVELS,
    CANDLESTICK, RELATIVE-PERFORMANCE, DRAWDOWN, and parts of DLM-UNUSUALNESS.
16. **Science version constants.** I list each sensor's method label. A separate science-version
    constant per sensor was not located, except CDD-MACRO01's (named in D-508, DECISIONS:21189-21191).
17. **The class of several reasons.** Where I wrote two classes (for example "PROVIDER /
    STRUCTURAL") the code does not say and I could not decide.
18. **The root cause of GAMMA_REPRICING_FAILED** on MSTR and NVDA.
19. **Trace depth.** Fully traced (31): INT-IVSC01, VS-V03-MATCHED-NATIVE, VS-IVS01, D111-TEMP01,
    PCR-UPATH-I01, PCR-UACT-I01, PCR-UQTR-I01, CDD-MACRO01; DLM-MAX-PAIN, ZERO-DTE,
    UNUSUAL-CONTRACTS, QUOTE-FRICTION, OI-CENTROID, PREMIUM-YIELD, EXPECTED-MOVE, GREEK-EXPOSURE,
    IMPLIED-PROBABILITY, GAMMA-EXPOSURE, VWAP, EVENT-WINDOW; DLM-PCR-OI, PCR-VOLUME, PCR-HISTORY,
    OI-CHANGE, OPTIONS-VOLUME, HV-CONTEXT, IV30, IV-CONTEXT, IV-RV, IV-TERM, IV-CRUSH. Partly
    traced: the other 63.

## Learned in the run
(Empty until phase 1 runs. Each line: date, sensor, what was seen, evidence.)
