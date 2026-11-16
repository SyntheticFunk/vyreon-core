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
   on two nodes are the build's first step** (owner, 2026-11-16), not part of 2.5.
4. **(3) The build, phase 1 (owner, 2026-11-16).** a. the newest two weeks as a test sample, and
   fix what they reveal; b. back to **2025-04-10** (about 401 sessions: every sensor at full
   window on the newest week; no old-layout snapshot, no 2023 gap), and fix what comes up.
   Before it: the unusualness flag line states its pool (owner: fix it), one final image from
   one commit, the fixes of step 2.5 audited.
5. **(4) Reporter on the phase 1 database.** First a proper paid report (none has been produced
   yet: it needed the database) beside the free one (all there, a lot of numbers); then **a
   significant body of iteration on the reports themselves** (owner, 2026-11-16) until free and
   paid are generated with confidence.
6. **(5) The launch decision:** pay for Intrinio's Startup tier and integrate Reporter's output
   into the website. After it, or when wanted: the deeper history (the old snapshot layouts are
   readable since D-251..D-254; the 2023-01-18..03-07 gap needs a ruling), the FIELD campaign.
**End goal (owner, 2026-11-16): the whole system lives on DigitalOcean and runs by itself:**
capture, the day, the ordered pass, Reporter and the website, **reports published without user
intervention**. Today Reporter runs on the laptop and feeds the website by hand. So step 5
builds Reporter to read core's API and to run headless (a scheduled job beside core), with its
own publish gate and a channel notice on a hold, not a person's check. **Reports publish on
Sundays** (owner): the week's last session is Friday's, so Friday evening after the close and
Saturday are the slack for a held day or report. Reporter is C#; it can run as a weekly job on
the core node (no extra droplet).
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
- **THE FINDING OF 2.5 (12:38 UTC): the engine cannot read the provider's option snapshots before
  about May 2024.** Real files looked at today (samples in the session scratchpad `snap2022/`):
  2022-06-07 19:50 is eight part files (`part` "0".."7", listed 7 to 0), each with its own
  header of **20 columns** (the engine's `HEADER` has 14: the old files add TOTAL ASK VOLUME, ASK
  HIGH/LOW PRICE, TOTAL BID VOLUME, BID HIGH/LOW PRICE, so ask and bid sit in other columns),
  contract ids in padded OSI form (`FB____220610C00110000`, today `XLE_270617C95.00`), and the
  last row of a part cut mid-line. 2023-06-15 and 2024-03-15 are one file (`part` the number
  **1**), still 20 columns, modern ids. `close_quotes.rs` requires the 14-column header exactly
  (`v-day-2206d`: `snapshot header CONTRACT ID,...`). The days that work fetch quote files from
  2024-05-20 on. So the build cannot fetch closing quotes for roughly 2022-05-27 to spring 2024
  until a reader for the old layout exists; where exactly the layout changes is not yet found.
  **Do not use image `ed66a433`: its rule "parts are 0..n-1" refuses a single file numbered 1,
  i.e. every modern day.** The 2022 block is stopped (Job deleted, jobs 43 to 51 cancelled); the
  idle fourth node was deleted 12:37 UTC (pool `day` is 3 nodes, $0.50 an hour). The three later
  days go on under d55f6439; watch what `v-day-0716` does with its sessions before 2024-05-20.
- **Pinned (12:43 UTC): the layout changes between the sessions 2024-05-17 (20 columns, 59 MB) and
  2024-05-20 (14 columns, 22 MB).** 2022-12-15 is still eight parts; where the parts end
  (before 2023-06-15) is being found. An agent is writing the reader for the old layouts on
  `pass-s3i` (D-251..: normalise at ingestion to the engine's 14 columns and modern ids; parts
  by number; a single file is numbered 1; real slices as fixtures) with leave to list snapshots
  through the MCP and fetch the signed files. Image `ed66a433` (pushed) stays unused.
  **Owner's choice, asked 12:40 UTC:** keep the start at 2022-05-27 (my plan: the reader) or
  start the build at 2024-05-20 and add the earlier two years later.
