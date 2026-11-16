# Milestones

The one living status document: current state and what is next. History is in git (the long form
is at tag `milestones-2026-11-15`); decisions are in `docs/DECISIONS.md` (core, C-numbers) and the
engine's `docs/DECISIONS.md` (D-numbers); the plan is in `docs/ARCHITECTURE.md`.

## Steps to the MVP (the list shown to the owner on a status request; keep it current)
1. **Two-day cluster trial.** Done 2026-11-13.
2. **Engine ready for the build.** In progress. To close it (a Codex audit after each of a to e,
   and one more on the merged whole before 2.5: repeated passes are the method, owner 2026-11-15):
   a. [done] the seven fix tracks merged into `pass-fx-all` (done 2026-11-15); core's fixes on
      `pass-core-fix` (pushed); *audit of the merged round* (round a, triaged)
   b. [done] the follow-ups `pass-fx2-{receipt,style,small}` finished and merged, full suite;
      *audit* (round b, triaged: nothing above the line)
   c. [in progress] real days on the merged round: clean 2025-12-10 on both droplets (done: `r2`,
      `m4`), then on the final candidate a clean day, a membership day and an early date (not
      started); the AAPL tape-cache fixture regenerated (not done); *audit of whatever the days
      made us change*
   d. [in progress] triage of the audits to the owner (done; rulings 2026-11-15 20:25 UTC); fixes
      above the cut line (C-041), each batch with its real day (batches 3 to 8 written, their real
      day pending); *audit* (rounds c to f)
   e. [not started] `pass-s3i` brought up to date by a merge (not a rebase: its commits are pushed
      and cited by hash in the decisions and audits); engine to `worker` and core to `main`
      together; images rebuilt; *audit of the merged branches (the gate into 2.5)*
   f. [in progress] housekeeping: this file pruned to state and next steps; the commit before the
      prune is tagged (`milestones-<date>`) so the old one is one command away. Git is the history.
