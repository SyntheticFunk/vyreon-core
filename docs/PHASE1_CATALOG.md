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

## Criteria (draft, for the owner)
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
- **The slot share ("% of readings") does not count role and coordinate withholdings.** The census on 11-25:
  DLM-UNUSUALNESS / role / INSUFFICIENT_HISTORY 13,657 on 64 assets, and 30+ further lines. What is expected on a
  nightly and what is not is a question for the population (`vyreon-core faults`). SCIENCE / OPS.
- A core test failed once under load and passed alone (`watch.rs::a_problem_opens_while_the_digest_counts`, a
  timing assertion). TESTS.