- **Reader written, 13:00 UTC: `pass-s3i` 518faf7e (pushed), D-251 to D-253; tested on real slices
  only, no whole session fetched yet.** Three layouts, from real files: (1) 2022-05-27 to
  2023-01-17: eight parts "0".."7", 20 columns, padded ids; every part holds every root, contracts
  dealt across parts, none twice (2022-06-07: 1,458,015 rows); **each part is cut at a multiple
  of 1,024 decompressed bytes and the cut contracts are in no other part** (HOOD lost 28 that
  day, none of the 64; the cut root moves by session: IBB on 2023-01-17). (2) 2023-03-08 to
  2024-05-17: one file numbered 1, 20 columns, modern ids, rows ending early, 7-digit
  timestamps, `Max` for some asks. (3) from 2024-05-20: 14 columns. All normalised at ingestion;
  old-layout quote files state `provider_layout=`, `provider_ids=`, `provider_cut=N`; FB rows
  renamed to META; the modern path's bytes unchanged. **Risks for the owner:** (a) **no listing
  for about 2023-01-18 to 2023-03-07** (service errors; two dates answer a garbled listing):
  those sessions cannot fetch quotes as the endpoint answers today; (b) a cut can hit one of
  the 64: its contracts get no quote, stated, unrecoverable; (c) the middle era lists far fewer
  contracts (2023-03-15: 637,567 rows; EMB 161 against 1,316 in 2022): the coverage gate may
  withhold thin chains; (d) 2022 files hold FLEX-named roots (`2SPY`, `1FB`: 1,885 unquoted
  rows, 48 roots) filed as adjusted roots: about 48 adjustment requests a session; (e) `Max` read
  as no ask is the agent's reading; (f) 2024-05-20 itself carries few trades (34,970 rows with
  one against 220,553 on 05-17). Image of 518faf7e building; round m audits the reader.
- **`v-day-0716` (2024-07-16, build d55f6439) refused by its plan 13:05 UTC after 133 minutes of
  download** (22,450 capture requests): `close_quotes` Missing for 11,559 option attempts (the
  window's sessions before 2024-05-20), `option_history` Missing for 61 assets. So a day needs
  quote files over its whole 224-session option window: under the old build only days from
  about 2025-04-10 can run; 2025-04-11's window starts 2024-05-21, just inside. Its jobs
  (old build) cancelled 13:06 UTC; it runs again under the reader's build. (The `run-day.sh`
  watcher died of my edit, as noted; the pod log is the source.)
- **13:07 UTC: image `vyreon-worker:518faf7e` pushed; build `518faf7e+56852725ab12` registered and
  admitted** (`evidence-518faf7e.json`). Under it: 2024-07-16 scheduled (tape, day, pass;
  `WORKER_TAG=518faf7e run-day2.sh 0716b tape`) and the block 2022-06-06..06-15 (one job, eight
  passes; `run-day2.sh 2206e`). Pool `day` back to 4 nodes ($0.668 an hour). Either pod may
  claim either job (same image). First things to look for: FETCHED lines for sessions before
  2024-05-20 in the fill log (`provider_layout` in the quote files), no `snapshot header` error.
- **Round m (audit of the reader), 13:08 UTC: `2026-11-16-cluster-round-m/layouts.md`: no
  wrong-number or nondeterminism defect; the 14 fields map correctly, cut tails are dropped,
  modern bytes preserved.** Three findings, being fixed on `pass-s3i`: a bad timestamp in one
  asset's row fails the whole stream; **an asset left without a quote file is not a withholdable
  input, so one asset's refused rows stop the day for all** (the fix must also say what a day on
  or around a session with no snapshot at all does: the 2023-01-18..03-07 gap); a lone part
  file numbered 1 is admitted. The two runs under 518faf7e go on meanwhile.
- **Regression caught by the cluster, 13:07 UTC:** under 518faf7e the tape job of 2024-07-16
  (job 52) failed twice at `ETHA daily bars` (ETHA first trades 2024-07-23; the same job passed
  under d55f6439 with 63 members). Cause to confirm: round l's ed66a433 writes the claim's
  universe in `run_tape` and the listing decision changes. The first fix agent is repairing it.
  Job 52 cancelled (core holds that session's 63 tape caches from the old build's tape job);
  `run-day2.sh 0716c` started 13:16 UTC for the day (job 53). The 2022 block (`v-day-2206e`, job
  55) is downloading. Round m's three fixes are on `pass-s3i` 7977a40c (pushed, D-254: a
  security whose snapshot rows are refused is that security's missing options input; not in
  the running image). **A session with no snapshot at all still refuses its own day, and days
  within about 200 sessions after it run with every asset's option-history readers withheld:
  the 2023-01-18..03-07 gap is not crossable with option readings; whether such a session ends
  the option history like a coverage terminal (D-166) is the owner's call.**
- 13:19 UTC: the tape regression fixed on `pass-s3i` f4b0b62a (pushed; D-249 amended): the tape job lists
  the claim's members for the session less any asset before its own first session; cause as
  suspected (with a universe in the home ETHA's record was skipped and "no record" counted as
  listed). Not on the cluster yet: the next image waits for what `v-day-0716c` and `v-day-2206e`
  show.
