# Milestones

Current state only (pruned 2026-11-26 by the owner's rule, AGENTS "Running the build"). The full previous
record, every run, release, finding and time, is tag `milestones-2026-11-26` (tag 31aca20 on commit edeaf00);
the one before it is `milestones-2026-11-21` (f0d2b98); git history holds the rest. Read top to bottom: Terms,
Resume here, the plan of record, the steps, what waits on the owner, the standing rulings, then reference lists.
Decisions: core `docs/DECISIONS.md` (C-numbers), engine `docs/DECISIONS.md` (D-numbers); plan
`docs/ARCHITECTURE.md`; provider faults `docs/PROVIDER_FAULTS.md` (P-numbers); runbook `deploy/k8s/README.md`.
Paths under `~/claude-artifacts/` are on the laptop (`checks/`, `audits/`, `cluster-scratch/`).
Times are MT = the owner's clock, UTC−6 year-round (owner, 2026-11-24), unless marked UTC. Write a time only
after `date`. Record each item as it lands, with its commit; prune again by tagging first.

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

## Resume here (new session, 2026-11-26 16:00 MT; the handoff below verified against the cluster)

**PHASE 1, FIRST BUILD: STOPPED 2026-11-28 12:09 MT AT 93 DAYS BY THE EVALUATION (it ran from 2026-11-27 02:42 MT). The fix pass and a second build come next; only the nightly runs. Read this block first; the 2026-11-26 blocks below are history.**
- Running build: engine `760a8760+93486ee1fd46` (released 02:39 MT: stage 1 + D-564..D-568, the date gate
  retired, the evening audit's findings fixed); core image `9982f6a` (C-086..C-098). Ceiling 1,800; nightly
  reserve 600; recompute OFF for the run.
- Queue: 12 day blocks over 2024-07-09..2026-11-20, 597 sessions, blocks of 50 (the newest of 47). Job 30091
  (2026-09-17..2026-11-20, the newest block, pod day-block-5qhh8-trmwx) started 02:41 and is the build's first real
  day under core's gate; the other 11 blocks (550 sessions) are QUEUED and core starts them by itself once that
  day publishes clean, one every 20 minutes as the budget allows. Nothing is held out.
- **2026-11-27 07:27 MT: the build's first phase-1 day published clean** (2026-09-17: 2,853 base slots, 4
  counted withheld, 0 engine faults; 286 min as a block's cold first day). Core's gate opened by itself ("published
  a real day clean") and the second block started at 07:30; the ramp adds one about every 20 minutes as the budget
  allows. Day 2 (2026-09-18) went straight into compute: its download had run beside day 1.
- 10:01 and 10:07 MT: the first recorded provider refusals (228 on 3 workers, with 8 blocks running); the alert
  fired by itself; core halved the ceiling as designed; no job failed. Intrinio's body read (generic words, no
  headers; nothing fatal, no secret). The minute count does not explain them (core counted 753 to 1,599; the
  droplet idle). Catalogued, HIGH, with the hypothesis of a per-second limit. Not acted on.
- **PLAN OF RECORD AFTER THE DAY LAYER (owner, 2026-11-27; supersedes the older "one large fix pass" wording
  where they differ):**
  1. Populate the database (phase 1's day layer, then the history passes).
  2. Test-integrate Reporter against core and list the measurements that are blocked; an empty plot is blocking.
     It reads only, so it may start on the days already published, before step 1 ends (the history-based
     readings need the history passes).
  3. Build the single-sensor recompute (today a scoped recompute costs almost a full day each: catalog).
  4. Repair the blocked sensors in priority order: fix, then recompute that sensor (a gap from a one-off failure
     is recomputed straight away).
  5. The remaining catalogued issues.
  6. Evaluate the list of what needed an agent ("What needed an agent" in `docs/PHASE1_CATALOG.md`): each line
     becomes something the system does by itself, or a stated reason it stays by hand. The owner, 2026-11-27:
     "the system needs to eventually function without you".
  7. Settle the sweep's unresolved items: the 37 "not established" items and the 13 unchecked "Listed, not fixed"
     rows (the four files `~/claude-artifacts/checks/decided-not-built-2026-11-27*.md`, each under "Not
     established" / "Not checked"); most need the cluster or the owner's decision.
  **Evaluation at about 100 published days (owner, 2026-11-27):** decide then, on evidence, whether to stop this
  build and run a second pass with the gates turned into recorded flags (so every sensor's values are seen), the
  known time costs reduced (the reference check's wait first) and anything that blocks publishing fixed; or to
  let this build finish and repair sensors one at a time. The evidence: the scorecard draft, the census by era,
  the gate register with firing counts, the run's costs, and a size for the conversion work. Not decided: it is
  "a good plan if necessary". If a second pass is chosen, its list of changes is written down, and kept short,
  before work starts.
  Part of that evaluation (owner, 2026-11-27): a robust investigation of every base-layer sensor that is not at
  100% coverage (the owner first said under 95%, then tightened it: "That is a strict standard, but i mean look
  what we just found"): for each, the gate or cause with its origin and evidence base, the published standard
  that applies, and what it would take to reach 100% over the sample. Each investigation is scoped to one sensor
  and carries its evidence (see VS-V03 and CDD-MACRO01 in the catalog for the shape). A first draft over 23 days is in
  `~/claude-artifacts/checks/phase1-run/base-coverage-latest.tsv` (query beside it; it misses the company, FIELD
  and M0-PRINT-SIZE layouts: build the proper count from `core.withheld_tally` with totals before the evaluation).
  The owner's reason: the universe is meant to grow to the full market, so a sensor must cope with thin chains
  and sparse points, measuring in one standardized way over time.
  **EVALUATION DECIDED (owner, 2026-11-28, at 93 published days; "the evaluation is looking pretty well decided by
  the coverage statistics in my eyes"; "yes to draining the backfill, yes to recording these decisions"):**
  1. A second pass: ONE build of the database with all sensors publishing. This build (760a8760) is not carried
     to the end. Its 93 days (plus the nightlies) stay as the evidence for the fix pass.
  2. What withholds: a genuine provider absence. Everything else is market data and publishes; a check that
     fires is recorded as a flag beside the value (owner: "Genuine provider faults = withhold, but beyond that,
     market data is market data"). Not more guard rails: robust sensor definitions (owner).
  3. A canary-style small block before scaling again.
  4. The reference check on a random spread of dates, not every day.
  5. Built properly, not hurried (owner).
  Still under discussion, NOT decided: which standards to adopt (volume scope: regular session or the whole
  consolidated day; the macro series as levels, rates of change or both; point-in-time vintages), and whether a
  few known-wrong cases stay withheld. The owner asked for a reading of `/home/diggz/market-research` (the
  product's original design) before choosing.
  **Backfill stopped 2026-11-28 12:09 MT** (`vyreon-core backfill stop`, the flag in `core.queue_control`): the
  twelve day blocks 30091–30102 cancelled as LEASED. NOTE: the owner approved a DRAIN (each worker finishing its
  day); core has no drain outside a build hand-over (C-077), so the stop ended the days in flight. The nightly
  (priority 1: 30110, 30111, 30103) is untouched. `backfill resume` clears the flag; blocks are scheduled again
  by hand.
  **Method for the second pass (owner, 2026-11-28: "for all sensors for the next build we want to systematically
  go through and undo whatever guards are causing them to be withheld"; started the same day, before 100 days):**
  1. Sort every stated withholding reason (36 on the non-FIELD sensors over 92 days:
     `~/claude-artifacts/checks/phase1-run/withholding-reasons-latest.tsv`) into three classes, each with its code
     location, its decision and its evidence base, by reading the code and not the reason's name: (i) a guard (the
     data is present and the engine declines); (ii) missing on our side (an input not captured or fetched);
     (iii) missing at the provider (a stated absence). Read-only; three agents by family, results in
     `~/claude-artifacts/checks/reason-classes-2026-11-28-*.md`, each verified before it is used.
  2. Guards become recorded flags, so the value publishes; class (ii) is investigated; class (iii) stands.
  3. Rebuild, then judge each sensor on its published values (coverage by complete readings; FIELD apart, as a
     derived cross-section).
  Whether and when the rebuild happens is still the 100-day evaluation's decision.
  **The owner's directions for that fix pass (2026-11-28, on the three reason-class files; "something to be done
  for the after-100-counts fix pass"; "My ideal result is that we build the database anyway, then we can
  investigate oddities"):**
  1. QUOTE_BOOK_ONE_SIDED: a provider absence; it stands.
  2. The 2% volume check (SOURCE_VOLUME_RECONCILIATION_FAILED): goes.
  3. The "no certificate" reasons: publish and report honestly what could not be certified; still to investigate.
  4. Tape against the official bar: investigate before deciding.
  5. Option-chain reasons: an established standard where one exists; a maths or definition correction where it
     is a zero denominator.
  6. A rebased input series (the macro sensor): a robust, standard way to handle rebasing.
  7. Company sensors: examine how each withholding is classed; filings are sparse by nature.
  General (owner): "wherever we are having issues publishing data we need to look for established standards";
  the aim is generalizable sensors that can be turned on and left on. Standards research with sources:
  `~/claude-artifacts/checks/standards-2026-11-28.md` (in progress). The three class files are agents' readings:
  each row is verified before it is acted on (verified so far: the dates of rows 1 and 2, PATH-L03-L07's zero
  denominator, the macro input's change on 2026-11-24).
  The owner's aim: an MVP that publishes. Coverage of 80% per sensor allows about one missing day a week.
- **Fixes during the run (owner, 2026-11-27):** fixes are made on branches as found; nothing is released (no
  engine image, no core image) until phase 1's day layer is done, except the per-second request bound if provider
  refusals recur. **Unreleased fixes, kept current here:**
  - RELEASED 2026-11-27 11:56 MT (the agreed exception): core image 4789dcd = main + core-s6 (C-099), with
    `VYREON_CORE_INTRINIO_PER_SECOND` = 30, after provider refusals recurred (10:01, 10:07, 11:33, 11:45 MT; 393
    in all). The restart caught the twelfth block's pod in its first minute (day-block-dh8dc-csl9t, job 30102:
    "tape caches ... vyreon-core: error sending request", exit 1); its lease must expire before it is handed out
    again. core-s7 and core-s8 below do NOT contain a release of their own: they still sit on core-s6's commits and
    are unreleased.
  - core `core-s7` (same worktree, from core-s6, head 1f8fa6e): C-100, the channels' wording (holes where an id
    was stripped; the worker-death line; days published per progress post; the spend line's age and no negative
    rate; whole readings withheld beside the "% of readings"). Verified by the coordinator: full suite 283 passed,
    0 failed. Not run against production. Releases together with core-s6 (it contains it).
  - core `core-s8` (same worktree, from core-s7, head 54d55ff): C-101, nothing pauses the queue by itself: a tape
    job out of attempts and a refused bundle are contained to their job and day (migration 0071: table
    `core.bundle_refusal`; view `core.claimable_job` no longer holds day blocks behind a FAILED tape job). Verified
    by the coordinator 2026-11-27 11:38 MT: full suite 284 passed, 0 failed; the only remaining caller of
    `work::pause` is the operator's command (`vyreon-core/src/main.rs:458`). It changes what the queue hands out:
    NOT run on the cluster; a day block capturing its own tapes after a failed tape job has not been watched.
    Contains core-s6 and core-s7.
  - core `core-s9` (same worktree, from core-s8, head 514f57a): C-102, the reference suite runs 6 processes on a
    worker (`VYREON_REFCHECK_JOBS=6` in `50-keda.yaml` for `day-block`, `day-block-daily`, `refcheck`), after four
    workers were OOMKilled in a check (8 processes of ~1.5 GiB plus the worker against a 12 GiB pod). A manifest
    change only; it reaches workers started after it is applied. NOT applied, not run. Contains core-s8.
  - TO DO (core, observability rule): a watch check for a measurement that goes from published to withheld on
    nearly every asset between two days (CDD-MACRO01's industrial-production input changed base on 2026-11-24 and
    part of the sensor was blank on all 64 for four days with no notice). TO DO (engine): the macro unit guard's
    governed base (the series now reads 2022 = 100 by an agent's provider read, to re-check), then re-capture.
  - TO DO (engine): the worker sizes the suite's process count from the memory it has; the suite's own log goes
    to the archive; profile the suite's single-process first stage (hours on the 2024-09..2025-02 dates).
  - TO DO, from the decided-not-built sweep (owner, 2026-11-27: "1-4 are very much to be done if possible";
    catalog entry "Decided and not built"): (1) the live-day trades-API capture, its use to be designed again
    under the 2026-11-26 direction (publish with the gate recorded, a robust definition) before it is built;
    (2) done: `PROVIDER_FAULTS.md` P-04 corrected; (3) D-404's skip-and-state for the EVC history view
    (`day.rs:1150`); (4) D-396's missing rows against the snapshot, first establishing whether it is the cause of
    VS-O04's unknown open interest (D-517); (5) the recompute rule (core's planner still selects by method
    version): before recompute is switched back on, or replaced by deliberate scoped recomputes. None started.
  - engine: none yet. Known and not started: the reference check's own log is in no archive (D-566 note); the
    stage-1 audit's leftover operations findings (catalog items 11–30).
- 11:26 MT: 11 blocks running, 1 queued, 3 days published on this build (2026-09-17, 2026-09-18, 2024-07-09), no
  restart, no refusal since 10:07. First pace inside a block: about 195 min a day (catalog).
- 12:18 MT: all 12 blocks running (job 30102 recovered by itself, attempt 2, pod day-block-fhj6v-85xnn); 5 days
  published on this build, 0 engine faults; no provider refusal since 11:45 with the per-second bound at 30 (the
  most granted in any second: 30; 1,200 to 1,620 requests a minute).
- 13:52 MT: 11 days published, 0 engine faults, 12 blocks running, no restart. Refusals returned at 13:49 and
  13:51 under the per-second bound (28, against 49 to 179 before it): the bound did not end them; cause still
  not known (catalog). Nothing failed.
- 14:55 MT: 14 days published, 0 engine faults, 12 blocks running, no restart, no failed job. A third episode of
  refusals under the per-second bound at 14:44 (175, 159 on `/options/interval`, three workers in the same stage):
  596 refusals in all today. Catalogued; nothing failed; not acted on.
- Run diagnostics are kept as data, one row per check (owner, 2026-11-27: "keep monitoring and cataloging run
  diagnostic information"): `~/claude-artifacts/checks/phase1-run/snapshot.sh` (read-only) appends to
  `snapshots.tsv` there: blocks by state, days published, engine faults, days in the last hour, refusals, worker
  deaths, pods and restarts, peak process memory, median compute and download minutes, requests and budget waits
  in the last hour, open problems. Run it at every check. Per-day detail stays in core (each day's run report,
  `vyreon-core steps <job>`, `vyreon-core timings`, `core.provider_refusal`).
- 16:28 MT: 22 days published, 0 engine faults, 12 blocks running, no restart; no refusal since 14:44. THREE
  blocks are idle waiting on long reference checks (2024-09-18's for 170 min; core raised NO_PROGRESS for job
  30093): catalogued, HIGH; not acted on (the checks are working, deadline 600 min).
- Found 2026-11-28 06:24 MT (`core.job` 30104–30108, `vyreon.day_revision`): five ordered passes DID run on this
  build, for 2024-07-09, -10, -11, -12 and -15. They were created Fri 11:11 MT and each ran when its day
  published, taking 1 to 4 min from the day's publication to the derived revision. How they came to be created
  (which command or automatic step, and why only these five) is not established; the days from 2024-07-16 are the
  17 the NOT_DERIVED problem lists. So the line below is true of every other session, not of these five.
- NOT scheduled yet: the ordered passes (the history layer). The plan says once, oldest-first, after the day layer;
  schedule them when the day layer is well under way (`vyreon-core schedule ordered-pass`), not before reading
  C-046/C-062.
- The two test days: canary 2025-12-10 on a51cb572 published 01:08 MT, 252 min, 0 engine faults, job DONE 02:40
  (reference check: 94,363 checked, 2,482 failed, 230 unexplained). Re-run of 2026-11-25 on c2397caa published
  02:12 MT, 281 min (download 127 with the 30-minute EXQ01 wait), revisions 261/262/263: 0 engine faults (168 on
  the afternoon's nightly), 9 provider absences; ACT01 back on all but 2 assets (LQD, MDY: MISSING_REQUIRED_INPUT
  on 2 roles); PCR-UQTR-I01 publishes (no date-gate reason left); `run-logs`, `act01-auto`, `exq01-events` KEPT;
  no provider refusal recorded. Its reference check was still running at 02:42 (job 30089, draining).
- 02:55 MT: the 11-25 re-run's pod was killed for memory 42 minutes into its reference check, after the day had
  published (job 30089 DONE; its reference report is missing; catalogued, HIGH; core alerted). Not acted on:
  nothing stops publishing. Watch the nightly's check (next session queued Sat 06:30 MT) and block pods.
- A release script still polls at its step 12 for those two pods' ends (detached; harmless; `--from 12` resumes
  it if it died).
- THE COORDINATOR'S JOB NOW: monitor, catalogue (`docs/PHASE1_CATALOG.md`), investigate only what stops days
  publishing or loses evidence. Fix anything real an audit finds. Owed: the checkpoint after 10–20 published days
  (logs kept, steps, census, refusals, draw latency); numeric stop conditions proposed after the first day; the
  stage-1 audit's leftover operations findings; at the first recorded provider refusal, read its body.

**STATE AT 2026-11-26 20:10 MT (read this first; it supersedes the NEXT steps further down where they differ).**
- The 11-25 nightly published 19:28 MT and ended 20:05 MT (job 30062; its ordered pass 30063 done). The queue is
  idle; nothing is released yet. Running build still 714e47a3; core image still the old one.
- Found on that nightly (catalog, "During the run"): the download idled 59.5 min for EXQ01's cutoff; ACT01 lost 28
  assets to HTTP 429 (140 engine faults) at 1,540–1,770 counted requests a minute. Intrinio's usage endpoint:
  one limit, 2,000 a fixed minute for the account. Which limit refused: NOT KNOWN.
- Owner's calls this evening: (1) EXQ01 wait shortened (done as 30 min, not the 15 approved: the largest capture
  on record took 18 min; engine D-564, fd319730, pushed on `worker`); (2) re-run 11-25 beside the canary to
  recover the 28 assets (trades data lasts to about 12-02); (3) budget: record refusals, back off, reserve for the
  nightly, every user under one count; ceiling stays 1,900 unless these give no answer (then 1,750 or 1,800);
  (4) instrumentation 1–5 before the run (logs kept, fuller per-day report, step history, fault census, provider
  ledger); (5) "stop reactively attacking issues": from here everything else is a catalog line.
- BUILT, NOT MERGED: core branch `core-s2` (worktree `/home/diggz/wt/core-s1`, head 7ae990c): C-095 (refusals
  recorded, ceiling halved 5 min after a 429, nightly reserve 600, PROVIDER_THROTTLED) and C-096 (`run-logs`
  archive category, `core.lease_step_log`, `vyreon-core faults` and `steps`); migrations 0067, 0068; suite 276
  passed, 0 failed. Engine: a fork works in `/home/diggz/wt/eng-s1` on E1 (refusal reports to core), E2 (ACT01
  retries up to 30 min on throttling), E3 (pack and upload `run-logs` per day), E4 (per-day report: download
  parts, idle waits, memory, refusals). Its report is not in yet: verify it before merging.
- NOT SOLVED: the droplet's capture (`vyreon-capture.timer`, 12:00 and 02:00 UTC; 12:00 ran 2 h 43 min on 11-26,
  ACT01 and EXQ01 included, paced by itself) uses the same key outside core's count. Core is ClusterIP: the
  droplet cannot draw from it. Options for the owner: retire the droplet's capture once the cluster's archive is
  verified to hold the same (recommended), or expose core. Catalogued; not worked.
- Owner, later the same evening: ceiling 1,800 (core-s2 2fe13a2: the droplet's capture cannot be counted); the
  droplet's capture stays through phase 1 (its purpose is the nightly's safety net; it held all 64 ACT01 assets
  for 11-25); the run's rules and criterion 0 are at the top of `docs/PHASE1_CATALOG.md` (authorized).
- `docs/SENSORS.md` (2026-11-26): the map of all 94 measurements (inputs, parents, withholding reasons, the
  11-25 census read against it). Read it before judging a sensor's withholding; add what the run teaches there.
- MERGED since: core-s2 into main (7e697a7); eng-s1 into `worker` (a51cb572, pushed; the coordinator reran the
  eight touched crates: 949 passed, 0 failed). Evidence: `~/claude-artifacts/cluster-scratch/evidence-a51cb572.json`.
  Owner: the self-reporting work (digest summary, per-day flags from thresholds) goes in BEFORE the canary, built
  while the worker image builds; core is rebuilt once at the end.
- **RELEASED 2026-11-26:** core image a0240d8 at 20:48 MT (C-086..C-096; ceiling 1,800); engine build
  `a51cb572+acffd2a117e3` at 20:52 MT (the old build withdrawn; daily build set). Backfill lane resumed 20:52 for
  two single-day blocks only: job 30086 = the CANARY on 2025-12-10, job 30087 = the re-run of 2026-11-25 behind it
  (core's gate runs one block at a time until a real day publishes clean; a parallel run would need the nightly's
  priority set by hand in SQL, not done). Nothing else is queued. Phase 1 is NOT queued.
- Audits of the evening batch started 20:53 MT (Codex and Gemini; scopes `core-evening.md`, `engine-evening.md` in
  `~/claude-artifacts/audits/scopes-2026-11-27/`; reports in `~/claude-artifacts/audits/2026-11-26-evening/`).
  Triage blockers only.
- **Engine build `c2397caa+3933cc2a6d95` released 21:26 MT (D-567: the date gate retired; verified by the
  coordinator: 755 passed, 0 failed; tape caches stay valid).** The hand-over moved the 11-25 re-run to it: job
  30089, running since about 21:28 beside the canary (job 30086 on a51cb572, draining: it finishes its day; the
  release script waits at its step 12 until then, background task). So: the canary proves a51cb572; 30089 is
  c2397caa's first real day and core's gate for it. Phase 1 is queued on c2397caa after 30089 publishes clean, with
  NOTHING held out (the gate is gone). Evidence `~/claude-artifacts/cluster-scratch/evidence-c2397caa.json`.
- **Owner's standing rule restated 2026-11-26 late: "fix anything you find in audits... I'd rather fix things asap
  than let them exist quietly."** The run's "catalogue, don't react" covers what the run shows about sensors, NOT
  audit findings. Triage of the evening batch: core 14 real of 17, engine 18 real of 20, no blocker. Two forks fix
  all of them: core branch `core-s4` (worktree `/home/diggz/wt/core-s1`, C-098), engine branch `eng-s3` (worktree
  `/home/diggz/wt/eng-s1`, D-568; operations only, no label may move). When verified: core ships at once; the
  engine ships as the build phase 1 is queued on (its first block under core's ramp is its real day). STILL OWED
  under the same rule: the stage-1 audit's catalogued real findings (catalog items 6–30): the operations ones
  next; the science ones need method versions and wait for the fix pass unless the owner says otherwise.
- **Core image 60bf58a released 22:08 MT (C-098, merged core-s4):** all 14 real core findings of the evening
  audit fixed plus the uncertain one (real); the coordinator reran the suite: 280 passed, 0 failed. Migration 0070
  (additive; truncates the withholding tally so the watch recounts). Two budget changes, both at findings: the
  nightly's reserve is on from its ask; a lease whose validity ended over 10 minutes ago no longer counts. Both
  running blocks kept their leases. Census after the fix on 11-25: 83 lines (61 before). Left, said by the fix:
  `core.budget_lease` is never pruned (deleting rows is the owner's call); `watch::FAULT` reads one body schema
  only, so an engine fault stated inside a company or FIELD body would not count for the stop rule or the gate
  (hypothesis; it touches the gate).
- **Engine fixes ready, NOT released (22:40 MT):** `worker` = 760a8760 (D-568: the evening audit's 18 real
  engine findings, operations only, no label moved; the coordinator reran the eight crates: 955 passed, 0 failed);
  image `vyreon-worker:760a8760` built; evidence `~/claude-artifacts/cluster-scratch/evidence-760a8760.json`.
  Core main also holds 48cafad (a republished day's logs accepted), not yet in a released image. RELEASE BOTH
  only after jobs 30086 and 30089 have ended (the c2397caa release script still waits at its step 12 for 30086 to
  finish its day; do not overlap releases). Then queue phase 1 on the 760a8760 build under the ramp.
- **CANARY PUBLISHED 2026-11-27 01:08 MT (job 30086, 2025-12-10, build a51cb572, one cold day): 252 min** =
  download 98, plan and surfaces 35, intents 30, intent B 89, publish 1. Base revision 259 (2,853 slots) and
  day-local 260 (1,105): 0 engine faults, 0 counted withheld; no restart; no provider refusal. `run-logs` KEPT
  (4.0 MB) and `act01-auto` KEPT; the run report carries the new fields (fill parts, cutoff wait 841 s, memory,
  refusals {}). Memory: the container's peak reads 12,291 MiB against a 12 GiB limit (the cgroup's figure counts
  file cache); the processes peaked at 6,268 MiB and the largest child at 7,002 MiB. Its reference check was
  running at 01:16. The engine can complete a block.
- Audit reports are in (21:24 MT; Codex and Gemini, core and engine); two fresh readers triage them into
  `~/claude-artifacts/audits/2026-11-26-evening/triage/`. D-567 and C-097 were NOT in the audited diffs.
- Proposals for the fix pass (not decided) and the cost of a single-sensor recompute are in
  `docs/PHASE1_CATALOG.md`.
- (superseded by the lines above) the date gate on replay quotes (D-411 gate 3, `QUOTE_UNVERIFIED_FROM`)
  "should not exist".** A fork prepares its retirement on engine branch `eng-s2` (worktree `/home/diggz/wt/eng-s1`,
  D-567), first establishing whether cached tapes before 2026-11-06 stay valid. Not merged. When phase 1 is
  queued, HOLD OUT 2026-11-06..2026-11-20 until that ships, so those sessions are computed once.
- **Core update released 20:57 MT: image d2c0bb2 (C-097, merged core-s3):** withholdings counted per revision,
  a "To watch" line for flagged days, the digest's Run lines, `vyreon-core timings`. The canary's lease (job
  30086, pod day-block-pzmdf-kfqkf, started 20:53) survived the restart. A waiter reports when 30086 ends.
- (done, superseded by the line above) core branch `core-s3` (worktree `/home/diggz/wt/core-s1`), a fork building C-097 (the tally of
  withholdings, per-day flags, the digest's Run block, `vyreon-core timings`); read-only over stored data; ships
  as a core update when verified.
- THEN: merge `core-s2` into main and eng-s1 into `worker`; build both images; core release, `backfill stop`, worker release with new evidence; canary on
  2025-12-10 and the 11-25 re-run in parallel, the Codex + Gemini audit of this batch beside them (blockers
  only); freeze; phase 1; the checkpoint after 10–20 days.
- **The handoff's "cluster runs only core plus the nightly" was false (proven, core log + core.recompute_plan):** at
  14:43 MT the billing token made spend known, which released C-079's held recompute (plan 7); at 14:44:48 it queued
  17 sessions as 8 full day blocks on 714e47a3 (2024-07-16..09-05, 2026-11-23..24). **Cancelled 2026-11-26 ~15:58 MT**
  (owner: "go ahead and cancel things"), jobs 30078..30085; the nightly (30062, pass 30063) untouched; pods gone in 90 s.
  C-079 plans once per build (plan 7 SCHEDULED, busy {}), so it does not re-queue them. core-s1's C-087/C-091 stop flag
  covers this ("no recompute is decided" while stopped). Into the audit brief as a real case of automations conflicting.
- **The owner's run model (2026-11-26, replaces plan steps 3–5 below where they differ):** the canary runs ON a golden
  day (one step, not two); once it passes the system is frozen, scaled through core, and computes all of phase 1 while
  monitored, recording ALL faults; at the end the catalog of faults is evaluated: fix all, then recompute selectively
  if possible, fully if necessary. Owner: "We keep getting trapped encountering and diagnosing small issues, random
  decisions" — so until the freeze only what blocks the canary is diagnosed; the rest is catalogued for the fix pass.
- **The run is a characterization run, not serial reactive debugging (owner, 2026-11-26):** one constant build over
  the whole domain; faults are data, recorded complete (reason, sensor, asset, date, queryable from the database:
  checked at the canary); criteria written before the catalog is read. **The coordinator's job during the run
  (owner):** keep the run alive and the record complete; monitor progress; watch for anomalies; search for ways to
  improve. Scope (owner: both "are potentially scope expanders at this stage"): an anomaly means the run itself
  struggling (memory deaths, stalls, throttling, cost, days failing): assessed when seen. Everything else, science
  oddities and improvement ideas included, is one catalog line with its evidence, not worked during the run. Only
  blocker and operations fixes ship mid-run.
- **UUP dividend: RESOLVED, not a blocker (verified in the code, 2026-11-26 ~16:40 MT).** The owner remembered
  right: D-154 (2026-11-02) projects a commodity pool's (DBA, UUP) distribution from the trailing fed funds rate;
  production applies it (`h5_surface.rs` `pool_income_of`). The test fixture was fed last year's 1.3171 raw, a path
  production never takes: the 44-day expiry's WITHHOLDING is that artifact. Under D-154 (0.9657 vs 0.9269 paid) no
  expiry is outside its bounds and IV30 is 0.0723 all the same (engine test, eng-s1 2026-11-26; my first note here
  said ≈0.064 from D-154's old record: wrong for this build). 0.0723 is the upper part of what UUP's thin book allows
  (5.8–7.3% by an at-the-money calculation): quote uncertainty, CATALOG. Where D-154 misses badly (2024-07-11: 1.84 projected, 1.3171 paid) D-560
  withholds the expiry: a stated absence, not a wrong number. CATALOG (fix pass): replace D-153/D-154's projections
  with the standard forward from put–call parity (Cboe VIX methodology §3(a)(ii)); UUP's thin book needs a fallback.
- **Reference-suite rulings census** (`~/claude-artifacts/checks/refsuite-rulings-standards-2026-11-26.md`, agent,
  spot-checked): 63 rulings: STANDARD 10, OWN 44, RETIRE 6, UNSURE 2, REPLACE 1 (#7 SAR: TA-Lib reverses on a touch, the
  engine on a cross; changes published SAR values: CATALOG). Hammer family (#6, #39) vs TA-Lib's CDLHAMMER: CATALOG
  (TA-Lib wholesale or our own, on the population). Owner, 2026-11-26: the amassed owner rulings are likely overfit to
  sparse, picked dates; the populated phase-1 dataset is where sensors get examined and reworked.
- **STEP 1 (integration) DONE 2026-11-26 16:45 MT; STEP 2 (the audit) RUNNING:**
  - **Engine candidate = branch `eng-s1`** (worktree `/home/diggz/wt/eng-s1`, head 42a5d083): eng-s1b merged in
    (a4c0e83e); M0 pins renewed (D-562); D-561's test corrected (it failed on eng-s1b as committed: the handoff's
    "verified" was wrong there; the engine code was right); UUP production case tested; six unused suite rulings retired
    (57 left). Tests: the ten changed crates pass with the retained fixtures copied to `workspace/fixtures` (git-ignored,
    489 MB, from `/home/diggz/vyreon-engine/workspace/fixtures`; without them 47 tests fail "missing fixture"), except
    `the_m0_lockstep_leaves_a_bounded_worker_room` (laptop-dependent, not in the diff: CATALOG). Reference pytest: 292
    pass, 107 SKIPPED for captures not on the laptop (golden dumps): not run anywhere for this batch; the canary's
    in-worker reference check is what covers them.
  - **Core candidate = branch `core-s1`** (head 4371627): main merged in (main's 20-core.yaml settings kept); C-093:
    every progress post ends with "Running now: …", and the morning database line keeps "% of readings measured"
    (owner). Unit tests and the 208 database tests pass.
  - **D-560 census:** running on 7 saved sessions → `~/claude-artifacts/checks/forward-bounds-census-2026-11-26.txt`.
  - **Audit:** Codex and Gemini on both candidates, scopes `~/claude-artifacts/audits/scopes-2026-11-27/`, reports
    `~/claude-artifacts/audits/2026-11-26-stage1/`. Then triage, fix every real finding, release core (after a nightly
    has published), the canary on golden day 2025-12-10.
  - **Golden days (hypothesis, from engine DECISIONS):** 2025-12-10, 2026-01-14, 2026-09-18, 2026-10-16.
  - **AUDIT DONE, TRIAGED, BLOCKERS FIXED (2026-11-26 17:30 MT):** reports and triage in
    `~/claude-artifacts/audits/2026-11-26-stage1/`. Engine (Codex; Gemini gave no report: headless permission denial):
    22 findings, 1 blocker fixed (VS-IVS01 receipts stamped H5-SURFACE@25, now the stage's own label; D-563, head
    c60e2085). Core (Codex + Gemini): 30 findings, 8 blockers at 5 sites fixed (C-094, head 95e8912; suite 273 ok):
    recompute OFF for the run; the fault count (never NULL, direct owners read, C-090's "every asset" amended); counted
    every minute; apply-core.sh refuses a lagging daily build; "Running now" counts every kind. Everything else:
    **`docs/PHASE1_CATALOG.md`** (30 items; the one file the owner opens).
  - **NEXT:** (1) the owner's two calls below; (2) merge core-s1 → main and eng-s1 → the release line; (3)
    `release.sh --core` once no nightly is mid-run, then `vyreon-core backfill stop "<reason>"`; (4) the engine through
    release.sh; (5) the canary: a one-day block on 2025-12-10, read as the plan of record says, plus the nightly on
    the new build and the record check (every fault, disagreement and absence queryable from the database); (6) freeze,
    schedule newest-first, the ramp. VYREON_CORE_RECOMPUTE goes back "on" only after the run.
  - **Owner's two calls, decided and landed (C-094 amended, 2026-11-26 ~17:40 MT):** the canary test is "can the
    engine execute a small block": the gate holds the ramp only for a measurement faulted on every asset (the
    coordinator's one addition); a local fault is a catalog line. No count of failed days pauses the queue ("the
    nightly should never get paused").
  - **Merged and built (17:50 MT):** core main 64fbe76 (image `vyreon-core:64fbe76`, release dry-run clean); engine
    release line `worker` = c60e2085 (image `vyreon-worker:c60e2085`); evidence for the worker release:
    `~/claude-artifacts/cluster-scratch/evidence-c60e2085.json`. WAITING for the 11-25 nightly and its pass to finish
    before `release.sh --core 64fbe76` (C-088 is in this release).
  - **The canary is also the benchmark (owner):** time by stage, memory peak by stage against 12 GiB (D-515's first
    real block), requests and throttling, where the node idles (download concurrency). Improvements found go to the
    catalog with their number; an operations-only setting may be proposed before scaling.
- **Owner request:** live jobs in the progress channel: done on core-s1 (C-093).

## Resume here (HANDOFF, 2026-11-26 ~15:35 MT)
0. **HANDOFF STATE (15:35 MT):**
   - every agent of the old session is finished or stopped; all four branches are clean and committed; nothing
     is merged, released or audited;
   - the cluster runs only core plus the 11-25 nightly (lease since 13:45 MT; publish expected ~17:30–18:00);
   - spend is MEASURED since 14:43 MT ($351.57 month-to-date at DigitalOcean's 2026-11-25 22:14 MT refresh).
   **First:** the alignment talk with the owner. Then the plan of record's steps 1 (integration), 2 (Codex audit),
   3 (remediate, freeze through the canary) and the run.
1. **The backfill is stopped** (2026-11-26 13:37 MT): 606 phase-1 jobs cancelled; reference re-runs off
   (`VYREON_CORE_REFCHECK_RERUN_HOURS` "off", 878bd78). Stopping = cancel the jobs AND turn off every automation
   that queues backfill-type work (C-073 re-runs; C-079 recompute is held while spend is unknown).
2. **The nightly runs on** (perishable prints and trades: the daily run never stops). The nightly 2026-11-25 is
   recomputing after the 13:30 MT core outage (20-core.yaml applied raw, ~20 min down).
3. **Running:** workers on build `714e47a3+5b2beb37d0c5` (engine frozen line + two blocker hotfixes; KEDA and
   `VYREON_CORE_DAILY_BUILD` on it); core image 8f4fdc9.
4. **Published phase-1 days so far are not keepable:** 2024-07-09 and 07-10 (90e2f228, INT-IVSC01 fault) and
   2024-07-11 (714e47a3, 3 engine faults, below). Production recomputes all 597 days.
5. **Stage-1 branches: done, committed, NOT merged, NOT audited** (verified 2026-11-26 ~15:00 MT):
   - **engine `eng-s1`** (worktree `/home/diggz/wt/eng-s1`, 17 commits on worker 714e47a3, D-515..D-522, clean):
     - D-515: the next day's download waits for the day's plan and for its measured peak + 2 GiB (never fails a day);
       not yet run on a real block;
     - D-516: the reference-check join is the check's own CPU (no change);
     - D-517: VS-O04 withholds a change an unknown-OI expiry may touch (PROVIDER_OI_INCOMPLETE). NVDA's real
       2024-06-21 gives 0.0329786 with 08-02 (the reference: 0.032980); the old 0.0321638 = the silent drop. The
       cause of the unknown OI stays OPEN;
     - D-518: the reference-suite fixes, and every family reported ran / not ran (a family that cannot run fails);
     - D-519: the intent logs label stated absences;
     - D-520: company capture slows together under 429s (open: a long capture can still trip NO_PROGRESS);
     - D-521: IVSC's end-to-end test is OPEN (needs a day store of the new build: day blocks hard-code
       keep_transient false; copy the run dir off the pod before cleanup);
     - D-522: compact content address for M0's source sets (D-500 item 5).
     Published values change only for VS-O04 (`@4-D-517`, curve `@2-D-517`); the rest is bytes only (M0 stream root,
     the task keys). Targeted Rust suites and the reference pytest pass (`test_field_own_iv` already failed on
     worker: missing fixture).
   - **engine `eng-s1b`** (worktree `/home/diggz/wt/eng-s1b`, 6 commits 548aaf72..50383cc2, D-560/D-561, clean):
     - **D-560:**
       - SURFACE-FORWARD is judged by Hull's no-arbitrage bounds on the forward from the expiry's own quotes (Hull
         ch. 11: the European bounds with dividends, American put–call parity with dividends, at tradeable
         bid/ask). This replaces the owner rulings of 10-30, 10-31, 11-04 and 11-20.
       - The 7 offsets of 2024-07-11 are inside the bounds (re-fetched 19:55 snapshots; tightest VGK 68.51 in
         [68.07, 70.45]); XLP 15d and UUP 190d are outside. Of 07-11's 71 forward failures, 66 are inside; URA 50d
         and XLY 1d (ruled before) now count as engine errors.
       - ENGINE (H5-SURFACE@26): an expiry whose forward lies outside its own quotes' bounds is withheld, and none
         borrows its carry.
       - **DEVIATION FROM THE OWNER'S DECISION (3), for the owner:** the cross-expiry (neighbour) carry test was
         NOT adopted. At the Treasury rate it fired on 10–16% of expiries. The bounds test fired on ~0.5–1% of
         ~1,070 expiries per session in saved runs, mostly 1–3-day expiries, 1–10 bp of spot. The census on this
         build's refit goes to `~/claude-artifacts/checks/forward-bounds-census-2026-11-26.txt` (pending).
       - **The census was STOPPED UNFINISHED at the handoff.** Re-run it as evidence for D-560 before the audit
         closes: on eng-s1b, `cargo run --release -p h3-iv-surface --example forward_bounds_census -- <saved run
         dirs>` (it was run on `~/claude-artifacts/vyreon-b0-scratch-2026-10-25/fwd/<date>/<fit>`, e.g.
         2025-12-10/median-all and 2026-03-20/huber-all), with the output to
         `~/claude-artifacts/checks/forward-bounds-census-<date>.txt`.
       - D-153 is kept. Values change: σ, Greeks and fits on withheld expiries. **UUP 2025-12-03 IV30 0.0577 →
         0.0723** (its 44-day expiry withheld).
       - **The owner CONFIRMED the bounds test in place of the neighbour test (2026-11-26: "standards are way better
         than me").**
       - **UUP check (Intrinio data, 2026-11-26):**
         - realized vol to 12-03: 4.1–4.2% (20/30 sessions);
         - Intrinio's ATM IV on the clean 16-day expiry (12-19, no dividend inside): 6.68% call, 6.11% put;
         - the 2025 distribution had its ex-date on 12-22 at $0.9269, after $1.3171 in 2024; every expiry after
           12-22 holds it;
         - Intrinio's own IV on the 44-day expiry breaks (call 5.65%, put 28.7%).
         CORRECTION (the owner caught it): IV30 is a constant-maturity variance interpolation between the expiries
         around 30 days (the Cboe VIX method). The 16-day IV is only its lower bracket, so these references do NOT
         decide between 5.77% and 7.23%. The open question is the upper bracket: every expiry after the 12-22
         ex-date rests on the treatment of that dividend (D-138 projects from the trailing year, $1.32, against
         $0.93 actually paid). The market standard is the dividend or forward implied by put–call parity.
         OPEN for the next session: check UUP's straddling expiries and whether the projection should give way to
         the parity-implied dividend for funds whose payout follows rates. Owner: find standards or references for
         checks like this.
     - **D-561:** when T's own chain is empty, EMPTY_DELIVERED_POPULATION comes first in VS-O04 and VS-L01 (engine
       and suite; VS-O02's suite path is on eng-s1).
     - Suite-rulings pass: only ADX moved (Wilder's as the primary reading); the rest is NOT STARTED.
     - Open: renew the M0 labels (pin test not run); two h3-asset tests and one suite test fail on missing
       fixtures (also on worker).
   - **Merging eng-s1 with eng-s1b:** conflicts are expected in tools/reference (`__main__.py`, `published.py`,
     `opt_oi.py`, `chain_ctx.py`, `rulings.py`, `surface/run.py`, `surface/streams.py`), the METHOD bump in
     `stage.rs`, the M0 labels, and the end of docs/DECISIONS.md.
   - **core `core-s1`** (worktree `/home/diggz/wt/core-s1`, 8 commits 566fef9..cff9b9e, migrations 0064–0066,
     C-086..C-092, clean; full suite with Postgres 272 ok):
     - the ramp + one-real-day gate (C-086; small gap: a Running pod not yet claimed can let a second block start);
     - `vyreon-core backfill stop|resume|status` (C-087; a DB flag, no restart);
     - lease grace at core start (C-088);
     - a nightly restart alerts (C-089 → C-091; nightly deadline alerts start at VYREON_CORE_DEADLINES_FROM
       2026-11-30, the owner's call);
     - the stop rule for systematic breakage only, ceiling 12 (C-090);
     - the plain channels (C-091): progress = blocks started/finished + the database line; alerts = 🔴 needs you /
       ⚠️ problem / ✅ resolved, worker deaths with their pod cause, NO_PROGRESS after 30 min, a deliberate stop
       silent, a 3-line digest;
     - slow start after budget pressure (C-092);
     - `deploy/k8s/ops/apply-core.sh`; release.sh step 11 fixed.
     Deploy risks:
     - 0064 drops core.revision_tally, so builds count as unproven until the watch recounts;
     - 0064/0065 run under a 10 s lock_timeout (retry if busy);
     - the Role change (get pods) needs 20-core.yaml applied;
     - when merging, KEEP main's 20-core.yaml: ceiling 750, re-runs "off" (back to 6 once `backfill stop` is used).
     Deploy order: merge → audit → `release.sh --core` → `vyreon-core backfill stop "<reason>"` if it should stay
     stopped → workers via release.sh → the canary → schedule.
   - **Laptop disk:** delete `/home/diggz/wt/target-<branch>` when each branch merges (core-s1 8.6G, eng-s1 17G,
     eng-s1b 8.2G). target-worker is 96G: the image build cache; check whether it can be trimmed.
6. **Spend ceiling $750** is committed in 20-core.yaml (3b5094f) and goes live with the next core release (a core
   restart now could cost the running nightly its lease before C-088 ships). The owner's read-only billing token is
   in place (secret `vyreon-core-do-billing`): spend is measured by core's hourly read.
7. **Next (plan of record, owner ~16:00 MT):** fix everything (target 11-27/28) → one focused Codex audit (Codex
   only) → remediate → freeze on a passed canary → hand off to a fresh session, which runs the release and the build.
Restart the loop with `/loop follow docs/MILESTONES.md under the rules in AGENTS.md`. Cluster commands:
`KUBECONFIG=~/claude-artifacts/cluster-scratch/kubeconfig` (refreshed by `deploy/k8s/ops/kubeconfig.sh`, also run by
release.sh and health.sh), `VYREON_DOKS_CLUSTER=5f56e6e0-1c74-4dc9-bfdb-962ee55ebff3`. State: `deploy/k8s/ops/health.sh`,
`vyreon-core queue`, `vyreon-core evidence list|show`, `vyreon-core refcheck list|show`, `core.queue_control` when pods
find nothing to claim. Releases only through `deploy/k8s/ops/release.sh` (`--core` for core; never apply 20-core.yaml raw).

## PLAN OF RECORD: fix everything, audit, freeze, hand off (owner, 2026-11-26 ~16:00 MT)
1. **Fix everything we can** (target 11-27/28): the three stage-1 workstreams (eng-s1, eng-s1b, core-s1) plus:
   - the compact provenance form (D-500 item 5, owner-approved);
   - the standard-first pass over the reference suite's rulings;
   - an attempt to reproduce VS-O04's root cause;
   - the tape scaler oscillation.
   Deferred, with reasons: IVSC's end-to-end test (the canary covers it; refreeze from a gate day's store), the
   day-hold decision (needs the run's data), and the rule census beyond blocking rules (needs the population).
2. **One Codex audit** (Gemini's quota will not return in time; owner: Codex only), focused three ways:
   - the fault lines the engine surfaced, as CLASSES, asking where else each pattern exists: writer/reader identity
     mismatch (M0 staged file, IVSC digest), silent drops of unknown data (VS-O04), resource admission (memory),
     forward/carry consistency, checker blind spots (a family not running), automations conflicting (re-runs vs
     cancellations, scaler vs budget, leases vs a core outage);
   - whether each fix actually fixed its fault, verified on the real cases;
   - a normal review of everything changed.
3. **Remediate every real finding, then FREEZE.** The freeze candidate passes the canary: one real day on one node,
   0 engine faults, every reference disagreement classified, memory peak within the node.
   **The canary is a one-day block** (owner, 2026-11-26: "couldn't we just queue a tiny block"), with no dedicated
   job:
   1. Release the candidate.
   2. `vyreon-core schedule day-block FREE_REPORT_64 <build> <day> <day> 1` (a large recent day). Core's gate
      (C-086) runs a new build's backfill alone until it has published a real day with 0 ENGINE_FAULT.
   3. Then read:
      - the reference check, every disagreement classified;
      - every check family ran (D-518);
      - the memory peak against the node;
      - the time taken;
      - the nightly on the new build.
   4. Pass: schedule the run, and the ramp takes over. Fail: fix it and queue another one-day block.
   About 4 h and $0.70 per canary.
4. **HANDOFF to a fresh session** (owner, 2026-11-26, moved earlier). It happens once the four agents now running have
   reported (eng-s1, eng-s1b, core-s1, the rulings census) and their results are verified and written here. The new
   session does steps 1's integration, 2 and 3 (integration, the Codex audit, remediation, the freeze), then the
   canary, the release and the build (the run design below), then one fix pass and selective recompute. Reporter starts
   once the run is smooth. It begins with the alignment talk; the owner: "a fresh Claude's independence will be
   valuable there anyway".
5. **Golden days become known-good** (owner, 2026-11-26: "seems to be a decent plan"):
   - Reuse the engine's golden dates: 2025-12-10 (main), the retained 2026-09-18, and the other two of the "four golden
     days" (to identify from the engine records; not yet pinned).
   - They prove "code moved without changing a number", NOT correctness (engine DECISIONS: "The golden runs show code
     moved without changing a number, not that the numbers are right").
   - After the fix pass: compute them on the frozen build and review them (reference check fully classified, the
     Cboe/VIX cross-check, the owner's reasonableness eyeball). Explain every difference from the old golden values
     by a named method change.
   - Then they are the canary's comparison baseline.
6. **Pilot checkpoint:** once the most recent ~300 sessions are done and checked (newest-first), the pilot is safe;
   older history is valuable but off the January critical path.

### Run design after the freeze: brute-force phase 1, then one large fix pass (owner, 2026-11-26)
Why the previous 3–4 days produced 3 keepable days: builds released to many nodes without one real day end to end,
twice; scaling before fit was measured (memory per node, request contention); every fix restarted the run; hand
operation errors. The "prove small" stages (owner, 2026-11-26 earlier the same day) are superseded: Stage 2 (the
proving set) is dropped; Stage 1 is the "fix everything" step above.
1. Land (now step 1 of the plan above):
   - the memory gate (D-515, done);
   - core's ramp, the one-real-day gate, the lease grace and the safe scripts;
   - the VS-O04 withhold fix;
   - the reference-suite fixes;
   - one Codex audit of that batch.
   Forward/carry rule, VS-O04 root cause and other science continue on branches.
2. Release:
   - core holds one block until a real day publishes with 0 ENGINE_FAULT, then ramps as the budget allows (ceiling
     8 → 12);
   - blocks run newest-first: the most recent ~252 sessions first, then the rest;
   - every ordered pass is scheduled ONCE up front and runs oldest-first after the backfill (C-046: the 12 history
     sensors read every earlier day; passes run early are redone and differ).
3. All 597 days. Faults and reference disagreements are collected and classified as they come.
4. One large fix pass over the population (also the census data), one release, recompute only what changed, by
   method version.
**Stop rule redefined:** stop only for systematic breakage: days failing to publish, or one fault hitting a whole
measurement on every asset (as M0 and IVSC did). Local value errors are recorded for the fix pass. C-084's
counting is amended to match (C-090 on core-s1).
**Owner rule (2026-11-26):** a fault found in production does not restart the run. A fix that changes no published
value recomputes nothing; one that does recomputes only the affected measurements, by method version (C-079),
after the run.
VS-O04 must be understood: the investigation runs in parallel; if it is not resolved before release, O04 is
recomputed in the fix pass.
Owner wants to discuss further what to do better (2026-11-26).

**Stage-1 items still in flight** (from the superseded "prove small" list; done when each lands with its commit):
1. Memory (owner: the engine fix, not bigger nodes): the next day's download starts only when memory allows (D-515).
   Measure one large 2025 day stage by stage on one node.
2. A real day end to end is a release requirement: release.sh refuses a build that has not published one real day.
   IVSC's ignored end-to-end test (D-400 stale fixture) is deferred: the canary covers it; refreeze its fixture from a
   gate day's store.
3. Core: the ramp (owner-approved: one new backfill block per 20 min at most, none while the budget waited in the
   last 5 min); `backfill stop|resume` (turns re-runs off and on); safe core-setting changes. All on core-s1.
4. Small items: company-capture 429 spacing (P-30); the intent log's "fault:" wording for stated absences (IBIT/ETHA
   empty chains read as faults); release.sh step-11 false stop (core-s1); the idle wait for the previous day's
   reference check (D-516: unchanged, D-405 stands).
5. One Codex audit of the stage-1 changes (Codex only, owner 2026-11-26; focus in the plan above, step 2); every
   real finding fixed.

**2024-07-11 classified (714e47a3; 20 unexplained reference disagreements: SURFACE-FORWARD 9, VS-O02 6,
SURFACE-SIGMA 2, SURFACE-INPUTS 1, FIELD-SUP01 1, VS-O04 1):** 3 engine faults change published values, so the
day is BAD.
- **VS-O04 NVDA:** the 2024-06-21 reference point is wrong (0.032165 vs 0.032980; the reference is right under
  D-028). The CAUSE IS UNPROVEN: "no 06-12 quote file" was retracted as inference (history_start is 06-11); D-475's
  E3 named two candidates (a 06-13 requote without 06-12's file, D-443; or a curve computed before D-441). The
  "block-start determinism break" was overstated to the owner. Fix in hand: an expiry with unknown OI is withheld,
  never silently dropped (D-517); the cause is to be reproduced or left open.
- **UUP 190d forward:** the forward implies a projected distribution too high; both rights are uninvertible.
- **XLP 15d forward:** leaning engine; the implied carry is inconsistent with the neighbouring expiries.
- **Suite faults:** FIELD-SUP01 hash newline, VS-O02 reason order, SURFACE-SIGMA D-145 not mirrored,
  SURFACE-INPUTS dead rows (P-34), a NameError in surface/streams.py that skipped the M0 stream checks (D-518).
- **By design under the 10-30 σ rule:** 7 forward offsets inside the no-arbitrage band.
**Owner decisions 2026-11-26 on it:**
- (1) **the 10-30 rule's "a systematic offset is never judged in price" is replaced by the standard test**: a forward
  inside the put–call parity no-arbitrage band of the quotes (Hull) is quote uncertainty, recorded and labelled;
  outside the band it is an engine error; inside the band but inconsistent with the neighbouring expiries is rule
  (3)'s job. Verify the 7 are inside the band before it lands.
- (2) one withholding reason when T's own chain is empty: EMPTY_DELIVERED_POPULATION, the fact about T first,
  across O02/O04/L01;
- (3) one GENERAL rule for implausible forwards instead of UUP/XLP special cases: an expiry whose implied carry is
  inconsistent with its neighbours beyond what the quotes allow is withheld; drafted, then tested before adoption
  (overfit check).
- Owner direction: prefer standard, generalizable tests over our own decisions wherever possible (AGENTS priorities 2
  and 4). **Agreed split (the census split):** before the freeze, standard-first for every rule that BLOCKS (causes an
  engine fault, an unexplained reference failure, or a withholding on clean inputs in stage 1's evidence); each is
  replaced by a published standard or one general rule, or justified, before the freeze. The reference suite's
  rulings get the same pass now (they change no published value). Every other engine rule waits for the census on
  the phase-1 population (owner 2026-11-16/11-21).
**Audit rulings settled (owner, 2026-11-26, recommendations accepted):** daily 11: one gamma figure (D-101), the
1-year book not scored separately; D-500 item 5: source ordinals recorded in a compact content-addressed form
(hash), for the fix pass; VS-L01's code name and company #17 (D-304): left to the census.

**Seven ways to improve the odds, all adopted (owner, 2026-11-26):**
1. **Secure the pilot first:** production runs newest-first, so the ~63 sessions before 2027-01-08 plus the nightlies
   are safe early. First check what ORDERED_PASS carries forward across days (recent-first may force redoing passes).
2. **Reporter:** starts ONCE the database is progressing smoothly and is just monitoring (owner: "we can't do
   anything without a database"); not before.
3. **The nightly is pilot-critical:** the freeze canary includes a nightly run on the new build; a missed nightly
   alerts on its own.
4. **The checker must check everything:** the canary and the gate require every reference-check family to have run
   (an import error silently skipped the M0 checks on 07-11).
5. **Small releases:** each batch merged into the frozen line has published one real day first (~$0.50), not only at
   release.
6. **Random days, not only hand-picked hard ones:** covered by the full run. The brute-force pass is the population;
   the proving set was dropped.
7. **Operation:** steps that went wrong by hand become scripts and gates (stage 1 item 3); every scale-up step waits
   for its check to pass.
Estimates, unproven: 2.2 h/day/block was measured on
2024 days, one block alone.

## Steps to the MVP (the list shown to the owner on a status request; keep it current)
**DATED COMMITMENT: the paid pilot (live on www.vyreonlabs.net/early-access, read 2026-11-16).**
One customer has paid USD 10 for four weekly email issues on Sundays **2027-01-10, 01-17, 01-24,
01-31** (sales close 01-09; automatic full refund if issue 1 is not sent by 23:59 ET on 01-10;
refunds on request to 02-07). Promised: all 64 securities; readings flagged when far from the
security's typical level AND in the top or bottom tenth of its last 63 sessions (the engine's
`unusual.rs` WINDOW = 63, robust z: matches); "what could not be measured, and why"; data through
the latest session; no signals or forecasts. So by 2027-01-09 we need: (1) history back at least 63
sessions before 2027-01-08 (phase 1 covers it); (2) **daily go-forward runs** computing each new
session from the build onward (built and running since 2026-11-23, C-051: core queues T at 07:30 New York the
next day; KEDA's `day-block-daily` starts it (C-066); CronJob `daily` suspended as the fallback (C-068); deadline
alerts from `VYREON_CORE_DEADLINES_FROM` 2026-11-30); (3) Reporter producing the paid issue from core's API, sent
through beehiiv. Owner, 2026-11-16: the deadline will be met; target free reports live in December
2026. Intrinio's Startup tier is required for redistribution rights and covers free and paid reports
(bought 2026-11-26, below); ideally free and paid are ready together. Google Ads all-time
(prototype): 1.58M impressions, 156k clicks (CTR 9.88%).

**Sequence (owner, 2026-11-21): phase 1 → verify phase 1 → Reporter → web; then the launch decision
and the ads.** The paid issues go by email, so they depend on Reporter, not on the web rebuild. Reporter starts once
the database progresses smoothly (seven ways, 2).

Done:
1. [2026-11-13] Two-day cluster trial (6 h 57 min cold). [11-16] Engine ready for the build (audit rounds a–k).
2. [2026-11-17/18] Cluster verification days; two-week sample 2026-11-02..13 (10/10; Cboe, EDGAR, Tiingo, FRED).
3. [2026-11-19] Bulk option files equal the API; tape caches verified by sample (C-059).
4. [2026-11-21] Three optimization passes; byte proof 11,870/11,874; D-412 close from the listing market's print.
5. [2026-11-22] Test block; KEDA + autoscaled pools; backups to Spaces with a passed restore drill (C-067).
6. [2026-11-23] Daily runs on; core watch, progress channel, plain-language Discord (C-065, C-072, release 4).
7. [2026-11-24/25] release.sh, REFCHECK re-runs, budget-aware KEDA, JOB_THRASHING, NO_PROGRESS (C-073); scoped
   recompute REPUBLISH (D-462/C-074); draining release (C-077); spend watch (C-078); recompute after a method change
   (C-079); fast reference check (D-466, ~12×).
8. [2026-11-25] Full-engine Codex audit remediated (D-469..D-514, ~116 findings); engine frozen at 93a04676; 880
   tapes re-captured under the new reader identity (C-083); two systematic faults caught by the one-day gate and
   hot-fixed (90e2f228, 714e47a3).

Next:
1. **Phase 1, the build** (plan of record above; range and depth C-060): stage 1, the gate, production newest-first
   under the ramp; ordered passes after; the whole-store gate. Range: 597 sessions, today's values final (warm-up
   chain 596 = flag-line pool 252 + z 63 + sma_30 29, then the 252-session weekly baseline).
2. **Verify phase 1** (owner, 2026-11-21: "We do phase 1, try to break it, fix what breaks, then move towards
   Reporter generating free+paid reports, and go live with web"; "worth proving this system as much as possible"):
   1. History reference check as the whole-store gate (`tools/reference`: z-scores, percentiles, one-year windows,
      flags, technicals, EVC baselines) and (d) the flag-agreement test (`tools/external/README.md` "Flag
      agreement": D-070's percentile half on the 14 Cboe pairs, both directions ≥ 0.80 with ≥ 10 index-extreme
      sessions; VXSLV/SLV and VXFXI/FXI on 63-session panels only). Bands are not moved after the fact.
   2. Continuity plots for the owner's eye: every sensor × asset (jumps the market did not make, breaks at
      code-change dates).
   3. Event walk around each real event: splits and reverse splits (NVDA, ETHA, AVGO), WMT's listing transfer
      2025-12-09, special dividends (EWJ 2025-12-16), every early close, the P-23 era boundary 2025-11-26, the
      bulk-file fault days (P-20, P-21), the 2025-12-05 ETF splits.
   4. Determinism at scale: random sessions re-run on another node, byte for byte with provenance masked
      (`~/claude-artifacts/checks/byteproof/compare.py`).
   5. Look-ahead audit: nothing uses data stamped after its cutoff (filings, macro releases, restated captures).
   6. Failure injection: kill a node mid-day; restart core during a publish.
   **Rule census (owner, 2026-11-16):** "with agent driven development on things of this scale ... we just sort of
   accumulate rules over time and the result can be an unfortunate agent stuck in spaghetti constraints". Never
   fired, fired only on its origin case, or contradicted by the population → retire or merge; prune agents'
   instructions the same way. No rule tracking is added ("feels like complexity that will cause problems ... If
   rules are being applied on some days but not others then we shouldnt be doing it"): the census reads the outputs
   (absences by reason, per sensor, asset, date; continuity). **Origin (owner, 2026-11-21):** every rule was
   written on ~40 sparse dates; the build is the first population. "We are building a measurement system, we don't
   want one-off handling, we want generalizable sensors." **The target (owner, 2026-11-22): patches fitted to a
   case, not standards applied to rare market events** (OCC adjustments, early closes, specials stay). Criteria
   written down before the census is read. The census covers the machinery too (job kinds, interacting
   automations; AGENTS). Event rules to census (owner, 2026-11-16): first chain, strike adjustment, OI outage, split
   and rename translation, receipt ratio, bank not-applicable, refused snapshot rows, capture failures, provider
   comparison classes; plus the whole-range missing-contract scan (raw deliveries, all 597 days). One sensor
   affected → single-sensor rebuild; many → full rebuild; decided from the evidence.
   **Coverage report (owner, 2026-11-23), with the census:** from the bodies: (1) overall share supported vs the
   80% goal; (2) by sensor × asset; (3) by reason class — structural, provider faults, ours; (4) monthly (era
   effects).
   **Phase-1 measurement report (owner, 2026-11-23), priority 1, 2, 3:** (1) right numbers (gate a's unexplained
   rate per day and sensor; gates b–d; determinism; no look-ahead); (2) continuity, incl. no step where the
   publishing build changed; (3) paid layer: flags per asset-week and week, calibration, persistence;
   (4) provider faults per 1,000 asset-sessions; (5) operations: time, cost, requests, retry rates, daily time to
   publish, alerts that needed a person.
   **Expected absences in range (not faults):** P-23 tape eras (before 2025-11-26 thin trades, before 2025-09-04 no
   quotes); replay quote widths withheld from 2026-11-06 (P-04, D-411); EVC01 quote-width flags from ~2026-06,
   volume flags from ~2026-09 only if P-04 is reconciled; QQQ 2026-09-28 (P-21); EMB histories short to ~2025-10
   (D-290); SAI-EXQ01 and M0-PRINT-SIZE EVENT_SOURCE_NOT_RETAINED before 2026-10-23; IBIT before 2024-11-19 and
   ETHA before its options (EMPTY_DELIVERED_POPULATION); ACT01 before the trade-count feed 2024-11-26.
   FIELD null-coordinates check (before launch, cheap): delivered FIELD-SUB01/SUP01/DIM01 and M0-FIELD-ANALYTICS
   slots have null reader coordinates — confirm intended (gap-scan finding 10).
3. **Reporter on the phase-1 database:** the gap scan's work list
   (`~/claude-artifacts/audits/2026-11-21-reporter-gap/reporter-gap.md`; 14 confirmed findings in today's Reporter,
   wrong numbers among them): 1. spec/profile reconciliation; 2. versioned ready-week manifests; 3. recorded core
   responses as fixtures (D-102); 4. projection outcomes failed / unavailable / private / stream-backed; 5. pinned
   week serving in core + paginated adapter; 6. free reports and FIELD panels repaired; 7. D-070 membership, 11
   rankings and peers in the engine; 8. weekly aggregates in the history pass; 9. 64 free editions + the paid email
   with the publish-time number check; 10. the headless job core calls; 11. core-owned delivery (web receipt,
   beehiiv); 12. qualification on two real weeks, then a full issue. Reporter branch `d397-own-analytics` (worktree
   `/home/diggz/wt/reporter-d397`, not pushed) is verified against a real revision before merging. Weekly comparison
   (owner, 2026-11-17): per sensor this week vs last on one chart, from derived-layer weekly aggregates. **Codex
   reviews the design too** (read-only): an accessibility pass and a utility pass. **Intrinio attribution required**
   on every page and issue that uses Intrinio data (below).
4. **Web rebuild:** the early-access site → the full free reports (64 per-asset pages), the paid product and its
   explainer. **Priority (owner, 2026-11-24): SEO/AEO first** — "ads is only a forcing function, it's not a natural
   draw"; requirements: (1) stable URLs, an evergreen page per asset plus dated weekly pages that accumulate; (2)
   answer-first summary, details and charts below; (3) schema.org structured data, sitemap each Sunday, fast static
   pages; (4) **derived measurements only — never raw Intrinio values or enough raw detail to reconstruct them**
   (Intrinio: "so long as our data isn't reproducible"); (5) explainer pages and reports linked both ways.
   **Maximum utility for human and machine readers; depth and accessibility (owner, 2026-11-23):** the product is
   the explained report, not the API; each measurement in one plain sentence, what is normal, what we cannot tell.
   **AEO and SEO both required** (owner, 2026-11-23); open: which AI crawlers to admit. **Page sketch (owner
   endorsed, 2026-11-23):** (1) summary: the week's 3-5 most notable readings by the flags' own scoring; (2)
   sections by theme, each opening with one plain sentence and one chart; (3) full detail incl. stated absences,
   with the structured data. Verification page (owner, 2026-11-18): each external check, its band, latest result,
   failures and fixes, plain voice (no "accurate", "audited", "institutional-grade"). Naming Cboe's indices waits on
   Cboe's permission reply (sent 2026-11-18); check Tiingo's and OCC's naming terms. Design inputs:
   `vyreon-reporter/docs/publication-design/`, `market-research/VYREON_PAID_CROSS_ASSET_PRODUCT_CONCEPT.md`,
   `~/claude-artifacts/website-plan-2026-10-12.md`, `~/claude-artifacts/literature/SYNTHESIS-*.md`.
5. **Launch decision:** proven unattended operation first (owner, 2026-11-22: explicit go-live
   criterion; the owner may be mobilized to field work for weeks): two weeks of daily runs, history
   passes and the Sunday issue with nobody touching the system, judged afterwards from the digest and
   the log; and the owner can restart the system after a bad day from the repository's runbooks, `health.sh`
   and Discord alone, with no agent (owner, 2026-11-22: not "the only one who knows how to run the system").
   Then security before launch, Reporter's output live on the web; then the ads (standing rulings).
   After it, or when wanted: deeper history (old snapshot layouts readable since D-251..D-254; the
   2023-01-18..03-07 gap needs a ruling), the FIELD campaign, social posting, CI.
   **Post-launch (owner, 2026-11-24):** automated social teasers of the week's top 1-3 unusualness readings (the top
   of the paid report's own ranking; deterministic templates; names that a flag fired and links to the report; never
   Intrinio data reposted). **Month-6 review (owner, 2026-11-24):** judged by trend on raw GA4 against the pre-launch
   baseline (~29 users/week): organic search growing 3 months running; readers returning; paid retained past months
   1-2; free → paid conversion; thresholds from observed months. First look early May 2027, before Intrinio's price
   doubles (~2027-05-24). Roadmap after launch (owner): repurposing research after phase 1, full history, then the
   forward-looking research and the no-signal line as PRODUCT_INTENT.md states them.

## Decisions waiting on the owner
- **Rulings census** (`~/claude-artifacts/rulings-census-2026-11-26.md`; 71 rulings: retire 24, engineering 24,
  intent 9, standard 8, keep 6, unsure 7).
  - The owner's five decisions are applied (8d0b685).
  - Next session, mechanical: retire the pure duplicates; turn the engineering items into C-numbers (11 already
    have one).
  - For the owner's glance: the 9 intent items (they go into PRODUCT_INTENT; the engine's copy is canonical, so sync
    both), the 8 standard items (4 citations were named from memory: Fowler "Two Hats", SRE book chapters, Saltzer &
    Schroeder, IEEE 1028; verify them before citing) and the 7 unsure.
- **Intrinio Startup — bought (owner, 2026-11-26 afternoon).** Intrinio moved the account to Startup effective
  2026-12-01 after our bug reports opened with untrue "commercial customer" wording (drafted on our side); since then
  bug reports state the data fault only — no company, customer or license wording — and the owner approves the text.
  Intrinio confirmed the deal by email as a 50% refund of the first six months' payments. The order form: 1-year
  term, auto-renews; $333/mo ×6, $666/mo ×6, then $999 (+5%/yr from month 25); billed quarterly in advance; +10%
  without auto-pay; 2,000 calls/min (unchanged: the plan's times stand); 3 websockets. Display to individual end
  users; commercializing the app is allowed when demand is not primarily the data and there is substantial
  analysis. **ATTRIBUTION REQUIRED: on any page where Intrinio data is used, the Intrinio logo or a reference with a
  follow backlink to intrinio.com** → Reporter, the web, the email issues and probably the social teasers.
  Advised the owner: get the refund into the order form or a signed addendum; ask whether email/social count as
  "the application", and about firms/institutions; ask about per-endpoint limits under 2,000/min (P-30). Email sent;
  replies delayed by the US holiday. Entitlements after the upgrade (access codes, 2026-11-26): all 16 of 2026-09-29
  kept, plus api, edi_otcb, historical_tick_otc_delayed. Also to ask Intrinio: an EEM 2023-12-20 dividend backfill and
  the by-day dividend/adjustment feeds (P-22).
  Drafted for the owner, 2026-11-26:
  - is there a concurrent, per-second or per-endpoint limit behind the 429s below 2,000/min? The owner suspects
    simultaneous requests;
  - can the REST limit be raised temporarily for the backfill? (Intrinio's FAQ: raised case by case.)
- The 7 forward offsets inside the parity band: verify before rule (1) lands; rule (3) drafted and tested first.
- RSS watchdog in place of the 2 GiB per-worker address-space cap (D-007; asked 2026-11-20).
- D-404's consecutive-gap limit (WMO uses 5 days): not ruled.
- D-495: surfaces before 2023-01-03 withheld until the calendar holds 2019 (outside phase 1).
- Optimization items that change an integrity rule or a diagnostic (Q01 source identity in history endpoints; the
  fill's A/B diagnostic; the prefetch nice switch). Low priority.
- Finer D-192 tenor containment (keep a one-year surface without its long expiries): an option, not done.
- Whether an absent dividend currency on a US listing may be read as USD (2026-11-16).
- The production droplet's 2025-12-10 store (177 withheld activity slots): rebind and republish on the owner's word;
  the droplet's retirement once the cluster's daily runs keep the perishable prints and its database is carried in.
- Deeper history: the 2023-01-18..03-07 snapshot gap. EEM 2023-12-20 (P-22): accepted and stated; the
  coverage-start rule (total-return windows before an asset's dividend coverage start are unknown basis) goes to the
  census.
- The day hold (10% of slots lacking a provider input): whether a held day publishes its good assets with the rest
  stated, judged on the build's held days.
- A tape-verify DIFFER (C-059), if one appears: recheck vs replace.
- After phase 1: every superseded old-build revision deleted in one cleanup (owner, 2026-11-22: "i would literally
  overwrite them or delete them"; preview first).

## Standing owner rulings that still govern the work
- **Spend:** ceiling **$500 in total** (2026-11-21, for phase 1 and its reruns; was $400);
  running total kept here, owner warned before a step passes it; billing alert declined
  (2026-11-21: "we will be watching"). Standing infrastructure is just "on". *[Retired 2026-11-26 (owner, census
  #6/#7): "worker nodes and test droplets … without asking"; slots scale only through core.]*
  *[Ceiling now $750 (owner, 2026-11-26): phase-1 brute force plus the fix pass; raise in steps rather than run
  unbounded. Worker slots scale only through core (AGENTS "Running the build").]*
- **Sequence (2026-11-21):** phase 1 → verify phase 1 → Reporter → web, Reporter work beside phase 1;
  phase 1 the priority (2026-11-17: "The database build is 10-ish days alone for phase 1. Then we can
  sprint towards live launch readiness."). Phase 1 = 2024-07-09..2026-11-20 (C-060, owner 2026-11-21; was the newest two weeks, then back to 2025-04-10);
  deeper history after launch (2026-11-16). Universe: the 64 for a long time, robust to renames and
  delistings (2026-11-14). FIELD campaign after the build; only structural changes before it.
  *[Reporter starts once the database is progressing smoothly (owner, 2026-11-26, seven ways 2).]*
- **Phase-1 QA (2026-11-21):** the plan, gates (2026-11-18; per day and whole store since 2026-11-21) and censuses in the launch brief;
  no rule tracking added (2026-11-16). *[The launch brief's QA plan, gates and census now live in Steps, Next 2.]*
- Free and paid together ideally, free first if need be (2026-11-14).
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
  explained; optimizations and value fixes in separate commits. *["phase 1 from one image" retired 2026-11-26
  (owner, census #35): blocker hotfixes through the gate are allowed (AGENTS).]*
  Optimization passes repeat while the measured gain pays for the effort ("reality judges").
  *[Production: science frozen, only blocker and operations fixes ship mid-run, each through the gate (AGENTS
  "Running the build", owner 2026-11-26).]*
- **Audits:** C-041: every verified finding fixed; only one above the cut line (a wrong number in the
  database, or only a full rebuild corrects it) holds the step; failure-surface audits after a real
  fault. At most three agents at once on the laptop. *[Retired 2026-11-26 (owner, census #40/#41): the
  operations-feature audit stop (its end condition was met) and "D-412's build cut", which was the coordinator's
  own (commit 769c911: "(mine)").]*
- **One audit per release (owner, 2026-11-24):** "make all changes, THEN audit once (both codex and gemini), then
  commit"; "fix everything the audits find … repeated audits is not helping. Time is not a valid reason to half ass
  things." ONE audit by both per release; EVERY real finding fixed (ordinary and rare), verified by tests and real
  data; no second audit round ("if it's rare that is converged": rare findings trigger no further round, they are
  not skipped). A rule for this stage (pre-launch), revisited after launch. "Every finding that we validate with our
  data is one less bug to go live." A finding claiming a wrong number gets an occurrence count in the published DB
  before it can justify a hand-over.
- **Data sources:** bulk option files where they exist and match the API (2026-11-19); provider
  analytics from the bulk files' first computation; readers get only the engine's own IV/Greeks
  (D-397); volume in whole contracts, exposure share-equivalent, each role stating its unit (D-390);
  CPI the unrevised `$CPIAUCNS`, INDPRO and expected earnings state their basis (2026-11-16); curve
  H.15 → Intrinio `$DGS*` → FRED API (D-105); close from the listing market's print (D-412).
  Stay on Intrinio (2026-11-22). Market conventions (2026-11-22: "Go with the market conventions for sure"):
  D-439..D-441, D-080.
- **History windows (2026-11-20, D-404):** compute over the sessions present, never interpolate,
  state each missing session, withhold only comparisons needing a missing endpoint; floor 75% of a
  fixed span (40 CFR Part 50 App. K §2.3, App. T §3.1; "scan back is going to produce weird
  behavior"); history-window sensors only. EVC01 189 of 252 plus its 20-sample minimum ("requiring
  75% coverage for the history required to compute a sensor requiring history is sensible"). The
  80% coverage target is a goal, not a rule.
- **Recompute (owner, 2026-11-26, census #58, replacing 2026-11-24's "OR a method version changes"):** recompute
  only where published values change. A day whose values do not change keeps its older method label (truthful).
  When it cannot be told whether values change, recompute. After phase 1: scoped recompute (D-104/D-462).
- **Reference checks:** on every computed day in the worker before raws are deleted (2026-11-18);
  tape owners by the tape worker's verdict; pre-verdict caches "cache code verified by sample".
- **Operations:** workers stateless and credential-free, core signs one-object upload URLs
  (2026-11-17); restatable captures kept write-once in core (C-043); Discord one alert per problem
  and a 09:00 MT verdict digest ("I want to open it and immediately know if i gotta go look at it or
  its running ok"), channel only; availability needs are minutes; no AI in the production path.
  Discord in plain language (2026-11-25): no job/lease/build/decision codes; 🔴 only when the owner is needed.
  Provider retries exponential with full jitter, honouring Retry-After (2026-11-24, D-461). Automation debts ride the
  next release rather than costing an extra hand-over (2026-11-24).
- **Standards (2026-11-14):** the published standard first, cited; an owner decision is the last
  resort; a rule built on sparse data is judged by the built population.

## Listed, not fixed
(About 25 items fixed since the last prune were dropped: the automation debts, claim race and retry, tape memory,
the reference-check regression, the D-458/VS-O04 validator, drains, ARKK caches, NO_PROGRESS alarms and the like.)
- **Operations:** a slow nightly on `day-intel` (11-24: 5.3 h vs ~2 h on AMD; node type, memory pressure or requote
  work); why a day needs ~13 GB (11-24 nightly peaked 6.5 GB); downloads bound by per-worker concurrency, not the
  budget (~800-1,270/min of 1,900) — candidate: more in-flight requests per worker; tape scaling oscillates (scaler
  stops priority-0 work 5 min after any budget wait) — smooth bursts or scale by headroom; P-30 throttling under
  budget unexplained (survivable since D-463/D-464).
- Ordered passes at full range: each loads the whole published store (est. ~1 GB peak); measure the last passes.
- **The 252-session weekly baseline (C-060)** in the review Reporter reads (unbounded in `vyreon-api`), before the
  first paid issue.
- After the build: fix the graph declarations C-060 lists; check every company's DLM-IV-CRUSH event lies inside it.
- **Trades-API switch before daily publishing** (owner, 2026-11-18: daily runs take trades from the trades API,
  quotes from the replay; D-089 amendment not built). Without it GLD's EVC01 volume baseline falls below 189 by
  ~mid-December 2026 (P-04).
- Deadline alerts on from 2026-11-30; then the engine droplet retires (end state).
- Frozen raw tape slice (D-102) for the close-selection tests; reader digest of non-test code only (h3-replay).
- `company_inputs.EARLY_CLOSES` lacks 2022-11-25, 2023-07-03, 2023-11-24, 2027-11-26 (fix before deeper history and
  before 2027-11).
- One-year-window census (owner, 2026-11-20): one definition of "a year" anchored on `return_1y`; census every
  1-year/52-week window, then one decision with before/after.
- D-404 remainder: daily P01/PATH views over a missing bar, `evc_history`, a DLM-PCR-OI-over-gap test;
  kernel-contract JSONs still say MIN63.
- Engine small: `Occ::quote_file` returns None silently on an unreadable file; the ordered derive fails the whole
  object on a population load failure; INT-IVSC01's `comparison_qualification` says METHOD_CONTINUITY_UNKNOWN.
- Dated reminder: the CPI/INDPRO release table (D-380) ends at November 2027 data; extend before then.
- OCC ledger (C-055): a new entry needs a new core image; specials with no memo listed, not entered; record the
  ledger's hash with each published day.
- Core `read` routes serve full bodies (by decision while only Reporter and the owner hold keys); API end-to-end
  before launch.
- Provider: bug report #3 sent 2026-11-21 (pre-2025-11-26 replay regeneration); Cboe permission reply pending;
  provider faults to log (not report): earnings_disclosed_at NY time with Z, sec_unique_id forms, macro page boundary.
- Test-suite audit (`~/claude-artifacts/audits/2026-11-21-test-census/tests.md`): gaps #1-#4, #7-#9; D-102
  rewrites #14, #15, #19; then mutation testing. Retained fixtures need a real build run (`tools/golden/fixture.py`).
- Engineering before go-live: `cargo audit`, `pip-audit`, an image scan, a secret scan of vyreon-core's public
  history. Security (owner, 2026-11-17): least-privilege tokens replace the full-access DO token; the reader API
  stays inside the cluster; a read-only exposure check and an attack-surface audit.
- D-412 left by reason: halted-ETP close, zero-volume `9`, S1–S3; ETP TWAP/T-WAM tiers; Network-B listing from the
  provider's current record.
- Options: ACT01 picks its cohort by raw strike, not translated across a strike reduction; a partial OI outage
  without an adjustment publishes; D-349's residual; `bulk_snapshot_absent` only measured; a refused bulk listing
  falls back to the API.
- Rounds i–k: a faulted reading with no value in its 63-session window is not stated in unusualness; FND02 roles
  equal FND01's on 72 of 81 pairs; a translation refusal publishes NO_GOVERNED_COHORT_CERTIFICATE, not its cause; a
  retained view's collision reads ENGINE_FAULT until `--rebind`; the collision guard sees only T−5's chain.
- INDPRO's 2025-12-03 macro capture stated WITHHELD_WRONG_UNIT (not looked into).
- Style items 3–7 (round a); round e; overlay 3, 5, 6, 7; a corrected official daily volume between two workers'
  captures; a missing macro capture's containment.
- Deeper history only: D-176's `unknown` 2020-01-02 ledger entry; no `source_coverage` rows for option intervals and
  trade counts; META's VS-P01 cold start after FB→META; D-441 debut zero inside the walk; the first 21 sessions after
  2022-05-27 withhold OI.
- Core: the claim does not send `max_attempts`; superseding a refused tape session means deleting its rows.
- `r2` against `p1`: 11 items to check (`day-r2-vs-p1.md`).
- The zero curve on a 2022–2024 date; the hold for a tenth of slots lacking a provider input (owner 2026-11-13); a
  tolerance for the Greek exposure's unsolved share (owner 2026-11-15).
- VS-O06 volume identifiability floor (D-455/D-456): census check against Cboe's per-contract EOD files.

## Machines and money
- **Spend:** DigitalOcean $351.57 month-to-date at 2026-11-25 22:14 MT (balance API); baseline $180.41 at
  2026-11-21 06:09 UTC, before phase 1. Ceiling **$750** (owner, 2026-11-26; live in core with the next release).
  Measured from now on by core's hourly billing read (C-078). Estimate, unproven: production ~7–8 days, ~$230–260 at
  $0.167/node-h. Warn the owner before crossing the ceiling.
- **Kubernetes cluster `vyreon`** (nyc1, id `5f56e6e0-1c74-4dc9-bfdb-962ee55ebff3`): pool `core` 1 ×
  `s-4vcpu-8gb-amd` with CloudNativePG (volume 250 GiB) and core `vyreon-core:8f4fdc9`; worker pools `day`
  (`s-8vcpu-16gb-amd`) and `day-intel` (`s-8vcpu-16gb-intel`, owner 2026-11-22), $0.167/h each, autoscaled 0..13;
  KEDA 2.21.1 ScaledJobs per kind (day-block max 8, ceiling 12 with the ramp). Full-day jobs request 12Gi, limit
  12Gi (C-085). Backups: Barman plugin to Spaces `vyreon-pg-backup` (weekly full + WAL, ~30 days; restore drill
  passed 2026-11-22). Secrets: `vyreon-worker`, `vyreon-core-webhook`, `vyreon-core-progress-webhook`,
  `vyreon-core-spaces`, the backup key, `vyreon-core-do-billing`. Registry `vyreon` (nyc3). Images with known
  defects must not be used: `ed66a433`, `76169889`. H5-FIELD is bit-identical on both pools.
- **Droplet `ssh vyreon-engine`** (8 vCPU, $112 a month): Postgres, the daily capture and archive
  (`vyreon-capture`, `vyreon-archive`: the option-trade cohort prints the provider will not serve
  again, to its 250 GB volume and Spaces `vyreon-data`); notices muted to
  `/var/log/vyreon-notices-muted.log`; health and heartbeat timers stopped (2026-11-17). Retires per
  the end state. Not checked since 2026-11-15: whether `/root/vyreon-engine-worker` and
  `/root/worker-proof` (57 GB) are still on it.
- **Droplets:** web (`Telemetry-Backend`), `vyreon-engine-shared-8vcpu-amd`, the cluster's nodes. Limit 30.
- **Spaces `vyreon-data`:** perishable archive; DataShop order `reference/cboe-datashop/order_000107661/`
  (laptop copy `~/claude-artifacts/datashop/`). Keys (gitignored, 600, never printed): `api.txt`,
  `spaces-storage-access-key.txt`, `tiingo.txt`, `~/fred-api.txt`, `do-full-token.txt`, `pg-backup-*.txt`, the
  billing token file.
- **Laptop:** worktrees `/home/diggz/wt/<name>`, cargo targets `/home/diggz/wt/target-<name>`
  (delete at merge; `du -sh /home/diggz/wt/target-*`); `/home/diggz/wt/target-worker` is the image build dir
  (deploy/worker/build.sh). Test droplets: `tools/testbed/tb` if one is needed again.

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
- 2026-11-22..24: backups and a restore drill (2 min 38 s, digests equal); KEDA starting work from a zero-node pool;
  daily runs from the queue; D-459 bounded tape memory with the carry-over gate 26/26 sessions byte for byte.
- 2026-11-25: H5-FIELD bit-identical across AMD (EPYC Zen 2) and Intel (Cascade Lake, i7-12700H): one d114 binary,
  digest 39bb103fbe457875. The one-day gate caught two systematic faults on one node (M0 source identity on
  93a04676, INT-IVSC01 on 90e2f228) instead of on 12 blocks. Tape re-capture of 880 tapes done under the new reader
  identity, 0 failed.

## Not yet proven
- 8+ blocks running at once without memory deaths (4 of 8 died 2026-11-26 before C-085/D-515); time per day at
  scale; whether presigned downloads count against the 2,000 a minute.
- The canary (AGENTS: every engine merge computes one real day on one node before shipping).
- The ramp, the one-real-day gate and the lease grace in core (C-086..C-088) on a real run.
- The history reference check over built histories; the flag-agreement test on the built store; refcheck across
  ~600 sessions.
- Deadline alerts (from 2026-11-30); the trades-API path; two weeks unattended.
- Scoped recompute vs full day byte identity on a real day (`ops/scoped-check.sql`, README §11).
- Reporter on core's API; the web's receiving end; the API end-to-end.
- Not run on values: the preferred-dividend subtraction (no real case); D-165/D-167 on a rename
  date; D-166 on a post-listing date; an early date before the tapes (2023-05-24), option intervals
  (2022-12-08) or trade counts (2024-11-26); the split-basis scores (need a built history).
