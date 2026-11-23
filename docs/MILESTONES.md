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
in 7 scanned months; remaining paths need stacked faults. Old Jobs deleted by hand again (2nd time: automation debt
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