- **13:33 UTC: the reader reads real 2022 snapshots.** `v-day-2206e` (job 55, build 518faf7e)
  fetched the seven sessions 2022-05-27..06-07, each eight parts, about 50 MB, 62 symbols,
  125,000 to 132,000 rows, `provider_layout` 20-column, `padded_ids` true, `cut` empty (no cut
  line on a universe asset); no header error, nothing refused; **the plan accepted 2022-06-06 and
  the day is computing since 13:26 UTC**, the first session ever run before the tapes, the
  intervals and the trade counts. `v-day-0716c` (job 53) is downloading. On the old build
  `v-day-1210` (computing since 13:04 UTC) and `v-day-0411` (13:09 UTC) passed their plans after
  about three hours of download.
- 14:14 UTC: 2022-06-06's first half (intent A) COMPLETE, 496 of 496, `faulted_engine` 0
  (`summaries-2206e/`); its second half started 14:03 UTC. `v-day-1210` and `v-day-0411`
  accepted their inputs 13:48 and 13:52 UTC and are in intent A; `v-day-0716c` downloading.
- **2022-06-06 PUBLISHED on the cluster, about 14:40 UTC (build 518faf7e):** both summaries
  COMPLETE, `faulted_engine` 0 (496 of 496; 2,232 of 2,604 with 372 withheld for absent input);
  2,767 base and 1,071 day-local slots for 62 members (IBIT and ETHA not listed), no ENGINE_FAULT
  body. Stated as the decisions require: SAI-ACT01, SAI-ACT01-INTRADAY, INT-ACT-TV15, SAI-EXQ01,
  SAI-EXQ01-TEMPORAL, M0-PRINT-SIZE PROVIDER_COVERAGE_START for all 62 (D-236); D111-TEMP01,
  INT-TEMP-EVC01, PCR-UACT/UPATH/UQTR-I01 and four FIELD records TAPE_UNAVAILABLE (D-166,
  D-249). **The first session before the tapes, the intervals and the trade counts, from an
  empty disk.** Not yet looked at: the option values themselves from the old-layout quotes
  (dump and inspect), META's FB-era bodies. The block went on to 2022-06-07 (download 14
  minutes, computing since 14:53 UTC). `v-day-1210` and `v-day-0411` are in intent B (first
  halves: 512 of 512 each; 2025-04-11 says COMPLETE_WITH_FAULTS with `failed_other_tasks` 1 and
  no measure fault: to read when it ends). `v-day-0716c` still downloading (14:55 UTC).
- **2025-04-11 (build d55f6439) finished its second half COMPLETE_WITH_FAULTS: `faulted_engine` 2,
  2,682 of 2,688, both VS-OIJ01 (15:37 UTC, `summaries-0411/`).** (1) EMB: its history export fails
  at 2024-12-30, a session on which the provider delivered EMB's whole chain (868 contracts) with
  open interest 0 (60,951 on 12-27, 60,929 on 12-31): `OIJ01 zero positive OI without a provider
  fault`; VS-OIJ01 would be an ENGINE_FAULT for EMB on about 200 sessions. (2) ETHA: its first
  chain (168 contracts, no open interest) faults VS-OIJ01 where a stated absence is due
  (C-034); its four change sensors are correctly EMPTY_DELIVERED_POPULATION. An agent is fixing
  both on `pass-s3i` (D-255..) and checking whether VS-O02/O04/DLM-OI-CHANGE publish a wrong
  change across such a zero session (their exports completed). 2022-06-07's first half is
  clean (496 of 496); `v-day-1210` in intent B; `v-day-0716c` downloading.
- **Failure-surface audits (owner, 2026-11-16; `tools/audit/README.md`, template
  `scopes/failure-surface.md`):** three started 15:40 UTC on `pass-s3i`, reports to
  `~/claude-artifacts/audits/2026-11-16-surface-round-n/`: `provider-eras` (what else assumes
  today's provider shapes on old dates), `one-bad-session` (what else fails or computes across
  one degenerate session in a window), `empty-home` (what else exists only as a side effect on
  a stateless worker, and what a newest-first build's history readings state before earlier
  sessions exist). Triage with a verifier before fixing; Codex has about a third of its
  allowance left until 2026-11-19.
