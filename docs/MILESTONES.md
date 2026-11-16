# Milestones

The one living status document: current state and what is next. History is in git (the longer
forms are at tags `milestones-2026-11-15` and `milestones-2026-11-16`); decisions are in
`docs/DECISIONS.md` (core, C-numbers) and the engine's `docs/DECISIONS.md` (D-numbers); the plan
is in `docs/ARCHITECTURE.md`.

## Steps to the MVP (the list shown to the owner on a status request; keep it current)
1. **Two-day cluster trial.** Done 2026-11-13.
2. **Engine ready for the build.** Closing. A Codex audit ran after each lettered item:
   a. [done] the seven fix tracks merged (`pass-fx-all`); *audit* (round a)
   b. [done] the follow-ups merged; *audit* (round b)
   c. [closing] real days on the candidate: `x2` clean on `pass-fx10`; **`y2` and `x3` running on
      the merged candidate `pass-s3i` d55f6439**. Left for a droplet with real data: re-freeze
      the AAPL tape-cache and the `etha-first-chain` fixtures. An early date and a pre-listing
      day need a provider key: they are step 2.5's.
   d. [done] audit triage and fixes: batches 3 to 10, rounds c to i. Converged (owner, 2026-11-16).
   e. [done 2026-11-16 10:00 UTC] `pass-s3i` = `pass-fx10` + D-162 by merge (done); core `pass-core-fix` merged into
      `main` (done, pushed); *audit of the merge* (rounds j and k, fixed); images built on the
      laptop and pushed; engine fast-forwarded to `worker` (d55f6439).
   f. [done] housekeeping: this file pruned 2026-11-16 twice (tags above). Prune again, with a new
      tag, when it passes about 300 lines.
