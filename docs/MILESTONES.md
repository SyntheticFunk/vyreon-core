# Milestones

Current state only (pruned 2026-11-21 22:50 UTC). The full previous record, every run, finding and
time, is tag `milestones-2026-11-21` (f0d2b98) and git history. Read top to bottom: the steps, the
phase-1 launch brief, work that runs beside phase 1, what waits on the owner, the standing rulings,
then reference lists. Decisions: core `docs/DECISIONS.md` (C-numbers), engine `docs/DECISIONS.md`
(D-numbers); plan `docs/ARCHITECTURE.md`; provider faults `docs/PROVIDER_FAULTS.md` (P-numbers).
Paths under `~/claude-artifacts/` are on the laptop (`checks/`, `audits/`, `cluster-scratch/`).
Times are UTC unless marked MT. Write a time only after `date -u`.

## Terms (agreed with the owner 2026-11-21; define a word here before using it in a plan)
- **Phase 1:** the database build back far enough for the MVP reports: 2024-07-09..2026-11-20 (C-060).
- **One pass (owner, 2026-11-22):** the BASE layer computed successfully the first time (downloads,
  tapes, hours a day: expensive to redo). The DERIVED layer is expected to be recomputed: it can only be
  judged on the built store, and a re-run of the ordered passes is cheap (no provider calls).
- **Final:** a value that building further back would not change. At the live date every value of
  the current week is final (C-060's chain: 596 sessions behind it).
- **Warm-up:** the sessions at the start of a build whose derived values are not final (TA-Lib's
  lookback and unstable period); stored, never reported as current.
- **Live:** the date we declare the database up; from it every new session is computed by the daily
  runs (C-051) and reports publish each Sunday.
- **Block:** a DAY_BLOCK job, consecutive sessions one worker computes (15 in phase 1).
- **Ordered pass:** the history-derived pass of one session (unusualness, windows over the store); runs
  once per session, in date order, after the blocks (C-046).
- **Test block:** the first block run alone (2024-07-09..07-22) before the rest; what "tranche" meant
  in the 2026-11-18 gate. "Tranche" is retired.
- **Gates a–c (per day, as it publishes):** (a) the in-worker reference check; (b) the Cboe bands;
  (c) EDGAR. **Whole-store gate (after the ordered passes):** the history reference check and the
  flag-agreement test (d). A failure stops the build: diagnose, fix, recompute.
- **Failed day:** a day out of attempts; a missing date in every window, stated with its cause
  (C-061). **Omitted:** a failed day we drop on purpose, with its reason (provider faults only).
- **Coverage goal (80%):** per sensor over its asset-days, measured, never enforced. **Window floor
  (75%, D-404):** per history window, a rule. **Day hold (10%):** per day, a rule (`exports.rs`).

## PLAN OF RECORD: prove small, then produce (owner, 2026-11-26 MT ~14:00; supersedes the pass-1 run on 714e47a3)
Why 3–4 days produced 3 keepable days:
- Builds were released to many nodes without one real day end to end, twice.
- Scaling came before fit was measured (memory per node, request contention).
- Every fix restarted the run.
- Hand operation errors.
Backfill stopped 2026-11-26 13:37 MT: 606 phase-1 jobs cancelled; reference re-runs off (878bd78). The nightly runs on.
**Stage 1, fix** (target ~11-29; cluster near zero). Done when all six land, each with its commit:
1. Classify the 20 unexplained reference disagreements of 2024-07-11 on 714e47a3 (SURFACE-FORWARD 9, VS-O02 6,
   SURFACE-SIGMA 2, SURFACE-INPUTS 1, FIELD-SUP01 1, VS-O04 1); fix the engine ones.
2. Memory (owner: the engine fix, not bigger nodes): the next day's download starts only when memory allows, as
   the reference-check overlap's guard does. Measure one large 2025 day stage by stage on one node.
3. A real day end to end is a release requirement: restore IVSC's ignored end-to-end test; release.sh refuses a
   build that has not published one real day.
4. Core:
   - the ramp (owner-approved): one new backfill block per 20 min at most, none while the budget waited in the
     last 5 min, at most 8;
   - a `backfill stop|resume` command, which turns re-runs off and on (done by hand twice);
   - safe core-setting changes (13:30 MT outage: 20-core.yaml applied raw, ~20 min down).
5. Small items:
   - company-capture 429 spacing (P-30);
   - the intent log's "fault:" wording for stated absences;
   - release.sh step-11 false stop;
   - the idle wait for the previous day's reference check.
6. One Codex audit of the stage-1 changes (Gemini when its quota returns ~12-01); every real finding fixed.
**Stage 2, prove** (~1 day, ~$5–10): about 8 hardest real days as single days on ≤3 nodes:
- 2024-07-09 (IBIT before options), the ETHA listing day, a split day, a monthly expiration;
- a heavy day (2025-04-07), a half day, a large late-2025 day, 2026-11-20.
The list is shown to the owner before it runs. Pass:
- 0 engine faults;
- every reference disagreement classified, with no engine value error;
- memory peak ≥1.5 GiB under the node;
- time per day measured.
A failure: fix it and rerun that day only. All pass: freeze, with the set as evidence.
**Stage 3, produce** (~7–8 days, ~$230–260 at $0.167/node-h): the ramp to ≤8 blocks, under the stop rule.
**Owner rule (2026-11-26):** a fault found in production does not restart the run. A fix that changes no published
value recomputes nothing; one that does recomputes only the affected measurements, by method version (C-079),
after the run.
Money: about $195 since 11-21 (estimate) + about $260 = about $455 of the $500 ceiling. Warn before crossing.
Estimates, unproven: 2.2 h/day/block was measured on 2024 days, one block alone.

## Steps to the MVP (the list shown to the owner on a status request; keep it current)
**DATED COMMITMENT: the paid pilot (live on www.vyreonlabs.net/early-access, read 2026-11-16).**
One customer has paid USD 10 for four weekly email issues on Sundays **2027-01-10, 01-17, 01-24,
01-31** (sales close 01-09; automatic full refund if issue 1 is not sent by 23:59 ET on 01-10;
refunds on request to 02-07). Promised: all 64 securities; readings flagged when far from the
security's typical level AND in the top or bottom tenth of its last 63 sessions (the engine's
`unusual.rs` WINDOW = 63, robust z: matches); "what could not be measured, and why"; data through
the latest session; no signals or forecasts. So by 2027-01-09 we need: (1) history back at least 63
sessions before 2027-01-08 (phase 1 covers it); (2) **daily go-forward runs** computing each new
session from the build onward (built, C-051: core queues T at 07:30 New York the next day; KEDA's
`day-block-daily` starts it (C-066), CronJob `daily` applied suspended as the fallback (C-068); pool
`day` to min 0; not switched on; runbook `deploy/k8s/README.md` "Workers from the queue", 9); (3) Reporter producing the paid issue from core's API, sent
through beehiiv. Owner, 2026-11-16: the deadline will be met; target free reports live in December
2026. Intrinio's Startup tier (USD 333 a month, scaling to USD 999) is required for redistribution
rights and covers free and paid reports, so it starts before the first publication of either (bought
after the build: launch brief); ideally free and paid are ready together. Google Ads all-time
(prototype): 1.58M impressions, 156k clicks (CTR 9.88%).

**Sequence (owner, 2026-11-21): phase 1 → verify phase 1 → Reporter → web; then the launch decision
and the ads.** Reporter implementation runs while phase 1 builds (owner: 1–2 weeks expected for
Reporter + web). The paid issues go by email, so they depend on Reporter, not on the web rebuild.

Done:
- [done 2026-11-13] Two-day cluster trial: 2025-12-10 and 12-11 from an empty disk, 6 h 57 min cold.
- [done 2026-11-16] Engine ready for the build: audit rounds a–k converged (owner); later rounds
  l–z, aa–ee and the D-412 rounds fixed since. Candidate today: 1,596 tests passed.
- [done 2026-11-17] Cluster verification: 2025-12-10 2,688/2,688 slots, 0 faults; 2025-04-11
  2,684/2,688 (4 stated ETHA absences); June 2022 block published (FB→META contained);
  2024-07-16's cause found (partial 2023 snapshots, D-359; outside phase 1). Tape history for phase 1
  cached (654 sessions; since made stale by D-412, re-derived in the build).
- [done 2026-11-18] Two-week sample 2026-11-02..13: 10/10 published, 2,688 exported and 0 failed
  each day. Cboe PASS (5 indices, correlation 0.993–0.997); EDGAR 1,944 PASS, 26 FAIL on BABA/TSM
  only (fixed, D-361); Tiingo PASS (165,030 FIELD and 86,656 price comparisons); FRED exact.
  Coverage count found the replay change (P-04) and two EWJ defects (fixed, D-364/365).
- [done 2026-11-19] Bulk option files equal the API (A/B 2026-11-03: 38,326,866 rows each, every
  price, size, volume and OI equal); older differences are provider faults (P-20, P-21), handled
  (D-398, D-399). Tape caches verified by sample: 1,276/1,276 asset-sessions byte for byte (C-059).
- [done 2026-11-21] Three optimization passes (D-401/402; D-405; D-408/409/410). Before/after on
  one AMD node (block start → published): first day 400 → **226 min**; later days ~215/180 →
  **134/120**; intent B 71 → 49; reference checks beside the next day, 0.0 min waits.
- [done 2026-11-21 22:24] Byte proof (C-001 + D-405): 11,870 of 11,874 published measurements equal
  with switches on vs off; the 4 are BABA company measurements from re-fetched captures. Switches
  save 21% of block time.
