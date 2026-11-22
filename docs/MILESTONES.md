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
session from the build onward (built, C-051: core queues T at 07:30 New York the next day, CronJob
`daily` (`deploy/k8s/40-daily.yaml`), pool `day` to min 0; not switched on; runbook
`deploy/k8s/README.md` "Daily runs"); (3) Reporter producing the paid issue from core's API, sent
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
   sensors." Base layer censused once its days are in, derived after the ordered passes; the criteria
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
5. **Launch decision:** Intrinio's Startup tier (display/redistribution rights; covers free and paid),
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
  starts, from 2026-11-23 (`VYREON_CORE_DAILY_FROM`/`_BUILD`, apply `40-daily.yaml`); set
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
  recomputation has a known time and money cost its actually WORTH backing shit up." Then delete the
  laptop safety dump `~/claude-artifacts/db-backups/vyreon-20261116T*.dump`.
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

## Before the full build: the system runs without a watcher (owner, 2026-11-22: "do all 4")
Tonight's test-block tapes needed an operator for four things: Job completion counts (the Job ran out
with 51 tapes left and 8 nodes idle), node sizing and idle deletion, spotting non-failure oddities, and
fault judgement. The first three move into the system; judgement no longer blocks (C-061 sets a failed
day aside; three standing failures pause and alert).
1. **Claims:** core's claim race fixed (try the next claimable job while any remain) and the worker's
   claim retried on a connection that never reached core (loose ends below).
2. **Workers and nodes follow the queue:** KEDA (designed 2026-11-10, `deploy/k8s/README.md` "After
   this run"; not installed) with one ScaledJob per kind (tape, day block, ordered pass) on the
   `postgresql` trigger counting `core.claimable_job`; pool `day` autoscaling min 0, max N, so nodes
   appear with pending pods and leave when idle. Proven on a real run before the full build.
3. **Core's watch takes over `health.sh`:** stall (claimable, no live lease for 15 min), pods finding
   nothing while work waits, worker nodes up with no worker, spend against the ceiling: each an alert;
   plus the progress channel (below).
4. **Hard caps instead of judgement:** the autoscaler's max nodes and KEDA's max replicas set from one
   node's measured request draw (owner's rule 2026-11-19) so the system cannot pass 1,900 requests a
   minute or the spend ceiling.

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
- **Progress channel (owner, 2026-11-22; before the full build, with the claim fixes):** a second
  Discord webhook (`progress-webhook.txt`, git-ignored; cluster secret `vyreon-core-progress-webhook`,
  key `webhook`; silent test post 204). Route core's logged notices there, batched: each day published
  (slots published/withheld by reason, refcheck, minutes, memory), block start/finish with its node,
  tape and pass progress every 30 min, spend at each node change, set-aside days with cause, builds
  admitted/withdrawn. The alerts channel is unchanged ("go look or running ok"). Silent, no mentions.
- **Core claim race (before the full build):** with 24 tape pods claiming at once, about half got "no
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