3. **(2.5) Cluster verification. IN PROGRESS since 2026-11-16 10:00 UTC (owner's pre-approval of
   04:00 UTC).** Runbook `~/claude-artifacts/cluster-verification-runbook.md`. Order (tape history
   only moves earlier): 2025-12-10; 2025-04-11 (ETHA's first chain); 2024-07-16 (AVGO 10-for-1,
   ex 07-15, and before ETHA); the block 2022-06-06..06-15 (FB to META). **The newest two weeks
   on two nodes wait for the owner** (they need about 232 tape sessions: the build's tape phase).
4. **(3) The build:** newest first back to 2022-05-27, sized and approved by the owner.
5. **(4) Reporter and web** on core's API and the built history.
6. **(5) Judge the reports; the FIELD campaign; the go-live choice.**
Working practice: at most three agents at once on the laptop; an audit after every landed step;
every verified finding is fixed, only one above the cut line sends the candidate round again
(C-041 amended). Write a time only after `date -u`.

## Now (08:55 UTC 2026-11-16)
**Build candidate: engine `pass-s3i` d55f6439** (worktree `/home/diggz/wt/s3i`, target
`/home/diggz/wt/target-s3i`, release built, pushed) = `pass-fx10` 8a40183e (worktree
`/home/diggz/wt/fx9`, decisions to D-245) merged with the cohort across a split or rename (D-162,
D-163, D-167), plus D-246 (collision guard) and D-247. Suite: 1,221 passed, 12 failed (eight
`option_field`, two tape-cache, two `etha-first-chain`: all fixtures). `worker` (dc279ff1) is an
ancestor: a fast-forward. Core `main` holds C-036 to C-042 (8 unit, 40 database tests pass);
the cluster still runs `vyreon-core:6e15a27`.

**Real days, all 2025-12-10** (a day is clean only if `h4-day-summary-*.json` says
`faulted_engine` 0; dumps in `~/claude-artifacts/testbed-dumps/`: p1, r2, m4, m5, m6, x2):
- `m6` (`pass-fx8`, 192.241.147.166, ETHA unlisted): clean. Against `m5` 128 slots, all explained.
  First real cover numbers (D-228 amended): BABA's Greek exposure covers 0.679 of its open
  interest (617 unresolved rows, BABA1 and BABA2), UNG 0.991, the other 61 median 0.996.
- `x2` (`pass-fx10` 8a40183e, 137.184.61.134, all 64): clean, 4,679 slots. Against `m6` 83 slots,
  all explained (12 FIELD for membership, 63 unusualness at 1.6.0, eight DBA and UUP slots that
  lose PROVIDER_INPUT_MISSING: D-239 proven; D-238 ran without a fault).
- **`y2` (`pass-s3i` d55f6439, 192.241.147.166, ETHA unlisted) finished clean 09:58 UTC:** both
  summaries COMPLETE, `faulted_engine` 0, 4,608 slots, no `Connection refused`. Against `x2`
  (ETHA left out) 110 slots differ, all as predicted: 12 FIELD (membership); the five split
  assets' ACT01 family (30 slots) and their unusualness (5); and M0-GAMMA-CHANGE's `method_sha256`
  on all 63 with no value change (the merge touched its method closure). **The split translation
  is measured on real data:** XLB, XLE, XLK, XLU, XLY each publish SAI-ACT01 with 68 supported
  roles (0 before), SAI-ACT01-INTRADAY 343, INT-ACT-TV15 14 to 98, bodies stating CLEAN_SPLIT;
  SAI-EXQ01 6 supported with EVENT_SOURCE_NOT_RETAINED, M0-PRINT-SIZE withheld the same way.
  No other asset moved: the pooled feed check kept its verdict. Dump `y2`. (Judged 10:00 UTC.)
- **Still running on `pass-s3i` d55f6439:** `y2` (192.241.147.166, ETHA unlisted, started 06:10 UTC) and
  `x3` (137.184.61.134, all 64, started 08:12 UTC). Both homes got the five split assets' cohorts
  by `vyreon-inputs fill --category act01 2025-12-10` with the key passed on stdin (nothing
  stored): XLB, XLE, XLK, XLU, XLY each a translated cohort, none refused by D-246. To judge:
  against `x2`/`m6` only those five assets' ACT01 family (and what pools it) may differ, plus
  one change of every task key; `y2` against `x3` only membership.
- Test droplets hold no provider key: a day whose plan lacks an input is refused (`x1`: the T-5
  macro capture, made on the laptop with `capture_source` and copied to both homes' `h5-macro/
  2025-12-03`; `y1`: the split cohorts). Check the plan line of a new run before leaving it.

**Step 2e closed and step 2.5 started (owner's pre-approval), 2026-11-16 10:00 UTC:**
- Engine `worker` fast-forwarded to `pass-s3i` d55f6439 and pushed. Images pushed to the registry:
  `vyreon-worker:d55f6439`, `vyreon-core:f491ea9` (fresh registry credentials through `do.sh`,
  two hours, in `cluster-scratch/docker/`).
- Core redeployed on the cluster (`kubectl set image`, both containers; the budget stays 1,500 a
  minute): migrations 0012 to 0015 applied. Build `d55f6439+62fb506e43f3` registered (4,679
  slots) and admitted (`cluster-scratch/evidence-d55f6439.json`).
- **Pool `day`: 2 x `s-8vcpu-16gb-amd` created 10:02 UTC ($0.334 an hour for the two; id
  `5b683697-6614-4a62-9fb9-315a10a840a3`). Delete it when the runs end** (`do.sh DELETE
  /v2/kubernetes/clusters/5f56e6e0-1c74-4dc9-bfdb-962ee55ebff3/node_pools/<id>`).
- 2025-12-10 scheduled under the new build (tape, day block, ordered pass). Running:
  `cluster-scratch/run-day.sh 1210 tape` (Jobs `v-tape-1210`, then `v-day-1210`; it saves the
  pod's `h4-day-summary-*.json` to `cluster-scratch/summaries-1210/` while the pod lives). Jobs
  come from `cluster-scratch/mkjob.py KIND NAME COMPLETIONS PARALLELISM`. Then: Job
  `v-pass-1210` (ordered pass), cancel the re-queued 12-11 pass of build `0ca127ea`, dump, compare
  with `x2`.
- 10:09 UTC: the tape job of 2025-12-10 done in 7 minutes (64 caches new to core: the stored
  ones were another method version); `v-day-1210` claimed job 24 with tape history from
  2025-12-10 and is downloading. 10:18 UTC: 2025-04-11 scheduled (tape, day, pass; ETHA is a
  member) and `run-day.sh 0411 tape` started on the second node.
- 10:20 UTC: `v-day-0411` claimed job 27 (tape history from 2025-04-11) and is downloading.
  **10:49 UTC: pool `day` grown to 4 nodes ($0.668 an hour; the same node-hours, sooner);**
  2024-07-16 scheduled (tape, day, pass) and `run-day.sh 0716 tape` started; the block
  2022-06-06..06-15 scheduled as one eight-session job with its eight passes and `run-day.sh
  2206` started (no tape job: before 2023-05-24). Core's universe shows Meta as FB on 2022-06-08
  and META on 06-09. The ordered passes (11 queued) run last, in date order: Job from
  `mkjob.py ordered-pass v-pass 11 1` once every day block is done.
- **Found by the cluster, 10:51 UTC (D-248, fixed on `pass-s3i` 1250232b, pushed; NOT in the
  running image):** a worker's empty home has no `h5-acq` until a tape cache is unpacked, and the
  fill refuses such a home (exit 2), so a block before the tapes (every session to 2023-05-23)
  failed at once. Job 32 (2022-06-06..06-15) failed three times (the third by the 2024-07-16 pod,
  which claimed it in a race); core set 2022-06-06 aside (FAILED, C-035) and re-queued the rest
  as job 41, the other pods ran on (C-040 held). Workaround running: Job `v-day-2206b` with an
  init container that makes the directory (`cluster-scratch/job-v-day-2206b.yaml`): it claimed
  job 41 (2022-06-07..06-15) and is downloading. **To do:** `queue retry-failed` and run
  2022-06-06 the same way when a node frees; rebuild the worker image from the fixed commit
  before the build (new build identity), fast-forward `worker` then.
- 10:53 UTC, all four nodes busy: `v-day-1210` (job 24), `v-day-0411` (27), `v-day-0716` (30,
  second pod), `v-day-2206b` (41). Watchers: `run-day.sh` per tag; **I edited `run-day.sh` while
  the 1210 and 0411 watchers were running it, so their final print may fail: read
  `cluster-scratch/summaries-<tag>/` directly.**
- **Second failure on the 2022 block, 11:36 UTC:** with the directory made, `v-day-2206b`
  downloaded 2022-06-07 for 20 minutes (about 5,000 requests), the fill exited 1 and the plan
  refused the day: `options` Stale (62 assets without one requoted source, T and 2022-05-31),
  `close_quotes` Missing (434 = 62 assets x 7 sessions without a quote file), `consolidated`
  Missing (62: a tape asked for before the tape start?), `option_history` Missing. Two attempts
  of job 41. The provider does serve the option snapshot (asked through the MCP: 2022-06-07
  19:50 UTC, eight parts of about 6 MB), so the cause is on our side and not yet known: the fill
  log dies with the pod. 11:37 UTC: Job `v-day-2206c` (attempt 3) runs the worker wrapped to print
  `/work/logs/vyreon-inputs-fill.log` on exit. After it: 2022-06-06 and 06-07 are FAILED days;
  **do not start another attempt on this block before the cause is fixed** (three FAILED days
  pause the queue, C-035).
- **Cause found 12:05 UTC (`cluster-scratch/v-day-2206c.log`):** for a session before the tapes
  the fill still asks the replay endpoint (`replay_capture ... --publish-unavailable`, D-166) and
  expects 404; Intrinio now answers HTTP 500 (confirmed through the MCP for 2022-06-07 and
  2023-05-23), which the fill takes as Fatal; it stops before the quote files are fetched (no
  snapshot request in the counts), hence every other refusal. An agent is fixing it on
  `pass-s3i` (D-249: coverage decides before the endpoint is asked) and checking whether the
  quote files have a second defect in 2022 (the nearest snapshot is at :50, in eight parts).
  Then: rebuild the worker image, register and admit the new build, cancel the block's jobs of
  build `d55f6439`, schedule 2022-06-06..06-15 again under the new build, `queue retry-failed`
  not needed (new jobs). 2022-06-06 and 06-07 are FAILED days of the old build; the fourth
  node idles meanwhile. Also asked of the agent: why every macro capture states INDPRO
  WITHHELD_WRONG_UNIT. (12:04 UTC)
- **Fixed 12:14 UTC on `pass-s3i` 5f5f90b9 (pushed): two independent defects, not one (my 12:05 reading
  was wrong: quotes run before consolidated and a Fatal ends only its own category).** D-249: before
  the replay source's first session `replay_capture --before-coverage` records TAPE_UNAVAILABLE
  with no request. D-250: an option snapshot in several part files is admitted (2022: eight
  parts; the code required exactly one); requote runs only on sources with a quote file, so a
  failed quote fetch keeps its reason and retries. Ordinary day: identical bytes. **Not proven
  without a real run:** that the 2022 parts are separate gzip streams with today's header, that
  no contract sits in two parts, that the listing's order is stable, that META's FB-rooted rows
  land in META's file. INDPRO's WITHHELD_WRONG_UNIT is the raw gate, expected (D-044: the
  effective gate overrides; D-110 item 2 wrongly says the override is retired).
  Building the worker image of 5f5f90b9 and auditing the four changes (round l,
  `2026-11-16-cluster-round-l`). Job 42 (2022-06-08..06-15, old build) cancelled 12:10 UTC.
- **12:22 UTC: worker image `vyreon-worker:5f5f90b9` built and pushed; build
  `5f5f90b9+252c04820c46` registered and admitted** (`evidence-5f5f90b9.json`). The block's old
  jobs (32 to 41) cancelled; 2022-06-06..06-15 scheduled again under the new build (one
  eight-session job, eight passes) and `WORKER_TAG=5f5f90b9 run-day2.sh 2206d` started (plain
  manifest: it tests D-248 too). `mkjob.py` takes the image tag from `WORKER_TAG` (default
  d55f6439); `run-day2.sh` is the copy to use from now. The ordered passes are now of two
  builds: eight of 5f5f90b9 (2022), three of d55f6439 (2024-07-16, 2025-04-11, 2025-12-10), to
  run in date order, so the 2022 passes first.
- **Round l (audit of D-248..D-250), 12:28 UTC: `2026-11-16-cluster-round-l/early.md`.** Four
  findings and a suspicion, all being fixed on `pass-s3i` (no separate verifier; the fix agent
  re-verifies): the snapshot's parts must be ordered by their `part` number and be exactly
  0..n-1 (the provider lists them 7 to 0: today's code hashes listing order and would admit a
  listing with a part missing: the one wrong-number risk); a contract repeated across parts
  counted twice in the adjusted-terms price check; the tape-history clamp and the tape job
  ignore a claim's earlier source start (latent: core sends none). **The block running under
  5f5f90b9 (`v-day-2206d`, job 43, downloading since 12:22 UTC) is therefore evidence for values
  but not for 2022 quote-file identities: the part order will change.** A third image follows.
- (Order rule used) each older tape job was scheduled only after the later days had claimed: 2025-04-11 (as soon
  as `v-day-1210` has claimed), 2024-07-16, then the block 2022-06-06..06-15.
- **`x3` (`pass-s3i` d55f6439, 137.184.61.134, all 64) finished clean 11:18 UTC:** both summaries
  COMPLETE (2,688 of 2,688; 512 of 512), `faulted_engine` 0, 4,679 slots. **Against `y2` on the
  other machine, with its own separately fetched cohorts: only the 12 FIELD membership slots
  differ.** Against `x2`: the five split assets' ACT01 family (30) and unusualness (5), and
  M0-GAMMA-CHANGE's method hash (64, no value). Dump `x3`. Real days on the test droplets are
  done; an agent is re-freezing the `etha-first-chain` and AAPL tape-cache fixtures from
  137.184.61.134's real archive: done 11:32 UTC. **`pass-s3i` 170bd77d (pushed): suite 1,225
  passed, 8 failed, only the `option_field` fixture tests.** `etha-first-chain` (real captures
  2025-04-09..04-14, unmodified; the shared, git-ignored directory under the main checkout's
  `workspace/fixtures`: older branches fail against it; the old copy is in
  `~/claude-artifacts/fixtures-old/`) and the AAPL tape cache (only `derived.json` was stale).
  Core's own copy of the tape-cache fixture now differs (its tests pass). The two test droplets
  have no task left ($0.334 an hour together): deleting them is the owner's word.
- Ledger: test droplets $0.334/h (both since about 2026-11-13); cluster core $0.083/h; pool
  from 10:02 UTC.

**Before 2.5 (kept for reference):** `vyreon-worker:d55f6439` (build identity
`d55f6439+62fb506e43f3`; inventory `~/claude-artifacts/cluster-scratch/inventory-d55f6439.json`)
and `vyreon-core:f491ea9`, built on the laptop. The trial's scratch (kubeconfig, job manifests,
`do.sh`, token files) is in `~/claude-artifacts/cluster-scratch/` (mode 700); the kubeconfig
works (node `core-3aju5s`, `vyreon-pg-1`, no `day` pool). Before scheduling: republishing
2025-12-10 re-queues the 12-11 ordered pass under build `0ca127ea` (cancel it); tape history is
the earliest TAPE job not cancelled and a DONE one cannot be cancelled (`work.rs`).

**Audit rounds** (Codex, read-only; `tools/audit/`; reports under `~/claude-artifacts/audits/`):
a to g `2026-11-15-fix-round-{a..g}`; h (fix 9) and i (fix 10) `-round-h`, `-round-i`; j (the
merge) and k (its fixes) `2026-11-16-merge-round-{j,k}`. Nothing above the line since round j's
collision finding (D-246). The pre-build rounds on the engine ended with k.

**Branches and worktrees** (`/home/diggz/wt/<name>`): candidate `s3i`; `fx9` (branch
`pass-fx10`); the merged batches `pass-fx-all` to `pass-fx8` and their parts are pushed and their
worktrees can go (preview and ask: about 40 worktrees). Core: `main`; `/home/diggz/wt/fx-core`.
Engine `worker` checkout: `/home/diggz/vyreon-engine-worker`.

## Decisions waiting on the owner
- **Delete the two test droplets** (137.184.61.134, 192.241.147.166; $0.334 an hour together)
  once the fixtures are re-frozen? Their real days are done and dumped; each holds the 2025-12-10
  archive, re-downloadable. Asked 2026-11-16.
- **The newest two weeks on two nodes** need about 232 tape sessions (the build's own tape
  phase): run them inside 2.5, or with the build? Asked 2026-11-16.
- Answered 2026-11-16 03:55 UTC ("go with your recommendations"): the old run directories deleted
  on both test droplets (137.184.61.134 keeps `p1`, `r2`; 192.241.147.166 keeps `m4`, `m5`, `m6`;
  done 04:10 UTC: 184 and 182 GB free); D-236 stays as it is (the whole ACT01 family withheld 2022-12-08..12-14); the
  pooled quantities stay pooled over the assets present; with exactly one visible share report the
  company metrics publish, left until the build shows how often.
- The production store's 2025-12-10 (177 withheld activity slots) is not touched until the merged
  engine is deployed there; it then needs the rebind and a republish, on the owner's word.
- A missing daily bar withholding an asset's surface for about three years is left until the build
  shows one (2026-11-15; its label is fixed).
- **Step 2.5 is pre-approved (owner, 2026-11-16 04:00 UTC):** if 2e closes while the owner is
  away, start the cluster verification at once, on Fable, without waiting for the swap to Opus or
  for a spend approval. Limits unchanged: the $200 total ceiling, idle droplets deleted; a finding
  above the cut line or anything beyond 2.5 (the build) still goes to the owner. Write the
  hand-off for Opus all the same, so the owner can swap when back.

## Listed, not fixed
- From rounds i to k: a faulted reading with no value in its 63-session window is not stated in
  unusualness; FND02's standardized roles equal FND01's on 72 of 81 pairs and both score; a
  translation refusal publishes NO_GOVERNED_COHORT_CERTIFICATE, not its cause (a new published
  reason: five owners and a cause class); a retained view's collision reads ENGINE_FAULT until
  `--rebind`; a resumed worker's stale view is not reselected; the collision guard sees only
  T-5's delivered chain. The guard's refusal has run on no real chain (none collides).
- INDPRO's macro capture states WITHHELD_WRONG_UNIT on the 2025-12-03 capture (seen 2026-11-16;
  not looked into: whether T's does too and what reads it).
- Style items 3 to 7 (round a triage directory). Findings for after the build (round e summary).
- Overlay 3, 5, 6, 7; a corrected official daily volume between two workers' tape captures; a
  missing macro capture's containment (design choices; round d triage).
- An `unknown` ledger entry (D-176's 2020-01-02) may make dead listings' open interest unknown on
  2022-05..2023-04; core has no `source_coverage` rows for `option_intervals` and
  `option_interval_trade_counts`, the engine's compiled dates apply (tag: batch 5, fix 7 entries).
- `r2` against `p1`: 11 items to check; M0 values were not compared (`day-r2-vs-p1.md`).
- First cluster days: the zero curve on a 2022-2024 date (`python -m refcheck.options.zero_curve
  <H.15 csv>`); the hold for a tenth of slots lacking a provider input (owner 2026-11-13).
- From the build: a tolerance for the Greek exposure's unsolved share (owner 2026-11-15); the
  unusualness line and `unusual_score`, which have never run on real rows.

## Standing owner rulings that still govern the work
- Spend ceiling: $200 in total over about a month (owner, 2026-11-14, from the session's notes;
  the old file's newest figure was $100 of 2026-11-13; droplet limit 30).
- Spend rules (2026-11-12): the worker pool is deleted after every run; the cluster and registry
  only on the owner's word; ask before exceeding the ceiling; new spend for 2.5 and the build goes
  to the owner first. Test machines are deleted when the step's tests are done (2026-11-13).
- Order: the database before general rules, no rule tuned on the golden dates (2026-11-11);
  sensor-level withholding and a changing universe before the build (2026-11-12).
- Build: newest first (2026-11-12), back to 2022-05-27 (step list), sized and approved by the
  owner. Kept over "back to 2021-09-27" (2026-11-14) and "pause at lookback depth" (2026-11-12).
- Universe (2026-11-14): the 64 stay for a long time (64 slots, the date's members from the
  claim), robust to what can happen to them: a ticker change above all, a delisting by design.
  Scale and a general FIELD are deferred. Kept over "N assets in full" (2026-11-12).
- After the build: the FIELD campaign, only structural changes before it (2026-11-14); Reporter
  and web, then judging the reports (2026-11-14); single-sensor compute when a real snag needs it
  and the full range for analysis after revenue (2026-11-12). After step 2 (2026-11-12): the daily
  capture moves to a small droplet or the cluster's core node; the 8 vCPU droplet is deleted.
- Cut line, C-041 (2026-11-15): the audit loop exits on no verified finding above the cut line (a
  wrong number in the database, or only a full rebuild corrects it); a real day after each merged
  batch. Kept over "two consecutive clean audits" (06:30 UTC the same day).
- Audits (2026-11-15): one after each of 2a to 2e and one on the merged whole. At most three
  agents at once on the laptop. `pass-s3i` by merge, not rebase (kept over the 08:45 UTC note).
- Standards (2026-11-14): the published standard first; an owner decision is the last resort.
  Go-live (2026-11-14): Intrinio's Startup tier; free and paid together, free first if need be.

## Machines and money
- **Droplet `ssh vyreon-engine`** (authoritative, 8 vCPU): Postgres, the daily capture, health
  checks; nightly archive to its 250 GB volume `vyreon-engine-v1` and the Spaces bucket
  `vyreon-data` (C-004). $112 a month. Not looked at on 2026-11-15: whether
  `/root/vyreon-engine-worker` and `/root/worker-proof` (57 GB) are still on it.
- **Test droplets** (8 vCPU, $0.167/h each, $112 a month; no timers, key or webhook; each holds
  the 2025-12-10 archive and a scratch store). By their DigitalOcean names (checked 2026-11-16):
  `vyreon-test-1` is 192.241.147.166 (keeps `m4`, `m5`, `m6`, `y2`); `vyreon-test-2` is
  137.184.61.134 (keeps `p1`, `r2`, `x2`, `x3`). **The 2026-11-15 entries of
  the old file and the audit notes call 137.184.61.134 "test-1" and 192.241.147.166 "test-2":
  go by the address.** Both carry `pass-s3i` d55f6439. No third test droplet exists.
- **Kubernetes cluster `vyreon`** (nyc1, created 2026-11-11): pool `core`, 1 x `s-4vcpu-8gb-amd`
  ($0.083/h), with CloudNativePG, a 60 GB database volume and core. Pool `day` was deleted
  2026-11-13 00:35 UTC; "the core node stays". Registry `vyreon` (Basic, nyc3, $5 a month) holds
  worker images up to `0ca127ea`, older than the pass. About $67 a month while they stay.
  All four exist (checked through the DigitalOcean API 2026-11-16 00:00 UTC; registry 0.97 GB).
- **A real day on a test droplet:** `tools/testbed/tb` (needs `TB_KEY`, `TB_ENGINE`, `TB_OUT`):
  `tb deploy IP` (the clean tree's release build; refuses while a day runs), `tb run IP DATE NAME`,
  `tb wait IP NAME`, `tb status IP NAME`, `tb dump IP DATE BUILD NAME`, `tb compare BASE NAME
  [SYM,..]`, `tb hide IP PATH`, `tb restore IP`. After a deploy that changes quote-file terms the
  day is refused until `terms-rebind-day.sh DATE NAME` runs on the droplet: `vyreon-inputs terms`
  (no provider call), `act01_auto_source --rebind`, then `run-day.sh`. The disk guard wants 60 GB.
  The script is `tools/testbed/terms-rebind-day.sh` (copy it to `/opt/vyreon-test`).

## Proven so far (on real data)
- 2026-11-09: a fresh 2025-12-10 agrees laptop against droplet to within 1e-11 (`engine-v1`).
- 2026-11-10: the droplet's published day goes through core and reads back identical; a wrong
  bundle is refused; the reader API returns every session, published or explicitly not.
- 2026-11-11: ranged daily bars equal the single-day bars; a day through the worker path equals
  the droplet's (2,853 slots, no numeric difference), and with the day-local bundle and the
  ordered pass all 1,826 derived slots too.
- 2026-11-13: the two-day cluster trial: 2025-12-10 and 2025-12-11 each from an empty disk, tape
  jobs before, the ordered pass after; 6 h 57 min cold, 1.24 GB peak memory, 26,795 requests.
- 2026-11-13/14: a failed history export or surface, or a missing options capture, tape or daily
  bar, withholds only its readers and dependants; the other assets are identical.
- 2026-11-14/15: slots by date membership (D-164); split stages 1 and 2 (D-161, D-163, recomputed
  independently); a member unlisted; the gap-only day `g3`; the ACT01 rebind (59 cohorts).
- 2026-11-15: `r2` and `m4` above; the zero curve equals QuantLib to 2e-13 on four 2026-11 curves;
  the q = 0 default measured (35 of 23,104 asset-sessions).

## Not yet proven
- The merged candidate on a finished day (`y2`, `x3` running); the split cohort's readings.
- An early date: before the tapes (2023-05-24), option intervals (2022-12-08) or trade counts
  (2024-11-26); a real pre-listing day (the fill skipping an unlisted asset).
- D-165 and D-167 on a rename date; D-162; D-166 on a post-listing date; a day after a 10-for-1
  split; the split-basis scores (need a built history).
- History in the ordered pass over several days; the cross-day supersede rule; a block of more than
  one day; the same day equal first in a block and mid-block; rebuilds and recomputes for real.
- The rebuilt images; KEDA and scaling to zero; more than two nodes; the node type by cost per
  published day; backups (C-013); whether presigned downloads count against the 2,000 a minute.