3. **(2.5) Cluster verification:** 2025-12-10; 2025-04-11 (ETHA's option listing); a day before
   2024-07-23; a block over the FB to META rename (June 2022); a day after a 10-for-1 split. Then the
   newest two weeks on two nodes. New spend: the owner first.
4. **(3) The build:** newest first back to 2022-05-27, sized and approved by the owner.
5. **(4) Reporter and web** on core's API and the built history.
6. **(5) Judge the reports; the FIELD campaign; the go-live choice.**
Working practice: at most three agents at once on the laptop; an audit after every landed step.

## Now (2026-11-16 00:00 UTC)
**Final candidate: engine `pass-fx8` (99d79488, worktree `/home/diggz/wt/fx8`, pushed).** Release
build done; its suite fails only the known tests (8 `option_field`, 2 tape-cache fixture). No more
fix batches before the real days: a new finding is listed unless it is above the cut line with data
behind it. Nothing is on `worker` or core's `main`. Over `pass` (d028637a) it holds, by subject:
- *Surface and pricing:* Treasury par yields bootstrapped to a zero curve (D-225, H5-SURFACE@21);
  exercise style (D-199/200); an American-only fit; split readers (surface, company, EVC01).
- *Open interest and the ledger:* adjusted roots never dropped as frozen and counted in the
  exposure cover (DLM-GREEK-EXPOSURE@2.2.0, D-170c); the ledger defect `r2` found; negative OI;
  duplicate-row volume; Greek exposure totals publish the unsolved count and the solved
  open-interest share as support, no gate (D-228).
- *Activity (ACT01 family):* PROVIDER_COVERAGE_START while T-5 is before 2022-12-08 (D-236); count
  readings withheld MISSING_REQUIRED_INPUT before trade counts exist, 2024-11-26 (D-237);
  SAI-ACT01-INTRADAY's copies score once (DLM-UNUSUALNESS@1.5.0); a summary array with no supported
  cell withheld; one asset's inventory aborting every ACT01 job; META's family 2022-06-06..15.
- *Listings, company, macro, day:* a newly listed asset's short history in five price sensors and
  V03's monthly rule exact to the OCC By-Laws (batch 4); company shares and guards; macro damage
  and 429; the FIELD panel; the earnings-anchor SQL defect `r2` found; wall-clock out of receipts
  (D-201); tape-cache upload retry; m0 1 and 4; under all of it the first round's seven fix tracks.
- *Core, `pass-core-fix`* (worktree `/home/diggz/wt/fx-core`, pushed, not merged): C-037 on; C-042:
  a hand publish of a history bundle needs `--vouch`. A hand publish of a base: pause the queue,
  wait for leased day blocks, publish, resume.

**Proven on real days (2025-12-10, both on `pass-fx-all` e9cce438):**
- `r2` (137.184.61.134, all 64): exit 0, no faults, 4,679 slots, no state change against pre-fix `p1`. It
  found two defects of the new build: the ledger rule (30c68c20, `frozen_by_adjustment`) dropped
  six live contracts expiring 2025-07-18 (BABA -5,725 of 2,667,522; MSTR -1,838 of 2,614,287); an
  SQL alias bug (f79c7af2, `load_events`) lost the earnings anchor on the store path (BABA, TSM).
  Both are fixed from `pass-fx5-all` on; **the fix awaits a re-run** (the totals must return; BABA
  and TSM state NO_UPCOMING_EARNINGS_DATE). Reports: `.../2026-11-15-fix-round-a/day-r2-*.md`.
- `m4` (192.241.147.166, ETHA not listed): exit 0, no faults, 4,608 slots. Against `r2` every per-asset slot
  of the other 63 assets is equal in every non-identity leaf; only the 12 FIELD slots differ. The
  membership day holds and the two machines agree.

**Running or waiting:**
- `m5` (192.241.147.166, `pass-fx5-all` 497c1608, ETHA unlisted) running since 2026-11-15 21:35
  UTC; in its history stage at 23:53. `r3` (137.184.61.134) refused by the disk guard (45 GB
  free, 60 needed).
- Next: deploy `pass-fx8` to both test droplets when `m5` ends and test-1 has disk; a clean day, a
  membership day, an early date; regenerate the AAPL tape-cache fixture from a droplet's raw tape.
- Before that day, check on a droplet: whether captured dividend adjustment records carry
  `split_ratio` (D-206), and how many root-days state no exercise style (D-199).

**Audit rounds** (Codex, read-only; tool in `tools/audit/`; under `~/claude-artifacts/audits/`):
- a, `pass-fx-all` and `pass-core-fix`: `2026-11-15-fix-round-a/triage/SUMMARY.md` (and
  `data-scans.md`: the archive scan on test-2 found no latent condition).
- b, the follow-ups: `2026-11-15-fix-round-b/triage/`; nothing above the line.
- c, batch 3: `2026-11-15-fix-round-c/triage/SUMMARY.md`. d, batches 4 and 5:
  `2026-11-15-fix-round-d/triage/`. e, batch 6: `2026-11-15-fix-round-e/triage/SUMMARY.md`.
- f, fix 7: `2026-11-15-fix-round-f/counts.md` (three findings, all fixed in `pass-fx8`). g, those
  three changes: `2026-11-15-fix-round-g/` (started 2026-11-16 00:02 UTC).

**Branches and worktrees** (`/home/diggz/wt/<name>`; cargo target deleted when a branch is merged):
- Merged batches, oldest first, pushed: `pass-fx-all` e9cce438 (`fx-all`), `pass-fx2-all` db15396f
  (`fx2-all`), `pass-fx3-all` a58bced0 (`fx3-all`), `pass-fx4-all` abf1ae22, `pass-fx5-all` 497c1608
  (`fx5-all`), `pass-fx6-all` 5afdde54 (`fx6-all`), `pass-fx7-all` 703bade8 (`fx7-all`), `pass-fx8`
  99d79488 (`fx8`). Parts: `fx3-a`, `fx3-b`, `fx4-a`, `fx4-b`, `fx5-curve`, `fx5-pull`, `fx5-greek`,
  `fx6-a`, `fx6-b`, `fx7-counts`.
- `pass` d028637a and `pass-s3i` 286f5c68 (plus D-162, cluster only), pushed. Core: `pass-core`,
  `pass-core-fix`. Engine `worker` checkout: `/home/diggz/vyreon-engine-worker`.

## Decisions waiting on the owner
- Delete ten old run directories on 137.184.61.134 (`vyreon-test-2`) to free about 142 GB: `a3`,
  `a4`, `d1`, `i1`, `l1`, `s1` and the empty `k1`, `q1`, `r1`, `r3`; keep `p1` and `r2`. Asked
  2026-11-15.
- D-236 withholds the whole ACT01 family on 2022-12-08..12-14, including readings that need only
  T's intervals: keep or narrow.
- The production store's 2025-12-10 still carries 177 withheld activity slots; it needs the rebind
  and a republish. Owner's word before touching production (2026-11-14).
- The rule for the pooled quantities (financing spread, activity feed check, unusualness line:
  pooled over the assets present) was "owner to confirm" on 2026-11-14; no answer is recorded.
- With exactly one visible share report the company unit guard has nothing to compare and the
  metrics publish: leave it until the build shows how often (recommended), or treat as unevaluated.
- Answered, not waiting: a missing daily bar withholding an asset's surface for about three years
  is left until the build shows one (2026-11-15; its label is fixed).

## Listed, not fixed
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
  `vyreon-test-1` is 192.241.147.166 (runs `g3`, `j1`, `u1`, `m3`, `m4`, `m5`); `vyreon-test-2` is
  137.184.61.134 (runs `p1`, `r1`, `r2`, `r3` and older; 45 GB free). **The 2026-11-15 entries of
  the old file and the audit notes call 137.184.61.134 "test-1" and 192.241.147.166 "test-2":
  go by the address.** Both carry build `497c1608+55b96cfbd3ee`. No third test droplet exists.
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
- `pass-fx8` on any real day; batches 3 to 8 on a finished day; the two `r2` fixes on a re-run.
- An early date: before the tapes (2023-05-24), option intervals (2022-12-08) or trade counts
  (2024-11-26); a real pre-listing day (the fill skipping an unlisted asset).
- D-165 and D-167 on a rename date; D-162; D-166 on a post-listing date; a day after a 10-for-1
  split; the split-basis scores (need a built history).
- History in the ordered pass over several days; the cross-day supersede rule; a block of more than
  one day; the same day equal first in a block and mid-block; rebuilds and recomputes for real.
- The rebuilt images; KEDA and scaling to zero; more than two nodes; the node type by cost per
  published day; backups (C-013); whether presigned downloads count against the 2,000 a minute.
