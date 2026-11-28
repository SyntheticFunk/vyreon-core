# Phase-1 catalog

What the characterization run finds, for the one fix pass after it (owner, 2026-11-26: one frozen build over
2024-07-09..2026-11-20; faults are data; fix all afterwards, recompute selectively if possible, fully if necessary).
One line per item: what, the evidence, where it is recorded. Nothing here is worked during the run unless it is the
run itself struggling (memory deaths, stalls, throttling, cost, days failing). Criteria for the read are written
before the population is read (below, to be agreed with the owner before the run ends).

**Kinds:** SCIENCE (a sensor's rule or method), CHECKER (the reference suite), OPS (core, cluster, scripts),
TESTS (a test that does not prove what it claims), PROVIDER (goes to `PROVIDER_FAULTS.md` too).
**Status:** OPEN unless marked. Proven = verified in code or data by the coordinator or a second reader;
hypothesis = not yet.

## Rules for the run (owner, 2026-11-26 evening: "all sensible and authorized")
- **The pre-canary line:** after the C-095/C-096 and D-564..D-566 batch, a change goes in before or during the run
  only if it prevents losing evidence that cannot be had again, or stops days publishing. Everything else is a line
  here, whoever thinks of it.
- The audit of that batch runs beside the canary, not before it.
- **Checkpoint after the first 10–20 days publish:** the run's record works (logs in the archive, step history,
  the census, refusals recorded). The run does not pause for it.
- Numeric stop conditions are proposed after the first day, from measured rates.
- **The derived (history-reading) measurements have never run on a populated history** (owner, 2026-11-26: "the
  derived measurements all require history, so effectively none of those have been tested"). Their first real
  test is the ordered passes after the day layer is built; that pass is part of the characterization, read with
  the census like the day layer, and its faults are catalog lines too.
- A fresh session once phase 1 runs.
- **Approved by the owner for after the run starts** (2026-11-26: "you shouldn't have to go looking"; core only,
  from stored data, so not before the canary): (1) a timing and fault summary in the daily digest (median day,
  share by stage, a block's first day, the census's top lines); (2) alerts from thresholds, not judgment (a day
  at twice the median, a sensor's coverage under its own norm; the numeric stop conditions are the first);
  (3) every thing found only by looking becomes a check. The list of those, so far: the nightly's idle hour
  (no alert yet); ACT01 lost to throttling (PROVIDER_THROTTLED, C-095); the recompute blocks queued by a new
  build's methods (said in the log only).

## Proposals for the fix pass (NOT decided; discussed with the owner 2026-11-26, to be confirmed before use)
These came out of conversation while the run was being prepared. None is a ruling. Read them as options to put to
the owner when the fix pass is planned.
1. Let a gated sensor publish and record the gate: where a rule withholds a value that exists, publish the value and
   record, by name, each gate that would have withheld it, so the census can count how often a gate fires and what
   the values were. A literal missing input stays an absence.
2. Define a sensor to survive incomplete input where the mathematics allows: a ratio (a VWAP, a share) may hold under
   random message loss; a total (volume) does not, and could come from the official bar. Untested.
3. Recompute the gated sensors that way before the catalog is read, from the tapes core already caches.
4. A candidate standard for a day's own input: D-404 takes 40 CFR Part 50 for history windows (75% of the sessions,
   computed on what is available, stated); the tape's volume check is 2% (D-089). Whether the same completeness
   logic fits a same-day input is a question for the population.
5. Treat a recorded provider fault as a claim to test on the population, not as settled.
6. Classify a withholding by whether a second source for the same session agrees. A provider fault: the provider
   disagrees with itself or with an official figure (a tape short against official volume, a file that changes on
   re-download, an absent input), usually on the provider's calendar and across all assets. A market condition:
   the sources agree while the values move (spreads wide on the tape and on an independent book; volume changing
   in the tape and the official figure alike), usually uneven across assets and tied to stress or events. Where
   the sources agree the sensor should publish. Open: several inputs have one source only, and there is no
   adequate second source yet.
   Options for a second source, in the order leaned to (not decided): (a) what is free for the purpose: the
   official daily volume already delivered, Cboe's bars as in the 2026-11-20 investigation, the exchanges' daily
   summaries; (b) Intrinio against itself on the nightlies: the trades API (last 7 days) beside the tick file, each
   day, as a running record (D-411 already has live days reconcile against the trades API: first establish what
   the nightly does today); (c) a paid second feed only as a last step, with the vendor's permission in writing.

## The stance on the provider (owner, 2026-11-27; recorded at the owner's word)
"Trust, but verify." Intrinio is the provider and the default source: "unless data is literally missing from
intrinio, we have to assume that its market data." The external references are what verifies (the reference
suite, official figures, public benchmarks). A recorded provider fault is a claim to test, not a reason to distrust
the provider in general; "the provider is untrusted" is not a rule.
Applied to the known cases (figures from `PROVIDER_FAULTS.md`, not re-measured): November 2026's wider spreads and
heavier quote traffic are present data: market data, published (D-567). No quote messages before 2025-09-04 is
literally missing: a stated absence. OPEN, for the owner: a tape that is present but short (33–77% of official
volume before 2025-11-26; 1–10% short from 2026-11-06). Proposed reading, not decided: what is present is used,
the shortfall is stated beside the value, and only an absent input withholds.

## Evidence that a sensor is ready for publication (proposed 2026-11-27; recorded at the owner's word so it is not
lost; to be confirmed before the scorecard is run)
A series is one sensor on one asset. The owner's preference: all 64 assets publish cleanly for a sensor to count
as ready (the strict reading of "across all asset histories").
1. Coverage: published on at least 80% of asset-days, per asset. (Needs the withholding record to count what
   publishes as well as what is withheld.)
2. Continuity: no more than about one missing day a week; a history-based reading has at least 75% of its window
   (D-404).
