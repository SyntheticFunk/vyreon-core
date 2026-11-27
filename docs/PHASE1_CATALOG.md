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

## The owner's principle for the evaluation (2026-11-26, verbatim)
"Given that a withheld sensor is a lost sample, and knowing that we have a lot of rules and overfitting due to
sparse-date testing... my proclivity is to let everything publish then we need to systematically investigate and
ensure our sensors generalize. [...] The idea is for the design of each sensor to be numerically stable by design. A
sensor dying on 361 weekdays is a definition or rule problem." And: a reported provider fault is a claim, not a
fact, until the population shows it.
What follows for the fix pass (agreed by the owner, 2026-11-26, who added the record of the gates): (1) a gate
that withholds becomes, where it can, a stated quality beside a published value, so the value a rule suppressed can
be judged; **each published value records every gate that would have withheld it, by name** (so the census counts,
per sensor and gate, how often it would have fired and what the values were when it did); (2) a sensor is
defined to survive incomplete input where the mathematics allows (a ratio such as a VWAP or a share is stable under
random message loss; a total such as volume is not, and can come from the official bar); (3) the gated sensors are
recomputed with their gates as annotations before the catalog is read, from the tapes core already caches. The
first pass runs as built: it gives how often each rule fires.
A candidate standard for same-day sensors, for the fix pass (not adopted): D-404 already takes 40 CFR Part 50
(Appendix K §2.3(a) and (d), Appendix T §3.1(b)): a history window needs 75% of its sessions and is computed on
what is available, stated. Its scope is history windows only; a same-day input that is missing still withholds, and
the tape's volume check is 2% (D-089). Whether the same completeness logic fits a day's own input (a VWAP from a
tape holding at least 75% of the official volume, the share stated) is to be fitted on the population, not ruled.
The owner: "neither you nor I can simply do some sort of blanket ruling... what we need to do is generate N samples,
and then fit our sensors to reality."

## Criteria (draft, for the owner)
0. **The intent's own test** (`PRODUCT_INTENT.md`, "Continuous by construction", owner 2026-10-30): "each sensor
   publishes on at least 80% of asset-days, reached through numerical stability, never by filling; a miss is
   diagnosed (engine → rework, provider → stated absence, structural → not applicable). Sensors that read their
   own history (percentiles, unusualness, changes over time) can only be judged on the built database: their
   coverage is measured after the rebuild." Read per sensor over the population; the census needs each sensor's
   total (roles and coordinates published, not only withheld) to state that share: to add after the run, from
   stored data.
   Owner, 2026-11-26: during the first pass the test is read on the **base layer** ("everything should be
   populating"); the derived layer should be full too but cannot be assessed until the history exists.
1. An absence is correct when the provider or the market gives no basis (structural): stays, stated.
2. A rule that withholds on clean inputs, fires only on its origin dates, or is contradicted by the population:
   retire or replace by a published standard.
3. An engine fault: fixed, its days recomputed by method version.
4. A sensor is judged on its whole population: share measured, continuity, agreement with the reference check.

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
22. RESOLVED (C-094 amended, owner): failed days never pause the queue; the nightly is never paused.
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
- **One sensor over a range costs almost a full day each** (owner, 2026-11-26: "a system where we can legitimately
  just queue up 1 sensor over xyz dates"). `vyreon-core republish ... --plan` on build a51cb572, 2024-07-09..12:
  PCR-UQTR-I01 (3.2% of the day's slots, 99 s of its own compute) is 2.7 node-h a day against 3.6 full; VS-IVS01
  and its dependants (40.9%) cost the full day. The overhead is the whole input download (5,796 s, options and
  quotes included for a tape sensor), intent A's history nodes (2,744 s) and one job per day. FIRST ENGINEERING ITEM
  OF THE FIX PASS, before any sensor change: fetch only the inputs a sensor reads (core caches the reduced tapes),
  skip the stages it does not depend on (`docs/SENSORS.md` section 3), run scoped days in blocks. OPS, HIGH.