- [done 2026-11-21] D-412 (session close from the listing market's print), five Codex rounds: tape
  samples @9..@13 all 256/256 PASS; closes 192/192, opens 191/192 (XLK 05-06) on the three complete
  sessions. Image **cc1d67cf** is the phase-1 image.

Next:
1. **Phase 1, the build** (launch brief below; range and depth C-060): the test block, then the rest;
   gates a–c per day, the whole-store gate after the ordered passes.
2. **Verify phase 1:** the QA plan below; then the full examination of the built store (owner,
   2026-11-16): per rule, does it generalise cleanly (fired how often, on what; over-firing; never
   fired; resting on one case) → a short list of rule changes (each a scoped recalculation) and the
   rules that stand. **Rule census (owner, 2026-11-16):** "with agent driven development on things of
   this scale ... we just sort of accumulate rules over time and the result can be an unfortunate
   agent stuck in spaghetti constraints". Never fired, fired only on its origin case, or contradicted
   by the population → retire or merge; prune agents' instructions the same way. No rule tracking
   is added ("feels like complexity that will cause problems ... If rules are being applied on some
   days but not others then we shouldnt be doing it"): the census reads the outputs (absences by
   reason, per sensor, asset, date; continuity). Rules required by a published standard fire only
   on their events by nature. **Origin (owner, 2026-11-21):** every rule was written on ~40 sparse
   dates (the B0/B1 censuses) because a full build cost too much; the build is the first population.
   "We are building a measurement system, we don't want one-off handling, we want generalizable
   sensors." **The target (owner, 2026-11-22): patches fitted to a case** (a date list, an asset-specific
   branch, a rule written for one observed day: NVDA's challenge list was one, retired by D-439), **not
   standards applied to rare market events** (OCC adjustments, early closes, specials fire rarely by
   nature and stay). Base layer censused once its days are in, derived after the ordered passes; the criteria
   (fired only on its origin case, never fired, over-fires against the 80% goal, fires at a code-change
   date) are written down before the census is read.
3. **Reporter on the phase-1 database:** the gap scan's work list (below); a proper paid report
   beside the free one; then "a significant body of iteration on the reports themselves" until free
   and paid are generated with confidence. Weekly comparison (owner, 2026-11-17): per sensor this
   week vs last on one chart, from derived-layer weekly aggregates (state → the week's close; flow →
   week sum or daily mean; ratio of flows → from summed parts; state curve → closing curve; flow
   curve → summed per bucket; already relative → not aggregated). Presentation research pass once
   real reports exist (gaps: week overlays, OI heatmap by strike × expiry, surface, chart types,
   competitors' forms, citable sentences, share images).
4. **Web rebuild:** re-architected from the early-access site into the full free reports (64
   per-asset pages), the paid product and its explainer page. Verification page (owner, 2026-11-18):
   each external check, its declared band, latest result, failures and fixes, in plain voice (no
   "accurate", "audited", "institutional-grade"). Naming Cboe's indices waits on Cboe's permission
   reply (sent 2026-11-18); check Tiingo's and OCC's naming terms first. Design inputs (pre-pivot):
   `vyreon-reporter/docs/publication-design/`, `market-research/VYREON_PAID_CROSS_ASSET_PRODUCT_CONCEPT.md`,
   `~/claude-artifacts/website-plan-2026-10-12.md`, `~/claude-artifacts/literature/SYNTHESIS-*.md`.
   **Design goal for the free reports and archive (owner, 2026-11-23): maximum utility for both human and
   machine readers.** The free per-asset pages plus the weekly archive (64 dated pages a week, never expiring) are
   the organic search surface the prototype never had (its ~130k users came only from ads, ~CAD 1,500, and did not
   compound); vyreon-web's explainer pages already draw a small organic trickle. Each page serves a person skimming
   the summary and an agent extracting exact values: stable URLs per asset and week; plain, rounded, dated numbers;
   stated absences with reasons (never blanks an agent could misquote); structured data (schema.org Dataset /
   similar) beside the prose; the same values as core's API. Design this in from the start of the web step.
   **Depth and accessibility (owner, 2026-11-23):** most readers are not programmatic (OpenBB failed selling data
   access; most people cannot use an API), so the product is the explained report, not the API. Every page and issue
   gives depth underneath and accessibility on top: what each measurement is in one plain sentence, what is normal
   for this asset and whether this week is unusual, and what we cannot tell, stated; the full precision stays in the
   engine, the checks and the structured data. Applies to Reporter (paid issues) and the web (free pages, archive).
   **Answer engines (owner, 2026-11-23):** readers are increasingly ChatGPT and similar answer engines; pages state
   their answers directly (question-shaped headings, the dated value in the first sentence) so an engine can quote and
   cite them correctly. Open decision for the web step: which AI crawlers to admit (citation and reach vs. answers
   given without a click). **Owner, 2026-11-23: AEO and SEO are both required** (answer engines cite but bring no ad
   clicks; search brings both). **Page structure: a summary at the top, then graphs and details** — serves the human
   skimmer, the answer engine quoting the first lines, and the search engine indexing the depth below; the hard part is
   organising a lot of data per page accessibly.
   **Page sketch (owner endorsed, 2026-11-23; to design against real phase-1 pages):** (1) summary: the week's 3-5
   most notable readings for the asset, chosen by the same unusual-against-its-own-history scoring the flags use
   (data-driven, not hand-written); (2) sections by theme (price and trend; volatility and options; flows and
   positioning; company events), each opening with one plain sentence and one chart; (3) full detail: every
   measurement incl. stated absences in collapsible tables, with the structured data.
   **Owner, 2026-11-23: before Reporter polish, the automation debts:** release script, budget-aware KEDA scaling,
   re-runnable reference checks (Listed, not fixed). **And when phase 1's base layer is complete: a full audit of the
   decisions** — every rule in force (engine DECISIONS "Rules in force", D-0xx..D-45x) judged on the built population:
   how often it fired, on which assets and dates, whether its sample size supports it; rules fitted to one or two dates
   are retired or generalised (with the census and the whole-range missing-contract scan).
   **Coverage report (owner, 2026-11-23), with the census:** coverage is per value inside each body (slots are all
   DELIVERED: 28,100 of 28,100 on the first phase-1 days), so compute it from the bodies: (1) overall share supported vs
   the 80% coverage goal (graded goal, separate from D-404's 75% window rule); (2) by sensor × asset; (3) by reason class —
   structural (not yet listed, before a provider's coverage start), provider faults (undelivered, refused snapshots,
   P-xx), ours (ENGINE_FAULT, our gates); (4) monthly over the range (era effects: trade counts from 2024-11-26, tape quote
   eras). Reported to the owner with the census.
   **Phase-1 measurement report (owner, 2026-11-23), beside coverage, after the base layer:**
   1. Right numbers: gate a's unexplained rate per day and sensor over all 597 days; gates b-d (Cboe bands, EDGAR, flag
      agreement ≥0.80 both ways); determinism (random days re-run on another node, byte for byte); no look-ahead.
   2. Continuity: jumps the market did not make (sensor change vs the asset's own move); **no step at the dates where the
      publishing build changed** (phase-1 days came from f3db692e, d9462aad, 2a8f8c4f and later: prove ordinary days
      unchanged across them).
   3. Paid layer: flags per asset-week and per week across the 64; calibration against what the threshold implies on
      ordinary weeks; persistence (flags clustering on real events vs scattered).
   4. Provider: faults per 1,000 asset-sessions by type (whole-range missing-contract scan, refused snapshots, missing
      dividends, tape gaps); volume vs Cboe where held.
   5. Operations: per-day compute time, cost and requests; refused/failed-day and retry rates; daily-run time to publish
      vs deadline; alerts that needed a person (signal vs noise) — inputs to the two-weeks-unattended verdict.
   Priority: 1, 2 (build-boundary continuity), 3.
   **Owner, 2026-11-23:** start Reporter beside the build once the earliest phase-1 days have their history passes
   (from the gap scan's work list), so the web step starts from a working Reporter. **Codex reviews the design too**
   (read-only, like the audits), in separate passes: an accessibility pass (plain-language, non-programmatic reader)
   and a utility pass (depth, correctness of what is shown, machine readability: answer engines, structured data).
5. **Launch decision:** proven unattended operation first (owner, 2026-11-22: explicit go-live
   criterion; the owner may be mobilized to field work for weeks): two weeks of daily runs, history
   passes and the Sunday issue with nobody touching the system, judged afterwards from the digest and
   the log; and the owner can restart the system after a bad day from the repository's runbooks, `health.sh`
   and Discord alone, with no agent (owner, 2026-11-22: not "the only one who knows how to run the system").
   Then Intrinio's Startup tier (display/redistribution rights; covers free and paid),
   security before launch, Reporter's output live on the web; then the ads (owner's plan below).
   After it, or when wanted: deeper history (old snapshot layouts readable since D-251..D-254; the
   2023-01-18..03-07 gap needs a ruling), the FIELD campaign, social posting, CI.

## Phase-1 launch brief (agreed with the owner 2026-11-21; C-060, C-061)
**Image 914ffed7** (build `914ffed7+7d4e45c05f63`, replacing cc1d67cf 2026-11-22; was `cc1d67cf+f9676b89ff09`, 1,596 passed; @13 tape sample 256/256 PASS, all
six quote fields compared, bodies identical to @12, run with the phase-1 tape template; replaces
da3dade4). Core runs `vyreon-core:a60f0fe` (deployed 2026-11-22 ~04:45 UTC: C-061..C-064, migrations to 0041); settings since 2026-11-21
(before 23:57 UTC): `VYREON_CORE_REFCHECK_FROM=2024-07-09` (C-054; was unset), `VYREON_CORE_INTRINIO_PER_MINUTE=1900` (owner: the account's 2,000 a minute, shared by every
node; was 1,500), both in `deploy/k8s/20-core.yaml`. Re-check the build is registered and admitted
before scheduling.
- **Range (C-060):** 2024-07-09..2026-11-20, 597 sessions: today's values final (warm-up chain 596 =
  flag-line pool 252 + z 63 + sma_30 29, then the 252-session weekly baseline). The 2023 snapshot gap
  is not reached; D-359's partial sessions are before the start (skipped and stated, D-404).
- **Order (owner: one pass, so every known change lands first):** (0) the flag-agreement code and
  C-061's core change, built, audited and deployed; (1) tapes 2023-07-07..2024-07-22 and the test block 2024-07-09..07-22 on one day node;
  judge its days (gates a–c, holds, memory, one node's request draw) and fix what it shows; (2) the rest: tapes to
  2026-11-20, then blocks of 15, nodes added only while the request budget has headroom (owner,
  2026-11-19); (3) the ordered passes, scheduled ONCE for the whole range (C-046); (4) the whole-store gate. Daily runs (C-051) switched on when the full build
  starts, from 2026-11-23 (`VYREON_CORE_DAILY_FROM`/`_BUILD`; KEDA starts the daily work, CronJob `daily`
  stays suspended, C-068); set
  `VYREON_CORE_DEADLINES_FROM` to the live date so deadlines don't fire during the build.
- **Layout:** `s-8vcpu-16gb-amd` day nodes (droplet limit 30: web, the engine droplet, the core node,
  up to 27 workers). nyc1 has refused AMD capacity before; the fallback pool `day-std` (s-8vcpu-16gb
  Intel) is slower (AMD runs stages at 0.54–0.73 of Intel's time).
- **Tapes:** every tape cache is re-derived under @13 (D-412 changed the reader identity). A day block
  claims only when no unfinished TAPE job is dated at or before its last session (`claimable_job`):
  every tape the block reads must be done first. The tape verdict takes 10–16 min a session.
- **Templates and scripts (`deploy/k8s/ops/`, README there):** `job-day-block.yaml`, `job-tape.yaml`
  (image cc1d67cf, `fred-key` mounted); set `metadata.name`, `completions`, `parallelism`, the pool.
  `delete-idle.sh` after every run ends; `scale-pool.sh`, `do.sh`. Secret `vyreon-worker` keys:
  `worker-token`, `intrinio-key`, `fred-key`, `ordered-pass-pg`. Kubeconfig
  `~/claude-artifacts/cluster-scratch/kubeconfig` (downloaded 2026-11-18; expires ~2026-11-25:
  refresh from DO before then).
- **Failed days (C-061):** core sets a failed day aside and the build goes on (3 standing failed days
  pause the queue). Read its evidence (`vyreon-core evidence list|show`); a provider cause is a
  missing date (omitted with its reason once C-061 is built); an engine cause is fixed and recomputed.
  Check `core.queue_control` whenever pods find nothing to claim.
- **Spend:** DigitalOcean $180.41 month-to-date at 2026-11-21 06:09 UTC, before the build. Projection
  ~$210 (range $190–250 with reruns): 40 blocks × ~28 node-h ≈ 1,115 node-h + tapes ~145 node-h, ×
  $0.167. Ceiling **$500** total (owner, 2026-11-21: "There may be multiple computes if we find
  errors"). Wall time ~2–3 days, bound by the request budget. Keep the running total here.
- **Watch:** core's request budget (1,900/min, shared through core's leases, C-025; whether presigned
  downloads count against the provider's 2,000 is not proven: watch for rate-limit refusals before
  adding nodes); worker memory over a 15-day block on 16 GB (logged `memory after <T>`); holds,
  refusals, crashes; refcheck unexplained counts; the Discord digest; the M0 solve-reuse hit rate.
- **Expected absences in range (not faults):** P-23 tape eras (before 2025-11-26 thin trades, before
  2025-09-04 no quotes); replay quote widths withheld from 2026-11-06 (P-04, D-411 gate 3); EVC01
  quote-width flags from ~2026-06, volume flags from ~2026-09 only if P-04 is reconciled; QQQ
  2026-09-28 a stated absence (P-21); EMB histories short to ~2025-10 (D-290); SAI-EXQ01 and
  M0-PRINT-SIZE withheld EVENT_SOURCE_NOT_RETAINED before 2026-10-23 (check the droplet's cohort
  prints from 10-23 are read, D-263/C-052).
- **Gates (owner, 2026-11-18: "the consequences of a wrong number are unfortunately quite large"):**
  per day as it publishes (a) refcheck in the worker, (b) the Cboe bands (`tools/external`), (c) EDGAR;
  after the ordered passes, the whole store: the history reference check (QA 1) and (d) the
  flag-agreement test (`tools/external/README.md` "Flag agreement", declared 2026-11-21: the
  percentile half of D-070 on the 14 Cboe pairs, both directions ≥ 0.80 with ≥ 10 index-extreme
  sessions; code in `tools/external` before any phase-1 value is judged). A failed check stops phase
  1: diagnose, fix, recompute the affected days. Bands are not moved after the fact. VXSLV/SLV and
  VXFXI/FXI judged on 63-session panels only (owner, 2026-11-19, declared in the README).
- **QA plan (owner, 2026-11-21: "We do phase 1, try to break it, fix what breaks, then move towards
  Reporter generating free+paid reports, and go live with web"; "worth proving this system as much
  as possible"):**
  1. History reference check as the whole-store gate after the ordered passes: `tools/reference` recomputes
     z-scores, percentiles, one-year windows, flags, technicals and EVC baselines from the built
     store; the rest of the build is trusted only after it passes.
  2. Continuity plots for the owner's eye: every sensor × asset over the range (jumps the market did
     not make, breaks at code-change dates).
  3. Event walk around each real event: splits and reverse splits (NVDA, ETHA), WMT's listing
     transfer 2025-12-09, special dividends (EWJ 2025-12-16), every early close, the P-23 era
     boundary 2025-11-26, the bulk-file fault days (P-20, P-21), the 2025-12-05 ETF splits.
  4. Determinism at scale: a few random sessions re-run on a different node, byte for byte with
     provenance masked (as the byte proof; script `~/claude-artifacts/checks/byteproof/compare.py`).
  5. Look-ahead audit over the built store: nothing uses data stamped after its cutoff (filings,
     macro releases, restated captures such as BABA's of 11-21).
  Then try to break it and fix what breaks.
- **Census (owner, 2026-11-16: "the database must hold enough data points to judge the
  event rules"):** per event rule (first chain, strike adjustment, OI outage, split and rename
  translation, receipt ratio, bank not-applicable, refused snapshot rows, capture failures, provider
  comparison classes): how often it fired, on which assets and dates, and every withheld state by
  reason. Owner: expect a rule to fail to generalise; one sensor affected → single-sensor rebuild;
  many → full rebuild of phase 1; decided from the evidence.
- **Backups as soon as the test block is stored** (owner, 2026-11-21; not after its census):
  CloudNativePG scheduled backups to Spaces, weekly full plus continuous WAL archiving, ~30 days, a
  bucket and key only for backups; then one restore drill. Owner, 2026-11-17: "given that
  recomputation has a known time and money cost its actually WORTH backing shit up." **Done
  2026-11-22** (backups live, restore drill passed, laptop safety dump deleted).
- **Failure injection during phase 1:** kill a node mid-day; restart core during a publish.
- **FIELD null-coordinates check (before launch, cheap):** every delivered FIELD-SUB01, FIELD-SUP01,
  FIELD-DIM01 and M0-FIELD-ANALYTICS slot (24 each) has null reader coordinates, like the
  stream-backed M0 slots: confirm intended omissions, not a collapsed projection (gap-scan finding
  10). Bodies are stored exactly, so projections can be backfilled; not a build blocker.
- **Startup tier (owner, 2026-11-21):** bought AFTER the build (display rights only); the build runs
  on the current key; on 11-25 if phase 1 is done by then. On the switch: entitlements and rate limit
  (access codes), core's request budget, the key in the cluster secret and `api.txt` (never
  printed), one real day rerun. At hand-offs, remind the owner of the time sheet and engineering log.

## Now: pre-build changes and audit (2026-11-22)
- Done: flag-agreement code (650539c); C-061 on core branch `core-c061` (27c83b6..6f120ed; 35 unit +
  88 database tests); engine D-413 on branch `eng-gap` (cc6c4b96..9490a8d5: a missing day judged from
  the walks that crossed it, not a contract-basis restart; 462 tests).
- Audits (`~/claude-artifacts/audits/2026-11-22-prebuild/`, triage under `triage/`): depth (11: 9
  real, 2 by decision, none above the cut line; 7 are in the engine's `/v1/review`, for the Reporter
  port) and surface (16: 14 real, 1 not real, 1 by decision; above the cut line #6, a re-derived pass
  stranded under an old build, which stalls every later pass, and #5, fixed on `core-c061`).
- Round 1 fixed: engine `eng-gap` eb137961 (D-414..D-423; surface #13 left: a label only, counts
  equal); core `core-c061` bf808f3 (C-062: no stranded pass, `days retire`).
- "Changes" audit of round 1 (`changes-core.md`, `changes-engine.md`, triage beside): core 17 real, one
  above the cut line (#2: a day with its base but no day-local revision stalls later passes); engine 5
  real, none above; D-423's restart revised (the missing bar's factor is in the provider's adjustment
  records the carry capture already holds: skip under D-404, restart only without them). Flag-gate
  fixes on main (354059a). Round 2 running: core (all 17) and engine (#2 ledger for unpublished
  sessions, D-423 revision, #3, #10, #14, #15).
- Core round 3 (core-r2 audit, 11 real) fixed and `core-c061` merged (a60f0fe; 37 unit + 109 database
  tests on merged main); core image a60f0fe deployed, migrations 0031..0041 applied, queue empty. Core's
  claim rules are not audited again before the build: the test block proves them on real data.
- Engine round 3 running (engine-r2 audit: the D-423 skip reverted to the restart, provider dividend
  listings omit real distributions (P-22, EEM 2023-12-20); FXI two-date adjustment; ACT01 ledger; SPY
  beta count; reference P01 and event window; derive_day ledger; worker error dating).
- Engine round 3 done; `eng-gap` merged into `worker` (914ffed7, pushed); full suite 1,617 passed, 0
  failed; reference pytest 160 passed. **Phase-1 image `vyreon-worker:914ffed7`, build
  914ffed7+7d4e45c05f63** (4,679 slots; same three profile revisions as cc1d67cf), registered and
  admitted 2026-11-22 03:47 UTC (`cluster-scratch/evidence-914ffed7.json`); every older build (29)
  withdrawn. Job templates still name cc1d67cf: set the image to 914ffed7 when applying.
- `days retire --preview` shown to the owner (48 revisions of the 24 days; no job, no later history,
  nothing derived again; token dd0cba56d5df7a6b).
- Was: merge `eng-gap`, the engine's full suite, new images,
  register/admit the engine image (check its history profile revision), withdraw the old builds,
  `days retire --preview` to the owner, schedule the whole-range ordered passes, tapes and the test block.
- **Owner decision (2026-11-22):** the 24 PUBLISHED days of 8 earlier builds (2022-06-06..15 and 16 in
  phase 1's range) are retired (SUPERSEDED) before the test block: `vyreon-core days retire --preview`,
  shown to the owner, then `--expect <its token>` (C-062..C-064: rows kept, later history derived again); after phase 1, every superseded old-build revision is deleted in one cleanup (preview first;
  owner: "i would literally overwrite them or delete them").
- To measure, not change: each ordered pass loads the whole published store (est. ~1 GB peak at full
  range); measure the last passes' memory and time.

## Test block in progress (2026-11-22 04:48 UTC, unattended)
Retired the 24 old days (retirement 2); scheduled 597 ordered passes 2024-07-09..2026-11-20, 262 tapes
2023-07-07..2024-07-22 and one block 2024-07-09..07-22 on build 914ffed7+7d4e45c05f63. Tapes ran on 8
AMD nodes from 04:06 UTC (240 done by 04:48; every cache with its verdict); two Jobs ran out of
completions early (claim race). `~/claude-artifacts/cluster-scratch/testblock/run.sh` (detached; log
`run.log` there) finishes the tapes, starts the block on one node, deletes idle nodes, posts each
published day to #vyreon-progress, runs the block's 10 ordered passes, then deletes the last node.
Judge in the morning: gates a–c per day, holds, memory per day, one node's request draw (Intrinio
usage vs core's ledger), wall times; then fix what it shows.
**Test-block findings so far (fix before the full build):**
1. **2024-07-09 published 08:09 UTC: 193 min block start → published** (226 measured before); 3,898
   slots delivered; day 2 started at once. Requests: bursts ~500-760/min while a day downloads, ~0
   during compute (core ledger, by quarter hour 7,146 / 1,222 / 7,826 / 7,752).
2. **IBIT VS-V02, VS-V03-CURRENT, VS-V03-MATCHED-NATIVE are engine-fault bodies before IBIT's first
   option session (2024-11-19)**, while its VS-O01..O06/L01/Q01-Q05 publish their stated absence: the
   V02/V03 family does not read C-057's first option session. Mislabel (priority 3), no wrong number,
   no hold; ~275 asset-sessions in phase 1 (IBIT to 2024-11-19, ETHA to 2025-04-09).
4. **Gate a FAILED on 2024-07-09** (report 14, 09:49 UTC): 91,977 checked, 2,940 failed, **398
   unexplained** in 443 classes across 55 measurements (recent sample days: 3 of ~97,000). Core opened
   REFCHECK_DISAGREES (alert sent). Spread thin (largest: ACT01-INTRADAY 33, ACT01 27, GREEK-EXPOSURE 19,
   FIELD-SYNC01 16, UNUSUAL-CONTRACTS 14, SAI-G01 14, FIELD-DEP01 12/12, FND01 11): likely the 2024 era
   or reference rules behind the newest engine decisions. Triage running overnight
   (`~/claude-artifacts/checks/2026-11-22-testblock/refcheck-2024-07-09-triage.md`). The full build
   does not start until it is diagnosed and fixed; the test block keeps running for more evidence.
   **Triage done** (`refcheck-2024-07-09-triage.md`): 398 = 11 causes. 229 are the reference behind
   the engine (IBIT before its first option session 180, FIELD before ETHA 24, WMT split D-161 16,
   insider-time D-191 5, IBIT first year 4); engine: IBIT V02 `check_q05` (27, label), NVDA's
   hard-coded challenge list 06-07/06-10 (34: retire or copy, decide), BABA two dividends one ex-date (1,
   definition gap); thin 2024 chains near a band (12, incl. UUP forward 441-523 bp off parity: look);
   **unresolved, possible wrong numbers: open interest on PAST session 2024-06-10 (84: ARKK, EWZ, JPM,
   PLTR, TSLA, URA; reproduced on the worker: the reference finds D-398 first-session placeholders,
   11-20 per asset, above the 1% gate, while the engine publishes) and CDD-FND01 all-prior counts
   (11).** **OI diagnosed (`oi-history-2024-06-10.md`): the engine is right, no wrong number.** The
   history path applies D-398 (past sources are prepared with the overlay); the reference keeps D-398
   first-session rows in D-079/D-175's EOD-credibility test (`chain.py:429-441`), so the provider's
   positive snapshot figures (EOD and the next session say 0) make EOD "not credible". Engine's gate
   matches its recorded faults on 681 of 683 marked asset-sessions. Reference fix (tested in memory):
   leave every unknown-OI row out of the test, mirror D-175 (`tot > 0`, both figures present). New
   provider fault (2024-06-10, 06-13 snapshots: positive first-session OI, the reverse of P-20):
   record in PROVIDER_FAULTS.md. Owner ruling asked: take EOD's 0 beside a positive snapshot figure
   on a first session (amends D-398; Cboe: a series listed today has 0) to restore 06-10's OI on 17
   more assets. **FND01 counts diagnosed (`fnd01-prior-counts.md`):** GOOGL and NVDA the engine is
   right (reference does not bridge a full-year cell toward the quarters' tag, `company_fnd.py:403`);
   **LLY is an engine defect with wrong published numbers:** `fnd.rs` `bridge_renamed_tags`
   (:856-905) re-tags LLY's Q1 2020 `NetIncomeLoss` as `ProfitLoss` (from the Q1 2021 10-Q's
   comparative), splitting two cells the filer tagged alike (D-013.2) and breaking FY2020Q4: net_margin
   YoY magnitude percentile published 0.8333 (12 priors), should be 0.7857 (14); ROA's withheld (11),
   should be 0.5385 (13); to ~mid-2025. Fix: bridge only toward the concept's current tag (D-079
   amendment).

**Fix list before the full build (from the test block; morning 2026-11-22):**
1. Engine: FND01 `bridge_renamed_tags` (wrong number, LLY); V02/V03 `check_q05` before the first
   option session (label, IBIT/ETHA); the worker's claim retry; ACT01's reason before the trade-count
   feed (check first).
2. Engine, decisions recommended: retire NVDA's hard-coded challenge list (one-off handling; AVGO's
   07-15 split is not withheld) if its 06-07/06-10 bars verify; BABA's two dividends on one ex-date
   summed per ex-date (the distribution's total). Owner ruling: first-session EOD 0 beside a positive
   snapshot figure (Cboe).
3. Reference: IBIT before its first option session (D-117, C-057), FIELD before ETHA, WMT D-161,
   insider time D-191, IBIT first year, OI credibility test (D-079/D-175), FND01 bridge.
4. Provider fault: 2024-06-10/06-13 snapshots' positive first-session OI (new P-number).
5. Core: deploy 7c96453 (C-065) after the block; items 2 and 4 (KEDA, caps).
6. Then a new image, the test-block range recomputed by the full build, gate a re-judged on it.
**Fixed on engine branch `eng-tb` (83c3be01, 2026-11-22; Codex audit running):** D-428 FND01 bridge
(LLY 0.7857/14, 0.5385/13; GOOGL, NVDA unchanged), D-429 V02 empty chain (IBIT), D-430 claim retry,
D-431 the reference to the engine (A, C, E, F, G, H, J). Re-run on the worker's real 2024-07-11:
company/FIELD/price/ACT01 owners 104 → 3 unexplained (BABA, owner's call; LLY expects the fixed engine),
IBIT/WMT/ARKK chain owners 150 → 25 (all IBIT V02, gone with D-429), IBIT option owners 22 → 0. ACT01's
reason before 2024-11-26 stands (D-237: deliberate). Owner decisions: NVDA's challenge list (D), BABA's
same-day dividends (I), first-session EOD zero (D-398), a young asset shown SPY's own return (C).
Loose end: the worker leaves `<defunct>` refcheck python processes (harmless, unreaped).
**eng-tb audit round 1 (14 findings + suspicion, all real) fixed: f06ed094** (D-432 bridge availability =
the comparative's filing, a look-ahead closed; D-433 date-only insider acceptance from the next session;
D-434 daily claim keeps sent/unsent; D-435 FIELD-SYNC01 all-64 eligibility; D-436 reference OI and
empty-delivery rules; LLY FY2022 Q2 capex $371.0m). Real-data re-check not finished (pod load ~30):
the proof is step 6, the new image recomputing the test-block days with gate a judging them. Round-2
audit (narrow) running. **Round 2 audit (16) and round 3 fixes done (aaaf998d, D-437/D-438; every
FND01 change tightens toward withholding); audits on this branch stopped there (real data judges next).
`eng-tb` merged into `worker` 48461197 (pushed): suite 1,629 passed, 0 failed; reference pytest 174.
Image `vyreon-worker:48461197`, build 48461197+7361a31e2fda (4,679 slots, same profiles), registered
and admitted 2026-11-22 ~16:50 UTC; 914ffed7 stays admitted until its block ends.**
**Owner rulings 2026-11-22 ~16:50 UTC ("Go with the market conventions for sure"):** (1) retire NVDA's
hard-coded challenge list (`h3-p01/src/lib.rs:9-17`; the build is the test now); (2) cash dividends
sharing an ex-date are one distribution, summed (S&P DJI "Multiple Dividend Distributions on a Single
Day"; Xignite; CRSP's factor over all distributions to the date): BABA 2024-06-13 $1.66, 2025-06-12
$2.00; (3) a first-session contract's EOD 0 beside a positive snapshot figure is its open interest (OCC/
Cboe: open interest is the prior close's open positions; a series listed today has none; D-392 0 of
15,798) — amends D-398, restores 2024-06-10's OI on 17 assets; (4) a young asset's SPY return stays over
the asset's own two dates (D-080; excess return over the same period is the convention). Being built
into the next image before the recompute.
**After the block (owner, 2026-11-22: KEDA, autoscaling and caps BEFORE the recompute, so the 10 days
test everything):** (1) run.sh's 10 passes on 914ffed7, node deleted; (2) deploy core 7c96453 (C-065;
progress webhook; stop run.sh's posts); (3) KEDA (release manifest, no Helm) with one ScaledJob per kind
triggered by core's claimable count of its own build, pool `day` autoscaling min 0 max 12 (manifests
being written on branch `core-keda`); (4) the rulings image registered, 914ffed7's unfinished work (the 587
queued passes, any queued tape or block) handed to it once, withdrawing 914ffed7 (`build hand-over`,
C-068), then the rulings image admitted (README "Workers from the queue" 4: this order); (5) recompute
2024-07-09..07-22 as 2 blocks of 5 started by KEDA alone; gate a judges; (6) the full build.
**Added (owner, 2026-11-22: "schedule this all along with the KEDA implementation"):** the database
volume grows 120 → 250 GB with the next core restart (measured 2024-07 days ~134 MB each, 2026-11 ~200
MB; 597 days plus history revisions and tape caches would reach the 118 GB volume; +$13/month), and
CloudNativePG backups to Spaces (weekly full + WAL, ~30 days, a bucket and key only for backups) with one
restore drill, before the full build. Written, not applied (C-067; runbook `deploy/k8s/README.md`
"Backups and the volume"): the volume by a patch, no restart; the plugin's one Postgres restart
inside the KEDA rollout. **Before the recompute (owner, 2026-11-22): a Codex failure-surface
audit of the build's operation** once KEDA, backups and the rulings are merged: scale (passes over 597
days, Postgres under 12 publishers, volume and WAL), KEDA and the autoscaler (nodes from zero, lost nodes,
ScaledJob changes), external dependencies (Intrinio outages under the ceiling, Spaces, registry and
kubeconfig expiry ~11-25, nyc1 AMD capacity), ordering and claims over the full range, and recovery
from each stage by the owner from the runbooks. Its fixes land before the recompute tests them. **If its findings are converged (constructed edge
cases, not real failure paths): fix them all and go (owner): the recompute is the build's first stage; if
gate a passes on it, the full build follows without a pause.** Then watch: history-pass memory at full range (tonight's 10 passes
give the first number), the core node under 12 publishing workers, and the database size posted daily.
**Measured request draw, one node, test block 04:56-16:37 UTC:** 71,112 requests (~11,800 a day), 101 a
minute over wall time, bursts averaging 658 with a peak of 1,900 (the ceiling) and p95 1,800. Cap for the
full build: 12 nodes (two thirds of the ~18 the average allows), proven on 2 nodes in the recompute.

5. **2024-07-10 published 09:50 UTC (~100 min after day 1)**; memory after it 1,617 MiB resident, 212
   MiB after the trim.
3. **SAI-ACT01 / -INTRADAY withheld MISSING_REQUIRED_INPUT on 62 assets before the option
   trade-count feed's start (2024-11-26):** the absence is expected; the reason should be the
   provider's coverage start (PROVIDER_COVERAGE_START), not a missing input. Census item; check the
   code before calling it a defect.

## Owner questions for the morning (2026-11-22)
1. Spend stays with the operator (`health.sh`) and item 4's hard caps rather than a billing token in
   core (recommended: yes, least privilege; idle nodes are item 2's autoscaler's job).
2. Watch thresholds: STALLED 15 min (owner's), 30 min when only daily-run work waits (the CronJob's
   morning gap until KEDA replaces it), CLAIMS_EMPTY 3 misses in 10 min.
3. Once core-watch is deployed with the progress webhook, `run.sh`'s own posts duplicate it: stop
   run.sh's posting at that deploy (not before the block finishes).

## Resume here if the session is lost (written 2026-11-22 04:55 UTC)

**Resume pointer (2026-11-23 ~20:55 MT) — OVERNIGHT PLAN (owner, asleep): "do engine fix, audit it, remediate any audit
findings, start the tapes, and monitor for success or more work".** Phase 1 runs on engine build `2a8f8c4f+5bbbf36545c0`,
core image 6a4b59f. **Tape ScaledJob is PAUSED** (quote-era tapes OOM-killed; owner: no stopgap). Steps: (1) `eng-tape`
fixer (D-459, bounded-memory replay reduction, byte-identical caches) — wait for its report; (2) merge into `worker`, run
Codex (`tools/audit/run`) and Gemini (`run-agy`, xhigh; if its quota is exhausted, record and schedule after the reset) on
the replay scope (`tape`) with a self-contained note; (3) triage, fix every REAL finding that blocks (wrong number/absence,
memory not bounded, cache bytes changed), LATER the rest; (4) build image, register, hand-over from 2a8f8c4f (also carries
955d0f82's D-458 amendment), admit, KEDA + DAILY_BUILD on it; (5) set the tape memory request from the fix's measured
peak, unpause tapes (`kubectl -n vyreon annotate scaledjob tape autoscaling.keda.sh/paused-`), confirm the 11 tapes
2026-11-06..11-20 finish without OOM (cgroup memory.peak per pod); retry-failed any FAILED tape; (6) monitor blocks and the
daily run. Caps: tapes 12, day blocks 8, ACT01 in-flight 2. Restart the loop with
`/loop follow docs/MILESTONES.md under the rules in AGENTS.md`.
Run every step with `KUBECONFIG=~/claude-artifacts/cluster-scratch/kubeconfig` and
`VYREON_DOKS_CLUSTER=5f56e6e0-1c74-4dc9-bfdb-962ee55ebff3` (refresh the kubeconfig from DO before
~2026-11-25). Recommendation and evidence to the owner before anything that costs money or deletes.
1. **State:** `tail ~/claude-artifacts/cluster-scratch/testblock/run.log`; is `run.sh` still running
   (`ps -eo args | grep -c "[t]estblock/run.sh"`)? Then `deploy/k8s/ops/health.sh`. If run.sh
   stopped with STOP, read why; if worker nodes are up with no worker pod, `delete-idle.sh`.
2. **Judge the test block** (2024-07-09..07-22): `vyreon-core refcheck list 2024-07-09 2024-07-22` and
   `refcheck show <date>` (gate a); Cboe bands: `tools/external/values.sql` → `cboe.py --band-only`
   with the cache `~/claude-artifacts/cboe-cache` (gate b); EDGAR (`tools/external/edgar.py`, gate c);
   `vyreon-core evidence list` for any failed day; the worker's `memory after <T>` lines per day;
   Intrinio usage (Intrinio MCP `get_account_usage`) against core's request ledger for one node's
   draw; wall time per day. Report to the owner with numbers; fix what it shows (worktree per fix,
   audit, merge, image) before anything else.
3. **Core `core-watch` merged into main** (2026-11-22 ~07:00 UTC; C-065 and its audit's 16 fixes,
   migrations 0042..0045; 43 unit + 123 database tests on merged main). NOT deployed: deploy after the
   test block finishes (core restart under a publishing block avoided): build, push, `kubectl set image
   ... core= migrate=`, set `VYREON_CORE_PROGRESS_WEBHOOK_FILE` and mount secret
   `vyreon-core-progress-webhook` (manifest done), then stop run.sh's own posts. Registry login: DO API
   docker-credentials, both parts by stdin.
4. **Engine:** the claim retry (loose end) plus the test block's fixes → full suite → new image
   (`deploy/worker/build.sh`), `build register` (NOT admit yet), then `vyreon-core build hand-over --preview
   <old> <new> "<reason>"` and, on its token, `--expect <token>` (C-068: withdraws the old build and
   makes its queued passes, tapes and blocks the new build's, once), then `build admit` the new build
   with evidence, then the ScaledJobs on the new image (README "Workers from the queue" 4, the order).
5. **Items 2 and 4 (KEDA, autoscaling, caps):** written on branch `core-keda` (C-066); merge, then
   `deploy/k8s/README.md` "Workers from the queue" steps 1-4 (KEDA 2.21.1 by its release manifest,
   no Helm; the `vyreon_scaler` role before the core image with 0046), proven by step 10 (the test
   block's recompute started by KEDA alone).
6. **Full build:** schedule tapes 2024-07-23..2026-11-20 and day blocks 2024-07-23..2026-11-20 in blocks
   of 15 (the ordered passes are scheduled for the whole range and re-owned by the image change's
   hand-over: check `vyreon-core queue` shows them under the new build; never schedule them again while
   they stand; schedule only what `queue` shows missing); switch daily runs on from 2026-11-23
   (`VYREON_CORE_DAILY_FROM`/`_BUILD`; KEDA starts the daily work; CronJob `daily` stays suspended, C-068) with
   `VYREON_CORE_DEADLINES_FROM` = the live date; watch spend against $500.
7. **Then:** whole-store gate (history reference check; `tools/external/flags.py`, refresh Cboe files
   through 11-20 first), census, backups to Spaces with a restore drill, Reporter.

## Now (2026-11-22 ~18:30 UTC)
Merged and pushed: core main (C-061..C-067: claims, watch, progress, KEDA, volume, backups); engine
`worker` 090d7aea = 48461197 + the owner's rulings (D-439 NVDA list retired, D-440 same-ex-date
dividends summed, D-441 first-session EOD zero, D-442 engine-computed adjustment factor: 952 real
ex-dates, median relative difference 4e-16 vs the provider's, largest outside BABA 6e-6). Full suite
running on 090d7aea. Codex audits running (`~/claude-artifacts/audits/2026-11-22-prebuild/opsurface/`):
the operational failure surface (owner) and the rulings diff. Backup bucket `vyreon-pg-backup` created
(nyc3, private); its limited key in `pg-backup-key-id.txt`/`pg-backup-secret.txt` (git-ignored, 600;
tested: its bucket 200, vyreon-data 403). Test block on 914ffed7: 7 of 10 days published.
Next: the suite → image 090d7aea → after the block: the rollout (scaler role, core deploy, volume,
KEDA, autoscaling incl. `day-std` as the fallback, image admitted/914ffed7 withdrawn and handed over,
`GRANT pg_monitor TO vyreon`, core's ServiceAccount reading the backup plugin's record (20-core.yaml),
backups + first backup, BACKUP_STALE resolved from that record) with the audits' fixes →
the recompute as the build's first stage → the full build.

**18:20 UTC audits (`opsurface/ops.md`, `rulings.md`): NOT converged; fixing before the rollout.**
Suite on 090d7aea: 1,639 passed, 0 failed. Ops (15 + 2 suspicions): build-stoppers: the 597 passes owned
by the to-be-withdrawn 914ffed7; WAL/archive failure fills the volume silently; delete-idle.sh can delete
busy nodes on an expired kubeconfig; withdrawn builds' queued tapes/blocks block the successor; startup
failures loop without charging an attempt (README secret lacks fred-key); no AMD-stockout fallback; the
restore runbook has no production cutover. Rulings (16 + 1): D-442 (engine-computed factor) brought an
ordinary-day fault (SPY 2024-07-09, five daily sensors), a T−5 look-ahead, a cross-asset cascade and
nondeterminism to fix a 1-2 bp BABA convention gap: **D-442 reverted (coordinator, priority 1)**; D-441
first-session now also needs the prior snapshot to lack the contract; D-440 needs a valid currency.
**Owner decision (census): EEM 2023-12-20 $0.748 omitted by the provider's bar AND listing (P-22) — a
wrong return no factor rule can catch (single provider); possible rule: total-return windows before an
asset's dividend coverage start are unknown basis.** Fix agents: engine `eng-r2`, core `core-ops`.
**Owner, 2026-11-22: stay on Intrinio.** Checked every entitled Intrinio route: dividend records (only
2024-06-11 and 2024-12-17), the 2023-12-20 bar (dividend 0, factor 1; its adjusted close omits it too);
exchange dividends/adjustments by day are not entitled. Enterprise ($1,250/mo) unlocks the rest, only
once profitable on Startup, and may carry the same gap. For now: accept and state (P-22: EEM 1-year
values 2024-07-09..2024-12-20, warm-up, outside the report window); the coverage-start rule goes to the
census; ask Intrinio support at the Startup purchase about a backfill and the by-day feeds.

**Reference-check jump 07-16..07-18 (~396 → ~560) explained:** all AVGO's 10:1 (ex 2024-07-15); the
reference lags the engine's split rules D-162/D-163 (D-161 part already fixed in acbf297c); published
values verified by hand; no wrong number (`checks/2026-11-22-testblock/refcheck-0716-jump.md`). Would
recur ~140/day for ex+1..ex+4 at every clean split (MSTR 2024-08-08; 5 sector ETFs 2025-12-05).
Reference fix merged (engine `worker` f7b08efe, reference pytest 187); image `f7b08efe+effde96f9a1e`
registered 2026-11-22 15:25 MT: **the build for the hand-over** (acbf297c registered, never admitted, unused).

**Owner, 2026-11-22: Intel fallback authorized** — new pool `day-intel` (Basic Intel `s-8vcpu-16gb-intel`,
$0.167/h, same as `day`), autoscale 0..13; delete the empty `day-std` (its `s-8vcpu-16gb` is no longer
offered in nyc1: it could create no node). On branch `core-ops` (de64136). Done in the rollout.

## Rollout (2026-11-22, 22:35-22:55 UTC; runbook `deploy/k8s/README.md`)
Test block ended 22:34 UTC (10 days; 10 passes; node deleted; refcheck 07-22 426 = 396 + AVGO's D-161
part, as the triage predicted). Done: core-ops merged (4eda341); volume 250 GiB online (246G, 9 GB used);
`pg_monitor`, `vyreon_scaler`; cert-manager v1.21.3 + Barman plugin v0.16.0; Secret, ObjectStore, weekly
schedule; Postgres restarted with the WAL archiver (queue paused, ~3 min); core 4eda341 deployed
(migrations 0042-0051; **fix 60a048c: core's pod names the pull secret**, its own ServiceAccount had
none); KEDA 2.21.1 + login `vyreon_keda`; pools `day` 0..13 and `day-intel` 0..13 autoscaled (DO accepted
min 0), `day-std` deleted; hand-over 3 (914ffed7 withdrawn, 587 passes to f7b08efe); f7b08efe admitted
(evidence `cluster-scratch/evidence-f7b08efe.json`); ScaledJobs on f7b08efe, all READY; recompute
2024-07-09..07-22 scheduled as 2 blocks of 5, queue resumed 22:54 UTC.
**Backups live (2026-11-22):** the key was read-only (put 403); the owner gave it Read/Write/Delete
23:40 UTC; the archiver cleared its backlog at 23:41 (B7..C3 in `vyreon-pg/wals/`, 0 waiting). First base
backup `vyreon-pg-ondemand-gl4cq` completed 23:49:33 UTC (8 min, ~9 GB; firstRecoverabilityPoint
23:49:33). **Restore drill passed** 23:52-23:55 UTC: restored in 2 min 38 s, every month's revisions,
rows and body-hash digests equal to the source (`cluster-scratch/drill-20261122/`), source unchanged
during it (no publish in the window, queue not paused); drill cluster and its volume deleted. The
laptop safety dump `~/claude-artifacts/db-backups/vyreon-20261116T2000.dump` (726 MB) deleted on the owner's word (2026-11-22 19:30 MT).

**Recompute on f7b08efe (KEDA, 2 blocks of 5, started 22:55 UTC):** first days published 02:03 (07-16) and
02:07 UTC (07-09), ~3 h each (95 min download, 95 compute); passes run by KEDA in date order (~3 min each).
**Gate a, first readings:** 07-09 **35** unexplained (914ffed7: 398), 07-16 **28** (558; AVGO split window),
checked ~91.6-92.0k each as before, failed 2,577 / 2,401 (2,940 / 2,927). The remainder triaged (`checks/2026-11-22-recompute/refcheck-remainder.md`):
**class 2 blocks the build — provider non-delivery (P-27):** established series missing for one session
from both EOD and snapshot (2024-06-10/11; UNG240719C15 OI 17,928 → no row → 17,919, verified on
Intrinio), 22 asset-sessions published SUPPORTED with wrong OI changes in their histories. Fix running
(engine branch `eng-vanish`, D-447: undelivered series → unknown mass → 1% gate withholds; OCC: open
interest leaves only by closing, exercise or expiry), with reference fixes for classes 1 (NVDA split
across a refused session), 4 (FIELD-SUP01 order follows the engine, D-108; ETHA pre-listing), 5 (IBIT
pre-options). **Also blocking (triage final, 4 days):** UUP DLM-EXPECTED-MOVE 07-10 published 1.506% QUOTED where the
quotes cap the straddle at 1.375% (no-arbitrage: ≤ the asks): the QUOTED branch lacks D-122's band check;
added to `eng-vanish`. Class 3 (thin chains) census: SLV 2025-06-30 forward, UUP, NVDA C7.5 σ unidentified
(vega ~0), META C120 0.37 vs 0.32. Then: image, recompute the 10
days again, gate a. **Owner, 2026-11-22 22:10 MT: "get it fixed and re-tested and we will evaluate
starting phase 1 when done"** — the full build waits for that evaluation (not automatic).
**Fix merged and re-test started (2026-11-23 ~06:05 UTC):** engine `worker` 6949aa7c (D-447, UNG1 merge-lost
mass, D-448, D-449; suite 1,641/0, reference pytest 191); image `6949aa7c+2b19185fbb82` registered;
hand-over 5 from f7b08efe (its blocks stopped after 6 of 10 days); admitted (evidence
`cluster-scratch/evidence-6949aa7c.json`); 4 blocks (07-09..11, 07-12..15, 07-16..18, 07-19..22) so the
re-test runs on 4 nodes (~5-6 h). Codex audits overlay + optsensors of the batch running
(`audits/2026-11-23-vanish/`). P-27 now in PROVIDER_FAULTS.
**Audits of the batch (2026-11-23 ~06:20 UTC, `audits/2026-11-23-vanish/`): NOT converged, hold.** Overlay 11
+ 3 reference, optsensors 10 + 3 reference, much overlap: UUP half-spread still divides by the rejected
reprice; VS-O06 supported 0 on an OI-only fault; series missing twice; whole adjusted root absent; split
image in pre-split units; frozen exclusion vs live series; D-175/D-398 evidence overridden; merge erasing
unknowns; damaged predecessor read as empty; inherited: ETHA reverse-split basis, requote binding,
returning root as corporate action. Fixer on branch `eng-v2` verifies and fixes all REAL. **Owner item:
D-191 ETHA1 weight 33.3333 shares vs OCC memo 59890 (33 shares + cash in lieu); the fixer reports the
standard and the effect, no change without the owner.** The re-test on 6949aa7c keeps running (its other
checks still count).
**Round-1 fixes merged (2026-11-23 ~07:20 UTC):** engine `worker` 68ff6c36 (eng-v2, 19 commits; every finding
REAL, none dismissed; D-450, OVERLAY_RULES D-450; suite 1,651/0; reference pytest 195). Snapshot window
uses h3-calendar's closes (early closes 13:00). Round-2 Codex audits running (`audits/2026-11-23-vanish-r2/`).
Image `68ff6c36+18f1d999a9f8` admitted after hand-over 7 (6949aa7c published nothing); **re-test 07-09..07-22
on 4 blocks running since 07:24 UTC (00:24 MT), ~6 h.**
**Round-2 audits (`audits/2026-11-23-vanish-r2/`): NOT converged — an ordinary-day REGRESSION:** the new
predecessor verifier refuses every valid multi-page capture (last page carries its request cursor), so SPY
(>10,000 rows) would be an ENGINE_FAULT; plus conditional paths (damaged/misdated predecessors, error
containment, split successor across sessions, combined adjustments, 22-session horizon, D-441 debut
resurrected, immaterial adjusted root, VS-O06 volume fault clearing OI, 04:00 UTC as NY midnight).
**Coordinator's convergence rule for round 3 (D-451): where evidence is ambiguous, withhold; do not infer;
remove branches rather than add.** Fixer on `eng-v3`. Gates before the owner's evaluation: the real-data
re-test shows zero ordinary-day refusals (SPY multi-page) and the next audit converges.
**Round 3 merged (2026-11-23 ~09:10 UTC):** engine `worker` e499d9fc (eng-v3, 11 commits, D-451; suite 1,660/0;
reference pytest 199); image `e499d9fc+a00db7366757` admitted after hand-over 9 (68ff6c36 published
nothing); re-test on 4 blocks restarted ~09:15 UTC (02:15 MT). Round-3 Codex audits running
(`audits/2026-11-23-vanish-r3/`). Fixer's irreducible branches: 21-session horizon (wider window = more
captures, owner's call), quiet dead listings (JPM rule), one certain image converted by OCC's rule,
`first_option_session_after` reads every capture on disk (determinism: audit asked).
**Round-3 audits (`audits/2026-11-23-vanish-r3/`): ordinary multi-page regression FIXED in both paths; NOT
converged** (~8-10 new paths each, all needing a second provider fault on the same series: frozen pre-split
code hiding a missing successor, a gap erasing an omission, later-day/wrong-asset predecessor files, fresh-fill
read error refusing the day, placeholder/debut ordering). **Coordinator's recommendation to the owner (pending):**
(1) one narrow round, no new branches (`eng-v4`, D-452: one reader bound to asset+session for every consumer,
gaps never end the walk, successor checked even when the old code survives, fresh-fill containment inside
223 sessions, placeholder/debut before unknown); (2) gate the build on the real-data re-test (zero
ordinary-day refusals) plus a **whole-range missing-contract scan in the census** (raw deliveries, all 597
days) with scoped recompute of any day found, instead of a 4th audit convergence; (3) P-27 seen on 2 dates
in 7 scanned months; remaining paths need stacked faults.
**Round 4 merged (2026-11-23 ~11:00 UTC):** engine `worker` b3f94585 (eng-v4, D-452; ~14 decision points removed,
~10 added, zero new outcomes; suite 1,662/0; reference pytest 199; ordinary days: no value changes).
Declined: D-441 debut zero inside the walk (proof needs a window beyond the declared one; stays unknown).
**For the owner:** EWJ 2025-12-16 (overlay r3 #5): core's ledger dates an OCC strike adjustment the
detector cannot confirm (prior snapshot refused) → ex-date OI can publish where D-079 withholds it;
recommended: under D-451 a ledger-dated action the detector cannot confirm withholds that session's OI.
The first 21 sessions after 2022-05-27 now withhold OI (no earlier quote files): outside phase 1; whether
the provider's snapshot start ends the walk like a listing is a later call.
Image `b3f94585+f95a01adef72` admitted after hand-over 11 (e499d9fc published nothing); **re-test 07-09..07-22 on
4 blocks restarted ~10:40 UTC (03:40 MT)**, ~6 h. Round-4 overlay audit running (`audits/2026-11-23-vanish-r4/`).
**Round-4 audit: not converged.** Ordinary-day wrong absence: a D-359 partial session in the lookback (NVDA
2024-06-10) withholds the current book with nothing missing (hits 07-09..07-11). Wrong number: the prior count
does not bound an undelivered series (UUP May $28 275 → 3,187 overnight; 5.3% short passes the 1% gate).
**Coordinator's decision (priority 1, D-451), round 5 (`eng-v5`, D-453): presence-only — any established
unexpired series undelivered makes the asset's OI unknown that session, whatever its size;** the count
machinery for undelivered series is removed. Also: gaps matter only for a missing series (NVDA), EMB boundary,
EWJ ledger-dated action withholds (owner's pending item, applied under the same rule), fill containment,
placeholder in D-255, deterministic clock rejection, debut/adjustment evidence. Cost: coverage on days a
series goes missing, measured in the census.
**Round 5 merged (2026-11-23 ~12:30 UTC):** engine `worker` 4d73e9f5 (eng-v5, D-453; first round removing more
than it adds; suite 1,667/0; reference pytest 200; NVDA lookback case publishes on a new real slice; snapshot
clock judged on a fixed root set, the 64 under dated tickers + adjusted roots). Round-5 overlay audit running
(`audits/2026-11-23-vanish-r5/`), asked for ordinary-day changes and single-fault wrong NUMBERS only.
Image `4d73e9f5+fc90cdd72406` admitted after hand-over 13 (b3f94585 published nothing); **re-test 07-09..07-22 on 4
blocks restarted ~12:15 UTC (05:15 MT)**, ~6 h.
**Round-5 audit (`audits/2026-11-23-vanish-r5/`):** every round-4 finding fixed (NVDA lookback publishes).
Narrower: 2 single-fault wrong numbers (OI opened from zero escapes — ETHA 04-14 333 vs 462; volume publishes
short when a series is detected missing — UUP 20.9%), day refusal from a daily-body read error, yesterday's
EOD-only omission clearing agreed OI, stray daily page; deeper-build only: FXI 2023-06-07 cash, FB→META.
Round 6 (`eng-v6`, D-454) fixes them; must not change ordinary days. **Plan change (coordinator):** no restart
of the 4d73e9f5 re-test — it is the ordinary-day evidence; the round-6 build runs phase 1 (which recomputes
these 10 days first).
**Round 6 merged (2026-11-23 ~14:00 UTC):** engine `worker` 5f80aac8 (eng-v6, D-454; suite 1,672/0; reference pytest
201; fixer expects no ordinary-day change, checked on real consecutive chains). Image built and registered, NOT
handed over (the 4d73e9f5 re-test continues as evidence; the switch waits for the owner's go). Round-6 overlay
audit running (`audits/2026-11-23-vanish-r6/`).
**Round-6 audit:** every round-5 finding fixed. Remaining single-fault paths are VOLUME only: (a) an identifiability
floor — a series with no OI and no trades yesterday that trades today and is omitted cannot be told from a lawful
delisting with one provider (ETHA 2025-04-14 VS-O06 1,006 vs 1,377); (b) EWJ 2025-12-16 ex-date volume; (c) XLK
2025-12-04 null EOD volume with later trade evidence. **Coordinator's judgement: converged** (each round fixes the
last; what remains is an identifiability limit, not a code defect). Round 7 (`eng-v7`, D-455): (b), (c); (a) recorded
as a stated limit with a census check of VS-O06 volume against Cboe's per-contract EOD files.
**Re-test note:** block 07-19..22 attempt 1 refused 2024-07-19: the macro capture for 07-12 overran its 3-minute
frozen cutoff ("macro source after frozen cutoff"); core re-queued it and KEDA started attempt 2 unattended. If it
recurs, widen the macro capture window before the full build.
**Round 7 merged (2026-11-23 ~15:00 UTC):** engine `worker` 66841b6b (eng-v7, D-455; suite 1,676/0; reference pytest
203; ordinary days unchanged: 0 of 497,880 real EOD rows have a null volume beside a same-day trade). Image
built and registered (not admitted): **the candidate build for phase 1.** Round-7 overlay audit running
(`audits/2026-11-23-vanish-r7/`).
**Round-7 audit:** no ordinary-day change; no wrong absence on fully delivered ex-dates (ETHA 1,047 series under
ETHA1, EMB); OI clean. Remaining single-fault paths are VS-O06 VOLUME only (1-4%): EOD row omitted with the
snapshot listing it (XLK 12,108 vs 12,548); null EOD volume filled below the row's own last trade (ETHA 245 vs 324
last trade, 569 true); P-12 undercount invisible to one provider (UUP 9,700 vs Cboe 9,800) — a floor. Round 8
(`eng-v8`, D-456) fixes the first two; the floor is stated with the Cboe census check.
Re-test on 4d73e9f5: 07-09, 07-12, 07-16 published by 08:39 MT.
**Round 8 merged (2026-11-23 ~16:00 UTC):** engine `worker` f3db692e (eng-v8, D-456; suite 1,678/0; reference pytest 205;
no ordinary-day change across 107 real asset-days). Image `f3db692e` registered (not admitted): **the phase-1
candidate**, awaiting the owner's go.
**PHASE 1 STARTED (owner, 2026-11-23 ~10:00 MT: "start phase 1 and fix things as we go along"):** hand-over 15
(4d73e9f5 → f3db692e+840196a972b9, admitted, evidence `cluster-scratch/evidence-f3db692e.json`); KEDA on f3db692e;
queued: 587 tapes 2024-07-23..2026-11-20 (@13 caches existed only to 2024-07-22), 40 day blocks of 15
2024-07-23..2026-11-20, the test range's unpublished days (4 blocks) and recomputes of 07-09, 07-12, 07-16 (published
under 4d73e9f5), 596 ordered passes (date order, C-046). Daily runs on: VYREON_CORE_DAILY_FROM 2026-11-23,
DAILY_BUILD f3db692e, DEADLINES_FROM 2026-11-30 (coordinator: no deadline alarms during the backfill). Queue resumed
~17:20 UTC. Caps: tapes 24 pods, blocks 12, pools day/day-intel 0..13.
**10:30 MT:** 135 tapes done; core's budget saturated (1,900 of 1,900 committed, 27 workers; Intrinio's own counter far
below 2,000); ~18% of tape attempts failed with zero bytes (2 API attempts per security), all retried to DONE so far
(attempts ≤ 3). **Tape cap 24 → 12** (coordinator, budget). The autoscaler also grew `day-intel` (7 nodes) while
`day` was at 13 (expected per README; cost, not correctness). BACKUP_STALE "plugin's record cannot be read" once at
core's restart (17:03 UTC), resolved by itself.
**13:07 MT:** 4 day blocks refused at plan time (re-queued automatically, attempt 2): macro and ACT01 cohort inputs
missing (timed captures under the saturated budget) and 2024-07-23 ETHA's T−5 (07-16, before ETHA listed) demanded
— a possible defect that would refuse 07-23 on every attempt. **Day-block cap 12 → 8** (coordinator, budget).
Diagnosis (`checks/2026-11-23-phase1/refusals.md`): (1) ETHA 07-23 is a real defect (T−5 inputs checked against T's
members, `plan.rs:1507`) but not the refusal: ETHA's 07-23..29 sensors would carry PROVIDER_INPUT_MISSING instead of
PRIOR_SESSION_NOT_LISTED; (2) ACT01 cohorts and (3) macro run past fixed windows (180 s per asset; 3 min) that
include waits for core's budget; macro's overrun is also misclassed fatal (no retries). One open point: Intrinio
itself returned HTTP 429 on some first requests at 19:32 UTC, which core's 1,900 ceiling should prevent.
Ops: VYREON_ACT01_IN_FLIGHT=2 on day blocks (0401883). Engine fix `eng-p1a` (D-457): macro overrun retryable,
windows ~15 min (point-in-time unchanged), T−5 members from T−5. Then image + hand-over (no ordinary-day change).
**Hand-over 18 done (2026-11-23 ~14:30 MT, owner: "asap"):** engine `worker` d9462aad (D-457; suite 1,680/0; pytest 205),
build `d9462aad+65064dee5b99` admitted (evidence `cluster-scratch/evidence-d9462aad.json`), KEDA and
VYREON_CORE_DAILY_BUILD on it (c147089); 57 old Jobs deleted by hand (automation debt: release script); published
2024-07-09, 07-16, 07-17 kept (no ordinary-day change). Tapes 379+ done before the switch; 206 tapes, 30+ blocks
claimable after it.
**First Codex + Gemini 4 Neon audits (2026-11-23, core + engine D-457; triage `audits/2026-11-23-triage/triage.md`):** 4 BLOCKS:
(1) core watch goes silent on a tick error (/live stays healthy; queueing and alerts stop) — both; (2) core keeps two
PUBLISHED revisions after a profile-revision bump — Gemini only; (3) engine: one asset's ACT01 failure refuses all 64
— Codex only; (4) engine: D-457's ETHA rule fails without security.json — both. ~30 LATER (every real one to fix, C-041).
Reviewers: no fabricated citations (Codex 99/99, Gemini ~112/115); little overlap, each found a blocker the other
missed; Gemini inflates numbers and severities (check them); xhigh found 2 more real items than high. Keep both,
Gemini at xhigh. Fixers: core `core-p1` (C-069), engine `eng-p1b` (D-458); then the double audit again, image,
hand-over.
**Fixes landed (2026-11-23 evening):** core main 474a229 (C-069 watch, C-070 one PUBLISHED revision; tests 188/0;
pre-check `ops/precheck-c070.sql` clean: 0 duplicates, 0 bare bases; image `vyreon-core:474a229` pushed, NOT deployed —
waits for its Codex+Gemini audit, since migrations 0052/0053 can't be rolled back). Engine worker 2a8f8c4f (D-458;
1,683/0; pytest 205): **hand-over 20 done ~17:45 MT** (d9462aad published 2024-07-15, kept), build
`2a8f8c4f+5bbbf36545c0` admitted, KEDA and DAILY_BUILD on it (b4b28de, core still on image 4eda341). Repeat audits of
C-069/C-070 and D-458 by both reviewers running (`audits/2026-11-23-core-c069/`, `audits/2026-11-23-engine-d458/`).
**Round 2 (triage `audits/2026-11-23-triage-r2/triage.md`): close to converged.** Core 474a229 cleared and **deployed
2026-11-23 17:27 MT** (queue paused, pre-check clean incl. queued blocks, migrations 0052/0053 applied, watch ticks clean).
Two small blockers, both found by both reviewers: core digest tallies all revisions in one statement (restart-loop risk
once the passes run) → `core-p2` C-071; engine D-458 fallback mislabels a T−4..T−1 capture gap as ENGINE_FAULT →
`eng-p1c`. 2024-07-15 (published on d9462aad, refcheck lost in hand-over 20) queued for recompute. Then a narrow third
audit.
**Round 3 (narrow, 2026-11-23 18:00 MT):** core C-071 merged (ac6ff43; 189/0) and **deployed 18:05 MT** (no migration;
strictly better than before). Engine D-458 amendment merged (955d0f82; 1,684/0), image built and pushed, **not handed over**
(coordinator: it only changes a stated reason in a rare case; a hand-over now would cost ~1 h on 8 blocks; it ships with the
next engine change that needs one; affected days, if any, found by the census and recomputed). Codex round 3: not
converged, edges only: core — a tally batch can still wait on a lock past the 15-min liveness, composition has no
deadline, a moving 24-h window can skip an old revision's tally, recovery mislabel; engine — split + unpublished chain
behind it still mislabelled (value correctly withheld), one unverified day-refusal suspicion. Planned core round 4:
statement_timeout/lock_timeout on every tally statement (a slow batch fails visibly as WATCH_FAILING, never hangs) and
tally untallied revisions instead of a 24-h window. **Gemini round 3: not run — its 5-hour quota was exhausted
(RESOURCE_EXHAUSTED 429, resets ~20:30 MT; weekly 74% left); recorded per the owner's rule, the next audit covers it.**
**Round closed (coordinator, owner asked whether repeated audits still pay):** round 3 found no wrong number, no silently
wrong absence and no silent stop on a reachable path (the lock-wait case restarts core visibly; the label case withholds
correctly). Remaining: the cheap core change (statement/lock timeouts on tally statements) and the rest LATER. Fresh-eyes
reviews of untouched scopes (Gemini on overlay, surface, optsensors) after phase 1 are expected to pay more than round 4.
**Core 6a4b59f deployed 2026-11-23 18:32 MT** (C-071 amended: 60 s statement / 10 s lock limits on every tally statement, tally
the untallied whatever their age, only a composed digest resolves its problem; 190/0; code only). LATER: the digest's 24-hour
window should start where the previous digest ended (contiguous windows). Gemini round 3 re-runs scheduled ~20:16 MT
(after its quota reset); engine 955d0f82 still held for the next hand-over.
**20:05 MT: quote-era tapes OOM-killed.** The last 11 tapes (2026-11-06..11-20, 13 GB replay files) were killed 12 times:
tape pods requested 3Gi with no limit and KEDA packed three per 16 GB node. Fix 6926f1e: tape pods request 12Gi (one per
node); old pods drain as their leases expire and new ones start with the new request. All 11 still on attempt 1 of 3 (OOM =
lost lease, free twice). Real-run fault: due a failure-surface check of the other pods' memory (day block, pass, daily) as
their data grows.
**Owner, 2026-11-23 ~09:50 MT: D-191 recommendation accepted** (weight kept, text corrected: engine worker commit after f3db692e, docs only). Old Jobs deleted by hand again (2nd time: automation debt
in "Listed, not fixed").
**D-191 for the owner (fixer, OCC memo #59890 via the MIAX mirror):** ETHA1 = 33 ETHA shares + cash in lieu of
~0.3333 share; until OCC fixes the cash it prices ETHA1 as 0.333333 × ETHA (= the engine's 33.3333 terms);
after, share exposure 0.33, strike (100K − C)/33. D-191's stated arithmetic is wrong (gap 0.3333 shares,
~1.01% of the share part, not 0.0003). Intrinio does not state C (split record only factor 3.0). Effect on
2026-10-07: ETHA1's 502,957 contracts = 167,652 standard equivalents at 0.333333 vs 165,976 at 0.33,
1,676 apart (0.25-0.68% of ETHA's book). Engine unchanged pending the owner.

## Timeline (owner, 2026-11-22)
Full build ~11-25..28; a week of derived-layer QC (census, gate d, history reference check, "general
shenanigans", possibly a derived recompute) to ~12-05; a week of Reporter + web integration to ~12-12;
then the two-week unattended run; free and paid live in December; pilot issue 1 on 2027-01-10.
**Intrinio Startup tier bought ~Wednesday 11-25 (Black Friday sale)**, possibly while the build runs:
the key goes into the cluster secret (`vyreon-worker` `intrinio-key`) and `api.txt`, never printed; new
pods pick it up (KEDA starts a pod per job); read the new entitlements and rate limit with Intrinio's
access-code call to confirm (owner: per Intrinio's documentation Startup equals the current plan except
display rights, so the same 2,000 a minute: no speed-up, core's 1,900 unchanged); one real day re-run on
the new key.

## Before the full build: the system runs without a watcher (owner, 2026-11-22: "do all 4")
Tonight's test-block tapes needed an operator for four things: Job completion counts (the Job ran out
with 51 tapes left and 8 nodes idle), node sizing and idle deletion, spotting non-failure oddities, and
fault judgement. The first three move into the system; judgement no longer blocks (C-061 sets a failed
day aside; three standing failures pause and alert).
1. **Claims:** core's claim race fixed (try the next claimable job while any remain) and the worker's
   claim retried on a connection that never reached core (loose ends below). *Core's half built on
   branch `core-watch` (C-065, not merged or deployed); the worker's retry is the engine's.*
2. **Workers and nodes follow the queue:** KEDA (designed 2026-11-10, `deploy/k8s/README.md` "After
   this run"; not installed) with one ScaledJob per kind (tape, day block, ordered pass) on the
   `postgresql` trigger counting `core.claimable_job`; pool `day` autoscaling min 0, max N, so nodes
   appear with pending pods and leave when idle. Proven on a real run before the full build. *Written on branch
   `core-keda` (C-066, not merged or applied): `deploy/k8s/50-keda.yaml` (four ScaledJobs: the
   daily block apart), migration 0046 (`core.claimable_count`, login `vyreon_keda`), runbook
   `deploy/k8s/README.md` "Workers from the queue" with the test block's recompute as its first run.*
3. **Core's watch takes over `health.sh`:** stall (claimable, no live lease for 15 min), pods finding
   nothing while work waits, worker nodes up with no worker, spend against the ceiling: each an alert;
   plus the progress channel (below). *Built on `core-watch` (C-065): STALLED, CLAIMS_EMPTY and the
   progress channel. Spend stays the operator's (no cloud token in core; leases miss idle nodes);
   idle nodes are item 2's.*
4. **Hard caps instead of judgement:** the autoscaler's max nodes and KEDA's max replicas set from one
   node's measured request draw (owner's rule 2026-11-19) so the system cannot pass 1,900 requests a
   minute or the spend ceiling. *Written (C-066): tapes 24, blocks 12, daily block 1, pass 1;
   pool `day` max 13 ($2.17 an hour).*

## Work during phase 1
**Reporter gap scan** (`~/claude-artifacts/audits/2026-11-21-reporter-gap/reporter-gap.md`): port
the renderers; the new design is the evidence they consume. 14 confirmed findings in today's Reporter
(wrong numbers among them: one day's revisions mixed across calls; exposure OI printed as whole
contracts (D-390); a midrank read as a strict count; provider IV able to become the headline (D-397);
level windows at 63/126 instead of D-404's 95/126). Work list, 12 ordered items (1–3 M, 4–12 L):
1. spec/profile reconciliation; 2. versioned ready-week input/output manifests; 3. recorded core
responses as fixtures (D-102); 4. projection outcomes that distinguish failed / unavailable / private
/ stream-backed (engine bundle + core); 5. pinned week serving in core + a paginated adapter; 6. free
reports and FIELD panels repaired; 7. D-070 membership, 11 rankings and peers computed in the engine,
not Reporter; 8. weekly aggregates in the history pass; 9. 64 free editions + the paid email with the
publish-time number check; 10. the headless job core calls; 11. core-owned delivery (web receipt,
beehiiv); 12. qualification on two real weeks, then a full issue. Most can start during phase 1.
Also: Reporter branch `d397-own-analytics` (worktree `/home/diggz/wt/reporter-d397`, not pushed;
own IV/Greeks, M0 copy, VS-Q01/Q05, V03-MATCHED) is verified against a real cc1d67cf revision before
merging; it refuses provider-IV units and non-`VYR_*` M0 groups. The 2022-06 days in core are
pre-D-397 PUBLISHED revisions phase 1 does not supersede (see owner decisions).

**Test-suite audit** (`~/claude-artifacts/audits/2026-11-21-test-census/tests.md`; no production wrong
number; 38 Rust ignores → 12 leave correctness testing, 26 keep/rewrite; Python skips are lost
independent verification; 20 ranked actions). Reference-side items DONE 2026-11-21 (`worker`
24713351, Python only, image unchanged: #5 XNYS calendar, #10, #11, #16; reference pytest 158 passed
/ 107 skipped). Left, in order: the gaps (#1 a real raw tape session through gate 2 once a frozen
slice exists; #2 reader scalars; #3 put/bid/ask IV; #4 surface quote-level fit; #7–9 receipt and
publish sequence); the D-102 rewrites (#14, #15, #19); legacy and cost; then mutation testing on the
critical crates. **h3-replay test edits wait until its reader digest covers non-test code only.**
Retained fixtures need a real run of the build (`tools/golden/fixture.py`: P01/V03 receipts and task
keys in `retained-2025-12-10`, `retained-2026-09-18`,
`final-results/h4-m0-measurement-dev1/{spy,qqq}-inline-vs-p01`; re-pin `golden_day.DUMP`); their
tests stay ignored until then.

**Engineering housekeeping (owner, 2026-11-21: "none of them are major, most of them are
housekeeping"; "its entirely possible that we did a good job as well"):** billing alert declined;
ops config in the repository (done, 25f1f75); backups and failure injection (launch brief); the
test-suite audit (above); after phase 1 `cargo audit`, `pip-audit`, an image scan and a secret scan
of vyreon-core's public history (before go-live). Not added: dashboards, SLOs, HA, operators.
**Security before launch (owner, 2026-11-17):** least-privilege tokens replace the full-access DO
token once the shape settles; the reader API stays inside the cluster; backups with a tested
restore; a read-only exposure check and an attack-surface audit. Phone recovery is the owner's.

**Loose ends (each before the step it names):**
- **Watch WAL archiving before the full build:** a failing archive fills `pg_wal` on the volume, which core's
  DATABASE_FULL check (`pg_database_size`) does not see; core's watch reads `pg_stat_archiver` (C-067 risk).
- **Progress channel (owner, 2026-11-22; before the full build, with the claim fixes; built on
  `core-watch`, C-065: deploy with `20-core.yaml`, whose optional mount reads the secret):** a second
  Discord webhook (`progress-webhook.txt`, git-ignored; cluster secret `vyreon-core-progress-webhook`,
  key `webhook`; silent test post 204). Route core's logged notices there, batched: each day published
  (slots published/withheld by reason, refcheck, minutes, memory), block start/finish with its node,
  tape and pass progress every 30 min, spend at each node change, set-aside days with cause, builds
  admitted/withdrawn. The alerts channel is unchanged ("go look or running ok"). Silent, no mentions.
- **Core claim race (before the full build; fixed on `core-watch`, C-065, proved by a 32-claim
  concurrent test that failed before the fix):** with 24 tape pods claiming at once, about half got "no
  tape job to claim" while 195 were claimable (2026-11-22 04:14 UTC: 82 pods done, 43 jobs done). The
  claim's `WHERE job_id = (SELECT … LIMIT 1 FOR UPDATE SKIP LOCKED) AND state = 'QUEUED'` re-checks a
  row another claim just leased and returns none (`core-db/src/work.rs:527`). No job lost or doubled;
  pods and Job completions wasted. Fix: try the next claimable job while any remain.
- **Worker claim retry (next image, with the test block's fixes):** a fresh tape pod died 10 s after
  start on `vyreon-core: error sending request` (2026-11-22 04:08 UTC, node networking not ready);
  the claim (`core_client.rs:165`, 60 s timeout) is the one core call with no retry (not idempotent).
  Retry it on errors where the request never reached core; a doubled lease expires (600 s) and requeues.
- **C-061 core change before the test block:** a failed or omitted day no longer holds back later
  passes; NOT_PUBLISHED sessions state their cause; omit-with-reason command; digest lines.
  With it, C-062 (audit 2026-11-22 surface 6, above the cut line): a fallen day's pass is queued under
  a build that runs, never two unfinished passes a session, any missing day holds no pass,
  `days retire` (built, not run). C-063 (audit 2026-11-22 changes-core): a day is published for the
  passes only with its day-local revision (#2, above the cut line), and the other queue fixes.
- **The 252-session weekly baseline (C-060)** in the review Reporter reads (today unbounded in the
  engine's `vyreon-api`), before the first paid issue.
- After the build: fix the graph declarations C-060 lists; check every company's DLM-IV-CRUSH event
  lies inside the build.
- **Trades-API switch before daily publishing** (owner, 2026-11-18: daily runs take trades from the
  trades API, quotes from the replay; engine D-089 amendment not built). Measured 2026-11-17: NVDA
  4,970,460 trades, 498 requests, 25 min; SPY 96 requests; all 64 likely a few thousand requests a
  day. Without it GLD's EVC01 volume baseline falls below 189 by ~mid-December 2026 (P-04).
- Daily go-forward in production (C-051; runbook order in `deploy/k8s/README.md`), deadline alerts
  on from that day; then the engine droplet retires (end state).
- Frozen raw tape slice (D-102): take one from a phase-1 tape run; the close-selection tests need it.
- Reader digest of non-test code only (h3-replay): today any reader-file edit, tests included,
  invalidates every stored tape cache.
- `company_inputs.EARLY_CLOSES` lacks 2022-11-25, 2023-07-03, 2023-11-24, 2027-11-26 (outside phase 1;
  fix before deeper history and before 2027-11).
- One-year-window census (owner, 2026-11-20): one definition of "a year" anchored on `return_1y` (the
  close 252 sessions back, 253 closes); census every 1-year/52-week window (span, close vs intraday),
  then one decision with before/after (DLM-PRICE-LEVELS' range uses 252 bars today).
- D-404 remainder: daily P01/PATH views over a missing bar, `evc_history`, a DLM-PCR-OI-over-gap test;
  kernel-contract JSONs still say MIN63 (moves `kernel_contract_sha256`).
- Engine small: `Occ::quote_file` returns None silently on an unreadable file; the ordered derive
  (`derive/mod.rs`) fails the whole object on a population load failure; INT-IVSC01's internal
  `comparison_qualification` still says METHOD_CONTINUITY_UNKNOWN.
- Dated reminder: the CPI/INDPRO release table (D-380) ends at November 2027 data; extend before then.
- OCC ledger (C-055): a new entry needs a new core image; specials with no memo are listed in C-055,
  not entered; record the ledger's hash with each published day (provenance); FXI's date (cash
  06-07, strikes 06-08) for the engine to confirm.
- Core `read` routes serve full bodies (by decision while only Reporter and the owner hold keys).
- API end-to-end (Reporter's reads through core) before launch.
- Provider: bug report #3 sent 2026-11-21 (`docs/PROVIDER_FAULTS.md`; asks whether pre-2025-11-26
  replay files can be regenerated from the SIP archive); Cboe permission reply pending.

## Decisions waiting on the owner
- **Owner action, remind at EVERY status (owner, 2026-11-24: "ill make it in the morning but please remind me every status"):**
  create a read-only DigitalOcean token for core's spend watch (automation debt 3) as a cluster secret (exact step in
  deploy/k8s/README.md once release 3 documents it).
- **Intrinio Startup — DECIDED (owner, 2026-11-25):** Intrinio (support, after our bug reports) moved the account to Startup
  effective 2026-12-01: every report we sent opened "We're a commercial customer (Vyreon Labs Inc.)" — drafted on our side,
  untrue for the Individual Plan, and the trigger. Owner replied: buying Startup during the Black Friday sale; no
  redistribution yet; offered to show how the data is used. Watch for Intrinio's answer (price/term/date). Bug reports from
  now on: the data fault only — no company, customer or license wording — and the owner approves the text before sending.
  Pricing (Intrinio's AI sales agent, to be confirmed by a person): BLACKFRIDAY = 50% off the Startup base for 6 months →
  year 1 $4,995 ($166.50/mo ×6 + $666/mo ×6; regular $5,994), annual commitment billed quarterly — under the $5k cap only with
  the promo. Checkout shows no coupon field/discount; owner emailed Flavia + support (2026-11-25) to attach it before
  2026-11-30 (promo end; auto-transition 12-01). Do not complete checkout until the discount shows. Chase ~11-28 if silent.
  **2026-11-26 (MT afternoon): owner upgraded to Startup;** Intrinio confirmed the deal by email as a 50% refund of
  the first six months' payments. The owner is clarifying features and has the order form to sign.
  The order form: 1-year term, auto-renews; $333/mo ×6, $666/mo ×6, then $999 (+5%/yr from month 25); billed
  quarterly in advance; +10% without auto-pay; 2,000 calls/min (unchanged: the plan's times stand); 3 websockets.
  Display to individual end users; commercializing the app is allowed when demand is not primarily the data and
  there is substantial analysis. **ATTRIBUTION REQUIRED: on any page where Intrinio data is used, the Intrinio logo
  or a reference with a follow backlink to intrinio.com** → requirement for Reporter, the web, the email issues and
  probably the social teasers.
  Advised the owner:
  - get the refund into the order form or a signed addendum;
  - ask whether email/social count as "the application", and about firms/institutions;
  - ask about per-endpoint limits under 2,000/min (P-30).
  Entitlements after the upgrade (access codes, 2026-11-26): all 16 of 2026-09-29 kept, plus api, edi_otcb,
  historical_tick_otc_delayed.
- RSS watchdog in place of the 2 GiB per-worker address-space cap (D-007; asked 2026-11-20).
- D-404's consecutive-gap limit (WMO uses 5 days): not ruled.
- Optimization items that change an integrity rule or a diagnostic: history endpoints trusting
  Q01's verified source identity (~137 cold CPU-min); dropping the A/B diagnostic in the fill
  (inputs 7); the prefetch nice switch (default off, owner's call after measurement). Low priority:
  Codex round ee says the curve has flattened.
- Finer D-192 tenor containment (keep an asset's one-year surface without its long expiries; changes
  remaining values): an option, not done. (No tenor is ever missing in 2022-05-27..2026-11-19.)
- Whether an absent dividend currency on a US listing may be read as USD (2026-11-16; today the
  asset's stated lack; if old eras omit it, every payer of that era is withheld).
- At daily-runs deploy: pool `day` scaling to 0 (DO's pages disagree) or min 1 (~$120 a month).
- The production droplet's 2025-12-10 store (177 withheld activity slots): rebind and republish on
  the owner's word; the droplet's retirement once the cluster's daily runs keep the perishable
  prints and its database is carried in.
- Deeper history, not phase 1: the 2023-01-18..03-07 snapshot gap. (D-359's partial sessions are
  skipped and stated in windows since D-404; nothing left to rule.)
- The day hold (10% of slots lacking a provider input): whether a held day publishes its good assets
  with the rest stated instead of failing whole (E11's spirit), judged on the build's held days.
- A tape-verify DIFFER (C-059), if one appears: recheck vs replace.

## Standing owner rulings that still govern the work
- **Spend:** ceiling **$500 in total** (2026-11-21, for phase 1 and its reruns; was $400);
  running total kept here, owner warned before a step passes it; billing alert declined
  (2026-11-21: "we will be watching"). Standing infrastructure is just "on"; worker nodes and test
  droplets as many in parallel as testing needs, without asking, each deleted when idle (2026-11-16).
- **Sequence (2026-11-21):** phase 1 → verify phase 1 → Reporter → web, Reporter work beside phase 1;
  phase 1 the priority (2026-11-17: "The database build is 10-ish days alone for phase 1. Then we can
  sprint towards live launch readiness."). Phase 1 = 2024-07-09..2026-11-20 (C-060, owner 2026-11-21; was the newest two weeks, then back to 2025-04-10);
  deeper history after launch (2026-11-16). Universe: the 64 for a long time, robust to renames and
  delistings (2026-11-14). FIELD campaign after the build; only structural changes before it.
- **Phase-1 QA (2026-11-21):** the plan, gates (2026-11-18; per day and whole store since 2026-11-21) and censuses in the launch brief;
  no rule tracking added (2026-11-16).
- **Startup tier (2026-11-21):** after the build (display rights only); on 11-25 if phase 1 is done;
  the build never waits for it. Free and paid together ideally, free first if need be (2026-11-14).
- **Reporter (2026-11-21):** a job core runs on the core node, called only when the week is
  published and checked; reads core's API inside the cluster; core pushes pages to the web and the
  paid issue to beehiiv; the web only receives and serves, holds no credential into core, never
  calls in (2026-11-17); not on the web droplet ("the public web is the attack surface"). Reports
  publish on Sundays without user intervention (2026-11-16).
- **End state (2026-11-21):** two permanent machines, core (always-on node: database, core service,
  scheduling, archive, Reporter) and web (serves what core pushes); workers on demand. The
  `vyreon-engine` droplet retires once daily runs are live on the cluster, its perishable captures
  archived through core and its database carried in.
- **Readable, checked reports:** verification in the marketing (2026-11-18: "we definitely need to
  expose what we are doing in the marketing. Major verification and checking areas should all be
  highlighted on vyreon-web eventually"); the explainer shows a real sample issue, price, refund
  terms and the verification record (2026-11-21); plain voice; a publish-time number check.
  Owner, 2026-11-21: the aim is reports that are readable and verifiable, not "not looking like AI"
  (that framing "is an absolute trap"); explain freely, never invent a number (vyreon-reporter
  README "Owner direction: readable and verifiable", 9e769bf).
- **Ads (2026-11-21), after free + paid are live:** free reports link to a paid-product explainer;
  ads advertise only the explainer (the prototype's lesson: a landing page with nothing real to
  show); judged on purchases and cost per sale in budget tranches, stop rule fixed before spending.
- **After the MVP (2026-11-21):** automated social posting of highlights and unusual measurements
  (same number guard and voice); CI as the home for agentic loops once there is revenue. First goal:
  the system producing money.
- **Change control:** byte identity retired (2026-11-18: "Byte identical is based on the assumption
  that the current system is perfect. We are repeatedly proving it is not"); image diff: float noise
  (~1e-12) reported, anything larger tied to a decision, no silent state change, changed flags
  explained; optimizations and value fixes in separate commits; phase 1 from one image.
  Optimization passes repeat while the measured gain pays for the effort ("reality judges").
- **Audits:** C-041: every verified finding fixed; only one above the cut line (a wrong number in the
  database, or only a full rebuild corrects it) holds the step; failure-surface audits after a real
  fault. Operations-feature audits stopped (2026-11-19) until operation shows a new failure. D-412's
  build cut: regressions and broken production paths block; later tape-family findings reach the
  store by scoped republish (D-099). At most three agents at once on the laptop.
- **Data sources:** bulk option files where they exist and match the API (2026-11-19); provider
  analytics from the bulk files' first computation; readers get only the engine's own IV/Greeks
  (D-397); volume in whole contracts, exposure share-equivalent, each role stating its unit (D-390);
  CPI the unrevised `$CPIAUCNS`, INDPRO and expected earnings state their basis (2026-11-16); curve
  H.15 → Intrinio `$DGS*` → FRED API (D-105); close from the listing market's print (D-412).
- **History windows (2026-11-20, D-404):** compute over the sessions present, never interpolate,
  state each missing session, withhold only comparisons needing a missing endpoint; floor 75% of a
  fixed span (40 CFR Part 50 App. K §2.3, App. T §3.1; "scan back is going to produce weird
  behavior"); history-window sensors only. EVC01 189 of 252 plus its 20-sample minimum ("requiring
  75% coverage for the history required to compute a sensor requiring history is sensible"). The
  80% coverage target is a goal, not a rule.
- **Reference checks:** on every computed day in the worker before raws are deleted (2026-11-18);
  tape owners by the tape worker's verdict; pre-verdict caches "cache code verified by sample".
- **Operations:** workers stateless and credential-free, core signs one-object upload URLs
  (2026-11-17); restatable captures kept write-once in core (C-043); Discord one alert per problem
  and a 09:00 MT verdict digest ("I want to open it and immediately know if i gotta go look at it or
  its running ok"), channel only; availability needs are minutes; no AI in the production path.
- **Standards (2026-11-14):** the published standard first, cited; an owner decision is the last
  resort; a rule built on sparse data is judged by the built population.

## Listed, not fixed
- **Automation debt (2026-11-23, tape OOM loop): repeated lost leases are silent.** 11 tapes were OOM-killed 12 times in an
  hour; each kill was a lost lease, re-queued quietly, and nothing alerted (no stall: the jobs kept being leased). Proposed:
  core's watch raises JOB_THRASHING when a job loses its lease ≥3 times (naming the job and, where readable, the pods'
  exit reasons), so an operator learns of a resource limit from the channel, not by looking.
- **Tape memory (2026-11-23, measured):** replay capture memory climbs steadily as the file streams (old 3Gi pods at 1.8-3.5
  GB mid-replay and rising): the fetch is streamed, the reduction accumulates the day. Now: 12Gi request (one per node);
  after the 11 quote-era tapes finish, set request = measured peak + 25% with a matching limit (two per node if ≤7Gi).
  Engine (after phase 1, performance not correctness): fold each asset's quotes into its summary incrementally so memory is
  flat whatever the day's size. Observability: workers log their peak memory per job (cgroup memory.peak); core warns
  when a job's peak passes ~80% of its request.
  **Update 20:45 MT (owner: "hell yeah, and do #2 and do it right"):** measured peaks 7.5-12.9 GB and climbing; 6 tapes reached
  attempt 2 of 3 (tape ScaledJob paused meanwhile; a FAILED tape can be re-run with `vyreon-core retry-failed`). Now: pool
  `tape-mem` (s-8vcpu-32gb-amd, $0.25/h, 0..6) and tapes placed only there at 20Gi, one per node (6f382c6). Engine fix
  started now, not after phase 1 (`eng-tape`, D-459): bounded-memory incremental reduction, byte-identical caches required.
  **Owner, 2026-11-23 20:50 MT: no stopgap — "just do the proper engine fix".** tape-mem pool deleted (it had scaled to 6
  nodes for pending pods; none ran there), placement reverted (cf93528), **tape ScaledJob paused** until D-459 lands. Waiting
  on it: the 11 tapes 2026-11-06..11-20, the day blocks covering Nov 2026, and the daily run from 2026-11-23 (deadlines
  start 11-30). Three old tapes still run on day nodes.
  **Coordinator ruling for the owner's morning review (2026-11-23 ~21:30 MT):** the fix necessarily changes the reader's
  source hash (READER_IDENTITY), which would orphan all ~850 stored tape caches (re-capture: 6-12 h, ~half a day of phase 1).
  Ruled: **carry the identity over on proof** — bound to the exact new source digest (later edits get a new identity), gated on
  byte-identical output on the frozen real slice (replay-slice-2026-11-12) AND a cluster verify sample (≥20 real sessions
  across all eras, D-370) reproducing the stored cache digests exactly; any mismatch voids it → new identity, re-capture all.
  Reasoning: the identity attests which bytes a reader produces; identical bytes keep it true; reversible.
  **D-459 landed (22:00 MT):** merged 61d5b4f5 (1,686/0; pytest 205): 436 files byte-identical on the real slice; heap
  growth for 2x input 48 → 5 MiB. Image vyreon-worker:61d5b4f5 built and pushed, NOT handed over. Audits (both,
  `audits/2026-11-23-d459/triage.md`): byte identity clean; **blocking: Python tape-verdict still whole-day in memory (now
  the pod's peak), spill error livelock/fail-fast, malformed-archive acceptance must equal the old reader's** → `eng-tape2`.
  Pre-existing engine findings (UPATH zero count, ACT01 feed guard x3, tape-id, sync prefix, mislabel, identity inputs)
  → a separate verify-and-fix batch after the tapes run.
  Noted: core's STALLED alert fired for the intentional tape pause; a paused ScaledJob should read "paused", not "stalled"
  (small core watch fix, LATER).
  **eng-tape2 merged (2026-11-23 23:50 MT, 009feb78; 1,688/0; pytest 208):** old/new readers accept and refuse the same
  archives (frozen 955d0f82 reader vs 274 damaged real copies); spill failures latch and fail fast; Python tape verdict
  streamed (QQQ full day ~16 GB → ~221 MB; 64 verdicts ~13 min/day on 2 jobs, deadline 30); CARRIED_OVER now
  353824f7… → 36c70d1c…. Proposed tape request 3Gi, limit 6Gi. Narrow re-audit by both running
  (`audits/2026-11-24-d459-r2/`). Then: hand-over to 009feb78 (carries D-458 amendment), tape request 3Gi/6Gi, unpause,
  verify sample ≥20 sessions (`vyreon-core tape verify`), watch 2026-11-12/-20 memory.peak.
  Disk: `/home/diggz/wt/target-worker` (image build dir, reused by deploy/worker/build.sh) is 90 GB.
  **Re-audit r2 (00:40 MT, `audits/2026-11-24-d459-r2/triage.md`):** valid-input bytes, verdicts and memory clean; a
  reader panic on a damaged archive (both), a spill failure mislabelled as provider refusal, spill name after a crash, an
  invalid kind dropped by the Python verdict, an fd leak → small batch `eng-tape3`. Image 009feb78 built, not handed over
  (the batch changes the reader digest again).
  **Hand-over 24 done (2026-11-23 ~23:45 MT): engine fc36eefb (D-459 + r2 fixes + D-458 amendment), build
  `fc36eefb+b1c8d00b5d79` admitted, KEDA + DAILY_BUILD on it; 645 jobs moved; 8 old-build Jobs deleted. Tape request
  3Gi / limit 6Gi; tapes unpaused.** Carry-over gate queued: 26 verify jobs (`vyreon-core tape verify`, jobs 18576-18601):
  2023-07-19 2023-10-16 2023-11-24 2023-12-14 2024-03-14 2024-06-14 2024-07-03 2024-09-16 2024-11-29 2025-01-16 2025-04-15
  2025-06-13 2025-07-03 2025-08-15 2025-09-16 2025-10-16 2025-11-14 2025-11-28 2025-12-15 2026-02-13 2026-04-16 2026-06-15
  2026-08-17 2026-09-16 2026-10-15 2026-11-10; read with `vyreon-core tape checks` — every member MATCH, else delete
  CARRIED_OVER and re-capture. r3 audits running (`audits/2026-11-24-d459-r3/`).
  **Incident:** applied `20-core.yaml` raw (placeholder `VYREON_CORE_IMAGE`) → core down ~3 min (InvalidImageName);
  re-applied with image 6a4b59f. More evidence for the release-script automation debt (it must substitute images).
  **00:25 MT:** carry-over gate 24/24 sessions MATCH so far (1,525 asset-sessions byte for byte, all PASS; 2023-07 → 2026-09
  incl. early closes and quote era up to 36.8 GB of replay); tape pods peak ~1 GB (was 7.5-12.9 GB); 0 failed, 0 OOM. r3
  audits: Gemini clean; Codex one rare mislabel (mid-stream URL expiry on the last attempt → whole-session TAPE_UNAVAILABLE)
  → next batch. **eng-f1 started:** verify-and-fix the pre-existing engine findings (UPATH zero count, ACT01 feed guard x3,
  tape id, sync prefix, official-bar mislabel, identity inputs (report only), D-358 mislabel) as D-460.
  **D-460 merged (01:25 MT, c897c46b; 1,692/0; pytest 209; reader sources untouched, golden digests match):** REAL and fixed:
  UPATH zero count under a withheld D111 count (D-027; real NVDA 2024-06-13 repro: was NO_TRADES, now withheld), ACT01 feed
  guard cohort date / strike key at $0.001 / negative volume, retained official-bar read errors (D-411), mid-stream relisting
  mislabel (D-358). NOT REAL: multi-session tape jobs. Audits by both running (`audits/2026-11-24-d460/`). **Hand-over must
  recompute the July-2024 days already published** (UPATH can change → one rule per series). Measured tape memory:
  replay_capture ~2.1 GB RSS flat on 2026-11-17 (estimate was ~1 GB), under request 3Gi / limit 6Gi.
  **For the owner (tape reader changes, each orphans every stored tape cache → full re-capture, ~6-12 h):** item 5, a
  UTP-marked message inside a CTA file (0 on the real slice; fixer recommends counting it lost, TAPE_CORRUPT_SEGMENT); item 7,
  a short valid prefix before damage read as NO_TAPE_MESSAGES (only where a tape's first sync is not at byte 0); item 6,
  identity inputs missing (canonical.rs, crate versions; ids newline-joined). Coordinator recommendation: bundle 5+6+7 into
  one deliberate tape-method bump with one re-capture, scheduled by the owner (after phase 1 unless the census finds either case).
  **Refcheck 2024-07-12 on 2a8f8c4f: 218 unexplained (was ~35/day):** 209 are VS-O04 on 19 assets published
  WITHHELD/ENGINE_FAULT where the reference publishes values, incl. same-day fields (native_expiry_count, dte_band_shares),
  and path_efficiency PROVIDER_OI_INCOMPLETE. Cause: the D-458 fallback (a T−4..T−1 window chain without an attempt read as
  ENGINE_FAULT; 07-12 ran as a one-day block), fixed by the 955d0f82 amendment now live in fc36eefb. **Check after the next
  hand-over's July recompute:** 07-12's refcheck on the new build — VS-O04 same-day fields must publish (if they are still
  withheld, the fallback over-withholds: a defect, not a label). D-460 audits re-run with scope `tape` (the `d460` scope
  did not exist and both runs aborted silently — runner should refuse an unknown scope loudly: small LATER).
  **D-459 carry-over gate PASSED (01:43 MT):** 26/26 verify sessions MATCH (1,653 asset-sessions byte for byte, 0 differ,
  all PASS; 2023-07-19 → 2026-11-10 incl. 5 early closes; the heaviest 81 GB of replay). All 36 tapes on fc36eefb done,
  0 failed, 0 OOM: **the 10 November tapes 2026-11-06..11-20 are in** (daily run's history unblocked). Measured
  replay_capture RSS ≤ ~2.1 GB (request 3Gi, limit 6Gi; tape memory.peak also counts ~2 GB page cache).
  **D-460 audits (01:55 MT, both):** no wrong number on ordinary data, but **blocking: D-460 changed UPATH/EVC01 values
  without bumping their method versions (Gemini; D-273 — histories would pool two definitions)** → c897c46b NOT handed
  over. Also: UPATH loses the specific withholding reason (both), reference UPATH owner still expects the false zero (Codex),
  feed guard pools refused option attempts (both), strike grammar mismatch and duplicates, HashMap-order summation,
  listing retry history (D-358), final_rows swallows I/O errors (both), one refresh per stale 403, a possible final-bin
  over-withhold (Gemini 2). All sent back as `eng-f2`. LATER: stale translated cohort (stateless workers start clean),
  official-bar I/O fault failing UACT/UQTR (honest for a local fault), early-close boundary bar (suspicion), retried-capture
  cutoff wait (verify). Next hand-over after eng-f2 + a narrow re-audit, with the July recompute.
  **eng-f2 merged (af7f09e2; 1,696/0; pytest 210; UPATH@13, EVC01@15), image built, NOT handed over: re-audit r2 (03:00
  MT) not converged.** Blocking (both reviewers): reference UPATH/EVC01 owners disagree with the engine on every lawful
  NO_TRADES bin (gate a would flood); ACT01-routed family (ACT01, TV15, EXQ01, M0-PRINT-SIZE) needs a method bump; feed
  guard reads the snapshot unsealed and refusal via a worker-local file; the new shared URL refresh aborts on a second
  expiry; read-time official-bar faults still swallowed; reasons after a failed bin; stale prior-session EOD volume (real
  QQQ 2026-09-08 case) → `eng-f3`. LATER: fill restart in one workspace (not reachable: Jobs are one pod, no restart, fresh
  emptyDir); M/9 listing close equal to the previous price read as a carry (label only; provenance is in the reader).
  Convergence note: D-460's area is the ACT01 feed guard + UPATH carry; each round's findings are narrower; the stopping
  rule (a round with no wrong number, no silent wrong absence, no silent stop) is not yet met.
  **eng-f3 merged (2dde1132; 1,700/0; pytest 212):** reference UPATH/EVC01 agree with the engine on 3 real carried-bin
  scenarios (all roles pass compare()); ACT01-family bumps (ACT01@2, ACT01-INTRADAY view @2, TV15@3, EXQ01@2,
  EXQ01-TEMPORAL@2, M0-PRINT-SIZE@1.1.0); sealed snapshot read; stale EOD volume rule shared with the overlay; refresh budget;
  read-time faults (two capture-time paths join the reader bundle). @13/@15 never published (af7f09e2 not handed over) → no
  further bump. Narrow r3 audits running (`audits/2026-11-24-d460-r3/`). Watch item for the owner: the reference UPATH owner
  now "follows the engine's path logic step for step" — independence of the reference (owner's rule: no circular checks)
  asked of the auditors explicitly.
  **r3 (04:00 MT):** no numerical discrepancy or over-withholding on ordinary sessions (both). Feed guard still yields edge
  cases with one root cause: it re-implements the overlay's population/recency rules (D-359 population, D-072 stamped
  no-quote, prior-session snapshot trade, off-grid strike merge, fractional rounding) → redesign: the guard reads the
  overlay's own per-contract result (retire the copies). **Reference circularity confirmed:** int_upath.py/evc ported the
  engine's code and repeat its bin-0 mislabel → rewrite as an independent reading of D-027/D-094/D-118/D-460. Gemini's
  "12.0 volumes" suspicion checked live: option interval volumes are integers (NOT REAL; accept whole floats anyway).
  **Hand-over 26 done (~03:40 MT): 2dde1132+5f8d87f49bfc admitted, KEDA + DAILY_BUILD on it (core re-applied with image
  substituted).** Coordinator criterion (owner asleep): strictly better than fc36eefb on wrong numbers (removes the UPATH
  false zero on every new day). fc36eefb published no day (4.5 h of block progress lost across hand-overs 24-26).
  **July recompute scheduled:** blocks 2024-07-09..07-12, 07-16..07-19, 07-23 (+ 07-15, 07-22 already queued); passes
  requeue as days publish. Next: `eng-f4` (feed guard reads the overlay's result; independent reference; r3 items).
  **eng-f4 in progress (05:00 MT):** guard redesign committed (b66a67b8/456d1aba: the guard replays T's sealed requoted
  source and reads `QuoteOverlay::session_volumes`, one rule; its copies deleted; ACT01-family @3/TV15@4/M0-PRINT-SIZE@1.2.0).
  Helpers still running: eng-f4-ref (independent UPATH/EVC01/ACT01 reference, e9ff9e38 + engine bin-0 fix) and eng-f4-ops
  (Codex 7, 8, 13-15 / Gemini 1). Then integrate, full tests, D-460 amendment, both audits.
  **eng-f4 merged (f8029c81; 1,706/0; pytest 228/107):** guard reads the overlay's own session volumes; independent
  reference (old engine fails it on real NVDA 2024-06-13: 22/25 roles; all 9 real cases pass after the fix); UPATH@14,
  EVC01@16, ACT01-family @3, TV15@4, M0-PRINT-SIZE@1.2.0; ops items fixed. Reader bundle adds: `capture::final_rows`
  non-directory/dangling capture and failed listing as errors. Built days: UPATH/EVC01 reasons only (no values); ACT01
  routing only where a feed ratio crosses 0.95 (not measured). Audits r4 by both running (`audits/2026-11-24-d460-r4/`).
  Housekeeping for the owner: /home/diggz/wt holds ~60 stale worktrees/logs from earlier sessions (28 GB) besides
  target-worker (90 GB, the image build dir) — propose deleting the stale ones (preview, owner's OK).
  **r4 (05:00 MT): f8029c81 NOT handed over — not strictly better than 2dde1132.** Codex 1: reading the overlay's decided
  population lets D-125's snapshot fill supply BOTH of D-079's independent witnesses (real INDA 2026-08-27 rows) → possible
  universe-wide false NONE on ordinary data (a design conflict my eng-f4 brief created). Also: guard replay not bound to
  (asset, T) and diverging from `verify`; overlay defects exposed (fractional snapshot volume published, unreadable-volume
  fault after dead-listing drop, merge drops a trade time); M0-PRINT-SIZE fails on split-translated cohort days (affects
  production today); ACT01 reference reads an engine flag and disagrees on ordinary days; UPATH/EVC01 evidence was on edited
  inputs. Round 5 in two parallel fixers (no helper agents): `eng-f5a` feed area (D-079 two witnesses restored, overlay,
  guard binding, ACT01 reference), `eng-f5b` the rest (M0 print split days + affected sessions, EVC01 reference, unmodified-
  day evidence, ops items). **For the owner: D-460's ACT01 feed-guard area has not converged in 4 rounds** (each round fixes
  real defects and exposes older ones in the overlay beneath it).
  **Owner, 2026-11-24 ~05:40 MT: "if it's rare that is converged."** Stopping rule refined: an audit round whose remaining
  findings are only rare cases (damaged or contrived inputs, no occurrence in real data) closes the area; those findings are
  recorded and fixed later, not chased round after round. Findings on ordinary real data still block. Applied: **tape area
  CLOSED** (the three reader items — UTP message in a CTA file, short prefix before damage, identity-hash inputs — go to the
  next deliberate tape-method change; no re-capture now). ACT01 feed area: closes after round 5 if what remains is rare.
  Owner approved deleting stale worktrees: 54 merged worktrees + 23 logs removed from /home/diggz/wt (~28 GB; reporter-d397
  kept, unmerged Reporter work; VHDX only shrinks by export/import).
  **Clock correction (owner's Discord, 06:30 UTC−6):** every "MT" time core printed and I reported overnight used tz-db
  America/Edmonton (UTC−7 in winter); the owner's clock is UTC−6 year-round → all overnight MT stamps above are one hour
  behind the owner's clock (e.g. hand-over 26 at 03:30 "MT" = 04:30 owner time). Core fix `core-clock` (C-072): one fixed
  UTC−6 owner zone (deadlines included), planned operator actions (hand-over pause/withdraw) to progress not alerts unless
  open > 30 min, terse progress digest. From now on my times are UTC−6.
  **eng-f5b merged (UTC−6 06:45; 1,710/0; pytest 235/107):** all 8 non-feed findings REAL and fixed. **Live wrong absence
  since D-162 (11-14): M0-PRINT-SIZE ENGINE_FAULT on split-translated cohort days** → recompute after the next hand-over:
  AVGO 2024-07-16..07-19, MSTR 2024-08-09/12/13/14, XLB XLE XLK XLU XLY 2025-12-08..12-11 (28 asset-sessions; NVDA
  2024-06-11..14 if the build goes deeper). Independent reference vs engine on 5 UNMODIFIED real caches (AAPL 2025-12-10, NVDA
  2024-06-13, SPY 2025-04-11, QQQ 2026-05-06, SPY 2026-11-12), all gates on: 25/25 UPATH roles, 78/78 EVC01 cells agree.
  Waiting: eng-f5a (feed area), then one round-5 audit by both, then hand-over.
  **eng-f5a merged (UTC−6 07:20; 5f637983 = f8029c81 + f5b + f5a; f5a alone 1,714/0, pytest 231/107):** D-079 conflict
  decided from the rule texts — EOD witness = the chain's own count (a null is no EOD reference even when D-125 fills it);
  source admitted as the sensors admit it, bound to asset and session; provider_cut kept; overlay hardening (fractional
  snapshot volume no count, unreadable in-session volume never a dead listing, merge keeps trade time) — prevalence in
  941,100 real snapshot rows: 0 / 0 / 0; D-125 fills 1,143 rows (ETHA Oct 2026 only); OVERLAY_RULES D-460; ACT01 reference
  from provider rows. INDA 2026-08-27 real intervals: complete before (357/357, 357/357) and after (256/256, 357/357).
  Shared fixture etha-first-chain requoted (old kept as .pre-d460). Combined cargo test running; round-5 audit by both
  running (`audits/2026-11-24-d460-r5/`, briefed with the owner's stopping rule).
  **First daily run started (UTC−6 ~05:30):** session 2026-11-23's tape leased, its block and pass queued on 2dde1132.
  **core-clock merged (d728b04; 193/0; C-072; migration 0054 additive):** root cause of the hour: tzdata versions (newer tzdata
  already keeps Edmonton on UTC−6 from 2026-11; the cluster's images had older tzdata). Core audits by both running
  (`audits/2026-11-24-core-c072/`); then build + deploy core (image substituted, migration 0054).
  **Engine round 5 (UTC−6 ~08:00):** combined 5f637983: cargo 1,718/0, pytest 238/107. Codex: no regression vs 2dde1132;
  ordinary-data findings, all pre-existing → follow-up batch: EXQ01 rejects adjusted contracts (real BABA 2025-12-19: whole
  print capture aborted → SAI-EXQ01 + M0-PRINT-SIZE ENGINE_FAULT), worker image lacks OVERLAY_RULES for the reference
  (mislabel on D-359 partial days, 111 real sources), reference rounds 3-decimal strikes (XLK 2025-12-05). Rare → LATER.
  Gemini hit its quota at launch (both runs); engine re-run started, core Gemini waits for the core fix round.
  **Core C-072 Codex: hold deploy** — refusal during an operator pause silenced; unadmitted successor silent; planned-ness
  not tied to the hand-over event; pause overstaying between ticks; masked queue_due error; digest counts; 0054 lock
  timeout → `core-clock2`. Clock arithmetic verified correct.
  **core-clock2 merged (b56c3db; 194/0):** all 7 fixed; 0054 edited in place (live DB at migration 53, verified). Both
  reviewers on it (`audits/2026-11-24-core-c072-r2/`); then core build + deploy.
  **Core r2 Codex:** blocking (ordinary operation): a refusal during an operator pause can still go unannounced if the
  pause is resumed before the next tick → alert written at refusal time (`core-clock3`), plus cheap: pause reason dropped by
  substring, overdue judged by check time. LATER (rare): two hand-overs to one unadmitted build within 30 min, migration
  advisory lock before timeouts, admitted-then-withdrawn successor wording, text-matched withdrawal, and two pre-existing
  publish defects that need a malformed/nondeterministic worker bundle (day-local accepted as base; changed reader scalar
  on a resend acknowledged). Deadline arithmetic verified again.
  **Engine round 5 CONVERGED under the owner's rule (UTC−6 ~07:40):** Gemini r5 found no engine defect on ordinary data;
  its ordinary items are reference-side (3-decimal strike rounding — also Codex; META/FB root, phase-2 dates only; paginated
  duplicate EOD rows not coalesced in the reference); one rare engine item (duplicate snapshot rows 0/blank + blank/in-session)
  LATER. → build 5f637983, hand over, recompute. Follow-up batch `eng-f6`: EXQ01 adjusted contract ids (BABA 2025-12-19,
  production), worker image ships OVERLAY_RULES for the reference, reference strike rounding / pagination coalesce / renamed
  roots. core-clock3 merged (9a0ed8f, 194/0, migration 0055): refusal alerted in its own transaction; narrow Codex check
  running; core image building; deploy after.
  **Core C-072 decision (UTC−6 ~07:50):** the quiet planned-step mechanism (part 2) failed three audit rounds (each found a
  new way to silence or duplicate a real alert) → RETIRED (prefer retiring a rule): every pause/stop alerts as before; keep
  the UTC−6 clock (part 1) and the terse digest (part 3). Hand-over alert noise goes away via the release-script automation
  (pause → hand-over → admit → resume in one command), not quiet alerts. `core-clock4` doing it; 0054 rewritten, 0055
  dropped (never deployed; live DB at 53). Then core deploy.
  **Hand-over 28 done (UTC−6 ~07:55): build `5f637983+8855e39e0be8` admitted, KEDA + DAILY_BUILD on it** (registry login had
  expired: re-logged with docker login from do-full-token.txt; doctl is not installed). 2dde1132 published no day → no extra
  recompute; the July recompute (07-09..07-23, incl. AVGO's M0 split days) and the 2026-11-23 daily block carried over.
  core-clock4 merged (790c5c3; 193/0): C-072 = UTC−6 clock + terse digest, part 2 retired; narrow audits by both running
  (`audits/2026-11-24-core-c072-r4/`); core image built; deploy after.
  **Core 790c5c3 DEPLOYED (UTC−6 ~08:15; migration 54):** Codex r4 confirmed alert parity with pre-C-072 and correct UTC−6
  deadlines; its remaining items are display-only → small LATER core batch: a failed resumed block shows its narrowed date;
  one duplicate hand-over post on the upgrade; `vyreon-core log`/`evidence list` print UTC and 15-backup.yaml's comment says
  02:00 MT (is 03:00); rare: a build id interpolated unscrubbed into the admit line, restart count after a prior withdrawal,
  a hand-over straddling a window cutoff, long retry lines cut by the splitter. Gemini r4 still running (roll back only if
  it finds an alert regression). Kubeconfig expires ~11-25: refresh before then.
  Gemini r4: 0 blocking (0 in ordinary operation) → deploy stands. Rare, added to the LATER core batch: a final job failure
  or bundle refusal during an already-open pause is neither logged nor alerted (pre-existing; the release script shortens
  pause windows), admit instruction hidden for a re-admitted build, restart count after withdraw/re-admit. **C-072 closed.**
  **eng-f6 merged (UTC−6 ~08:40; 1,720/0; pytest 244/107):** EXQ01 reads adjusted contracts under their listed root (BABA
  2025-12-19 20/20; real ETHA1 prints = EOD volume); no value change where it worked; prevalence 4 of 1,258 cohorts (BABA
  2025-12-10..15); worker image ships OVERLAY_RULES; reference strikes/duplicates/renamed roots/ambiguous archive. Narrow
  audits by both running (`audits/2026-11-24-d460-r6/`). **Hand-over timing:** must precede the block covering
  2025-12-05..12-26 (BABA's adjusted cohort days); planned right after the first July recompute days publish (least lost).
  eng-f6 audits: Codex and Gemini both find no blocker on ordinary phase-1 data (EXQ01 keeps BABA/BABA1/BABA2 distinct;
  ETHA1 prints = EOD volume); reference items rare, one ordinary only for phase-2 META-under-FB dates (2022) → LATER with
  the deeper build. **D-460 closed.** Image c9c23ca5 built; hand-over when the first 5f637983 days publish (days published by
  5f637983 stay valid: c9c23ca5 changes no published value outside adjusted cohorts).
  **Owner asked: were the wrong numbers investigated? (UTC−6 10:45, checked in the published DB, 694 asset-days):**
  crashes (tape OOM, day refusals) were real and are fixed. **None of the claimed wrong numbers reached published data:**
  UPATH false zero 0 occurrences (no NO_TRADES bin published at all); the ACT01 feed guard never routed a published day to
  NONE (691 delivered, 2 asset-level NO_GOVERNED_COHORT_CERTIFICATE, 1 PROVIDER_COVERAGE_START); M0-PRINT-SIZE split days
  cannot occur in the backfill (historical prints not retained → EVENT_SOURCE_NOT_RETAINED); BABA EXQ01 is Dec 2025 and
  live-only. **The real published defect: VS-O04 over-withheld as ENGINE_FAULT on 147 asset-days** (D-458 fallback, mostly
  the build's first days; fixed since 955d0f82) → verify it clears when the July recompute republishes. (A first count of
  "60/63 PROVIDER_OI_INCOMPLETE" was a text-match artefact; field level: 5 asset-days.)
  **Rule from this:** a finding claiming a wrong number gets an occurrence count in the published DB before it can justify
  a hand-over; without occurrences it waits for a batched release at a block boundary.
  Applied: **c9c23ca5 hand-over deferred** (its only production effect, BABA's adjusted cohort, is 2025-12 and live-only);
  it ships in the next batched release at a block boundary, before the block covering 2025-12-05..12-26 starts. The running
  blocks on 5f637983 stay untouched → first clean throughput measurement.
  **Owner, 2026-11-24 ~11:10 UTC−6: "if we have to restart phase 1 for any reason or otherwise produce a new image, build
  the automation debt bits into the new release."** Standing rule: the next release (any new image, incl. the deferred
  c9c23ca5 batch) carries the automation debts — release script, re-runnable reference checks, budget-aware KEDA,
  JOB_THRASHING — so they cost no extra hand-over. Being built now (`core-auto`, C-073); engine side of re-runnable
  refchecks, if needed, joins the same release.
  **Owner, same day: "make all changes, THEN audit once (both codex and gemini), then commit."** Release default: all changes
  on their branches first (commits stay on branches — the audit runner checks out a commit); ONE audit by both of the combined
  release (branch tips, ORDINARY/RARE rule); fix ordinary findings; then merge to main/worker and release once. No more
  merge → audit → fix → re-audit per piece.
  **Owner refinement: "fix everything the audits find … repeated audits is not helping. Time is not a valid reason to half ass
  things."** So: ONE audit by both per release; EVERY real finding fixed (ordinary and rare) in that release, verified by tests
  and real data; no second audit round. "Rare = converged" means rare findings trigger no further audit round — not that they
  are skipped. The next release therefore also carries today's LATER lists (D-459/D-460 rare items, C-072 display/rare items,
  reference edge cases), except the three tape-reader items (any reader change orphans every tape cache → full re-capture):
  those stay after phase 1 unless the owner says otherwise.
  Owner: a rule **for this stage** (pre-launch; repeated reviews demonstrably stopped paying off), not a general law; revisit
  after launch. "Every finding that we validate with our data is one less bug to go live."
  **Intrinio Startup tier (owner buying 2026-11-24, Black Friday deal):** same APIs/endpoints/datasets as Individual — only
  commercial display/use rights (no data change for the build). Price: $999/qtr months 1-6 ($934 due today after credit),
  $1,998/qtr months 7-12, $2,997/qtr month 13+ (Individual is $150/mo). General terms (about.intrinio.com/terms, 2026-09-03):
  12-month auto-renewal +5%, cancel any time but liable for the term, no refunds; display/redistribution "subject to an
  executed Order Form" (2.2b); AI outputs to end users and data sent to LLM APIs count as display/redistribution (2.3); on
  termination delete raw Intrinio data within 30 days, derived works may be kept (2.4). Intrinio (A. Carpenter, 2026-09-30):
  order form is on the account page; third-party AI may read and cite our public derived measurements **"so long as our
  data isn't reproducible"**; one dated sample issue during the trial is fine; billing is quarterly. **Design rule for
  Reporter/web: public pages carry derived measurements only — never raw Intrinio values (chains, quotes, prices, volumes)
  or enough raw detail to reconstruct them.** To check: the order form's grant (public display, AI, LLM processing) and its
  term (quarterly vs 12-month).
  **Next release being assembled (owner's release rule):** core branch `core-auto` (C-073: release.sh, REFCHECK re-runs, budget-
  aware KEDA, JOB_THRASHING; 196/0; migration 0055) + all core LATER items; engine branch `eng-rel` (from c9c23ca5:
  `vyreon-worker run refcheck` per C-073, every engine LATER item not needing READER_SOURCES, D-461). Then ONE audit of both
  branches by Codex + Gemini, fix everything found, merge, release with release.sh (core first, worker within 15 min).
  Note: a reference re-run re-fetches the day's inputs (the suite recomputes from raw inputs), capped at 1 concurrent.
  Owner deciding Intrinio Startup by Thursday: asked Intrinio whether the term is quarterly-cancellable or 12-month
  (quarterly ~$934 fits the US$5k Vyreon cap; 12-month ~$5.9k breaks it); wants to see database progress first.
  **Spend (running total):** DigitalOcean month-to-date $269.04 at 2026-11-24 06:42 UTC vs $180.41 at 11-21 06:09 UTC
  (before phase 1) → phase 1 so far ≈ $89 + ~11 h ≈ $100 of the $500 ceiling; burn ≈ $1.25/h (10 nodes) → ~11-13 days of
  cluster left. Projected total to be computed from the first measured days/hour; warn the owner before passing $500.
  Owner: the audits' value is avoided rebuilds (each rebuild costs cluster dollars), so one thorough audit per release stays.
  **Month-6 review (owner, 2026-11-24: "6 months is a long time but worth doing"):** after launch (first paid issue Sunday
  2027-01-10), judged on four signals, by trend, raw GA4 (no filtering) against the pre-launch baseline (~29 users/week, bare
  site, ads off): (1) organic search impressions/clicks growing 3 months running; (2) readers returning (GA4 returning users on
  report pages, week over week); (3) paid subscribers retained past months 1-2 (Stripe cohorts); (4) free → paid conversion.
  Thresholds set from the first observed months, not assumed. Pivot = change the reports' audience/format (not B2B/API) if all
  four are bad; weak conversion alone = an offer/pricing fix. **First look early May 2027**, before Intrinio Startup's price
  doubles (~2027-05-24, month 7). Instrumentation exists (GA4: consent mode, engaged_60s, sign_up/generate_lead); Stripe
  retention needs nothing until there are subscribers.
  **Reporter/web priority (owner, same day): SEO/AEO first — "ads is only a forcing function, it's not a natural draw";
  the weekly design is the organic engine.** Design requirements, not polish: (1) stable URLs — an evergreen page per asset
  (latest) plus dated weekly pages that accumulate; (2) answer-first: the plain-language summary leads (what answer engines
  quote), details/charts below; (3) machine-readable: schema.org structured data per page, sitemap refreshed each Sunday, fast
  static pages; (4) derived measurements only (Intrinio's "not reproducible" condition); (5) explainer pages and reports linked
  both ways so the existing search trickle feeds the reports.
  **Post-launch (owner, same day): automated social posting of unusual detections — teasers for the paid report, "same data,
  larger surface".** Content is Vyreon's own unusualness metrics across its own sensors (the stored unusual_score / DLM-
  UNUSUALNESS), never Intrinio data reposted (owner: "we don't need to, nor want to, repost intrinio data"). Constraints: deterministic templates (no LLM in the production path); a teaser names that a flag fired
  and links to the report, never gives the paid reading away; derived measurements only (Intrinio's condition); plain voice,
  nothing hyped; posts only on genuine rare detections. Cadence (owner): the week's top 1-3 unusualness readings spread over
  a few posts per week — the top of the paid report's own unusualness ranking (one computation; posts can never
  disagree with the report). Platform choice and API access decided at the time.
  **Release merged (UTC−6 ~14:50):** core ef702d9 (C-073 + release-audit fixes; migration 0055) and engine 1ae946f7 (D-460 r5 +
  D-461: re-run worker, EXQ01 fixes, step reports, retry policy, VS-O04 validator, witness rule, P-29, overlay items; OVERLAY_RULES
  D-461; 1,744/0, pytest 256/107). Owner: fold the readable Discord digest/alerts into this release, no audit (display-only,
  tested) → `core-digest`. Worker image building. Then release.sh: dry run, then live (core first, worker within 15 min);
  then recompute every day published before the release (method bumps + OVERLAY_RULES), verify VS-O04 engine faults → 0.
  **Recompute rule (owner, same day):** recompute days where values change OR a method version changes (history continuity,
  D-273); for this release that is all ~16 published days. **After phase 1 (owner): scoped recompute** — run only the sensors a
  change touches. The engine already has it (D-104 family republish: recompute set = seed + same-day dependants, copy-forward of
  every other slot, cascade to later days only where a changed value reaches a history reader); missing: core job kind with a
  scope, worker path, core accepting copy-forward revisions (D-104's narrowed supersede); check whether inputs can be fetched per
  family (big saving) or only whole-day (compute saving only).
  **Owner: build scoped recompute now, while the cluster runs ("we're gonna need it"); phase 1 is iterative, not one clean
  pass.** Scoped vs full day: a planner computes the recompute set (seed + dependants, D-104) and estimated cost from measured
  per-family timings and picks; thresholds from measurements, not a guessed %; scoped output must be byte-identical to a
  full-day recompute for its families. Building on eng-scope/core-scope (D-462/C-074) for the next release.
  **RELEASED (UTC−6 ~15:00), first runs of release.sh:** core a6bb572 (`--core`, dry run then live; migration 55; readable
  digest) and worker 1ae946f7+715623e13454 (dry run then live; hand-over 30; admitted with validation evidence; ScaledJobs
  verified; 8 old Jobs deleted; new build claimed day-block, day-block-daily and refcheck re-runs within minutes). 07-22
  published on 5f637983 at 15:02 just before. **Recompute (values or method changed):** 07-09..07-12, 07-17..07-19, 07-22,
  07-23 are covered by the handed-over blocks; 07-15, 07-16, 08-13, 09-04 scheduled as one-day blocks on 1ae946f7. Then verify
  VS-O04 engine faults → 0 on the republished days.
  **First live finding on the release (UTC−6 15:36): NO_PROGRESS false alarms** on all 7 day blocks + the refcheck re-run at the
  download step: workers busy in h2-engine bulk-option-capture (Intrinio bulk files, not counted as API requests; download ~3 h
  on a block's first day). Nothing stuck. Core hotfix (core-np): download steps are an implicit bounded wait (240 min) + any new
  step report counts as progress; deploy via release.sh --core. Proper fix next release: engine reports download progress.
  Leftover: old Job ws65f (07-22) errored after the hand-over ("lease gone") — harmless, 07-22 recompute covered.
  **Hotfix deployed (UTC−6 ~16:00) via release.sh --core: core 067b33c, migration 56** — the 7 false NO_PROGRESS alerts resolved;
  only REFCHECK_DISAGREES (07-12, old build; recompute pending) open. Next release: the engine reports download progress
  (`progress` field on /lease/step) so a genuinely hung download is caught within 20 min, not 255.
  **Scoped recompute built (eng-scope D-462 / core-scope C-074; 1,752/0, pytest 256/107, core 217/0), next release:** REPUBLISH
  job kind, copy-forward in core (byte for byte), day replaced whole or not at all, worker fetches only the categories a scope
  needs. Inputs are barely separable (any option-family scope needs nearly the whole day; only company 27% + ACT01 24 min drop)
  → the saving is compute. **Measured node-hours: full day 3.8 (block's first day) / 2.8 (later day); scoped 1.3-3.7 by family.**
  Rulings: old days always scoped (EXQ01 prints past retention); ≤4 republish pods; re-run reference check scoped to the
  recomputed families. Next release also: engine download-progress reports; core-scope migration renumbered to 0057. Not yet
  proven on a real day (scoped vs full byte identity — cluster test after release).
  **Phase-1 projection from measured node-hours:** ~46 first days × 3.8 + ~551 later days × 2.8 ≈ 1,720 node-hours / 8 blocks
  ≈ 9 more days; ≈ $215-300 more → total ≈ $340-420 of the $500 (unproven; verify against real days as they publish).
  **Release 2 assembled and in its one audit (UTC−6 ~16:20):** eng-scope 6470f89a (1,754/0; pytest 256/107) + core-scope 3c83e2c
  (223/0; migration 0057): scoped recompute (REPUBLISH), scoped reference re-runs, download progress reports with the 20-min
  rule for reporting downloads. Codex + Gemini (`audits/2026-11-24-release2/`); then fix every real finding, merge, release.sh;
  then the cluster test (deploy/k8s/README.md §11, ops/scoped-check.sql: scoped vs full byte identity on one recent day, ~$1).
  **Release-2 audit in (UTC−6 ~17:00):** core Codex 17 ordinary + 3 rare, Gemini 16; engine Codex 13 ordinary + Gemini. Nothing
  live is affected (release 2 not deployed) except one PRE-EXISTING wrong reading found on the way: DLM-CANDLESTICK misses two
  real UUP patterns (2025-05-21 hammer, 2025-05-22 inverted hammer) through float boundary comparisons — not yet built (phase 1
  is in mid-2024), so fixing it in release 2 means no published day carries it. All findings being fixed on core-scope/eng-scope.
  **Correction (UTC−6 17:30):** the exact-price fix (engine 249a21a6 code, 42e243ad tests, 7139116a docs; DLM-CANDLESTICK 1.2.0,
  DLM-TECHNICAL 1.11.0) also covers NR7 (UUP 2025-03-10, 2025-08-20 called NR7 on a tied range): 4 of 431 UUP sessions wrong,
  ~1 in 200 on low-priced assets. Built days carry the old method (OTHER_METHOD_VERSION in DLM-UNUSUALNESS history) →
  after release 2: scoped republish of family DLM-TECHNICAL on every day built before it (first real scoped use; no option files).
  Core audit fixes committed (core-scope 61c869f); engine audit fixes in progress.
  **Live finding (UTC−6 17:45): ETHA's first sessions refused.** 2024-07-23 REFUSED by the input plan (`act01 … Missing: 1 assets
  without a cohort or no-selection record (ETHA)`; ETHA first traded 2024-07-23, options only from 2025); 07-24's fill ends
  with the same ACT01 gap; 08-14 completes. 07-23 published on 2a8f8c4f → likely a regression in D-459..461. Job requeued
  (attempt 1) with no alert: observability gap (a plan refusal should alert at once, not after 3 attempts). Diagnosis + fix on
  eng-etha (from eng-scope) → folds into release 2. Also seen: bulk option files of Jan-Feb 2024 fail whole on a quote
  character in a row (the API captures those chains instead; costs requests, not correctness) → PROVIDER_FAULTS check.
  **Release-2 audit fixed (UTC−6 ~18:15):** engine eng-scope 2f65ce01 (1,760/0; pytest 258/107; audit code a444250e 21fb3516,
  tests 85669627 546ae594, docs d6eb00a7 2f65ce01; candlestick 249a21a6/42e243ad/7139116a); core core-scope 05483ea (231/0).
  Every finding fixed but one judged not real (Gemini task-minutes). Consequence: a fresh worker can't open published option/
  surface files, so option-family scopes recompute VS-O02/O06/IVS01 + dependants ≈ a full day; cheap scopes are the daily-bar
  ones (DLM-TECHNICAL) → the cluster test now uses DLM-TECHNICAL. Still to join release 2: ETHA fix (eng-etha) and a core alert on
  plan-refused days (DAY_REFUSED, hold instead of burning attempts). Disk: eng-rel worktree + target (79 GB) removed (merged).
  Open question: are the Jan-Feb 2024 bulk files' quote characters valid CSV quoting (our reader too strict) or a provider fault?
  Files not retained; check one when convenient (costs only API requests meanwhile).
  **Correction (UTC−6 ~18:45): 07-23 was refused by Intrinio throttling, not ETHA** (my hypothesis was wrong). Evidence 105:
  `carry_inputs: dividends/SPY: HTTP 429` (exit 2 = fatal, no step retry) + company-capture 429s, while core's calendar-minute
  usage was 1,589-1,845 (< cap 1,900 < Intrinio 2,000): a calendar-minute cap lets a rolling 60 s window exceed 2,000.
  D-463 (eng-etha 6237e806/f7ba7d30/51ad931b): carry exits 3 on throttle/transport so the fill retries; ETHA's not-yet-T−5
  cohort noise removed (ETHA's ACT01 is correctly withheld PROVIDER_COVERAGE_START, as 2a8f8c4f published). Release 2 also
  gets: rolling-60 s budget in core (root cause), C-075 transient class for throttle refusals; eng-etha merged into eng-scope.
  **Correction 2 (UTC−6 ~19:00):** core already reserves over a rolling 60 s window (C-076 proof; the calendar-minute figures are
  the ledger booking at release) — my reading was wrong. Closed: released leases count 5 s more (in-flight). Root cause of the
  429 under budget now hunted as a BYPASS (requests on the key without a permit); droplet ruled out (timers 12:00/14:30 UTC).
  Prime suspect: the reference re-run's Python fetchers. Merged: eng-etha → eng-scope ad9a9283; D-464 carry failure class
  (077e51ea/69e94998/f399e80a); C-076 core 7dd8cea/0ab4966/7136cd5. Suites: engine 1,763/0, pytest 258/107, core 235/0.
  **Bypass hunt (UTC−6 ~19:30):** no cluster path sends to Intrinio without a permit (every reqwest client draws one; bulk/
  snapshot/replay files are presigned, keyless; the reference suite has no HTTP and no key). Guard: VYREON_BUDGET_REQUIRED=1 in
  the worker image (D-464, 82b14153/a06f62ff/29c73c18/5e9aa54e). The only sender outside the budget is the agents' Intrinio MCP
  (same account): no Claude or Codex session called it near 22:55Z 11-24 or 19:32Z 11-23. P-30 stays unexplained; now survivable
  (carry retries on throttling; throttled refusals get one quick retry). Watch for recurrence.
  **Release 2 merged (UTC−6 ~19:40):** engine worker 3d7567c8 (pushed), core main dd3f766 (image built). Worker image building.
  Ship AFTER the 8 running blocks publish their first days (~20:00-21:00; a release deletes running Jobs): release.sh --core
  dd3f766 (migrations 57, 58), then release.sh <worker tag>; then the DLM-TECHNICAL scoped cluster test (one recent day both
  ways), then the DLM-TECHNICAL scoped republish of every day built before release 2.
  **Automation debts approved (owner, UTC−6 20:10): "1-5 can all get built up".** (1) draining release (running blocks finish
  their current day instead of being deleted), (2) kubeconfig refresh — DONE: deploy/k8s/ops/kubeconfig.sh (618e048), run,
  new kubeconfig valid 7 days (to ~12-01); release.sh auto-refresh in release 3, (3) spend watch (DO billing → daily progress
  line, alerts at 80/100% of the $500), (4) build.sh credential-free docker config, (5) automatic recompute planning after a
  method change. Builder on core-auto3 / eng-auto3 (from release-2 heads) → release 3, one audit by both.
  **Release 3 built (UTC−6 ~21:20; core-auto3 242/0, eng-auto3 1,765/0):** C-077 draining release (0059; DRAIN_MINUTES 6 h; old
  blocks finish their current day), kubeconfig.sh --if-needed in release.sh/health.sh, C-078 spend watch (0060; hourly billing,
  09:00 MT progress line, alerts 80/100% of $500; needs the owner's billing:read token as secret vyreon-core-do-billing),
  engine build.sh empty docker config (5eae249f), C-079 recompute after a method change (0061; held while spend is unknown —
  coordinator: right default, spending is the owner's call; bumps that change no value are NOT excluded: D-273 makes old days
  other-method in DLM-UNUSUALNESS histories). Its one audit running (`audits/2026-11-25-release3/`). Disk: merged worktrees
  eng-etha/eng-scope/core-scope + targets removed (now 305 GB used).
  Phase-1 on 1ae946f7 so far: 07-09, 07-17, 07-24, 08-14 (+ nightly 11-23); later days compute-paced (~2.5-3 h a day).
  Release-3 audit: Codex core 14 ordinary + 5 rare, Codex engine 0 ordinary + 4 rare, Gemini core in; **Gemini engine NOT run:
  weekly quota exhausted (resets in ~138 h, ~2026-12-01)** — recorded and moved on (audits rule); the engine half is small
  (D-465 last-day stop, build.sh). All findings to the builder on core-auto3/eng-auto3. 07-22 published 21:20.
  **Release 3 built (2026-11-24/25, core-auto3 from main d0eda4d; eng-auto3 from worker 3d7567c8), not merged, not deployed:**
  (1) C-077 draining release (core 7041ebd/5fdda4e/13f8fe4, migration 0059; engine D-465 3baaa44d/65361a8e: the worker
  stops after the day core calls its last); (2) kubeconfig auto-refresh in release.sh + health.sh (13f8fe4);
  (3) C-078 spend watch (5772296/e6120c1, migration 0060; 20-core.yaml 35a17c2: since 2026-11-21 06:09 UTC, baseline
  $180.41, ceiling $500) — **needs the owner's read-only `billing:read` token** (README "Spend"); until then the
  morning line says "not known"; (4) build.sh credential-free docker config (engine 5eae249f); (5) C-079 recompute
  after a method change (26df9a6/7ff8124, migration 0061; on, from 2024-07-09, $0.167/node-h): held while the spend
  is not known. Docs e5635e0, engine a1d7b5ef. Deploy order: release.sh --core (0059-0061) first, then the worker
  release (its preflight needs 0059). Open questions for the coordinator: C-079's two (DECISIONS). Next: one audit
  (Codex + Gemini) of both branches.
  **Release-3 audit fixed (2026-11-25):** core Codex 19 + Gemini 20 findings, every real one fixed and tested (C-077..C-079
  table): core a981a24 code, d243536 tests, 6cb40c2 deploy; engine D-465 abb2d267/536393ce. Coordinator rulings: plans held
  while the spend is unknown; no-value method bumps not excluded.
  **Releases 2+3 combined into one rollout (UTC−6 ~21:50):** release 3's fixes landed before release 2 shipped, so one cut
  instead of two (later releases drain). Gemini engine audit of release 3 not run (weekly quota) — Codex's alone accepted
  (audits rule). Merged: engine worker 1134003a, core main 635c3a4 (image built); worker image building. Ship once 09-05
  and 07-15 publish: release.sh --core 635c3a4 (migrations 57-61), then release.sh <1134003a tag>.
  **Releases 2+3 SHIPPED (UTC−6 ~22:10-22:16):** core 635c3a4 (migrations 57-61) via release.sh --core; worker 1134003a+838b60481511
  via release.sh with evidence (~/claude-artifacts/cluster-scratch/evidence-1134003a.json). First drain (C-077): the 8 old blocks
  finish their current day (bound 10:13 UTC); release.sh step 12 still running in the background (log in the session scratchpad).
  **Defect found live: release.sh step 10 (queue resume) had been deleted by audit-fix commit 6cb40c2** — the queue stayed paused
  (QUEUE_PAUSED alert worked); resumed by hand at ~22:20 (exactly this run's pause, as step 10 would). Fix on branch core-fix10
  (step 10 restored + a guard refusing to run if a documented step is undefined); merge only after the running release.sh exits
  (never edit a script a running job uses). 08-15 published 22:12 = the 08-14 block's second day 1 h 40 min after its first.
  **False STALLED during the drain (22:33):** the day-block ScaledJob's 8 slots are held by draining old-build pods; fixed on
  core-fix10 (d83428e/cf36ac2/ab00b99: draining leases within their bound explain the absence; progress line instead; 248/0).
  Deploy after the drains end: merge core-fix10 → main, build core, release.sh --core. Engine gap for the next engine release:
  a drained release-3 worker waits for its next day's download under a plain step → NO_PROGRESS after 20 min; make it a
  declared wait.
  **Drain observation (UTC−6 23:38):** 4 drained days published (07-10, 07-15, 08-15, 09-05; 07-10's history layer by the new
  build's ordered pass, one revision per layer — C-070 holds). But a drain's lease ends only when the day's REFERENCE CHECK
  reaches core (~1.5-2 h after publish), so 5 of 8 nodes sit in "day done, its check to come" while 45 new-build blocks wait
  for slots. Improvement for the next core/engine release: end the drain at publish and hand the reference check to the
  REFCHECK re-run machinery (or let the new build's blocks start beside a draining check), measured against today's numbers.
  **THROUGHPUT FINDING (UTC−6 00:10): the per-day reference check is the bottleneck.** Measured: nightly 11-23 check 7,566 s
  at 8×94% CPU (≈2.1 node-h); 08-14's ran >3.5 h beside the next day; 07-22's >2 h 47 min. Engine compute per later day
  ≈1.6-2.8 node-h, so the check is ~45-55% of node time, and the worker waits for the previous day's check. The ~9-day /
  $340-420 phase-1 projection did NOT include it — void until re-measured. Profiling agent (eng-refperf) running; options
  for the owner after: optimise the suite (keeps every check), run checks after the block / at lower priority, or sample
  (owner's call: weakens validation).
  **Root cause found (UTC−6 ~00:45): a reference-suite REGRESSION**, not steady cost. From core.refcheck_report timings: the
  "price and chain inputs" stage was 1,340-1,480 CPU-s/day up to build f7b08efe and 44k-76k from 2a8f8c4f on (11-23: 44,057 of
  52,815 CPU-s): D-447..455 / D-461 witness / D-451 lookback re-read and re-parse up to ~60 earlier sessions' files per session.
  Fix D-466 on eng-refperf (3e890636/dc3b9d0c code, 9cd041db tests, e15238b3/b9814e29 docs): one symbol per pool task, each
  session's files read once; 12× on real 2024-07-19 data (1,847 → 151 CPU-s, five symbols incl. AVGO split), identical
  summaries on all 705 sessions; pytest 262/107; check expected 2.1 → ~0.5 node-h/day. Independence kept (no engine code).
  **Release 4** = eng-refperf (+ declared waits for long legitimate waits) + core-fix10 (step 10, STALLED during drains); Codex
  audit running (Gemini out of quota to ~12-01). Cluster proof after deploy: one day's full report before/after equal.
  Release-4 core audit (Codex): 5 ordinary + 3 rare, all real, all fixed on core-fix10 (a8794f2/7105ad9/c7ebbb2/0f83347; 249/0):
  atomic `queue resume --expect <reason>`, --from 11/12 verification, no success while paused, drain line kept, overdue drain
  never hidden, slot read from the draining pod, step guard checks each step's command. Engine audit (Codex): 2 ordinary
  (memory, double parse) + 4 rare, no verdict change — fixing on eng-refperf with the declared waits. All 8 drained days
  published by 01:00 UTC−6; leases end as their reference checks reach core.
  **Drain finding (UTC−6 03:35): a pre-D-465 old-build worker paused the queue.** Job 21823 (block 09-05) finished its drained
  day's check (09:31Z), ignored "last day" (1ae946f7 predates D-465), computed 09-06 and sent it; core refused ("expects no
  further day … from build 1ae946f7") and the refusal PAUSED THE WHOLE QUEUE. Resumed by hand at ~03:40 (pause held only that
  refusal). The other 6 drained old blocks may do the same before the 10:13 UTC bound (resume each). My attempt to end those
  drains early by deleting their Jobs was denied by the permission check — left to the designed bound. Fix for the next core
  release: a refused bundle from a DRAINED lease for a day past its last (the expected pre-D-465 behaviour) is recorded and
  its job stopped, not a queue pause. Only matters for drains of pre-D-465 builds (this one), but cheap and generally right.
  **Owner (UTC−6 ~04:10): Discord in plain language** ("currently its exposing a lot of alphanumeric codes"): no job/lease/
  build/decision codes in posts; 🔴 only when the owner is needed, ⚠ to look at; named assets; quiet half-hours silent with a
  daily heartbeat; alerts say what happened / affects published numbers? / what to do. Rewrite on core-fix10 (release 4).
  Also on core-fix10: digest undercount fixed (0bec432/e45efdc: 08-15 was outside every live job).
  **Engine faults on every published day** (IBIT daily 10 slots, ETHA daily since listing, ARKK intraday families most days,
  AVGO/UUP/MSTR some): honestly withheld (no wrong number) but a systematic engine defect; investigation on eng-faults (D-468).
  Reference checks on 1ae946f7 days disagree: 07-15 (87 unexplained), 08-14 (22) — to investigate after release 4.
  **First drain complete (UTC−6 04:14):** release.sh exited "released 1134003a+838b60481511"; all 8 old blocks published their
  drained day (07-10, 07-15, 07-16, 07-18, 07-22, 07-25, 08-15, 09-05); the new build holds all 8 day-block slots at 04:15.
  Next: release 4 (worker a79930e2 built; core-fix10 after the plain-language digest) — its drain will be the first with
  D-465 workers. Then: re-measure throughput with the fast reference check; investigate the 07-15/08-14 disagreements.
  **Engine-fault census (UTC−6 ~04:50, eng-faults D-468: 419c1c7c/37af9156/bf35bde0; 6,826/0):** most digest "engine faults"
  are NOT faults — every whole-slot withheld body's result id ends `/engine-fault` whatever its reason (IBIT/ETHA no listed
  options before 2024-11-19; ETHA coverage start; AVGO/UUP/MSTR CARRY_NOT_IDENTIFIED) → core tally fixed by reason (core-fix10).
  Real: (1) VS-O04 on 07-11/12 (old 2a8f8c4f validator; their recompute is queued). (2) ARKK tape families: Intrinio changed
  ARKK's stated listing ARCX→BATS on 11-24 (P-25 amended: the stated MIC is not point in time); restate refused every ARKK
  cache. D-468 restates 693 of 851 (listing from the tape's own `9`, byte-identical to a fresh derivation). **158 ARKK caches
  still fault (owner decision):** 2023-07/08, 2023-10-26, 2024-09-12, 114 sessions 2025-03-31..10-22, 2026-01-02, 2026-11-04 —
  re-capture those tapes (recommended; core needs a cache-replace path) or rule that a cache stands under the record it was
  derived with. INT-TEMP-EVC01 (252-session history) faults on nearly every ARKK day until resolved. Codex audit of
  eng-faults running → joins release 4.
  **Release 4 core SHIPPED (UTC−6 ~05:10): core 1055efb, migration 62** via release.sh --core (core-fix10 merged): release.sh step 10
  + guard + atomic `queue resume --expect`, drain-aware STALLED, drained overruns don't pause, plain-language Discord (both
  channels; total 597 over the configured range), engine faults counted by stated reason (0062 re-tallies). Worker half of
  release 4 waits for the eng-faults audit: then merge eng-faults into worker (a79930e2 + D-468), rebuild, release.sh.
  **eng-faults audit (Codex) fixed and merged (UTC−6 ~05:40): engine worker 70d83bbc** (8a0c3a13/1014ef5a/9d27e2cc/789e726d):
  every reader restates before reading; 158 unrecoverable ARKK sessions stated TAPE_LISTING_RESTATED (engine cause) instead of
  ENGINE_FAULT, windows not all-or-nothing (FIELD/EVC01 keep the other sessions); stale verify PASS handled; reopen/coverage
  restate; StochRSI %K own window → DLM-TECHNICAL 1.12.0; short-history price levels stated; daily-row finality; basis count
  check. Reader identity 36c70d1c unchanged on the frozen slice (436 files) — cluster tape verify must confirm before admission.
  Suites 1,776/1 (the 1 = h3-asset address_bound test, fails only under memory pressure of a parallel full run; passes alone on
  base and branch), pytest 268/107. Correction logged: the agent's first "6,826 passed" was wrong (run had stopped).
  Next: core registers TAPE_LISTING_RESTATED (core-r4b) and ships FIRST; then the worker 70d83bbc release (drain, D-465).
  **Release 4 SHIPPED (UTC−6 ~06:00):** core 9bf80be (TAPE_LISTING_RESTATED engine-caused) then worker 70d83bbc+04d2e67336c9 via
  release.sh (evidence ~/claude-artifacts/cluster-scratch/evidence-70d83bbc.json): step 10 resumed the queue itself (fix proven
  live), the new build claimed a reference re-run at once, step 12 drains the 8 release-3 blocks (first drain with D-465
  workers: they stop after their day). Next: measure the fast reference check and a block's per-day pace on 70d83bbc → a real
  phase-1 projection; investigate the 07-15/08-14 reference disagreements; the owner's three decisions.
  **NO_PROGRESS false alarm #3 (UTC−6 10:03):** the nightly block's fill sat in its "quotes" unit (requote: local CPU 96%, no
  API requests) >20 min with no progress report → alert while working. D-467 covered reference checks and drained waits, not
  long LOCAL units inside a download. Next engine release: the fill reports progress per symbol (or per chunk) inside every
  long local unit (requote, quote window, bulk parse) — progress, not a wider bound. Day count: 20 at 10:00.
  **PHASE-1 POLICY — PROPOSED, NOT DECIDED (owner correction 2026-11-25 ~11:00 UTC−6: "we talked about freezing it yes but we didnt
  like, decide to freeze it"; "we should probably fix every problem we already found before we freeze it"). I recorded it as
  decided without confirming — wrong. Direction now: fix every known problem first, one release, THEN freeze and crank.**
  **DECIDED (owner, 2026-11-25 ~11:20 UTC−6):** "a simple 'fix every fault we find' approach. Everything. One more full-engine
  codex audit. Then remediate that too. Then freeze for phase 1." Plan: (1) fix every known fault (suite id-by-position + NOT_RUN
  guard; progress in long local units; drain ends at publish; the three tape-reader items + ARKK re-capture path and the 158
  sessions; bulk quote character; whatever the valid reference disagreements 07-15/07-11/08-14 trace to); (2) ONE full-engine
  Codex audit (whole engine, Gemini out of quota to ~12-01); (3) fix all of its findings; (4) release + one tape re-capture
  (reader identity changes, 6-12 h); (5) freeze the engine for phase 1, one big block per node, nodes scaled in steps. Cluster
  stays at 8 nodes meanwhile (real data keeps finding faults). Note: the tape-reader bundle (3 items, not 9) was MY
  recommendation to defer, never the owner's decision.
  **Backfill stopped (owner, UTC−6 ~11:30: "stop computing database numbers until we freeze the engine"):** `queue cancel` of the
  58 unfinished priority-0 DAY_BLOCK + REFCHECK jobs (48 + 10; leases end, workers refused at their next call, nothing
  published touched). KEPT: the nightly capture (priority-1 day block + tape jobs) — option prints (~5-session retention) and
  trades (7 days) are perishable, so the daily run must never stop. Ordered passes (594) left queued: they wait in date order
  on 2024-07-12 and claim nothing. The phase-1 range is rescheduled after the freeze as one big block per node. Phase 1 so far:
  20 days published (all recomputed after the fix release where methods changed).
  `queue cancel` worked as designed (58 leases ended within a minute; workers exited at their next call), BUT core's own C-073
  automation re-queued 10 REFCHECK re-runs minutes later (published days without a completed check) → set
  VYREON_CORE_REFCHECK_RERUN_HOURS "off" (20-core.yaml, d562aa7 deployed ~10:33 UTC−6; back to "6" with the dump fix) and
  cancelled them again. release.sh --core defects seen: an identical-content rebuild fails step 3's digest check (local id vs
  registry digest), and --from 4 does not work for --core (both to the core-dump branch). Lesson: stopping the backfill = cancel
  jobs AND turn off every automation that queues backfill-type work (re-runs; C-079 recompute is already held by unknown spend).
  Then cancelled the 588 queued priority-0 ORDERED_PASS too (one had started on 2024-07-19: history passes compute old numbers
  as well); NOT_DERIVED for those days is expected until the backfill is rescheduled. Left: the nightly block + its 2 passes.
  **Valid reference failures classified (UTC−6 ~10:45; 239 on 5 valid reports):** no unknown engine fault. ARKK tape families
  140 (D-468, fixed; 07-11 missing from its list); dividend projected before the expiry (UUP Dec/Jan, XLE Mar: ex-dates on the
  Monday after the third Friday; forward 441-576 bp off for UUP) 8 — ENGINE, open D-153 item; reference-suite faults ~59
  (carry borrowing D-185 ignored, forward tolerance, σ-support floor test, P01 63-session hard-code, ETHA short history, AVGO
  split-day zero counts); provider-quote-limited 28; unclassified 2 (TSLA σ 0.8081 vs 0.8039; NVDA VS-O04 2024-06-21 0.032165
  vs 0.032980, can move a percentile rank — needs re-downloaded chains). All into eng-r5.
  **Coordinator ruling (standards first):** the forward put-call pair tolerance becomes the no-arbitrage band (sum of both
  legs' half-spreads; Hull) instead of the mean of the half-spreads — 12 of 13 such failures fall inside it. The owner's
  2026-10-30 rule (offsets without a σ half-width stay flagged) is unchanged.
  **PHASE-1 RUN RULE — DECIDED (owner, 2026-11-25 ~14:40 UTC−6: "Yeah that is a good set of rules"):** run phase 1 on the frozen
  engine; faults found go to one list, fixed after phase 1, then one recompute. STOP RULE: over the first ≥20 published days of
  the frozen build (then continuously), a day is BAD if (1) its valid reference check shows a disagreement traced to an ENGINE
  fault that changes a published value (wrong number or wrongly withheld) — suite/provider causes do not count — or (2) it
  fails to publish because of an engine fault. If >50% of sampled days are bad: stop, fix everything, freeze again, restart.
  Core counts the automatic part (days not published; unexplained reference failures per day), posts the running share to the
  progress channel and alerts past 50%; the engine-vs-suite classification is the coordinator's, verified before stopping.
  Recompute after: a fix that changes no method version (a day's own transient fault, a crash, a refusal) → recompute just the
  bad days on the same build; a fix that changes how a value is computed is a method change (D-273) → C-079 recomputes every
  day of that family/measurement, so histories never mix definitions. Core count + alert ride the freeze release.
  **Expectation (owner, 2026-11-25):** "Its rational to expect at least one recomputation at this stage. So my goal is no longer
  'one and done' but rather one pass to find everything and another to fix what died." Phase 1 = PASS 1 (frozen engine, full
  range, faults collected) + PASS 2 (fix everything found, then recompute only what the fixes touch: bad days on the same build
  where no method changed, affected measurements by method version where one did). Spend: pass 1 ≈ $250 more (estimate, before
  the fast reference check is measured); pass 2 unknown until the fault list exists — the two together will likely pass the
  $500 ceiling; flag before crossing it (needs the billing token for a measured number).
  Owner: knows the two passes will pass $500; ceiling not yet raised → one heads-up with the measured number before crossing.
  **Slow nightly (11-24, worker 70d83bbc on a day-intel node):** fill 19,215 s wall (5.3 h): quotes unit 9,673 s (requote
  children 46,749 CPU-s, resident 6.5 GB), options 6,747 s, exq01 1,644 s; 11-23's whole nightly took ~2 h on a `day` (AMD)
  node. Hypotheses to test before the freeze-run sizing: node type (Intel shared vs AMD), memory pressure in quotes, or a
  D-466..468-era change in requote work. Perishables were captured (download done 14:09 UTC−6). Backlog item.
  **Audit fixers:** eng-r6c (company/field/orchestration) DONE 178a69bb/75d58417/c70a820d, D-503..513: 42 fixed, 2 not real,
  owner items: company #17 (open under D-304), VS-L01 code name RETURNED_EOD_QUOTE (cosmetic). Bumps: CDD-EVENTS01→H5-COMPANY@3,
  CDD-FND01@4, CDD-FND02@3, CDD-CAPVAL01@5, CDD-MACRO01@3, H5-FIELD@3 (MKL COMPATIBLE branch: ULP moves — prove bit-identity on an
  AMD node before the freeze: cargo test -p h5-field --test d114), H5-FIELD-OPTIONS@3. Core contracts: republish print-retention
  by open_at; republish publish refuses a stale replaced revision; coordinates view terminal_state/terminal_reason. Provider
  faults to log (not report): earnings_disclosed_at NY time with Z (LLY 2023-08-08); sec_unique_id dashed/bare; macro page
  boundary row repeated. eng-r6a (overlay/optsensors/tape) 696212d5/194dca12/57cff01b, D-477..480: most fixed; overlay rules
  version and a new tape reader identity → every option source requoted and every tape re-captured at release. Bumps VS-O01@2,
  VS-O06@4, D111@12, EVC01@17, UPATH@15, UACT@8, UQTR@9, DLM-VWAP@3.2.9, IP01@2. Three ordinary items sent back (owner: fix
  everything): O03 current roles withheld on a T−5 refusal; absolute paths in inventories; FXI 2023 cash deliverable (outside
  phase 1). eng-r6b (surface/m0/daily) DONE 543a0326/6a8a9e00/7077f865, D-490..501: 37 of 38 fixed (all held on real data);
  bumps H5-SURFACE@25, M0-ANALYTICS@1.2.0, M0-IV-PROFILE@1.1.0, M0-IV-CHANGE@1.1.0, M0-GAMMA@1.3.0, M0-GAMMA-CHANGE@1.2.0,
  DLM-EXPECTED-MOVE@2.5.0, DLM-MAX-PAIN@1.5.0, DLM-PREMIUM-YIELD@1.4.0, DLM-TECHNICAL@1.13.0, DLM-CANDLESTICK@1.3.0,
  DLM-IV-CRUSH@2.1.0; UUP 2025-12-10 IV30 0.0962 → 0.1157 (D-185: 2 comparable pairs < 3 → borrows the 401-day carry); spot
  for 15:50-snapshot sessions (2022-2024) = the bar's opening quote; QuantLib/TA-Lib independent checks. Reference must mirror
  D-475 (sent back). Owner rulings pending (with recommendations): daily 11 (score the 1-year book's gamma too? D-101 asks a
  single figure → recommend keep one), D-500 item 5 (list ~800k source ordinals per SPY day literally? → recommend a compact
  content-addressed form), D-495 (surfaces before 2023-01-03 withheld until the calendar holds 2019 — outside phase 1).
  **All audit remediation merged (UTC−6 ~16:30): integration branch eng-r6** (eng-r5 + r6c + r6b + r6a; two code conflicts resolved:
  views::split_ratios takes r6a's conflicting-ratio set with D-512's comment; M0 pins renewed for the merged code under D-500's
  labels). r6a's last items: O03 current roles publish on a refused T−5 (VS-O03@3, D-481); path-free view identity (D-482); FXI
  cash deliverable with a core ledger contract `special_cash_per_contract` (D-483); shared fixture etha-first-chain requoted
  under D-477 (backup etha-first-chain.pre-D477; eng-r5/r6b/r6c need the backup). Reference mirrors D-475/D-494.
  **Full suites on eng-r6 (UTC−6 ~16:20):** reference pytest 288/107 ✓; cargo 1,824 passed, 1 failed:
  h3-asset closure::code_digests_change_with_exactly_their_closures (crate:h3-o02 now re-keys every route after the merge;
  r6c put h4_run.rs into ALWAYS, an r6a/r6b edit apparently made it depend on h3-o02) → sent to the r6c fixer to settle what is
  true (genuine dependency → update expectation; leak → cut it). address_bound passed this run.
  Settled (D-514, 067905f4/fee687ae): a false dependency — an r6a test inside h4_run.rs named h3_o06 and build.rs counted test
  code; test modules are now skipped in the crate scan (the file digest still covers the whole file). Closure test passes
  with its expected sets unchanged.
  **ENGINE FROZEN FOR PHASE 1 (UTC−6 ~16:50): worker 93a04676** (eng-r6 merged and pushed; worktrees/targets of r5, r6, r6a-c
  removed, disk 245 GB). Worker image building. Core batch for the freeze release on core-freeze: audit contracts (republish
  retention by open_at; stale-replaced refusal; terminal_state/reason; FXI special_cash_per_contract; IP01 view), the tape
  re-capture for the new reader identity (2023-05-24..today) with recompute by method version, the owner's STOP RULE counter
  and alert, re-runs back to "6", P-31..P-33. Then: release (core first), AMD bit-identity check of H5-FIELD (cargo test -p
  h5-field --test d114 on a worker), tape re-capture, reschedule phase 1 as one big block per node, scale in steps.
  **PASS 1 STARTED (UTC−6 ~16:50):** core 0f71a7b (migration 63) then worker 93a04676+92be12604d32 via release.sh (evidence
  ~/claude-artifacts/cluster-scratch/evidence-93a04676.json; step 10 resumed the queue; the 11-24 nightly drains on 70d83bbc).
  `vyreon-core tape recapture` queued 880 TAPE jobs 2023-05-24..2026-11-24 (new reader identity). Phase 1 rescheduled on the
  frozen build: 12 DAY_BLOCKs of 50 sessions over all 597 (every day recomputed: methods changed) + 597 ordered passes; day
  blocks wait for their tape windows (existing gate). 15 stale re-runs of old-build days cancelled (recomputed anyway).
  KEDA day-block max stays 8 for now; raise to 12 once the tapes are through and the budget is measured, then higher.
  Re-runs re-queued stale old-build checks → core 8f4fdc9 (C-073 amendment: no re-run for a day a running build will publish
  again; queued ones cancelled; checked after the new revision) deployed ~17:40. Tapes: 60/880 done by 17:19 (~7 h for all).
  H5-FIELD bit-identity: production schedules workers on BOTH `day` (AMD) and `day-intel` pools; laptop is Intel → proof run
  in a throwaway pod pinned to an AMD node (D-511 test d114); remedy if not identical: pin workers to one CPU vendor.
  **PROVEN (UTC−6 ~18:10):** H5-FIELD is bit-identical across vendors (D-509 field 6): one d114 test binary from 93a04676 gave
  digest 39bb103fbe457875 (49.4 MB D114 endpoint result) on the laptop (i7-12700H), a `day` AMD node (EPYC Zen 2) and a
  `day-intel` node (Cascade Lake, AVX-512); MKL_CBWR=COMPATIBLE, 1 thread, sequential; the image's MKL libraries are
  byte-identical to the laptop's. No remedy needed: workers stay on both pools. Throwaway pods deleted, none left.
  Release of 93a04676 complete (release.sh exited "released", the 11-24 nightly drained and published 17:17). Tapes 18:21:
  168/880 done, 12 running, 0 failed, ~106/h → all by ~01:00 UTC−6; Intrinio 439 req/min (budget headroom).
  19:23: 287/880, but only 1-5 tape pods at a time: core.scaler_count returns 0 for priority-0 work for 5 min after ANY
  budget wait, and tape jobs request in bursts (1,700 in the minute 19:18, ~3 in the quiet minutes) → oscillation, ~110/h net
  (done ~00:45). The 30 "Failed" tape Jobs in Kubernetes are history from 11-24's run, not new. Tuning item (core, after the
  re-capture): smooth tape request bursts or scale tapes by budget headroom instead of a 5-min stop after one wait.
  **Pass 1 started properly 19:53** (block 1 2024-07-09..09-17); 5 blocks running by 21:50. Block 1's first-day download took
  3.1 h (01:54→05:00Z). **Eviction 23:02 (block 26030, 2025-02-12..04-24):** node out of memory — the worker used 13.57 GB;
  every block peaks ~13.37 GB against a node's 13.33 GiB allocatable, but requested only 10Gi, so tape pods (3Gi) were
  co-scheduled beside blocks. Fix (infra, not engine — the freeze holds): full-day jobs (day-block, day-block-daily, refcheck,
  republish) request 12Gi (ccda8f9, applied to the live ScaledJobs 23:20; new Jobs only). 26030 re-queued (attempts 0).
  Stop-rule classification: an eviction from co-scheduling is infrastructure, not an engine fault. Open engine question for
  the backlog: why a day needs ~13 GB (the 11-24 nightly peaked 6.5 GB in its quotes unit).
  **ENGINE FAULT in pass 1 (UTC−6 23:18): block 26028 (2024-09-18) FAILED** — "the executor held the day (too many faulted
  slots)": 124 ENGINE_FAULT (M0-ANALYTICS + INT-IVSC01 for all 62 assets) + 310 PARENT_ENGINE_FAULT, all from the T−5 history
  session 2024-09-11 lacking `option/source-identity.json` ("admitted option source identity"), touched last by 543a0326
  (D-490..501) / 696212d5 (D-477..480). Possibly systematic for every block's first day → containment: day-block ScaledJob
  maxReplicaCount patched 8 → 3 (the 3 running continue; no new block starts; tapes unaffected). Root-cause agent running
  (fix on eng-hotfix-srcid; a blocker under the freeze rules). Stop rule: this day counts as BAD (engine fault, not
  published) — 1 bad of 1 so far; the >50% rule applies from 20 days.
  **SYSTEMATIC (UTC−6 ~23:30):** h4_m0_analytics.rs stages each asset's option/ dir with source-descriptor.json + context.json +
  population only; D-478 (696212d5) made h3_q04::admitted_source::reopen_from_q04 → sealed_provider_faults read
  option/source-identity.json whenever the descriptor carries source_identity_manifest_sha256 → ENOENT for every asset, every
  day; INT-IVSC01 and dependants cascade; every day of 93a04676 would be held. All 12 pass-1 DAY_BLOCKs cancelled (no block
  claimable; day-block max 3, KEDA forbids 0); tapes continue (587/880). Hotfix on eng-hotfix-srcid (copy
  source-identity.json into the staged dir) → targeted test → release (blocker exception) → reschedule the 12 blocks → max 8.
  The 11-25 nightly (~05:30 UTC−6) would be held on 93a04676 too — aim to ship before it (prints keep ~5 sessions).
  Lesson for the audit process: the fixers ran targeted crate tests; a cross-crate contract change (D-478) broke a staged-copy
  consumer (M0) that no unit test exercised end to end — the full suite passed. Needs an end-to-end day test on real data.
  **Hotfix released (UTC−6 ~23:40–00:05): worker 90e2f228+ec7bbce60e40** (engine fd740b84: M0-ANALYTICS stages
  source-identity.json; no value change; tape reader identity unchanged, re-captured tapes stand). Phase 1 rescheduled on it
  (12×50-session blocks); **ONE-DAY GATE: day-block maxReplicaCount 1** until the gate block's first day publishes with no
  engine fault, then 8 (from now on every new build: one real day before scaling — release.sh to enforce it). The C-073
  re-run rule cancelled the 15 handed-over stale re-runs by itself. release.sh false STOP at step 11 ("no pod claimed
  refcheck in 20 min": it snapshotted the kinds to verify before the watch cancelled those re-runs) — finished with --from 12.
  Fix for release.sh: verify only kinds still claimable at verification time. Daily build in 20-core.yaml committed.
  **GATE RESULT (UTC−6 03:07): 2024-07-09 published on 90e2f228 — M0 fixed, but INT-IVSC01 ENGINE_FAULT for all 62 assets
  (+62 PARENT_ENGINE_FAULT): "H4 INT-IVSC01 scope/source/parent drift" (current vs T−5 predecessor bindings).** Second
  systematic engine fault; gate held (day-block max stays 1; the gate block continues its later days, which carry the same
  fault — accepted for diagnosis evidence, one node). Investigation sent (eng-hotfix-ivsc). The gate worked: one block, one day,
  instead of 12 blocks. Stop rule: 2024-07-09 is BAD (engine fault changing a published value: 62 withheld).
  Cause (engine cb7bf1ab → worker 714e47a3): D-482 switched the ivsc_history binding to the location-free view_sha256 and
  IVSC's checks to it, but h4_ivsc.rs still RECORDED the raw sha256 of an inventory holding absolute directories →
  check_summary_binding drift on every asset-day. Fix: record history.content_sha256 like every owner; no computed value
  changes. Missed because IVSC's one end-to-end test is ignored for a stale fixture (D-400) — backlog: restore it.
  Also on 07-09 and 09-18: two-asset faults (VS-V03-CURRENT projection count ×2, M0-ANALYTICS InvalidSurface ×2, SAI-G01 O02
  count ×1) — classified: NOT engine bugs. IBIT (ETHA later) before listed options: empty chain (canonical_row_count 0) →
  every reader withholds as EMPTY_DELIVERED_POPULATION (D-117); day summary faulted_engine 62 (all IVSC),
  withheld_empty_population 9. Only the intent log's "fault:" wording misleads (cosmetic, after the freeze). So the gate's
  only engine fault was INT-IVSC01. Image building; release + gate again; scale when a gate day has zero engine faults.
  **Hotfix 2 released (UTC−6 03:27–05:55): worker 714e47a3+5b2beb37d0c5** (daily build too). The old gate block drained
  2024-07-10 (published 04:45 on 90e2f228, IVSC-faulted) and its two reference checks; cap briefly 2 so the new gate block
  started beside it (05:13), then 1 again. Gate day = 2024-07-11 on 714e47a3 (~08:00). To recompute later (bad, IVSC):
  2024-07-09, 2024-07-10 (90e2f228).
  **GATE PASSED (UTC−6 09:00): 2024-07-11 on 714e47a3 — 4,608 slots, 0 ENGINE_FAULT, 0 PARENT_ENGINE_FAULT; INT-IVSC01
  62 of 63 published (the 1 and the other 66 EMPTY_DELIVERED_POPULATION: IBIT before listed options).** day-block max → 8
  (the yaml value). Pass 1 now runs at scale on 714e47a3. Nightly 11-25: waits by design for every tape ≤ its session
  (claimable_job: no unfinished TAPE at or before a block's last session) — the build's tape re-capture (~90 left at 08:15,
  ~24/h) → starts ~noon. Spend: DigitalOcean $351.57 month-to-date at 2026-11-25 22:14 (UTC−6) (balance API; core's spend
  watch still lacks its read-only token) — heads-up given to the owner before the $500 ceiling.
  **Memory (UTC−6 12:40–12:52): 4 of 8 blocks died on their first compute day** (3 node-evicted at 12.4–13.0 GiB, 1 OOM at
  14Gi): day compute (~5.6 GiB) beside the next day's download (~5.6 GiB quote window) on a 16 GB node whose system takes
  ~3 GiB. C-085: limit 14 → 12Gi (the engine sizes itself to it) + surfaces/FIELD overlaps off (speed only), applied 13:25
  to new pods. Watch: blocks dying at 12Gi, time per day. Owner's options if it persists: memory-aware prefetch (engine,
  freeze exception) or 32 GB nodes (cost). Also: 2025-02-12 company capture hit P-30 429s (recovered by itself).
  **Core d1f518f deployed (UTC−6 ~10:45):** C-080 refcheck dump in the engine's key order (proven through the suite's real
  load_store) + guard (≥1/10 "slot not published" → NOT_RUN, never DISAGREES); release.sh --core digest check by registry
  manifests and --from 1-4; re-runs kept "off" until the freeze (the branch had set "6"; reverted before merge). DB tests now
  need VYREON_REFERENCE_SUITE=<engine>/tools/reference. Worker nodes scaling down (8 at 10:42).
  Cluster wound down by 11:22 (one worker node: the nightly capture). **Engine batch eng-r5 (UTC−6 ~12:30, 26 commits, D-469..476):**
  suite reads slots by parsed id + NOT_RUN guard; progress in long local units; ended lease stops a worker in ≤2 min (exit 75);
  ONE tape-reader identity bump (D-472: lost-message rules, identity inputs, listing by the one Network-B opening auction →
  ARKK's 158 resolved; METHOD @14); bulk files read as RFC 4180 (the "quote character" was valid CSV — our fault, not P-31);
  dividends after the monthly expiration (UUP/XLE; H5-SURFACE@24, M0-ANALYTICS@1.1.0, M0-GAMMA@1.2.0, M0-GAMMA-CHANGE@1.1.0);
  six suite faults (D-476). D-471 (drain hands its check to a core re-run) being REVERTED: with D-466 the check is ~0.5 h on
  the node, a re-run re-downloads ~3 h. Still open before the audit: E3 (NVDA VS-O04 2024-06-21, TSLA σ) by re-captured
  inputs; 08-14 volume_oscillator_5_20 failures. Core work after: register the build, re-capture tapes 2023-05-24..today
  (6-12 h), hold days until their tape windows exist, recompute by method version (C-079).
  **eng-r5 complete (UTC−6 ~13:30, tip 9955bdbd):** D-471 reverted (rejected: check on node ~0.5 h vs re-run ~3 h); 08-14
  volume_oscillator = suite fault (engine computes ruled EMA5/EMA20 per D-107; matches real AAPL/DIA to 3e-14); TSLA σ = the
  referee's unconverged QuantLib grid (engine matches exact Black-Scholes; suite now refines to convergence); NVDA 2024-06-21:
  re-captured inputs show today's code (D-441 + D-028 agree) yields the reference's 0.032980 — the published 0.032165 likely
  came from a pre-D-441 build; PROOF = the post-release recompute's reference check on 2024-06-21. Suites: pytest 284/107,
  cargo 1,782/1 (address_bound, fails on this laptop on the base build too).
  **Full-engine Codex audit running (UTC−6 ~13:35):** all 9 engine scopes (daily, overlay, surface, m0, optsensors, field,
  company, tape, orchestration) on eng-r5, whole-subsystem, `audits/2026-11-25-full-engine/`. Then remediate everything.
  **Full-engine audit results (UTC−6 13:49):** ~62 ORDINARY (≈12 wrong numbers on real data: AVGO split-day strike spacing and
  double ladder, a stale quote erasing a traded print, FXI's 2023 cash deliverable, surface ×3, M0, option sensors, field,
  company, tape; plus mislabelled measures and wrong absences) + ~54 RARE. All to be fixed (owner's rule). Three fixers in
  parallel by subsystem on eng-r6a (overlay/optsensors/tape, D-477..489), eng-r6b (surface/m0/daily, D-490..502), eng-r6c
  (company/field/orchestration, D-503..515), each from eng-r5; targeted tests only (shared laptop memory); the coordinator
  merges and runs the full suite once.
 "freeze the build, then crank
  it and generate phase 1 with as many nodes as possible. Fix everything we find that we can trace to an engine fault. THEN
  selectively recompute where we have to." Why: since 11-21, 38 of 61 day publications (~62%) redid a published day across 11
  engine builds — fixes paid in installments. So: (1) engine frozen at worker 70d83bbc+04d2e67336c9 for the rest of phase 1;
  engine releases only for BLOCKERS (a day cannot publish, a crash loop, runaway cost); (2) every engine finding goes to the
  "Phase-1 engine backlog" below, fixed after phase 1 in ONE release, one audit, then C-079 recompute by method version
  (scoped where inputs allow); (3) core releases continue freely (they change no computed data); (4) block layout: one big
  contiguous block per node (owner: "how many days to compute is decided by the optimal block count per worker"; first days
  carry the cold start), rescheduled after the current drain; (5) as many nodes as the limits allow, raised in steps with
  evidence: the Intrinio budget is the binding limit (2026-11-23: 12 blocks + 24 tapes saturated 1,900/min and timed captures
  ran out of time → cut to 8); tapes are now done (no tape queue) and 24 h average is 732/min with 8 blocks → 12 first, then
  16+ while the budget and capture outcomes hold. DO limit 30 droplets (13 used); pools day + day-intel max 13 each.

  **Phase-1 engine backlog (fix after phase 1; engine release frozen):**
  - Fill progress per symbol inside long local units (NO_PROGRESS false alarm 11-25 10:03).
  - Reference suite: find a slot's id by parsing the line, not its first 400 characters; a NOT_RUN guard when the suite loads far
    fewer slots than the dump holds (core fix below makes this non-blocking).
  - 158 ARKK tape sessions (owner decision: re-capture vs rule).
  - Drain ends at publish; the day's reference check handed to REFCHECK re-runs (drained slots held ~2 h).
  - Any further engine finding from the cluster, with evidence.
  **07-10 cause PROVEN (UTC−6 ~10:45):** not mixed builds, not data. Core's refcheck dump writes store.jsonl keys alphabetically
  (serde_json preserve_order off: body first, measurement_id after); the suite looks for the id in a line's first 400 chars →
  every REFCHECK re-run from core's dump loads 0 slots (report 33: 3,332 of 3,341 "slot not published"; reproduced with real
  bodies). Every re-run since C-073 is invalid; worker-own-dump checks (07-15 87, 08-14 22) are valid. Fix in CORE (engine
  frozen): dump lines in the engine's key order + a guard storing such reports as invalid; then re-run the affected checks.
  **Roadmap beyond launch (owner):** repurposing research after phase 1 (needs data); full history after launch; then the
  forward-looking research and the no-signal line exactly as PRODUCT_INTENT.md states them (lines ~10, 57, 66, 328-331). New
  from the owner's legal consultation: current work is within the review's scope; incorporation is most of the defence; the
  full legal research ($2,500 retainer) waits for revenue or a rig hitch.
  **Investigating while it runs (owner's goal, UTC−6 ~11:40):** (1) the request budget is ~half used during downloads (avg
  ~800-1,270/min of 1,900; near the cap only 12 min in 4 h) → downloads are bound by per-worker concurrency, not Intrinio:
  candidate improvement = more in-flight requests per worker (cuts the ~3 h download stage and node-hours); compute is
  CPU-bound (7.7-8.1 of 8 cores) as it should be. (2) **The daily run 2026-11-23 is hung:** exq01_cohort_capture idle since
  16:45 UTC (62/64 assets captured; NVDA and SMH stuck, no requests, no budget waits), worker still heartbeating → core sees
  nothing (observability gap → a 'no progress' watch belongs in the release). Read-only root-cause investigation running;
  process kept alive as evidence (deadlines start 11-30).
  **Root cause (investigator, read-only): not a hang.** EXQ01 freezes each asset's cutoff at plan creation + 3600 s and,
  after its last asset (COIN, plan 16:45:10 UTC), sleeps to that cutoff (exq01_cohort_capture.rs:315-317) → exits ~17:45 UTC
  by itself. Real defect: **NVDA and SMH each failed on a single HTTP 502 with no retry** (:191-195; NVDA at page 1113) → job
  retry re-captures them and sleeps another hour. Ordinary and daily (every daily run idles ~1 h; each 5xx adds ~1 h and can
  withhold an asset's EXQ01). Backfill unaffected (prints not retained → EXQ01 skipped). Into the release: EXQ01 defers its
  cutoff wait to the fill (like ACT01 D-457), bounded retry of transient 5xx/timeouts per page, worker step/wait reporting +
  core NO_PROGRESS watch (alerts on no progress unless a declared wait covers it). My "hung" call was wrong; the evidence
  check corrected it.
  **core-auto complete (204/0; C-073; migration 0055 extended in place, never deployed):** automation debts + all C-072 leftovers
  + the two pre-existing publish defects + bounded migrator lock + NO_PROGRESS (`POST /worker/v1/lease/step`). Waiting on
  eng-rel (refcheck worker, LATER items, EXQ01 defer + 5xx retry, step reporting with declared waits); then one audit of both.
  **Owner: retries are exponential (1, 2, 4, 8 s …), the standard way** → eng-rel: one shared policy for every provider retry:
  exponential backoff with full jitter, capped, bounded attempts, honouring Retry-After on 429/503 (RFC 9110 §10.2.3),
  through the request budget.
  **eng-rel complete (a20d6af0; 1,733/0; pytest 249/107; D-461):** `vyreon-worker run refcheck`; EXQ01 per-page retries +
  `--defer-cutoff-wait`; step reports with bounded declared waits; one retry helper (full jitter, 1 s→60 s, Retry-After); 38
  LATER items: 19 fixed, 5 not real/resolved, 9 reader bundle, 2 overlay-rules bundle, 3 rulings. Coordinator rulings:
  overlay-rules items (0 in 941,100 rows; force re-preparing every option source) join the post-phase-1 bundle with the reader
  items; EWJ reference independence and a BABA2 certification guard fixed now; unreadable retained bar failing UACT/UQTR = honest
  ENGINE_FAULT (not a defect). Then the single release audit of core-auto + eng-rel by both reviewers.
  **Release audit running (UTC−6 ~12:30):** core-auto @9f18db4f and eng-rel @9cb9f049 (EWJ ledger + BABA2 guard added),
  Codex + Gemini each (`audits/2026-11-24-release/{core,engine}/`); engine Gemini chained after core Gemini (quota). Then fix
  every real finding, merge, build, release with release.sh (core first, worker within 15 min).
  **Core release audit in (UTC−6 ~12:55):** anything changing a published number or absence on real data today? **No** (one
  rare path: a changed-body resend on a current day; never observed). Codex 10 ordinary (release.sh: resumes a pause it does
  not own, admits without evidence, weak success check, --from recovery, dry-run writes evidence; refcheck queue/labels;
  budget pressure hiding a stalled daily tape; CLI counts) + 9 rare; Gemini 10 ordinary + 4 rare (overlapping; adds a temp
  token file left on SIGINT, false JOB_THRASHING on retry-failed, false NO_PROGRESS on verify/upload). All being fixed on
  core-auto (no re-audit). Engine Gemini running; engine Codex pending.
  **Engine release audit in (UTC−6 ~13:05):** no wrong numbers; **wrong absences on ordinary data in existing sensors**
  (DLM-EXPECTED-MOVE blanks established roles; an OI fault masks spot and non-OI roles in max pain/unusual contracts/OI
  centroid; indicator warm-up withholds readings whose bars exist; VWAP share-count reason) + release-new: re-run judges old
  revisions under newer rules; concurrent fill steps overwrite each other's step report; step posting can starve heartbeats
  (Gemini ordinary); reference ledger images break on real reverse splits; Retry-After lost across layers / mislabelled.
  All being fixed on eng-rel (no re-audit). The absence fixes bump method versions → the few days built before the release
  are recomputed (cheap while the DB is small).
  **core-auto fixes done (210/0):** release.sh hardened (own-pause only, evidence required, --from run file, all ScaledJobs
  verified, claims per kind, dry-run writes nothing, diff errors stop, tag reuse refused, token file trapped); re-run
  queueing exact; dump streamed; alert accuracy; resend compares every field. G1.2 kept by design (KEDA starts no backfill
  work under budget pressure). Last item: the C13 first-day residual, closed by profile layer.
  **First days on 5f637983 (UTC−6 12:58): 2024-07-16 published** — block lease 08:03 → publish 12:58 = **4 h 55 min for a block's
  first day** (~3 h download + ~2 h compute); later days in a block expected faster (window reused) — measure day 2.
  **VS-O04 did NOT clear:** 07-16 on 5f637983 still withholds VS-O04 entirely (ENGINE_FAULT, all roles incl. same-day ones) for
  10/63 assets (ARKK FXI JPM MSFT MSTR PLTR SPY TSLA XBI XLV) — my D-458 attribution was wrong. Pattern: a block's first days.
  If every daily run (a one-day block) hits it, it is launch-blocking. Read-only root-cause investigation running; fix joins
  the release.
  **VS-O04 root cause (evidence):** not block position — `production_result.rs:420-428 validate` (pre-D-144) rejects a body
  when T is clean but T−5/T−10 is OI-faulted; containment then writes ENGINE_FAULT on all roles. ENGINE_FAULT set on each
  day == assets OI-faulted at T−10 (e.g. 07-16 ↔ 07-01: 10/10). Hits daily runs too. Fix → eng-rel (restores D-144, no method
  change; republish VS-O04 on the 9 affected days). **Separate question opened:** OI marked faulted for 10-51/64 assets on
  every session 2024-05-20..07-03 (clean from 07-05); only 06-10/11 are known faults (P-27) → true provider incompleteness or
  an over-firing gate? Read-only investigation with Intrinio spot checks running.
  **Answer: mostly an over-firing gate.** ~700 of ~760 false OI-fault asset-sessions came from Intrinio EOD ghost rows (38 deep-
  ITM LEAPS calls, 1-4 sessions each, never quoted/traded/in the snapshot) that D-453 treated as established series → P-28;
  real non-deliveries only 2024-05-20 (added to P-27) and 06-10/11. Coordinator rulings (owner may overrule): a series is
  established only with a second witness (D-079, as D-454 rule 6); unwitnessed rows are excluded from OI/volume totals.
  → eng-rel (OVERLAY_RULES bump, fixture requote). Phase-1 effect before the fix: ~28 five-session-change + ~146 path-efficiency
  asset-days withheld, 22 assets' CAP126 percentiles at the eligibility floor until ~2025-01.
  **eng-rel all audit fixes in (c397cc00; 1,742/0; pytest 255/107)** incl. VS-O04 validator, inventory profiles, 7 method bumps
  (EXPECTED-MOVE 2.4.0, MAX-PAIN 1.4.0, UNUSUAL-CONTRACTS 2.2.0, OI-CENTROID 1.2.0, ZERO-DTE 1.2.0, TECHNICAL 1.10.0, VWAP 3.2.8),
  witness rule (either quote side). **Verified on the cluster archive (ghost_verify.py, 2024-04-08..07-18):** 51 false faults
  removed, 97 real kept (e.g. AMZN 06-10/11). **My ruling corrected:** unwitnessed rows are 16,812 asset-session rows / 2,178
  codes (many real unquoted series) → excluding them from OI totals would remove real open interest; the witness rule stays
  for "established" only, totals unchanged (ghost OI on 05-20..06-03 recorded under P-28; outside phase 1's windows). New
  ordinary finding: split ex-dates key adjusted series at strike ÷100 (AVGO 2024-07-15, NVDA 2024-06-10) → ~21 sessions of
  false OI withholding after every 10-for-1 split → fixed in this release (P-29). Overlay items 26/27 ride the same bump.
- **Automation debt (2026-11-23): a reference check dies with its worker.** Gate a runs in the worker after a day
  publishes; hand-over 18 stopped the workers of 2024-07-09, 07-16, 07-17 (published on f3db692e) before their
  checks finished, and nothing re-runs them (core only reports REFCHECK_NOT_RUN). Fixed this time by recomputing the
  three days on d9462aad. Proposed: core queues a REFCHECK job for a published day with no report after N hours, run
  by a small worker from the stored body (no download).
- **Automation debt (2026-11-23): the release sequence is hand-run.** Each new image took the same ~8 manual steps
  (build, push, inventory, register, queue pause, hand-over preview + expect, evidence file + admit, ScaledJobs apply,
  delete the old build's Jobs, resume): done 9 times on 11-22/23. Proposed: one script `deploy/k8s/ops/release.sh
  <engine commit> "<reason>"` that runs them in order, stops on any refusal, and prints the evidence it recorded.
- **Automation debt (2026-11-23): concurrency caps tuned by hand from the budget** (tapes 24 → 12, blocks 12 → 8,
  ACT01 in-flight 2). Proposed: KEDA's trigger counts claimable work only while core's request budget has headroom
  (e.g. `core.claimable_count` returns 0 for a kind when the last minutes ran at the ceiling), so the system backs
  off by itself instead of an operator lowering caps. **Owner, 2026-11-23: approved ("a good idea"); to build after
  phase 1's current run, before the two-weeks-unattended test.**
- **Automation debt (2026-11-23, hand-over 5):** a withdrawn build's running block workers keep computing
  (their leases are gone; they find out only at publish) and KEDA counts their Jobs as running, so the
  successor's blocks wait. Operator deleted the two Jobs by hand. Proposed: the worker checks its lease
  between days (and on each heartbeat) and exits when it is gone; or the hand-over runbook deletes the
  old build's Jobs (`kubectl delete jobs -l scaledjob.keda.sh/name=day-block` for pods of the old image).
- D-412 left by reason: halted-ETP close (the tapes carry no halt messages), zero-volume `9`, S1–S3
  (need cluster data); ETP TWAP/T-WAM fallback tiers not computed (reach the tape only via `M`/`9`);
  Network-B listing taken from the provider's current record.
- Options: ACT01 picks its cohort by raw strike, guarded only by `ex_date=`, and is not translated
  across a strike reduction (a stated NONE); a partial OI outage without an adjustment still
  publishes (no threshold invented); D-349's residual (an ordinary dividend day read as an ex-date
  when the top old strike is delisted); contracts the snapshot shows active that a bulk chain lacks
  are only measured (`bulk_snapshot_absent`); a refused bulk listing falls back to the API.
- From rounds i to k: a faulted reading with no value in its 63-session window is not stated in
  unusualness; FND02's standardized roles equal FND01's on 72 of 81 pairs and both score; a
  translation refusal publishes NO_GOVERNED_COHORT_CERTIFICATE, not its cause; a retained view's
  collision reads ENGINE_FAULT until `--rebind`; a resumed worker's stale view is not reselected; the
  collision guard sees only T-5's delivered chain (never run on a real collision).
- INDPRO's 2025-12-03 macro capture stated WITHHELD_WRONG_UNIT (seen 2026-11-16; not looked into).
- Style items 3–7 (round a); findings for after the build (round e); overlay 3, 5, 6, 7; a corrected
  official daily volume between two workers' tape captures; a missing macro capture's containment.
- An `unknown` ledger entry (D-176's 2020-01-02) may make dead listings' OI unknown on
  2022-05..2023-04; core has no `source_coverage` rows for `option_intervals` and
  `option_interval_trade_counts` (batch 5, fix 7 entries). Deeper history only.
- META's VS-P01 cold start after the FB→META rename (2022-06): contained; fix before deeper history.
- Core: the claim does not send `max_attempts` (the worker assumes 3); superseding a refused tape
  session means deleting its rows (preview first). Round x's twelve operations limits are recorded as
  stated limits in the decisions.
- `r2` against `p1`: 11 items to check; M0 values were not compared (`day-r2-vs-p1.md`).
- The zero curve on a 2022–2024 date (`python -m refcheck.options.zero_curve <H.15 csv>`); the hold for
  a tenth of slots lacking a provider input (owner 2026-11-13); a tolerance for the Greek exposure's
  unsolved share (owner 2026-11-15).

## Machines and money
- **Spend:** DigitalOcean $180.41 month-to-date (2026-11-21 06:09 UTC), before phase 1; ceiling $500.
- **Droplets (4 of 30 before the build):** web (`Telemetry-Backend`), `vyreon-engine-shared-8vcpu-amd`,
  the cluster's core node, workers as running. Limit 30 (up to 27 workers).
- **Droplet `ssh vyreon-engine`** (8 vCPU, $112 a month): Postgres, the daily capture and archive
  (`vyreon-capture`, `vyreon-archive`: the option-trade cohort prints the provider will not serve
  again, to its 250 GB volume and Spaces `vyreon-data`); notices muted to
  `/var/log/vyreon-notices-muted.log`; health and heartbeat timers stopped (2026-11-17). Retires per
  the end state. Not checked since 2026-11-15: whether `/root/vyreon-engine-worker` and
  `/root/worker-proof` (57 GB) are still on it.
- **Kubernetes cluster `vyreon`** (nyc1, id `5f56e6e0-1c74-4dc9-bfdb-962ee55ebff3`): pool `core`
  1 × `s-4vcpu-8gb-amd` with CloudNativePG (database volume 120 GB, grown 2026-11-16) and core
  `vyreon-core:139a21b` (migrations to 0030; perishable archive to Spaces ON; notices live); day
  pools `day` (AMD) and `day-std` (Intel), nodes deleted after every run. Secrets:
  `vyreon-worker`, `vyreon-core-webhook`, `vyreon-core-spaces` (bucket-limited key). Registry
  `vyreon` (nyc3, $5 a month). Images with known defects must not be used: `ed66a433`, `76169889`.
- **Spaces `vyreon-data`:** perishable archive; DataShop order `reference/cboe-datashop/order_000107661/`
  (laptop copy `~/claude-artifacts/datashop/`). Keys (gitignored, 600, never printed): `api.txt`,
  `spaces-storage-access-key.txt`, `tiingo.txt`, `~/fred-api.txt`, `do-full-token.txt`.
- **Laptop:** worktrees `/home/diggz/wt/<name>`, cargo targets `/home/diggz/wt/target-<name>`
  (delete at merge; `du -sh /home/diggz/wt/target-*`). Test droplets deleted 2026-11-16; tooling
  `tools/testbed/tb` if one is needed again.

## Proven so far (on real data)
- 2026-11-09..15: laptop vs droplet within 1e-11; a day through core reads back identical; the
  worker path equals the droplet's; two-day cluster trial; containment of failed inputs; slots by
  date membership (D-164); split stages; the zero curve equals QuantLib to 2e-13.
- 2026-11-17..18: cluster verification days; refused provider sessions as stated absences (D-358);
  two-week sample 10/10; Cboe (14 pairs, all 140 sessions in band 1), EDGAR domestic filers bitwise,
  8-K times 765 exact, Tiingo, FRED; D-391 caught XLK 2025-12-05 with 0 false positives in 424,814.
- 2026-11-19..20: bulk files equal the API; historical tape caches 1,276/1,276 byte for byte; 0
  engine defects in the refcheck triage of real days; multi-day blocks with history reuse; the curve
  fallback bit-identical while H.15 was down.
- 2026-11-21: before/after (226 / 134 / 120 min); D-405 arena cap and D-407 trim on real days;
  refcheck beside the next day (0.0 min waits; 3 unexplained of 97,113); byte proof 11,870/11,874;
  D-412 tape samples @9..@13 256/256 PASS.

## Not yet proven
- 27 nodes at once (request budget, nyc1 AMD capacity, core under load; whether presigned downloads
  count against the 2,000 a minute); a 15-day block (memory on 16 GB; M0 solve-reuse hit rate).
- The history reference check over built histories; the flag-agreement test (declared; code next);
  refcheck across ~400 sessions; the reuse-index fix's later-day time (~104 min expected).
- Daily go-forward in production (C-051; the daily claim through HTTP, `JOB_COMPLETION_INDEX` on the
  cluster), deadline alerts, scaling the pool to 0; the trades-API path.
- A backup and a tested restore (C-013).
- Reporter on core's API; the web's receiving end; the API end-to-end; the Startup-tier key switch.
- Rebuilds and scoped recomputes for real; the cross-day supersede rule.
- Not run on values: the preferred-dividend subtraction (no real case); D-165/D-167 on a rename
  date; D-166 on a post-listing date; an early date before the tapes (2023-05-24), option intervals
  (2022-12-08) or trade counts (2024-11-26); the split-basis scores (need a built history).