3. Honest absences: every gap carries a reason that is a literal missing input or "does not apply".
4. Agreement with the independent check: no unexplained disagreement with the reference suite for that sensor.
5. Stability: no jump that lines up with something other than the market (a build change, a rule's start date, a
   change in the provider's files).
6. Reasonableness: values a practitioner would recognise; where a public benchmark exists, it tracks it.
7. It renders: its plot in Reporter is not empty and its explanation is true.
Rows 1 to 4 and 7 are mechanical: one scorecard from the database and Reporter, every sensor, pass or fail per
row with the reason. Rows 5 and 6 need a reader, only for sensors that passed the others. Outcomes: ready; a
repair candidate (fails only 1 or 2, with honest reasons: the single-sensor recompute); not ready (fails 4, 5 or
6, whatever its coverage).

## Criteria (the bar and criterion 0 are the owner's; the rest draft or proposed)
**The bar (owner, verbatim):** "My definition of functional is numerically stable across all asset histories with
>80% coverage, ideally 99%." Per sensor. The product is a data product: it requires functional sensors.
0. **The intent's own test** (`PRODUCT_INTENT.md`, "Continuous by construction", owner 2026-10-30): "each sensor
   publishes on at least 80% of asset-days, reached through numerical stability, never by filling; a miss is
   diagnosed (engine → rework, provider → stated absence, structural → not applicable). Sensors that read their
   own history (percentiles, unusualness, changes over time) can only be judged on the built database: their
   coverage is measured after the rebuild." Read per sensor over the population; the census needs each sensor's
   total (roles and coordinates published, not only withheld) to state that share: to add after the run, from
   stored data.
   Owner, 2026-11-26: during the first pass the test is read on the **base layer** ("everything should be
   populating"); the derived layer should be full too but cannot be assessed until the history exists.
   **How coverage is counted (owner, 2026-11-27; decided, not built):** by sensors with fully valid values on a
   day, not by the count of readings ("our coverage statistics should be based on # of sensors with full values,
   not the 4000-ish samples"). Three levels: (i) a reading (one sensor, one asset, one day) is complete when its
   required pieces are present; for a sensor whose reading is a series of intraday points, when at least 75% of
   that day's points are present; (ii) a sensor's coverage is the share of asset-days with a complete reading;
   (iii) the bar above is read on (ii), over all 64 assets. The 75% is the standard D-404 already cites for
   history windows, chosen by the owner here over "continuous through the day": 40 CFR Part 50 Appendix K
   §2.3(a) and (d), Appendix T §3.1(b) (as quoted in D-404; the appendix wording is to be re-read against the
   intraday case before it is built). Open, for when it is built: which pieces of a reading are required (a
   piece that is optional or not applicable by design must not make every reading incomplete); and whether a
   consecutive-gap limit is added (WMO-No. 1203 §4.4, noted in D-404 as not adopted). Scale, from a published day
   on build 760a8760: 94 measurement ids, about 4,700 readings a day (mostly 64 assets each, 17 for the company
   sensors, 1 for the cross-asset ones), a few to about 60 pieces in a reading. Today's headline ("% of readings
   published", about 99%) counts whole readings only; on the same 23 days the draft table has 55 of 64 base
   sensors at 95% or more.
1. (proposed tightening, not decided) An absence stands only where no robust definition could give a value: a
   literal missing input, or a case the measure does not apply to.
2. A rule that withholds on clean inputs, fires only on its origin dates, or is contradicted by the population:
   retire or replace by a published standard.
3. An engine fault: fixed, its days recomputed by method version.
4. A sensor is judged on its whole population: share measured, continuity, agreement with the reference check.
5. (proposed, not decided) The count of gates and rulings is reported before and after the fix pass; a new rule
   needs population evidence.
6. (proposed, not decided) The tape-read sensors before 2025-11-26: try a definition robust to the tape's shape
   first, tested against an independent bar on a handful of sessions; a late-2025 start or another source if that
   fails.

## Before the run (2026-11-26: integration, the stage-1 audit, the UUP check)
Sources: engine `docs/DECISIONS.md` D-562, D-563; core `docs/DECISIONS.md` C-094;
`~/claude-artifacts/audits/2026-11-26-stage1/triage/`; `~/claude-artifacts/checks/refsuite-rulings-standards-2026-11-26.md`.

### SCIENCE
1. **Dividend projections vs the parity forward.** D-153/D-154 project dividends; the standard (Cboe VIX methodology
   §3(a)(ii)) takes the forward from put–call parity and projects none. UUP 2024-07-11: 1.84 projected, 1.3171 paid
   (D-560 withholds that expiry). UUP 2025-12-03 IV30 0.0723 at D-154's amount, the upper part of what its thin book
   allows (5.8–7.3% by calculation). Proven (engine test, D-560 correction). Thin books need a fallback.
2. **D-560's gate reads eight two-sided strike pairs, not the full ladder.** On 2025-12-10, 6 of 316 admitted expiries
   lie outside full-ladder bounds; SPY and TSLA fronts have an EMPTY full-ladder interval (no forward satisfies every
   quote). A tolerance or a different construction is a population question. Proven by calculation on a saved capture.
3. **D-560 has no tolerance:** the census on 2025-12-10 withholds 7 of 1,018 expiries by 1–28 bp of spot (IBIT 9d by
   half a cent; HYG 16d and UNG 30d sit beside the 30-day point). Count per day in the run. Census file:
   `~/claude-artifacts/checks/forward-bounds-census-2026-11-26.txt` (not a pool replay: DBA/UUP use the file's amounts).
   Seven saved sessions (248 asset-days): 32 of 3,964 expiries withheld (0.8%); per day 7, 2, 0, 0, 0, 11, 12
   (12-10, 12-16, 12-22, 12-24, 12-26, 01-14, 03-20; the four middle days hold 14 assets each, the others 64).
4. **Parabolic SAR:** TA-Lib reverses on a touch, the engine on a cross (suite ruling #7 hides it); 4 cases in 13 saved
   days (EEM). Changes published SAR and trend values if adopted. Agent finding, TA-Lib source read; not re-verified.
5. **Hammer candlesticks** (rulings #6, #39): own rules vs TA-Lib's CDLHAMMER (10-bar averages). TA-Lib wholesale or
   ours, decided on the population.
6. **M0 trace on a refused expiry:** for a state-11 expiry M0 writes r, q, discount and forward and labels the
   coordinates OUTSIDE_SOLVER_DOMAIN; the solver never ran. Values are withheld correctly; the reason and trace are
   wrong. Proven by reading (triage).
7. **DLM-OI-CHANGE ignores expiries whose OI is wholly unknown** (reads known OI as a new listing). Rare; frequency
   unknown because VS-O04's unknown-OI cause is still open (D-517).
8. **The implied spot is chosen before D-560 rejects a fit** that may have moved it. Uncertain.
9. **European rows skip the forward bounds.** No European contract among the 64 (D-199/D-496).
10. **VS-L01 and VS-O04 reasons changed without a method bump** (D-561, by D-295): a recompute selected by method
    version misses old reasons. Matters only for days published before the frozen build (none kept in phase 1).

### CHECKER
11. A family can say RAN with no comparisons made (701 "no definition in this suite" skips on 2024-07-15; chain owners
    have no family entry). Reference errors are counted and reach core, so the run's record is not blind.
12. The referee's `engine_forward` discounts dividends at r; the engine at r − q (UUP 2025-12-10: 27.76724 vs 27.76877).
13. A damaged cached M0 stream is a SKIP, not a failure.
14. Three owners have no reference check: SAI-EXQ01, SAI-EXQ01-TEMPORAL, M0-PRINT-SIZE (by the owner's 2026-11-04
    decision; withheld on phase-1 history anyway).
15. Two suite readings deviate from the standard the engine follows: forward net of dividends (#33), downside
    deviation on simple returns (#38). Citations from memory: verify before use.
16. 44 suite rulings are our own decisions (no published standard found); judged on the population.
17. 107 of the suite's own tests skip without captures on the laptop; not run anywhere for this batch.

### OPS
18. D-515's memory gate trusts a small reading from a cached or interrupted fill (11 MiB + 2 GiB for a 5.6 GiB cold
    fill); it reads current RSS, not the peak. Not ordinary on fresh pods. WATCH IN THE RUN: memory deaths.
19. A download and the joined reference check can share the same headroom (rare).
20. D-520: a progressing company capture can still trip NO_PROGRESS (alert only).
21. RESOLVED (C-094 amended, owner 2026-11-26): the gate asks "can the engine complete a block"; only a
    whole-measurement fault on every asset holds the ramp.
22. PARTLY RESOLVED (C-094 amended, owner): failed DAYS never pause the queue. Still pausing the whole queue,
    the nightly included (verified 2026-11-27): a job that is not a day block out of attempts (a TAPE job;
    `work.rs` `pause` after the ordered-pass case) and a refused bundle (`core-api/src/worker.rs`, "bundle
    refused"). "The nightly is never paused" is decided, not built.
23. The stop flag does not cover REPUBLISH and TAPE jobs (by C-087; recompute is off for the run, C-094).
24. Core, rare or cosmetic (triage C3, C5, C9–C14, C16, C18, C19, G5, G11–G15): flag read outside the scheduling
    transaction; `vyreon-core queue` sweeping defeats the start grace; the stop rule ignores the history layer; digest
    wording beside a held recompute; a half-counted day in the readings share; release step 11 weak under holds.
25. Gemini's headless audit of the engine fails on a command permission (not quota); its settings are the owner's.
26. 47 engine tests need `workspace/fixtures` copied by hand (done twice now: missing automation).
27. `the_m0_lockstep_leaves_a_bounded_worker_room` fails on the 20-thread laptop (its uncapped case kept room).
28. `target-worker` is 96 GB on the laptop (image build cache): can it be trimmed.
31. A TAPE job out of attempts still pauses the whole queue, the nightly included (same shape as old item 22).
32. The core image build needs a hand-made Docker config on the laptop (Docker Desktop's credential helper fails:
    `DOCKER_CONFIG` at an empty `{}` plus `DOCKER_HOST`). Done by hand twice: missing automation.

### TESTS
29. D-518's and D-561's tests manufacture inputs (an empty store, a cleared endpoint): outside D-102's real-data rule.
30. D-522 validates the hash's form, not its members. D-515's tests accept the 11 MiB underestimate.

## During the run
(Empty until the canary passes. Each line: date seen, what, evidence, kind.)

Seen before the canary, on the 11-25 nightly (job 30062, build 714e47a3; logs copied from the pod, 2026-11-26):
- **The nightly's download idles an hour.** 168 min = 109 min of fetching (options 44, quotes 36, carry/macro/company/
  ACT01 19, EXQ01 8.5) + 59.5 min asleep until EXQ01's source cutoff (`exq01_cohort_capture` `DEFAULT_MARGIN_SECONDS`
  3600; the capture took 8.5 min; ACT01's window is 15 min, D-457). On the critical path of every nightly and of a
  block's first date where EXQ01 is captured. Proven (fill log, `fill::cutoff_wait`). OPS.
- **ACT01 lost 28 assets to HTTP 429** from Intrinio: three tries in 10 minutes, then given up (and the fill then
  slept the hour above). 140 engine faults on 11-25 (0 on 11-23 and 11-24); 177 of 2,853 readings withheld. Eight
  recompute blocks shared the budget at the time: 1,300–1,835 requests a minute in total, under core's 1,900
  ceiling. Hypothesis: the provider's real limit for those endpoints, or per second, is lower than 2,000 a minute.
  Count 429s per day in the run. OPS / PROVIDER.
- The 11-25 nightly's timings are not a clean benchmark: it shared the request budget with eight blocks.
- **The droplet's capture runs outside core's request count** (`vyreon-capture.timer`, 12:00 and 02:00 UTC; the
  12:00 run took 2 h 43 min on 11-26 and fetches ACT01 and EXQ01 as the cluster's nightly does). Not running when
  the 11-25 refusals came (21:07 UTC). Core is not reachable from the droplet. FOR THE OWNER: retire it once the
  cluster's archive is verified, or expose core. OPS.
- **Withholdings on most assets that are not about history** (census, 11-25 alone, one day is no population):
  PCR-UACT-I01 and DLM-VWAP / SOURCE_VOLUME_RECONCILIATION_FAILED on 55 assets; PCR-UQTR-I01 /
  QUOTE_COMPLETENESS_UNVERIFIED on 64; CDD-MACRO01 / INVALID_REQUIRED_INPUT on 64; VS-V03-MATCHED-NATIVE /
  INSUFFICIENT_ELIGIBLE on 25. All base layer except DLM-VWAP (day-local), by their revisions. If these hold
  across the run they are against criterion 0. SCIENCE.
- **The slot share ("% of readings") does not count role and coordinate withholdings.** The census on 11-25:
  DLM-UNUSUALNESS / role / INSUFFICIENT_HISTORY 13,657 on 64 assets, and 30+ further lines. What is expected on a
  nightly and what is not is a question for the population (`vyreon-core faults`). SCIENCE / OPS.
- A core test failed once under load and passed alone (`watch.rs::a_problem_opens_while_the_digest_counts`, a
  timing assertion). TESTS.
- **From the sensor trace** (`docs/SENSORS.md` section 4; the agent's reading unless marked verified). SCIENCE:
  - PCR-UQTR-I01 is withheld on every asset for every session from 2026-11-06 by a date constant (D-411 gate 3,
    provider fault P-04: the replay book's width is not verified). Verified in code
    (`h3-replay/src/official.rs:60`). 100% withheld on phase 1's last sessions and on every nightly until a
    quote-completeness check exists.
    NOT REMOVED (owner asked 2026-11-26, "if that's simple to remove"): the cause is ongoing, not one day.
    `PROVIDER_FAULTS.md` P-04: from 2026-11-06 the replay files' best bid/ask is about twice as wide as before
    (SPY median relative spread 1.36e-5 on 11-05, 2.63e-5 on 11-12) and carry 1–10% less volume; reported to
    Intrinio. Without the gate the published widths would be about double the real ones. It touches 11 of
    phase 1's 597 sessions and every nightly. The same fault is the SOURCE_VOLUME_RECONCILIATION_FAILED line
    below (the 2% gate fails 56–63 of 64 a day from 11-06, 3–9 before).
    OWNER, later the same evening: "the market has been under a lot of stress so bid-ask spreads widening and
    staying wide during what has essentially become a large market correction is not necessarily a provider
    fault. In fact this kind of change in the market data is exactly what we are here to try and reveal through
    our sensors." So the gate may be withholding the product's own signal. HIGH PRIORITY for the fix pass (the
    live product reads recent sessions): compare the replay spread with an independent quote source on both
    sides of 11-06 (P-04's record holds a midpoint comparison only); if it agrees, retire the date gate for a
    daily check that withholds only on disagreement. The volume deficit against official volume is a separate
    question the market does not explain. Not changed before the run: the tapes stay available, so the 11
    sessions and the nightlies can be recomputed; open questions: does the nightly take volume from the trades
    API as decided; is there a second Intrinio source for the best bid and ask.
  - SOURCE_VOLUME_RECONCILIATION_FAILED (PCR-UACT-I01, DLM-VWAP; 55 assets on 11-25) is the 2% day check against
    the official daily bar as first captured the night after; a preliminary bar is a recorded precedent (D-089).
    Is a nightly recomputed after the final bar: not established. Count it on history days.
  - CDD-MACRO01 / INVALID_REQUIRED_INPUT on 64 assets: 8 roles each points at one series (INDPRO), not delivered
    or failing a metadata gate that D-508 changed on 2026-11-25. Inferred.
  - DLM-IV-TERM / OUTSIDE_VALIDATED_SURFACE_WINDOW is unconditional for tenors 7, 14 and 180 (private roles):
    leave it out of coverage ratios.
  - DLM-EXPECTED-MOVE / WIDE_STRADDLE_QUOTE: a 25% half-spread limit plus a surface fallback a front expiry under
    7 days can never use.
  - DLM-PCR-HISTORY / HISTORY_OF_ANOTHER_METHOD_VERSION points at a VS-O06 label change between 11-24 and 11-25.
    Inferred.
- **PCR-UACT-I01's volume reconciliation fails on almost every asset on almost every published day, 2024 days
  included** (verified by query, 2026-11-26, 23 published base days: 62–64 of 63–64 assets carry
  SOURCE_VOLUME_RECONCILIATION_FAILED on at least one role on every 2024 day but one; 2024-09-05 has none;
  55–59 on the three nightlies). So it is not P-04 and not a nightly effect; the provider-fault record's "3–9 a
  day before 11-06" does not match the built days. A base-layer sensor against criterion 0. How many of its roles
  each asset loses, and why 2024-09-05 is clean: not established. SCIENCE, HIGH.
  CORRECTION (same evening, the coordinator had not read the record): this is `PROVIDER_FAULTS.md` P-23, known
  since 2026-11-20. Intrinio's replay files before 2025-11-26 hold 33–77% of the consolidated volume (median
  short by 37–57% to 2025-09-03, 59–66% to 10-22, 16–31% to 11-25, ~0.3% after), have almost no quotes before
  2025-09-04, and miss the closing auction print on 13–23% of sessions. D-411 withholds on it by the 2% day
  check (`consolidated.rs` `RECONCILIATION_TOLERANCE`), the official-bar equality and the two-sided-book rule.
  So for about 361 of phase 1's weekdays (to 2025-11-25) the tape-read sensors (PCR-UACT, DLM-VWAP, the tape
  price family where the close is missing, the spreads before 2025-09-04) will be withheld as provider absences:
  they cannot reach 80% over phase 1 on this source. The clean day 2024-09-05 is unexplained. FOR THE OWNER at
  the evaluation: these sensors' population is in effect 2025-11-26 onward unless another source is found.
- A rollback to an older core image is NOT one command: an older binary's migrate step refuses a database that
  holds migrations it lacks. A rollback needs a forward fix or a hand step. OPS.
- **One sensor over a range costs almost a full day each.** `vyreon-core republish ... --plan` on build a51cb572, 2024-07-09..12:
  PCR-UQTR-I01 (3.2% of the day's slots, 99 s of its own compute) is 2.7 node-h a day against 3.6 full; VS-IVS01
  and its dependants (40.9%) cost the full day. The overhead is the whole input download (5,796 s, options and
  quotes included for a tape sensor), intent A's history nodes (2,744 s) and one job per day. Proposed (not decided)
  as an early engineering item of the fix pass: fetch only the inputs a sensor reads (core caches the reduced tapes),
  skip the stages it does not depend on (`docs/SENSORS.md` section 3), run scoped days in blocks. OPS, HIGH.
- **Audit of the evening batch, core** (Codex and Gemini, 2026-11-26; triage
  `~/claude-artifacts/audits/2026-11-26-evening/triage/core-evening.triage.md`): 17 findings, 14 real, 2 by
  decision, 1 uncertain; NO BLOCKER. C-097 and engine D-567 were not in the audited diff. Catalogued, OPS:
  - The census reads only `body.measurement.value`, so it misses FIELD and company-schema withholdings
    (FIELD-FDDIM01, CDD-EVENTS01 on the frozen 2025-12-10 exports). Bodies are stored: recoverable; the tally
    (C-097) shares the path and needs a recount when fixed. READ THE CENSUS AS A LOWER BOUND until then.
  - A refusal message holding a newline or NUL would fail the watch's summary and restart core
    (`watch.rs:1104`); the released worker strips control characters, so it cannot send one. Fix with the next
    core release.
  - `budget::draw` scans the never-pruned `core.budget_lease` inside its lock; cost at the run's row counts is
    unmeasured. WATCH at the 10–20 day checkpoint (draw latency).
  - A pod evicted or killed for memory leaves no `run-logs` for its day in flight (the upload needs a live
    lease); its death, cause, steps and refusals are still recorded. Known and stated by D-566.
  - After a 429 the backfill may get little or nothing for minutes (ceiling halved, less the nightly's reserve):
    by decision (C-095); how long in practice is unmeasured.
- **Audit of the evening batch, engine** (Codex and Gemini; triage
  `.../2026-11-26-evening/triage/engine-evening.triage.md`, code read at a51cb572): 20 findings, 18 real, 1 by
  decision, 1 uncertain; NO BLOCKER, no point-in-time break, no number change. D-567 was not audited. Not yet
  written into the engine's DECISIONS (a commit there would move the release line off the released build): do it
  with the next engine change. Catalogued, OPS:
  - A refused request's body text enters the failure line and the retry classification (`retry.rs:303`, `:341`):
    it can crowd out the real failure, or turn a throttle fatal if Intrinio's 429 body holds words the classifier
    reads as fatal. Intrinio's 429 body has never been seen. AT THE FIRST RECORDED REFUSAL, READ ITS BODY
    (`vyreon-core budget`, `core.provider_refusal.message`): it settles this, and whether a body could hold a
    secret (no path found: the key travels as a Bearer header).
  - A `provider refusal:` stderr line can become the last line of a capture batch's output and hide its
    `"failed": N` at exit 0 (`fill.rs:2073`, `:1513`); the failure still surfaces, its per-chain class is lost.
  - ACT01 never carries a Retry-After (`act01_auto_source.rs:290`), so D-565's floor and stop do not apply to it.
  - Run logs are gathered before the perishables are archived (`vyreon-worker.rs:1104`): a stalled read or a
    full disk could delay the perishables' archive. Rare (37 MB of logs on job 30062).
  - EXQ01 could outrun its 30-minute margin under contention: by decision (D-564); the nightly's reserve is
    meant to prevent it.
- **Benchmark, the canary's download (job 30086, 2025-12-10, a cold single day, 2026-11-26 evening; from
  `vyreon-core steps`):** 98 min = options 39, quotes 35, carry/macro/company/ACT01 9, EXQ01 under 1, idle for the
  captures' cutoffs 14. The 11-25 nightly that afternoon: 168 min = 44, 36, 19, 8.5, 59.5. Options and quotes took
  the same time with and without eight other blocks sharing the budget: what limits them is not established. A
  history day still idles 14 min for ACT01's window (once per block, by D-457). OPS.
- **Why runtimes vary** (stored reports of the 23 published base days, 2026-11-26): compute is 86 to 328 min; the
  variation is all in two stages, intent B (40–164 min) and intent A's history nodes (21–140). Where task-minutes
  are stored (12 days), each stage's wall time is its task-minutes over about 7 on every day (intent B 6.9–7.3,
  intent A 7.0–7.5): the 8 threads stay busy, so it is not idle time or a queueing effect. What varies is the
  task-minutes themselves, for the same task list: intent B 277 to 589 (2024-07-23 against 2024-08-13, one
  build). Not established: whether a task takes longer because the node gives less CPU (shared vCPUs; a next
  day's download in the same pod) or because the day holds more work (contracts, expiries). The stored pair per
  task does not hold CPU-minutes; the worker's task-times log does (kept in `run-logs` from build a51cb572):
  compare CPU-minutes with task-minutes per stage on the run's first days. A later day of a block skips most of
  intent A (11–13 task-minutes against 283–598). OPS.
- **The canary day (2025-12-10, revisions 259/260, build a51cb572), census:** SAI-EXQ01 and M0-PRINT-SIZE
  EVENT_SOURCE_NOT_RETAINED on all 64 (a history day keeps no event source; by the owner's 2026-11-04 decision);
  CDD-MACRO01 INVALID_REQUIRED_INPUT on all 64 (so not a nightly effect: it is there on a 2025 day too);
  PCR-UACT-I01 and DLM-VWAP SOURCE_VOLUME_RECONCILIATION_FAILED on 7 assets (BABA, EFA, EWY, TLT, TSM and two
  more), as P-23's record says for the era after 2025-11-26; VS-V03 INSUFFICIENT_ELIGIBLE on 7. SCIENCE.
- **The "% of readings" figure does not count stated absences at slot level either:** on the canary day the tally
  says 0 of 2,853 withheld while SAI-EXQ01 and M0-PRINT-SIZE are withheld whole on 64 assets each (128 slots). The
  tally's "withheld" counts slots carrying the engine-fault marker (`watch.rs` `COUNTED`), not every withheld
  slot. So a day can read "100% of readings" with whole sensors absent. Read by the coordinator in the SQL and on
  this day; what the figure was meant to be is the owner's to say. OPS, HIGH (it is the owner's headline number).
- **Memory on the canary:** the container's cgroup peak is 12,291 MiB against its 12 GiB limit; it counts file
  cache, so it does not show how near the processes came to the limit (6,268 MiB the parent, 7,002 MiB the
  largest child; whether they overlapped is not recorded). No restart. WATCH memory deaths in the run. OPS.
- **2026-11-27 02:55 MT: a worker was killed for memory in a reference check** (pod day-block-kbrvz-8c6xp, job
  30089, the re-run of 2026-11-25 on c2397caa; OOMKilled, exit 137, 12 GiB limit). The day had published at 02:12
  and stays published; the kill came 42 minutes into its reference check (8 jobs), run alone on the block's last
  day. Core said it on the alerts channel ("killed for lack of memory"). Lost: that day's reference report (the
  list still shows the afternoon nightly's, job 30062) and the check's own log, which is in no archive (the
  evening audit's engine finding C7, documented, not fixed). The same session's check completed that afternoon
  in 35 minutes, when 28 assets had no ACT01 capture; the canary's (a 2025 day, no event source) took 91 minutes
  and completed. Hypothesis: the reference suite's work on a session with full ACT01 and EXQ01 captures needs
  more than the node gives at 8 jobs. If so the nightlies are the days at risk, and in a block a check runs
  beside the next day's work, so a kill there costs the day in progress and a cold start. Pod log kept:
  `~/claude-artifacts/checks/oom-2026-11-27/`. Re-runs of reference checks are off for the run. WATCH: every
  nightly's check; memory deaths in blocks. OPS, HIGH.
- **2026-11-27 10:01 and 10:07 MT: the first recorded provider refusals** (`core.provider_refusal`: 228 HTTP 429 on
  3 of 8 running blocks, two bursts of 179 and 49; PROVIDER_THROTTLED alerted by itself; no job failed). What the
  record shows:
  - Intrinio's words, the same on every one: "You have exceeded the limits for how frequently you can make API
    calls. Please contact support if you need a higher limit." No rate-limit header, no Retry-After. Nothing the
    failure classifier reads as fatal, no secret: the evening audit's worry about the body is settled.
  - Core's own count of requests in the minutes before: 1,340, 1,367, 1,341, 1,599, 1,399, then 753 in the minute
    of the first burst. Never near 2,000. Intrinio's usage endpoint read at 10:09 agreed with core's count for
    that minute (697 at 54 s against core's 736) and lists a one-minute limit of 2,000 per access code (`api`,
    `opra_historical_intervals`, `us_fundamentals`, ...), each far under.
  - The droplet's capture was not running (it finished at 12:02 UTC).
  - 126 of the 228 were on `/securities/{NVDA,MSFT,AMD}/prices`, each from a single worker within a minute.
  So the one-minute count does not explain the refusals, on this day or on 11-25. Hypothesis (not tested): a
  shorter window. A worker that gets a budget lease may send up to 600 requests at once, so several hundred can
  land in a second; core bounds the minute, nothing bounds the second. If so, lowering the minute ceiling does
  not help and pacing requests inside a lease would. Cost while it lasts: core halves the ceiling for five
  minutes after each refusal. WATCH: refusals per hour and whether any capture gives up. OPS / PROVIDER, HIGH.
- **The spend line lags by up to a day and can read backwards** (2026-11-27 10:36 MT): core's hourly readings all
  carry DigitalOcean's `generated_at` 2026-11-27 03:43 UTC (21:43 MT the evening before), and the month-to-date
  figure fell from 384.41 to 376.38 between two of them, so the line says "-$1.62 a day" with nine worker nodes
  running. Core reads what DigitalOcean serves; the figure is DigitalOcean's and is refreshed about daily. So a
  runaway cost shows a day late. By arithmetic, not measurement: 9 `s-8vcpu-16gb` nodes at $0.167/h are $1.50/h,
  12 are $2.00/h (about $48 a day), beside the core node and the database. OPS.
- Words on the channels (owner's paste, 2026-11-27): the plain-words rule (C-091) leaves holes where an id was
  ("release: (owner): ...", "Daily runs stopped: was withdrawn"); the memory kill's line says "about 52 min of work
  to redo; nothing left to redo"; the progress channel does not say when a day publishes. OPS.
- **The reference check's failed and unexplained comparisons are read once the database is full, not before**
  (owner, 2026-11-27: "something we need to investigate but only once we have a full database... Not something to
  investigate now, just record it."). Recorded so far on the frozen builds: the canary 2025-12-10 (job 30086):
  94,363 checked, 2,482 failed, 230 unexplained, 154 classes, 20 measurements, 8 reference errors. Each day's
  report is in core (`vyreon-core refcheck list`, `refcheck show <date>`); the REFCHECK_DISAGREES alert stays open
  through the run and is evidence, not an alarm. CHECKER / SCIENCE.
- **The live-day reconciliation against the trades API was decided and never built** (read-only trace,
  2026-11-27, `~/claude-artifacts/checks/nightly-volume-source-2026-11-27.md`; the coordinator verified the two
  key points). On a nightly every tape-read sensor takes its trades and volume from the replay file alone; no
  equity trades endpoint is called anywhere in the engine. The volume check compares the tape's own share count
  with the official daily bar at 2% (`consolidated.rs:447-453`). The engine's decision record says of the
  trades-API reconciliation "Live days (droplet phase, not built yet)" (`docs/DECISIONS.md:3467`); core's
  `PROVIDER_FAULTS.md` P-04 states it as the owner's decision. So since 2026-11-06 the nightlies withhold
  PCR-UACT-I01 and DLM-VWAP on most assets (55 of 64 on 11-25). ACT01 and EXQ01 are option captures (30-minute
  bars and trades of 20 contracts) and have nothing to do with equity volume. Tape and official volume are
  already kept per asset and day in each tape cache's `derived/derived.json`, so a daily record of the two needs
  no new capture. Not established: the size and sign of the difference on 11-25. For the MVP this is a candidate
  BLOCKER on recent days if the report needs intraday volume or VWAP. SCIENCE / OPS, HIGH.
- **Decided and not built: the sweep** (read-only, 2026-11-27,
  `~/claude-artifacts/checks/decided-not-built-2026-11-27.md`; 39 items settled: 8 decided and not built, 3 stated
  as fact and not true in code, 16 built since (stale text), 12 deferred on purpose; about 150 phrase hits not
  checked in detail and 7 not established, listed in the file). The consequential ones:
  1. The live-day trades-API reconciliation (above). Verified.
  2. `PROVIDER_FAULTS.md` P-04 still said the quote widths are withheld by the date rule; D-567 retired it.
     Corrected in that file 2026-11-27.
  3. Engine D-404's "skip and state" is not applied to the EVC history view: `vyreon-cli/src/day.rs:1150` fails on
     the first problem tape session. Code read by the coordinator; its effect on a day is not established.
  4. Engine D-396 (owner, 2026-11-19): a bulk option chain missing contracts the snapshot shows active should go
     to the API; only a count (`bulk_snapshot_absent`) exists. Its effect on open interest and volume: inferred.
  5. "Recompute only where published values change" (owner, 2026-11-26): core's planner still selects by method
     version; the commit cited as applying it (8d0b685) changed documents only. Verified. Recompute is off for the
     run, so nothing is affected yet; it matters before recompute is switched back on.
  Also: core's OCC volume check is a hand-run tool nothing schedules; `format=parquet` on the reader API is
  absent; there is no canary job and `release.sh` does not refuse without a real day; the D-140 rule on a missing
  bar is unimplemented. SCIENCE / OPS.
- **The pace inside a block, first measurement (job 30091, its second day 2026-09-18, 2026-11-27):** 195 min from
  one day's publication to the next day's start of compute: plan and surfaces 51, intents 9, intent B 104, then
  30 min waiting for the previous day's reference check to finish (it runs beside the day and is joined before
  the next day starts). The cold first day took 286 min. One day on one block, with other blocks starting beside
  it: not yet a rate. If it held, 50 days are about 6.8 days of wall time per block and the whole day layer
  about a week with 12 blocks, near 2,000 node-hours ($330 at $0.167/h, by arithmetic). The 30-minute join is a
  sixth of the cycle. OPS.
- **Decided and not built: the full sweep** (all phrase matches checked, 2026-11-27; four files
  `~/claude-artifacts/checks/decided-not-built-2026-11-27*.md`; about 330 entries read, most incidental). Beyond the
  first pass's items, labelled as the sweeps gave them unless marked verified:
  Affecting published values or the nightly:
  - Engine D-136 rule 2 (owner's dividend work): an actual ex-date replaces a projected one. The rule exists in the
    kernel but the dividend request ends at T, so no later ex-date ever reaches it. A dividend payer's surface,
    forwards, IV30 and Greeks rest on projected ex-dates (bears on catalog item 1, UUP). PROVEN for the request,
    inferred that no other path feeds it.
  - Core C-094: the whole queue still pauses on a TAPE job out of attempts and on a refused bundle (item 22).
    Verified. DURING THE RUN: either one stops every block and the nightly until `queue resume`.
  - Engine "Rules in force" 5: the company template is chosen by the ticker "JPM"
    (`h5-company/src/lib.rs`, `Template::for_ticker`). Verified. Ticker-specific handling.
  - Engine "Rules in force" 3: no production classifier of withholding causes exists; only a draft tool table,
    with 17 of 41 sampled reason codes unclassed (inferred). Bears on the census's ENGINE / STATED split.
  - Engine D-102: the trades-API tape as the intraday reference: only a coverage stub exists.
  - Engine D-274: longer P01/PATH views (still 153 captures). Inferred.
  - Engine note on D-162: a refused translation publishes a generic reason.
  - Core C-060: the weekly baseline fixed at 252 sessions; the engine's API still uses every earlier week. Inferred.
  Not affecting published values:
  - Core C-069: a post Discord refuses with a 4xx is retried forever ahead of every later post; background tasks'
    join handles are dropped. Core C-026: no login in the `vyreon_reader` role. Engine D-258: `/v1/review` omits
    the flag line's basis. Engine D-461: six tape-reader findings assigned to a later bundle and not done. Engine
    D-521: INT-IVSC01's end-to-end tests still ignored.
  - AGENTS.md states as fact what is only partly true: "8 worker nodes" (the manifests allow 12); "Scaling goes
    only through core" (hand Job manifests and `scale-pool.sh` still ship); the API's default detail.
  Not settled: 37 items across the three files, most needing the cluster or the owner; 13 "Listed, not fixed" rows
  in MILESTONES not checked. Stale text found: MILESTONES said the D-560 census stopped unfinished (it completed).

## What needed an agent (kept through the run; evaluated in step 6 of the plan of record)
Each line is something that happened only because an agent looked or acted. State as of 2026-11-27 noon.
| What | State |
|---|---|
| Resuming a paused queue after one job fails (a tape job out of attempts, a refused bundle) | being built on core `core-s8`, unreleased |
| Noticing provider throttling | alert built and working (C-095) |
| Reading why a worker died | alert built and working (C-091 `lease_death`) |
| Seeing when days publish | fixed on core `core-s7`, unreleased (C-100) |
| Slowing requests per second | built on core `core-s6`, unreleased (C-099); still needs someone to switch it on |
| Building images and running a release | by hand, with a Docker credential workaround for the core image |
| Queuing phase 1 in order (the newest block first, the rest once it was claimed) | by hand |
| Queuing the history passes after the day layer | by hand; not yet done |
| Re-running a day whose capture was lost | by hand (the 11-25 re-run) |
| Re-running a reference check that was killed | by hand; the automatic re-run is off for the run |
| Judging whether the run is healthy enough to continue | by hand; numeric stop conditions owed |
| Reading a pod's logs to explain a slow or lost step | by hand; `run-logs` are kept, nothing reads them |
| Checking what DigitalOcean actually charges against the spend line | by hand; the line lags a day |
- **A core restart kills a worker that is in its first minute** (2026-11-27 11:56 MT, the C-099 release): pod
  day-block-dh8dc-csl9t (job 30102, started 11:55) was fetching tape caches from core, got "error sending
  request" and exited 1. The eleven running blocks rode through the same restart (as they did five times before).
  The job waits for its dead lease to expire. A release during the ramp can hit a starting pod. OPS.
  RECOVERED BY ITSELF, no one acting (owner asked to be told): pod died 11:56; lease ended EXPIRED 12:06:12 and
  the job was queued again; core posted "a block (Jul 8 – Sep 16) ... stopped at 12:06: the worker exited with an
  error (code 1); about 11 min of work to redo; it starts again by itself" at 12:07; a new pod
  (day-block-fhj6v-85xnn) claimed it at 12:18:39, attempt 2. 23 minutes lost on one block: 10.5 for the lease to
  expire, 12.5 until the next pod claimed (the ramp's spacing and a pod's start).
- **The gate register** (read-only inventory, 2026-11-27, `~/claude-artifacts/checks/gate-register-2026-11-27.md`;
  the agent's reading: NOT yet verified by the coordinator; engine `worker` at 760a8760). 84 rows. Of 41
  candidate gates (thresholds 17, count minimums 12, consistency checks 10, dates 2): origin owner 12, a published
  standard 9 (six of them the one 40 CFR 50 citation of D-404), agent or engineering 20; evidence at origin none
  stated 10, one date 7, 2–5 dates 4, more 11, a standard instead of data 7, decision not read 2. About 140
  further reason codes named and not traced; 12 rows that fire in the census only partly traced. It judges
  nothing: it is what each gate is read against when the population exists. Firing counts in it are from 4 days
  and are not a rate. SCIENCE.
- **Refusals under the per-second bound (2026-11-27 13:49 and 13:51 MT):** 28 more (11 and 17) on 3 workers, with
  the grants of every second at 30 or less (`core.budget_lease`). Smaller than before the bound (179, 49, 105,
  59), not gone: one comparison, not a result. What the record rules out as the whole cause: the minute (1,590
  and 1,022 in the minutes before); a sustained rate (12:16 to 12:39 ran 24 minutes at about 1,600 a minute with
  none); the ten-minute total (15,090 before 13:49; it reached 16,170 without a refusal). Intrinio's usage report
  at 13:53 again shows only one-minute limits of 2,000 per access code, all far under. The refused requests are
  those of one stage of a worker's download (options intervals of ACT01, fundamentals, security prices, option
  snapshots), not the bulk of the traffic. Hypothesis, untested: a limit on those endpoints or on a short burst
  to one of them. No capture is known to have been lost. WATCH. OPS / PROVIDER.
  Concurrency is NOT ruled out (owner's question, 2026-11-27). From `core.budget_lease` since 07:30 MT, by how many
  workers were drawing in a minute: 1 worker, 137 minutes, none with refusals; 2 workers, 73 minutes, 1 with; 3
  workers, 56 minutes, 4 with; 4 workers, 32 minutes, 2 with; 5 workers, 2 minutes, none. Every refusal came with
  two or more workers downloading at once. Seven minutes with refusals in all: suggestive, not a result. Core
  counts requests granted; nothing records how many requests are in flight at one moment, which is what a
  concurrency limit would act on.
  14:44 MT, the largest episode under the per-second bound: 175 refusals on 3 workers in one minute, 159 of them
  on `/options/interval/<contract>` (ACT01's 30-minute option bars; Intrinio's access code
  `opra_historical_intervals`), the rest on price adjustments and fundamentals. All three workers were in the
  same stage of their download at once. Core had counted 1,590 to 1,650 a minute for the eight minutes before, by
  2 to 4 workers. So the bound on grants does not hold the episodes down (28, then 175), and the evidence now
  points at that endpoint when several workers call it together. No job failed; the days of the blocks hit so far
  publish with 0 engine faults. Pace over the last hour: 3 days published by 12 blocks. WATCH whether the
  episodes slow the run (each halves the ceiling for five minutes and sends ACT01 into its retries).
- **Blocks idle for hours waiting on a reference check (2026-11-27 16:28 MT; core raised NO_PROGRESS by itself for
  job 30093).** A day's reference check runs beside the next day and must finish before the day after starts
  ("joined"). Three of the twelve blocks were waiting on one with nothing else to do: 2024-09-18's block for 170
  min (its check of 2024-09-18 had run 213 min and was still working: 8 processes at 97% CPU, caches written
  minutes before; the pod held 10.4 of its 12 GiB), 2024-11-27's for 89 min, 2025-02-12's for 87 min. The seven
  checks that have completed on this build took 44 to 127 min (188 to 335 CPU-minutes). Earlier builds' records
  show the same split on 2024 days: 19 to 81 min for some, 192 to 358 min (1,300 to 2,150 CPU-minutes) for
  others; which family of checks is slow is not established. The check's deadline is 600 min. Cost: on such a day
  the block's cycle is the check, not the compute (2 h), so these blocks could run at half the pace or less.
  Memory during a check is near the limit: the one kill so far was in a check. WATCH: how long these three take;
  kills. OPS, HIGH (it bears on the finish date).
- **The records checkpoint at 23 days (2026-11-27 16:45 MT): the run leaves its records.** Of 23 published days:
  `run-logs` KEPT for 23, a full run report (`fill`, `memory`) for 23, the withholding tally for 23; step history
  for every lease; 644 refusals recorded. One thing does not scale: `vyreon-core faults` over the 23 days took
  121 s to print its first line (it took 0.6 s a day before the audit fix made it read every body schema; it re-reads the
  bodies instead of the stored tally). At that rate a census of 597 days is not usable. To fix on the unreleased
  core branch: the command reads `core.withheld_tally`. OPS.
  First of the three finished (2024-09-18, job 30093): the report says "suite 217 min, 396 CPU min"; the block
  waited 175 min after publishing its second day (13:38 to 16:33 MT) and then went on. 396 CPU-minutes is in the
  range of the quick checks (188 to 335), so this check was long in wall time, not in work: it ran beside the
  day's compute at the lowest priority and, by the record, did most of its work only afterwards. Why the last
  stretch took nearly three hours with eight processes busy is not established. The other two blocks were still
  waiting at 16:55 (115 and 114 min).
- **CDD-MACRO01: the industrial-production series is delivered and refused by a unit-label guard, on every asset
  on every day.** Verified 2026-11-27 in the published bodies (AAPL, 2024-07-09 and 2026-09-17, build 760a8760;
  `series_evidence`): CPIAUCNS and FEDFUNDS are SUPPORTED / MATCHED; INDPRO has 1,306 rows returned and
  `metadata_gate` = `support_state` = WITHHELD_WRONG_UNIT. From the stored tally: INVALID_REQUIRED_INPUT on 63 to
  64 assets on 23 of 23 published days. The guard (D-044, amended by D-508 on 2026-11-25; `h3-macro/src/lib.rs`
  about 292–338) accepts the unit label "Index 2017=100" or a check that the twelve 2017 values average within 0.5
  of 100. The data is present: this is a guard, not an absence. Which half of the guard fails, and since when,
  is not established. A candidate for the short list of any second pass: one input, one condition. SCIENCE, HIGH.
- **VS-V03 gives up on thin chains by a row count, not by its fit** (2026-11-27; the owner's intent for the
  sensor: "a robust local IV level by expiry, smile skew slope, and residual dispersion in log moneyness space.
  That should handle sparse chains"). In code (`h3-v03/src/lib.rs` 43–51 and the fit's comment at 124, read by
  the coordinator): one expiry (the first standard monthly 14 to 365 days out); a window in log-moneyness scaled
  by the strike spacing (the owner's D-086 and D-093; D-093 records 337 to 459 cells at 80%); then at least 3 rows
  with 1 each side for the level and at least 7 rows with 2 each side for the slope, else INSUFFICIENT_ELIGIBLE.
  The gate register traces 7, 2, 3 and 1 to D-029 (2026-09-29), an engineering choice on 12 assets over
  2026-08-14..09-18 with no reason stated for the numbers (the register's reading of the decision text). Fires
  so far on 9 assets over 4 days (DBA, EMB, HYG, INDA, TIP among them). The universe is meant to grow to the full
  market, so this does not generalise. Candidate published methods, FROM MEMORY, each to be checked against its
  source before use: at-the-money-forward IV interpolated in total variance between the bracketing strikes
  (level; needs two quotes); the 25-delta risk reversal and butterfly convention (needs quotes near 25 delta); SVI
  and SSVI (Gatheral; Gatheral and Jacquier: one fitted curve, SSVI pooling across expiries, the most forgiving;
  the owner's 2026-10-14 investigation used a pooled same-day SSVI); Cboe's VIX and SKEW methodology (needs a wide
  strip). Decision order: a published standard first. SCIENCE, HIGH.
  17:27 MT (snapshot row): 26 days published; four blocks waiting on a joined reference check (2024-11-27 for 147 min, 2025-02-12 for 145 min, 2025-07-10 for 2 min, 2026-04-24 for 18 min); no refusals since 16:39, no worker death.
  17:58 MT: 27 days published. The checks of 2025-07-10 and 2026-04-24 finished in 61 and 75 min (256 and 257 CPU-min). The two long ones still hold their blocks (178 and 176 min); NO_PROGRESS opened for job 30094. Looked at 30094's pod (read-only `ps`): the suite has run 221 min and is down to one busy Python process at 84% of a core, with the worker idle; its `refcheck.log` is empty (0 bytes), so what that process is doing cannot be read. HYPOTHESIS: these long checks end in a single-process stage that eight cores do not speed up. Not intervened: the check is working and its deadline is 600 min.
  18:29 MT: 29 days published; no refusals or deaths in 40 min. The two long checks still hold their blocks (209 and 207 min; NO_PROGRESS open on jobs 30094 and 30095). Checks that finished since: 2025-12-01 in 42 min (189 CPU-min), 2024-07-09 in 58 min (265). Precedent for a long one: an earlier build's check of 2024-07-09 (job 21814) took 366 min and 2,021 CPU-min.
- **Second memory kill, again in a reference check (2026-11-27 18:50 MT).** Job 30100 (block 2026-02-11), lease 3135, pod `day-block-j75pd-4rznp`: `core.lease_death` says "killed for lack of memory". Step history: it published and archived 2026-02-12 at 18:28, then sat 21 min at the step "reference check 2026-02-11 (joined) + reference check 2026-02-11" and died. The compute was over, so the check alone (or two checks at once, as that step text reads: HYPOTHESIS, not checked in code) exceeded the 12 GiB limit. Recovered by itself: lease 3143 on pod `day-block-hmt2q-f77ds` began 2026-02-13 at 18:49; both published days stand. Not intervened. Both kills so far (this and the 11-25 re-run) were in the reference check. OPS, HIGH.
  18:59 MT: 32 days published; 5 in the last hour; no refusals. The two long checks: 239 and 238 min.
  19:31 MT: 32 days published, 2 in the last hour. The check of 2025-02-12 finished: "suite 298 min, 668 CPU min"; its block waited about 265 min and went on (job 30095). 2024-11-27 still holds its block (271 min); 2024-09-19 is at 60 min. Job 30100 is downloading after its memory kill. No refusals, no new deaths.
  20:02 MT: 36 days published, 4 in the last hour. Six of twelve blocks are waiting on a joined reference check: 2024-11-27 (302 min), 2024-09-19 (91 min), 2025-07-11 (22), 2025-12-02 (17), 2026-04-27 (12), 2026-09-22 (3). No refusals, no new deaths.
  20:33 MT: 38 days published, 6 in the last hour. The four short waits of 20:02 cleared. Still waiting: 2024-11-27 (333 min), 2024-09-19 (122 min). No refusals, no new deaths.
  21:04 MT: 40 days published, 3 in the last hour. Still waiting: 2024-11-27 (364 min), 2024-09-19 (153 min). No refusals, no new deaths.
  21:35 MT: 41 days published, 3 in the last hour. A refusal episode: 275 in three bursts (21:07: 94 on 2 workers; 21:15: 144 on 3; 21:28: 37 on 1), total now 919; core raised PROVIDER_THROTTLED. Still waiting on reference checks: 2024-11-27 (395 min), 2024-09-19 (184 min; NO_PROGRESS open on job 30093). No new deaths. Note: the NO_PROGRESS problem for job 30094 shows "resolved 21:25" while its step is unchanged since 15:01: not looked into.
  22:06 MT: 43 days published, 3 in the last hour; no refusals since 21:28 (PROVIDER_THROTTLED closed), no new deaths. The check of 2024-09-19 finished: "suite 226 min, 462 CPU min"; its block went on. 2024-11-27 still holds its block (426 of a 600-min deadline). Long checks so far on this build: 217, 298, 226 min, all on 2024-09 to 2025-02 dates, and one unfinished.
  22:37 MT: 45 days published, 4 in the last hour; no refusals, no new deaths. 2024-11-27 still holds its block (457 of 600 min); 2025-02-13 is at 72 min on the block that had the 298-min check.
  23:08 MT: 46 days published, 3 in the last hour; no refusals, no new deaths. 2024-11-27 still holds its block (488 of 600 min); 2025-02-13 at 103 min.