- **2025-04-11 was NOT published (15:48 UTC): core refused its bundle, `422: M0-PRINT-SIZE/ETHA is
  listed twice`, after 5.5 hours** (23,467 capture requests), and the refusal paused the queue.
  The worker's log: one export fault, `ETHA--M0-PRINT-SIZE ... PRINT product
  scientific/calculation/parent linkage`, then `final_slots 2854` (2,853 are due). The
  open-interest fix agent has it as a third finding (D-257: why that export faults on ETHA's
  first chain day; a bundle never lists a slot twice and the engine checks its own bundle before
  publishing). Job 27 and its pass (28) cancelled, the old 12-11 pass (22, build 0ca127ea,
  re-queued as foreseen) cancelled, **queue resumed 15:49 UTC**. Still leased: 24 (2025-12-10),
  53 (2024-07-16), 55 (the 2022 block). 2025-04-11 runs again under the final image.
- **15:57 UTC: the three 2025-04-11 findings fixed on `pass-s3i` e6b5361a (pushed), none run on the
  cluster.** D-255: a chain with contracts and no positive open interest after one (within 21
  sessions) that reported it on contracts still listed is an outage: that session's
  open-interest sensors WITHHELD PROVIDER_OI_INCOMPLETE, the changes that touch it withheld.
  **A wrong number was being published before (read from the code): DLM-OI-CHANGE about -60,951
  and +60,929 around EMB's 2024-12-30, VS-O02's total 0.** Limit: a book that really closed
  every position in one session reads as an outage (a false absence). D-256: a first chain
  without open interest is VS-OIJ01's empty field (ZERO_ELIGIBLE_OI_MASS; NO_PRIOR_HISTORY).
  D-257: a slot is staged once and the bundle refuses a duplicate before sending; the cause was
  a leftover `.building` directory beside a fault body (any staged slot whose read-back failed
  could have doubled). Round o audits these with the four earlier unaudited commits
  (`2026-11-16-cluster-round-o`). The flag-line fix (D-258) is being written.
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
collision finding (D-246). **Audits keep running after every landed step (owner, again 2026-11-16: converged or not, even if it costs fixing time); convergence only decides what gates the build.** Rounds since: l (early-date fixes), m (snapshot reader), n (three failure-surface audits). Not yet audited: the round m fixes (0aad5c56, 6bcd0039, 7977a40c), the tape-listing fix f4b0b62a, and the open-interest fix in progress: audit them together when it lands.

**Branches and worktrees** (`/home/diggz/wt/<name>`): candidate `s3i`; `fx9` (branch
`pass-fx10`); the merged batches `pass-fx-all` to `pass-fx8` and their parts are pushed and their
worktrees can go (preview and ask: about 40 worktrees). Core: `main`; `/home/diggz/wt/fx-core`.
Engine `worker` checkout: `/home/diggz/vyreon-engine-worker`.

## Decisions waiting on the owner
- **Build depth, sized (15:42 UTC; `~/claude-artifacts/lookback-inventory.md`, read from the code,
  nothing run; newest session taken as 2026-11-13):** store-derived readings are supported from
  126 prior published sessions and at full window from 252; the unusualness flag line pooled
  over full-window scores needs 315 to 344; fixed tape depth 189; a day's own option window is
  223 prior sessions. **A day reads an old-layout snapshot only if it is on or before
  2025-04-09, and reaches the 2023 gap only if on or before 2024-01-25.** Recommendation to the
  owner: build back to **2025-04-10 (401 sessions)**: every sensor at full window on the newest
  week, no old layout, no gap. Floor 130 sessions (flag lines then rest on about 84 sessions,
  unstated). Deepening later changes `line_95`, EVC01's pool and partial percentiles on days
  already published (a re-derive, not a rebuild). **One misleading reading found:
  DLM-UNUSUALNESS `line_95` pools whatever scored sessions exist and states no pool size or
  span** (`derive/ordered.rs:166-181`): to fix before the build; INT-TEMP-EVC01 reads every tape
  session since the tape start (states `prior_count`).
- **Answered 2026-11-16 15:45 UTC: phase 1 is the newest two weeks, then back to 2025-04-10, then
  Reporter, then the launch decision (step list above). Fix the unusualness flag line.** The
  earlier note, kept for the reasoning: The MVP is a weekly
  report: it needs only the lookback that populates every sensor truthfully; a full database is
  an advantage; the worry is that short histories make the unusualness and lookback sensors
  less than truthful. Being answered with numbers: `~/claude-artifacts/lookback-inventory.md`
  (an agent, read-only: every sensor's lookback, where its history comes from, what it states
  when short, any path dependence). The snapshot gap of about 2023-01-18..03-07 and the old
  layouts only matter if the build goes that far: decide with the depth.
- The production store's 2025-12-10 (177 withheld activity slots) is not touched until the merged
  engine is deployed there; it then needs the rebind and a republish, on the owner's word.
- Answered 2026-11-16 15:30 UTC: **the newest two weeks are the first step of the build** (build
  them first to see what they reveal), not part of 2.5. **Delete unused droplets: the two test
  droplets were deleted 15:35 UTC** (their dumps are in `~/claude-artifacts/testbed-dumps/`).
- Answered earlier: D-236 as is; pooled quantities as built; the single share report left to
  the build; a missing daily bar left to the build; audits converge (C-041 amended); step 2.5
  pre-approved (running).

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
- **Test droplets:** both deleted 2026-11-16 15:34 UTC on the owner's word (dumps of their days in
  `~/claude-artifacts/testbed-dumps/`). A new one needs the 2025-12-10 archive downloaded again.
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
