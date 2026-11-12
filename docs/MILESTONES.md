# Milestones

The one living status document. History is in git; decisions in `docs/DECISIONS.md`; the plan in
`docs/ARCHITECTURE.md`.

## Where things stand (2026-11-10)
- Engine: `engine-v1` tagged (vyreon-engine a046bf54). A fresh 2025-12-10 agrees laptop vs droplet to
  within 1e-11 (2026-11-09). The droplet is authoritative: Postgres, the daily capture, health checks.
- Design accepted by the owner (`docs/ARCHITECTURE.md`, design pass 2); decisions C-001 to C-018.
- Kept inputs: the droplet archives prints, ACT01 pulls and company captures nightly to the volume and
  the Spaces bucket `vyreon-data` (C-004).
- Repo renamed from vyreon-manager (2026-11-10).
- The historical database build waits for the cluster (owner, 2026-11-08). Go-live is on the fixed 64.

## Plan (ARCHITECTURE §8)
1. **Core service** (this repo): schema, universe for the 64, worker API, publish through core,
   scheduler, budget, then the reader API (C-020). Exit: a droplet-published day goes through core and
   reads back identical. **Done.**
2. **Engine as worker** (engine v1.x: E1–E5, ranged bars, E10–E13), alongside 1. Exit:
   golden days through the worker path; the block-start test.
3. **Cluster:** DOKS, CloudNativePG, KEDA; build admission; a 2-node trial on a recent week with the
   measurements (C-006).
4. **First build:** the tape job for every session since 2023-05-24, day blocks from ~2025-03 on the
   fixed 64, the ordered pass behind them.
5. **Rebuilds and recomputes** exercised for real.
6. **After:** backups (C-013); the reader API for vyreon-reporter; N assets (E6–E9) and the backfill to 2021-09-27;
   the droplet retired.

Plan approved by the owner 2026-11-10 (audited: engine track 4–6 weeks, first database January 2027,
first build $200–350; of ~420 sessions the last ~170 carry full 252-session context).

Done:
- A1–A2: the workspace and the migration runner (`vyreon-core migrate`; tested on PostgreSQL 16:
  repeat runs, four concurrent runners, a changed file refused, a failing migration rolled back).
- A3: the store schema (C-021): the engine's tables partitioned by month, each row's date tied to
  its revision's, partitions made by `vyreon.ensure_months`, the reader views and read-only role.
  Tested with a real slice of 2025-12-10 from the droplet (AAPL, six sensors, both revisions): it
  lands in its month, bodies re-hash to their digests, a date filter reads one partition.
- A4, first part: engine builds and their slot inventories; a revision needs a registered build.
  The other core tables land with the steps that use them (C-022).

- A5: the universe for the 64 (C-023), from real Intrinio responses. `vyreon-core universe on
  FREE_REPORT_64 2022-06-08` lists the 64 with META as FB and IBIT, ETHA not listed; from 2024-07-23
  nothing changes, as the plan assumed. New fact: IBIT was a Defiance ETF's ticker 2022-09-08 to
  2023-05-01 (matters for the backfill, not the first build).

- A6: publish through core (C-024). **Milestone 1's exit is met:** the droplet's published
  2025-12-10 went through core into an empty database in 17.6 s and reads back identical (18,724
  lines compared: every slot, stream, scalar and revision). A wrong bundle (eighteen kinds tested)
  is refused and leaves nothing.

- A7: the queue, the budget, tape caches and the worker API (C-025). `vyreon-core serve` runs the
  service. Over real HTTP, a tape worker, a day worker and the ordered pass did 2025-12-10 with the
  real slice, a real AAPL tape cache, the engine's calendar and the real universe; a refused bundle
  paused the queue; a failing worker's log was kept. `vyreon-core schedule day-block FREE_REPORT_64
  <build> 2025-03-03 2026-11-10` makes the first build's 15 blocks over 427 sessions.

- A8: the reader API (C-026). With a key, `GET /v1/assets/AAPL/history?from=…&to=…` returns every
  session of the range, published or explicitly not, with each measurement's body or headline
  scalars; bodies and streams are served byte for byte. Tested over HTTP on the published real slice.

- A9: notices to the Discord channel through an outbox (C-027): queue paused and resumed, blocks
  done, builds admitted, the calendar nearing its end; silent, naming no one.

**Track A is complete** (2026-11-10, 32 tests on PostgreSQL 16 with real data). Not in it, on
purpose: automated admission (track B's proof harness writes the evidence; C-027), Parquet output,
core's own Intrinio fetch, Kubernetes manifests (milestone 3).

Now: track B, the engine as a stateless worker, on a branch off `engine-v1` in vyreon-engine
(B1–B7 in the plan; the items collected for it are under Open). Its proofs run on the droplet
outside 02:00 and 12:00 UTC, from a separate checkout.

Owner (2026-11-10): tracks A and B are both completed before any new DigitalOcean spend, then we
evaluate together. Spend is approved in principle (be mindful of costs), but nothing is created until
that evaluation. Track B's golden proofs use the droplet we already pay for.

## Track B work list (engine branch off `engine-v1`; plan B1–B7, ARCHITECTURE §7)
Each change is proven on the golden days: values equal `engine-v1` except what is declared.
1. B1: one declared input-window list for the plan and every reader (E12); a missing input withholds
   the assets it touches, not the day (E11).
2. B2: daily bars as one ranged request per asset (E7′, values identical); the determinism rules
   (E4: 200 option sessions, `last_adjusted` bounded, a fixed bar origin, one tape-history start),
   each value change declared. E4's exact rules are step 0 and need the owner where values move.
3. B3: the 18 day-local derived sensors computed in the day (E10); the OCC ex-date published as a
   base value; the ordered pass reads only the database, with parent revision ids from core.
4. B4: the budget drawn from core's leases; the five unmetered call sites behind the same gate;
   429 retried (E3).
5. B5: tape caches read from and written to core; a tape entrypoint (E2). The engine defines the
   cache's packing and `method_version`.
6. B6: `vyreon-worker`: claim, heartbeat, compute, bundle each day with `core-contract` (the day's
   reader scalars in the bundle, its identity check before bundling), publish, finish or fail with
   the log; stage timings and request counts with every day (E13).
7. B7: the container image, build identity baked in, disk guard by env, no git at runtime (E5).
Also: an inventory document per build for `vyreon-core build register`; the evidence file for
`build admit`.

Started 2026-11-10 in the worktree `/home/diggz/vyreon-engine-worker` (branch `worker` off
`engine-v1`; the main checkout is untouched). Engine tests there need
`VYREON_FIXTURES=/home/diggz/vyreon-engine/workspace/fixtures` (the fixtures are untracked).
- Done (8b0f183b): the readers' window lengths are `day_inputs`' declared windows, resolved at
  compile time (B1, first part; no value changes); `list_sessions all` prints the whole calendar
  with opens and closes (it equals core's loaded copy: 2,261 sessions, same digest).
- Done (the commit after it): the plan's new `daily_window` entry checks every daily capture the
  views read, per asset from its first session where the reader does (B1/E12). The droplet's archive
  agrees with the rule (SPY 380 sessions from 2024-06-06, ETHA 349 from its listing). Its
  real-archive test (`plan_verdicts`, ignored by default) runs with the golden proofs on the droplet.
- E11 (a missing input withholds its assets, not the day) is studied, not started. Facts (engine
  `worker` branch): the plan's per-asset entries already name the assets; a faulted task's slots are
  already published as complete withheld bodies in each family's schema (`Faulted`, reason
  `ENGINE_FAULT`; D-117 added the provider-limited `EMPTY_DELIVERED_POPULATION` the same way); h5-field
  already treats an absent member as missing cells (owners needing all 64 withhold, the others
  compute while 51 are eligible). What stands in the way: six stages read every asset's inputs before
  any task runs and stop the day on the first error (surfaces at T and T−5, intent A's history rows,
  intent B's views and sources, the trade guard, VS-IVS01's exports, the FIELD driver's missing-view
  refusal), and there is no reason code for an absent provider input. Carry, macro and company stay
  whole-day inputs. Design questions are with the owner (Open).
- Done: `core-contract` copied into the engine byte for byte (it builds from vendored crates only;
  `tar` and `filetime` vendored), and `vyreon-worker bundle` / `inventory`. The laptop's retained
  real exports of 2025-12-10 (448 slots, 128 streams of 200 MB, 625 reader scalars) were bundled by
  the engine in 1 s and published through core: every stored body hashes to its digest. The export
  loading moved from the store's binary to its library, unchanged.
- Done (B6, first form): `vyreon-worker run day-block` claims a block, runs each session with the
  engine's multi-day runner (download ahead, prune behind), bundles the day and publishes it through
  core, then finishes, or fails with the day's logs. The day has a publish-through-core mode beside
  its store path, which is unchanged. The client was run against a live local core: claim,
  heartbeat, publish of the engine-written bundle, the refusal of a second copy, finish.
  Not yet exercised: a whole computed day through the loop (it needs the droplet), stage timings and
  request counts in the bundle (E13), the build identity without git (B7).
- B5 (tape caches through core) is studied, not started. Facts: a day needs, per asset-session, only
  `capture/manifest.json` and `capture/derived/{derived.json, body.json, body-5m.json}` (the raw
  per-security files are read only to derive); a session without a tape is itself a small cache
  (`withheld_session`), not an absence. Three bindings decide the design:
  (1) a cache is valid only for the build's `READER_IDENTITY` (a hash of eight source files): that
  is core's `method_version`, and a build that touches those files recomputes every tape (~$15);
  (2) a cache names the daily attempt directory it was derived against and is re-derived (which
  needs the raw files) when the local name differs or the day's daily bar is absent;
  (3) so the day needs the daily bar of **every** session whose cache it reads, back to 2023-05-24,
  while it downloads only 380 sessions of bars. A full tape history has never run with a short bar
  window. Ranged bars (B2, done) supply them cheaply: 31 months an asset back to 2023-05.
  Later, for daily operation: a recent session's bar is re-captured for three sessions and its cache
  re-derived, which core's write-once rule does not allow yet.
- Done (B2, engine f71c2ae4..054189cf): daily bars are one ranged request per asset per calendar
  month (`INTRINIO-CI-DAILY-RANGE-1@1`, page of 100, so one page). The capture is published as the
  month's last missing session; every other session gets its own copy of the pages (hard links) under
  a manifest naming its date, and the verified decode reads exactly that date's row after the same
  URL, chain, clock and digest checks. So each session keeps its own capture root and every reader
  and view works unchanged; single-day captures still read. A fresh worker's 380-session window is
  19 requests an asset instead of 760 (1,216 for the 64, was 48,640).
  Proven on real data: AAPL's November 2025 captured through `h2-engine live-capture-batch` (frozen
  as the test's fixture), each bar equal to the single-day request's. On the droplet (read-only,
  2026-11-11): every archived single-day bar of the 64 assets, 39,009 bars from 2024-06 to
  2026-11, compared with ranged responses: 38,882 identical in every field, 127 recent bars differ
  only in `adj_volume`, which the engine does not read; no row missing, none without `factor`
  (P01's adjusted-price fallback does not arise), no response paged.
  Declared changes for the golden comparison: the daily sensors' binding statement (so their
  receipts' `binding_checksum`), every ranged session's capture root, and with it
  `current_daily_capture_sha256` and `source_input_set_sha256`.
  Not changed: T's own bar is still superseded by a single-day re-capture (128 requests a day).
  Left for the golden run: a whole day computed from ranged bars.
- Done (B4, engine 88bad073, b9046fb5): every Intrinio API request in the engine (the capture
  engine's transport, macro, replay listing, carry, company, closing-quote listing: seven crates)
  asks a request gate first. `vyreon-worker run day-block` serves the gate on a socket in its home
  from core's budget leases (100 requests at a time, given back when used, idle or near expiry)
  and prints requests by category at the end. Without `VYREON_BUDGET_GATE` nothing changes, so the
  droplet's daily operation is as it was. Against a running core: 150 permits from three threads
  were four leases, all released, and `core.request_usage` holds 100 and 50 by category.
  Not done: file downloads from presigned URLs are not gated (the open Intrinio question); the
  engine's own per-stream pacing is still on underneath; each site's handling of a 429 is not yet
  audited (E3); the counts are printed, not yet in the bundle (E13).
- Done (contract 2, C-030; core ecf69fa, engine the two commits after b9046fb5): the bundle carries
  each body-projected slot's coordinates view and the run's report (E13: stage seconds, requests by
  category). Core stores both and serves `GET /v1/assets/{object}/history?...&detail=coordinates`:
  every coordinate with its value, state, reason, unit and quality, exactly as the engine projected
  it (tested on a real engine-written bundle of AAPL's five body slots). Stream-backed M0 slots
  carry no view (it would double stream storage); their coordinates are in the stream core serves.
- B7 in code (engine 36b2def2 and the commit before): a build record beside the binaries
  (`build-record.json`: the commit and its code identity) gives the build identity where there is
  no checkout; the binaries' own compiled identity still decides clean or dirty, so a record cannot
  vouch for other sources (tested against the built binaries). `vyreon-worker` is now a stamped
  binary. `deploy/worker/Dockerfile` builds from `git archive HEAD` with the droplet's pinned
  libraries (MKL 2022.1.0, MPFR, GMP, the SQLite library's bytes) and runs
  `vyreon-worker run day-block`; the disk guard's second path is off by env. Built on the laptop
  (`vyreon-worker:36b2def2`, 1.5 GB): no git inside, every library resolves, the record's code
  identity is the one compiled into h3-asset, and the worker reaches its claim call. Its values
  against the droplet's binaries belong to the golden run.
- B5, the day-block side, done (engine 454e35d2 and the commit after): a cache is packed as the
  capture's facts and derivation (AAPL 2025-12-10: under 64 KB, the same bytes every time) and
  unpacked only after it reads as a day would read it. `vyreon-worker run day-block` first fetches
  from core every cache its archive lacks for the tape history, then the fill captures what core
  does not hold, as before. The reader's identity is unchanged from `engine-v1`, so caches derived
  by the droplet are valid for this build. Against a running core: a tape job stored AAPL's real
  cache once (a second put is not new, another session's is refused, the session is not done with
  63 members missing) and an empty archive got it back byte for byte.
  Two rules that make a cache current on every worker, both outside the reader's sources:
  a daily bar first captured after its day (UTC) is settled and keeps its attempt name (no
  superseding re-capture; also 128 fewer requests a day in a historical build); and the bar of each
  cached session before the 380-session window is captured too (ranged).
  **A value question for E4:** the droplet derived its oldest tape sessions with no daily bar
  (official volume unavailable); a tape worker always has the bar, so those sessions' volume
  reconciliation gate will run where `engine-v1` withheld. It is declared with E4.
  The tape entrypoint (`vyreon-worker run tape`) claims a tape job and, per session, captures the
  session's bar and its round-lot evaluation month (ranged) for every member trading by then,
  captures the tapes, derives and stores each cache, and marks the session done; it draws the
  budget and heartbeats as a day block does.
  **Proven on the droplet, 2026-11-11** (branch built clean there in 6 min, separate checkout and
  binaries; core on the laptop through an SSH tunnel): the tape job of 2025-12-10 took 6 minutes
  (tapes 330 s, 1.5 GB peak memory), stored 64 caches (1.3 MB in all) and finished. Against the
  droplet's own caches of that session all 64 fifteen-minute and 64 five-minute bodies are
  byte-identical; only provenance differs (the ranged bar's page digest, clocks, tape object
  records). The run found that the capture child, started with a cleared environment, did not
  see the request gate: fixed (engine f4dad345).
  Known limits: the droplet's existing caches cannot seed core (they name its superseding daily
  attempts, which a ranged-bar worker does not have), so the first build recomputes the tape
  history as planned; a build that changes the reader's eight source files recomputes every tape;
  a member core lists on a session that the engine's security record does not is refused at done.
- **The golden day through the worker, proven on the droplet 2026-11-11** (engine f4dad345, clean
  build `f4dad345+175685379b31`): `vyreon-worker run day-block` claimed 2025-12-10 from core (on the
  laptop, through an SSH tunnel), computed it on a copy of the droplet's home and published it:
  2,853 slots, 2,529 coordinate views, the run report, job done, 155 minutes (history nodes 37 min,
  the day's assets 54 min). Compared slot by slot with the day the droplet published
  (`71f3fe5d`): every slot state, version and event stream is equal, **no numeric value differs
  anywhere**, 1,573 bodies are byte-identical, and the other 1,280 differ only in the run's cutoff
  clock, the calendar digest (the frozen calendar grew to 2020–2028 in `engine-v1`, after that day
  was published) and the provenance digests the golden comparison already names.
  What this run did not exercise: ranged bars and core's tape caches inside a whole day (the copy
  already held the droplet's single-day bars and its caches; a ranged-bar day needs the tape
  history recomputed first), a block of more than one day, and the container image's binaries.
  A copied home is refused by the input plan at another path (its documents name absolute
  paths), so the proof mounted the copy at the home's own path in a private mount namespace; the
  real home was never visible to the run. Left on the droplet for further proofs: the checkout
  `/root/vyreon-engine-worker` and `/root/worker-proof` (binaries, the 57 GB home copy).
- E3 audited (2026-11-11), no code needed: of the seven request sites, replay listing, the JSON
  provider calls, company and carry retry a 429 themselves with backoff; the capture engine's
  transport and the macro capture fail the chain, and the fill retries the step after 60 s and
  300 s (three tries of 30 s for macro). A retry inside the capture engine's transport would send
  requests its account ledger does not record as attempts, so it stays at the step. With the gate
  holding all workers under core's ceiling, a 429 is the exception this handles.
- E11 mapped for coding (engine `worker`, 2026-11-11; decided in C-029). The work, in order:
  1. **The reason.** `h1-contract/src/lib.rs` `mod fault` (lines 32-58): `REASONS` (ENGINE_FAULT,
     PARENT_ENGINE_FAULT), `EMPTY_DELIVERED_POPULATION`, `SLOT_REASONS`, `is_reason`,
     `is_slot_reason`. Add `PROVIDER_INPUT_MISSING` to `SLOT_REASONS`. About 28 family validators use
     `is_reason` (two fault reasons only; only v03, v03-matched, g01 and m0-contract use
     `is_slot_reason`), so each family's withheld-body builder and validator must accept it;
     `h3-asset/src/node.rs` `fault_receipt` (961) and `fault_receipt_ok` (1041) check the closed list;
     `vyreon-coordinates/tests/faulted_bodies.rs:39` and `tools/reference/refcheck/causes.py` name the
     reasons; `h5-field/src/option_field.rs:41` `REASON_PRECEDENCE` ranks an unknown reason last.
  2. **The executor** (`h3-asset/src/h4_run.rs`): a missing-input marker on the intent's asset
     (`load_intent` 449 requires `option_eod` 503 and `daily_bar` 527 for every asset; the canonical
     re-serialisation check at 459 needs a struct field); `slot_reason` (994) returns the new reason
     for that asset's tasks (the D-117 template: `empty_slot` 958, `reads_empty_population` 966,
     `FaultQueue::push` 949, `fault_job` in catalogue.rs 1386, `execute_faulted` node.rs 1081);
     `outcome_of` (2503) and `summarize` (2519-2558) get a new outcome kept out of `faulted`
     (the 10% hold, `hold_threshold` 2478), and a hold when more than 6 of 64 assets are withheld.
  3. **The six stages** (`vyreon-cli/src/day.rs`, `views.rs`, `guards.rs`, `exports.rs`), easiest
     first: FIELD (day.rs 612-622: pass the present symbols; h5-field already treats the rest as
     missing members, `tests/fault_members.rs`); `surface_exports` (day.rs 591, exports.rs 370);
     `equity_trade_guard` (guards.rs 111; intent B must then not bind `security_interval`,
     views.rs 730); intent A `history_rows` (views.rs 235-268, 325); intent B `asset_views`,
     `evc_history`, `intent_b_sources` (day.rs 510-522; views.rs 482-563, 728-764, `act01_route`
     678-699); the surfaces (`surface_identity` views.rs 369-375; one `h5_surface` process for all
     assets, and its identity names every asset, so the directory key changes).
  4. **The plan** (`vyreon-cli/src/plan.rs`): per-asset blocking entries already name the assets
     (`Entry.assets`, lines 204-574); the day stops at day.rs 270 on any refusal. Per-asset absences
     must become withholding instead, up to the limit; `carry`, `macro` and `company` stay day-wide.
  Step 1 is done (engine 17202e14): the reason exists and all the families' containment bodies
  build and validate with it (their own tests now run over it); nothing produces it yet. Steps 2
  to 4 remain.
  **A smaller design for steps 2 to 4, chosen 2026-11-11 after reading the executor** (4,266 lines,
  its intent canonical and versioned): do not teach the executor about a withheld asset. The day
  driver leaves such an asset out of the intent (the executor already accepts 2 to 64 assets), so
  an ordinary day's intent, tasks and bodies are byte for byte what they were. The driver then
  commits the left-out asset's slots as containment bodies through a new `h3-asset` command built
  on `fault_job` and `execute_faulted` (they need no sources and no parents; D-113's
  `fault-export` is the precedent, but it looks the task up in the intent). Each stage loops over
  the present assets instead of handling an error per site; FIELD gets the present assets and
  treats the rest as missing members; the more-than-6 hold and the plan's per-asset refusals
  become the driver's rule. The fault count never sees these assets, so the 10% hold is
  untouched.
  Done (engine, the commit after 17202e14): `h3-asset contain-slot MEASUREMENT SYMBOL DATE CUTOFF
  MESSAGE DEST` commits and verifies the slot (VS-P01 for ETHA: all 16 roles WITHHELD,
  PROVIDER_INPUT_MISSING). Every owner the executor runs for an asset has such a slot (42 for SPY,
  42 of AAPL's 47). The other five are the company family, which h5-company computes from the
  day's company capture and which does not depend on the asset's option, bar or tape inputs, so a
  left-out asset keeps them. VS-IVS01 (from `h5_surface`) still needs its withheld export.
  Done (engine 773f93b1): the driver. When all that refuses the day is some assets' own Missing
  provider input (`options`, `daily`, `consolidated`, `option_history`, `daily_window`), those
  assets are left out, logged with what each lacks, and kept in `withheld-assets.json` for a
  resume; more than 6 holds the day; a stale input, a day-wide one, an absence naming no asset or
  any other input still refuses it. The stages run over the remaining assets; after the export
  index the left-out assets' slots are written as containment exports, and VS-IVS01 as the
  surface's own body with every reading withheld. Tested: the rule over plan entries, the slot
  coverage per asset, the withheld surface body through the coordinates projection.
  **Proven on a real day (droplet, 2026-11-11, build `c676d944+9f32b85c5a6a`):** 2025-12-10 in the
  home copy with UNG's option chain of the day set aside and filling off, published through core in
  166 min. The plan left UNG out (`PROVIDER_INPUT_MISSING`: options, option_history) and the day
  ran over 63 assets. Against the golden worker run: all 2,853 slots present; UNG's 43 slots are
  all withheld bodies naming the new reason; every other asset's per-asset slot has no numeric or
  state difference (differences are the cutoff clock and digests that cover the 64-asset input
  set; no stream's events differ). FIELD followed h5-field's own rules: breadth, distribution and
  concentration computed over 63 of 64; every pair without UNG kept its pearson, spearman and
  concordance exactly (only `common_scale_pearson` moved, 4th decimal), UNG's 63 pairs withheld
  `INCOMPATIBLE_COMMON_SUPPORT`; the owners that need all 64 (DIM01, RMT01, SUB01, SYNC01) withheld,
  with their existing reasons `NO_PRIOR_HISTORY` / `INCOMPATIBLE_COMMON_SUPPORT`.
  **Two things for the owner to judge:** (1) the driver withholds the whole asset, so UNG's price
  and tape sensors (VS-P01, PATH-*, …) were withheld although only its chain was missing, and that
  is also what costs FIELD its four all-64 owners; withholding only the slots that read the
  missing input is possible but is a larger change (the executor's intent requires every asset's
  chain). (2) `NO_PRIOR_HISTORY` is a misleading reason for "a member is missing today".
  Found on the way: the proof home lacked carry 2025-12-03 (the golden run's prune removed it); a
  one-day rerun needs T-5's carry kept or fetched.
  None of it is in the tape reader's eight source files. Tests need a real day with one asset's
  real input left out (as `h5-field/tests/fault_members.rs` does), and the golden days must be
  unchanged when nothing is missing.
- B3 mapped, not started (engine `worker`, 2026-11-11; an agent's read, unverified where marked).
  The derived layer is `vyreon-cli/src/derive/{mod,archive}.rs` (I/O) over `h5-derived` (science); it
  writes straight to Postgres as its own profile `VYREON-DLM-PROFILE-2.0.0` (1,826 slots) and the
  worker path calls none of it. What the 18 read from Postgres today, each of which has a file in the
  run's exports: T's VS-O02/VS-P01/VS-IVS01 bodies (`load_with`), the surface digest from the
  VS-IVS01 receipt (`surface_file`), M0-GAMMA's stream (`gamma_stream`; the export's is not gzipped),
  FIELD-DEP01 for BETA (`spy_pairs`), the 8-K ledger over all published days (`load_events`; T's own
  CDD-EVENTS01 carries it, equality unverified), the object lists (`expected_slot`). To change in
  the science crate: `event_records` returns EVENT-WINDOW and IV-CRUSH together (split);
  `greek_records` takes IV30 from the DLM-IV30 record (read VS-IVS01's `iv30`); the graph's parents
  follow. **Body identity changes:** every derived body hashes the list of published revision ids
  (`History::digest`) into `result_id` and names the base revision id in `parents[]`; a worker knows
  neither, so the 18's bodies and result ids will differ from `engine-v1` while their roles must
  not. Two things to settle before coding: (1) one profile or two (core takes a bundle only when
  its slots are exactly the build's inventory for that profile, so either the 18 join the base
  profile or the derived profile is split in two); (2) the technical seven run over every bar on
  disk, so their values depend on where the bars start: that is E4's fixed bar origin, an owner
  ruling. Also found: the ex-date scan reads 260 sessions of close-quote files but retention keeps
  200, and a missing file reads as "no ex-date" silently (unverified whether it has moved a value).
- B3 started (shape decided, C-031: a second profile and bundle). Steps: (1) science: EVENT-WINDOW
  split from IV-CRUSH (done, engine 3a000f2a; `greek_records` already takes IV30 as a value), a
  day-local body builder; (2) `vyreon-sensors`: a day-local flag, the two profiles, slots,
  inventory, graph; (3) `vyreon-cli`: inputs from the run's exports, the 18 computed per asset, an
  export set, the second bundle, a DayRun stage, E11's withheld assets; (4) core: contract 3 and
  the supersede rule; (5) the ordered pass and the ex-date as a base value. Proof on the droplet.
  Step 2 done (engine 7a3e603b): `DAY_DERIVED_PROFILE` (1,105 slots) and `HISTORY_DERIVED_PROFILE`
  (721) beside the old derived profile, which the old derive still writes; the worker's inventory
  lists base + these two. Step 3 next, as `vyreon-cli/src/derive/local.rs`, copying the day-local
  branches of `derive_day` (mod.rs 611-837) over file inputs: a one-day `Store` from the run's
  `exports/<OBJ>--<MID>/body.json` (roles filtered as `PARENT_ROLES`, `body_sha256` = digest of the
  file, `population_root_sha256`), the surface digest from the VS-IVS01 export's receipt, the
  M0-GAMMA export's plain `events/` (`gamma::read_central` reads gzip: add a plain reader),
  FIELD-DEP01's body for `spy_pairs`, CDD-EVENTS01's body for the events, `iv_today` = VS-IVS01's
  supported `iv30`. The laptop's frozen exports hold only VS-O02/O06/P01/L01/V02 and two M0 owners,
  so a laptop test can cover the chain sensors for DBA and VGK at most; the proof is the droplet's.
  **Step 3's computation proven (droplet, 2026-11-11, engine 64c1988d):** `vyreon-worker
  derive-local` over the droplet store's published base day 2025-12-10 written out as export
  directories (274: VS-O02, VS-P01, VS-IVS01, M0-GAMMA with its stream, FIELD-DEP01, CDD-EVENTS01)
  and the droplet home's archive and surfaces: 1,105 slots in 61 s, 0.4 GB; every role of every
  slot equals the droplet's published derived day (1,105 of 1,105, all 18 sensors, same method
  versions). So the event window from T's own ledger equals the one read over published days, on
  this day. Proof scripts: `/root/worker-proof/b3/` on the droplet.
  **Steps 3 and 4 done (engine 99a9ddbe, core b6b3490):** the day computes the 18 after its base
  bundle and sends a second bundle on it (`day_local` in the header; core keeps the flag, migration
  0010, and an earlier day's republished base no longer supersedes a later day's day-local
  revision). Checked on real data without a whole day: (a) with UNG's four parents replaced by the
  E11 run's withheld bodies, UNG's 18 slots withhold every measured role as `DEPENDENCY_WITHHELD`
  / `PROVIDER_INPUT_MISSING` and the other 1,088 bodies are byte-identical; (b) the droplet's
  exported base day and the derived bundle published into a scratch core: revision 2 bound to
  base 1, day-local, 1,105 slots each with its coordinates view, 5,528 reader scalars equal to the
  droplet's for these sensors. **Not yet proven:** the stage inside a real day run (it has only
  been built and unit-compiled there), the cross-day supersede rule (needs two real days: the
  multi-day block proof), and no laptop test from a frozen slice exists for `derive::local` (the
  laptop has no surfaces, populations or bar archive). Left in B3: step 5, the ordered pass over
  core's database for the 12 history sensors (C-020: a read-only database role).
  **Step 5 scoped (2026-11-11).** The ex-date needs no new published value: T's VS-O04 body (and
  VS-OIJ01's) already states `history_terminal: "CORPORATE_ACTION <date>"`, the latest OCC
  ex-date in [T-200, T] (`views.rs` `history_rows`; 2025-12-10: BABA 2025-06-12, XLB/XLE/XLK
  2025-12-05), which is the derive's `action` with a 200-session lookback instead of 260. Only
  OI-CHANGE and OPTIONS-VOLUME read it. **A value question for the owner (E4):** an ex-date 201 to
  252 sessions back restarts OPTIONS-VOLUME's 252-session window in the old derive when the files
  are still on disk, and would not under the 200 rule; a worker's home keeps 200, so the old derive
  already behaves that way on any pruned home. The work: `derive_day` computes only the 12, reads
  the 18's bodies it needs (UNUSUALNESS scores them) from the day's day-local revision, reads no
  archive, bounds its loads to the windows (252 sessions), writes exports and a bundle
  (`HISTORY_DERIVED_PROFILE`, naming core's base revision id) instead of inserting, and runs as
  `vyreon-worker run ordered-pass` under an ordered-pass lease; core's `vyreon.*` views must match
  the queries it makes (`accepted_measurement`, `reader_scalar`, `unusual_score`). The body's
  history digest then lists core's revision ids.
  **Step 5's computation done and proven on one day (engine 6ee28e14, 2026-11-11):**
  `derive::ordered` and `vyreon-worker ordered-day <date> <out>` (connection from
  `VYREON_CORE_PG_RO[_FILE]`, one read-only repeatable-read snapshot). Against a scratch core
  holding the droplet's exported base day 2025-12-10 and the day-local revision: 721 slots in 10 s
  (debug build, 0.6 GB); every role value of all 12 sensors equals the droplet's published derived
  day; 686 bodies have the same role order too, and 35 DLM-UNUSUALNESS bodies list the same roles
  in another order (the old derive read the base bodies in whatever order Postgres returned them;
  the pass reads them by measurement id). Bundled and published through core as revision 3 on base
  1: 636 reader scalars, and with the day-local 5,528 that is the droplet's 6,164. The bundle now
  carries the unusualness scores (none on a first day, as on the droplet). DLM-IV-CRUSH keeps the
  old derive's coupling (withheld with the event window when the day's chain, spot or surface is).
  **Not proven: any history.** 2025-12-10 is the first published day, so every window was empty;
  the droplet has no second derived day to compare with. A real check needs several days through
  the worker path (the multi-day block proof) against the old derive run on the same days.
  **The worker entry done (engine ce3c50b9, core 73f5a91):** `vyreon-worker run ordered-pass`
  claimed the 2025-12-10 job from a scratch core over HTTP, computed from its database, published
  revision 3 on base 1 (naming it) and finished the job. Writing it showed core would have refused
  a day's second bundle (the base publish moved the block on): a day block now posts
  `days/{date}/done` after its bundles (C-031).
  **B3 proven on a whole real day (droplet, 2026-11-11, build `ce3c50b9+25c5107c2761`):**
  2025-12-10 through `vyreon-worker run day-block` on the home copy, filling off, in 175 min (the
  day-local stage 67 s): base bundle published as revision 1, the day-local bundle as revision 2
  on it, `done`, job finished; then `run ordered-pass` (laptop build, core's database) published
  revision 3 and finished. Against the droplet's own published day: base 2,853 slots, no numeric
  difference, no slot or stream difference, 1,573 bodies byte-identical (the rest differ in the
  cutoff clock, the calendar digest and provenance digests, as in the earlier golden run);
  derived 1,826 of 1,826 slots present, every role value equal, 1,791 with the same role order
  and 35 DLM-UNUSUALNESS bodies in another order (explained above). One heartbeat request failed
  in transit during the publish and the next succeeded.
  **Left in B3:** the pass's base load bounded to its windows (it loads every published day);
  history, which only several days can prove (the two-node run's adjacent days are the first).
- Remaining in track B, each a multi-day change proven on golden days: B3 (the 18 day-local
  derived sensors into the day), E11 (decided, C-029), E4 (owner decisions
  first).

## Open
- Before the first build (ARCHITECTURE §8 step 0): E4's rules; stated absences on historical dates
  and expected coverage.
- Intrinio: do presigned file downloads count against the 2,000 a minute (per account).
- `unusual_score` is untested with real rows: the droplet has none yet (one derived day).
- Publish memory, measured on the full day: the core process peaks at ~0.7 GB and its database
  backend at ~0.6 GB. Not yet bounded; A7 runs one publish at a time (a day takes ~18 s).
- **E4 started: what each rule touches (engine `worker`, 2026-11-11).**
  (1) *Option sessions, 200:* the windows are declared once (`day_inputs.rs` `WINDOWS`:
  close_quotes 200, option_history 200, daily_captures 379, field_panel 376, ...); the one reader
  outside them was the derive's 260-session ex-date scan, which the ordered pass no longer makes
  (it reads T's VS-O04).
  (2) *`last_adjusted`* (`occ.rs:151`): the latest OCC-adjustment date before T "over every session
  with a daily capture", so it depends on how many daily sessions the disk holds, and mid-block a
  worker holds more than at a block's start. It feeds the quote overlay
  (`quote_overlay.rs` `frozen_by_adjustment`: a dead contract still holding open interest is dropped
  when its last trade predates the last adjustment, "whatever its distance"). **Real markers on
  the droplet:** on 2025-12-10 NVDA 2024-06-10, AVGO 2024-07-15, MSTR 2024-08-08 (splits, 337 to
  378 sessions back), BABA 2025-06-12, five sector funds 2025-12-05; on 2026-11-10 the same, the
  three splits now about 600 sessions back, only because the droplet's daily archive reaches
  2024-06-06. A worker with 380 daily sessions would drop the three splits on 2026-11-10. So the
  plan's "bound it to the window" changes populations for NVDA, AVGO and MSTR, against the
  overlay's stated intent. **Proposed instead (to measure, then the owner):** splits from the
  provider's split list at any age (one request per asset), the owner-confirmed table at any age,
  and the detections that need quote files (new roots, strike shifts) within the 200-session quote
  window; what that drops is a non-split adjustment older than 200 sessions that is not in the
  table. **Measured (droplet, real chains of 2025-12-10 and 2026-11-10, every asset carrying the
  marker):** rows holding open interest whose EOD quote is stamped at or before the adjustment
  date, the most the frozen rule could drop. Adjustments 3 sessions old (five sector funds,
  2025-12-05): 465 to 1,137 rows each, 0.2 to 2.1 million contracts of stale open interest.
  Every older one (BABA at 125 sessions, AVGO/MSTR/NVDA at 337 to 378, all of them again 230
  sessions later, ETHA at 25): **0 rows**. The rule only acts in the days after an adjustment, so
  any bound of a few weeks or more gives the same populations on those days. **But a plain
  200-session scan is not deterministic** (C-033, corrected): a day rebuilds every quote file of
  [T-200, T], the oldest files' markers look back past the files a worker holds, and detection by
  new roots or strike shifts needs those files. The rule to build: a fixed lookback L per file,
  and the quote window L+1 sessions longer. **Decay measured so far:** ETHA (adjusted 2026-10-06)
  972, 914, 914, 850 stale rows on the next four sessions, 0 from the fifth; XLE and XLK
  (2025-12-05) 1,137 and 1,065 through the fifth session, 1,080 and 1,003 on the sixth, then no
  capture until 2026-10 (0). L waits for XLE's chains of the following weeks (one request a day).
  (3) *Bar origin:* the technical seven read every daily capture on disk through T
  (`derive/local.rs`, `ArchiveCache::load_bars`); a worker holds 380. Needs bars from a fixed
  origin (2020-01-02 or the listing) as one ranged request per asset (C-008), read apart from the
  per-session captures.
  (4) *Tape-history start:* `VYREON_TAPE_HISTORY_FROM` (an environment variable, default
  2023-05-24); to come from core's claim.
- **Priority (owner, 2026-11-11): the database before general rules.** Rules tuned on the few
  golden dates have overfitted for weeks; the built database is where they are judged, and a
  recompute is cheap. So E4 is cut to what a distributed build needs (the same day gives the same
  values on any node and at any place in a block), each rule a plain declared default: the
  `last_adjusted` lookback fixed at 21 sessions with the quote window extended to match (not tuned
  on XLE; a query over the built database shows whether stale rows outlive it), the fixed bar
  origin, the tape-history start from the claim. E11's narrowing waits for the build to show how
  often inputs go missing; missed values, listings and delistings handled cleanly are the eventual
  aim (E6 to E8). The cluster is what builds the database and what keeps running costs below the
  single-node plan. Order: today's whole-day proof, E4's three defaults, cluster setup, the
  two-node run (spend to the owner first), the build.
- **E4's three defaults, state (2026-11-11 evening):**
  (a) *Tape-history start: done* (core `claims` carries `tape_history_from`, the first scheduled
  tape session of the profile; engine `day_inputs::set_tape_history_from`, called by the day-block
  and tape workers right after the claim; without a tape job the engine's own setting stands).
  Not yet run on the droplet.
  (b') **Built, untested on real data (engine 233d7670), and it found what rule (1) really
  meant:** `engine-v1` captured 131 option chains a day and let a home accumulate the 200-session
  history, so a worker from an empty disk would read a 131-session history and the droplet 200
  (the golden days read 131 only because the droplet's option archive began 2025-06-04). The
  capture window is now 223 sessions (200 + the 21-session marker lookback + 2), with quote files
  for the same sessions; markers, identity and requotes stay on the 200 the readers read; the
  plan requires the extra quote files. About +92 chains an asset at a block's start (~+6,000
  requests on ~39,000). **Values will differ from `engine-v1`** wherever a history was shorter
  than 200 sessions, the golden days included; what must hold instead is that a day is the same
  first in a block and mid-block, and on two nodes. Not yet run on the droplet (it needs filling
  on: 92 more sessions of chains and quote files per asset).
  (b) *`last_adjusted`, fixed 21-session lookback: the design as first written.* Sites: `occ.rs:153`
  (`last_adjusted` iterates `daily_dates`, every daily capture on disk: make it the 21 calendar
  sessions before the file's date); the files a marker needs are the quote files of each of those
  sessions and the one before (new-root and strike-shift detection, `occ.rs` `detect`), so the
  quote files must reach 23 sessions before the 200-session `close_quotes` window. Add a window
  rule (`day_inputs.rs` `WINDOWS`, e.g. `quote_detection`, prior 223) used where quote files are
  fetched (`fill.rs:373`, `:556`), planned (`plan.rs:522`) and retained (`retain.rs:79`); the
  marker check and rewrite stay on `close_quotes` (`plan.rs:595`, `fill.rs` `rewrite_markers`). A
  quote file lives in its session's option attempt directory, so those 23 sessions need their
  option chains too (about +1,500 requests a block): check how sessions 132 to 200 get theirs
  (`option_captures` is 131, `option_history` 200) before choosing how. The oldest file's
  `ex_date` and `prior_session_adjusted` have the same edge today (they read the two sessions
  before it); the longer window cures that as well. On the golden days the markers of NVDA, AVGO,
  MSTR and BABA go (older than 21 sessions), the sector funds' stay; headers are rewritten by the
  fill, and a run with filling off will refuse the stale headers.
  (c') **Bar origin built, untested on real data (engine, 2026-11-11):** the fill captures every
  session before the 379-session daily window as well, from the calendar's first session
  (2020-01-02) or the asset's own, as ranged months (`fill.rs`, the `earlier` sessions handed to
  `plan_symbol`; B2's range capture and links). About 54 requests an asset at a block's start for
  a 2025 day (~3,500), and a per-session directory for each of ~1,100 older sessions an asset:
  the time `link_range` takes over that many is unmeasured. The technical seven will differ from
  `engine-v1` (longer history).
  **Dry-run fill plan of 2025-12-10 on the home copy (build `89e13c3e+be7f7dadc3b5`, no network,
  10 s):** per asset 92 more option sessions (2025-01-22 to 2025-06-03: 5,888 chain captures in
  all), 92 quote-snapshot sessions to fetch, 3,330 ranged daily months to link (about 52 an asset,
  back to 2020-01), 507 quote files whose markers change (the old splits' markers go), and carry
  for 2025-12-03 again. The quote snapshots are the slow part of a download and their count goes
  from 132 to 224 sessions at a block's start: how long that adds is for the filled run to say.
  Script: `/root/worker-proof/dry-fill.sh` (private mount namespace, `vyreon-inputs fill --dry-run`).
  **All three defaults are built. Next: one run on the droplet with filling on** (the 2025-12-10
  home copy lacks 92 sessions of chains and quote files and the bars before 2024-06), after the
  02:00 UTC capture, to measure the added requests and time and to see the day through; then the
  same day computed as the second day of a two-day block must equal it.
  (c) *Bar origin, as first written:* `derive/local.rs` reads `ArchiveCache::load_bars` (every daily
  capture through T; a missing session restarts the series). To build: one ranged request per
  asset from 2020-01-02 (B2's `daily_range_chain` and `link_range` already fetch and verify ranged
  bars), kept as a range of its own that the technical records read, not per-session captures.
  Engine `worker` is at the tape-history commit, pushed.
- **Spend authorized (owner, 2026-11-11 ~15:00 Alberta time): the two-node run, up to $25.** Created
  through the DigitalOcean connector; the worker pool is deleted after every run; the owner is
  asked before the cluster or the registry is deleted. A ledger of what is created, when, and
  what it costs is kept here as it happens.
- **The two-node run, ledger (UTC, 2026-11-11).** The connector's token could not create
  anything (403); the owner put a full-access token in `do-full-token.txt` (git-ignored, mode 600,
  read by the scratch script `cluster/do.sh`, never printed).
  21:06 registry `vyreon` (Basic, nyc3, $5 a month). 21:06 cluster `vyreon` (nyc1: nyc3 had no
  capacity for the core size; Kubernetes 1.36.5-do.3; id `5f56e6e0-1c74-4dc9-bfdb-962ee55ebff3`)
  with pool `core`, 1 x `s-4vcpu-8gb-amd` ($0.083 an hour), running 21:12. Both images pushed.
  CloudNativePG 1.30.1, the database (PostgreSQL 16.15, 60 GB volume), core deployed. Found on the
  way: core's migration cannot create its group roles as the database's owner, so the cluster
  manifest now makes them at bootstrap (`postInitSQL`); the pull secret for the registry is a
  step the README lacked. Universe, calendar, build `89e13c3e+ede98f764f0d` registered and
  admitted for this trial, jobs scheduled (2 tape, 2 day blocks of one day, 2 ordered pass).
  Core's Intrinio budget set to 1,500 a minute (the droplet's capture shares the account).
  21:19 pool `day`, 2 x `s-8vcpu-16gb-amd` ($0.167 an hour each; id
  `58d217bf-4c48-499a-854a-786e9734197c`), ready 21:21. 21:21 the two tape jobs started.
  21:21 to 21:28 both tape jobs done (one session a node, 7 min, 64 caches each stored in core).
  21:28 the two day blocks started, one a node, each from an empty disk: claim, tape-history
  start from the claim, tape caches from core, download. By 00:01 and 00:21 each had filled
  everything but the tapes (2 h 32 min and 2 h 53 min from cold with E4's windows: 224 option
  sessions and quote files, bars from 2020; 38 GB; about 19,000 and 24,000 requests).
  **00:41 stopped: a bug.** Both fills then began capturing tapes from 2023-05-24 (64 and 100
  sessions done; all 640 would have taken about two days): the fill is a process of its own and
  was not given the tape-history start the worker took from its claim (my change of 2026-11-11
  replaced an environment variable with an in-process setting). Fixed with a test (engine
  14c7846a); the image is rebuilt and the two days start again from empty disks. Cost of the
  mistake: about 3.3 hours of the worker pool, $1.10, and ~51,000 Intrinio requests.
  **To delete after the run: pool `day` (always); the cluster and registry on the owner's word.**
- **Images ready on the laptop (2026-11-11):** `vyreon-core:6e15a27` (142 MB; a later docs-only
  commit does not change it) and `vyreon-worker:89e13c3e` (1.51 GB), whose own build identity is
  `89e13c3e+ede98f764f0d` (clean) with an inventory of 2,853 + 1,105 + 721 slots (saved in the
  session scratchpad as `cluster/inventory.json`; regenerate from the image, README step 2).
- **Cluster setup written, nothing created (2026-11-11):** `deploy/core/Dockerfile` (built on the
  laptop: 142 MB; it migrated a scratch database to 0011 and loaded the universe from its own seed
  files), `deploy/k8s/` (namespace, the CloudNativePG database, core's Deployment and Service, the
  three worker Jobs) and `deploy/k8s/README.md`, the runbook with every paid command marked and
  the prices read from DigitalOcean's API: core node `s-4vcpu-8gb-amd` $0.083 an hour, worker
  `s-8vcpu-16gb-amd` $0.167 an hour, about $10 for the two-node run torn down after (the
  registry's $5 a month is half of it), about $67 a month if the core node, volume and registry
  stay. Migration 0011 gives the ordered pass its read-only group role. Plain Jobs for the first
  run; KEDA and autoscaling to zero come with the build (two things to settle are in the README).
  The manifests have been parsed, not applied: no cluster exists to apply them to.
  Also fixed: a claim with no tape job scheduled failed to answer (the null tape-history start).
- **First major deliverable (owner, 2026-11-11):** two cluster nodes, each computing a golden
  day from an empty disk (adjacent days 2025-12-10 and 2025-12-11, tape jobs before, the ordered
  pass after, compared with the droplet's published days). No cold-start rehearsal on the droplet
  first (a cold day already ran there on `engine-v1`, 2026-11-09) and no plain-droplet stage. Order:
  finish B3's whole-day proof, E4, cluster setup (free until nodes exist; the spend goes to the
  owner before anything is created), the run. E11's narrowing alongside.
- **Decided by the owner, 2026-11-11 (C-032):** E4 goes ahead (values must not depend on the
  disk); E11 is to withhold only the sensors that read the missing input and their dependants,
  not the whole asset (a change to what was built and proven today); a day in two publishes is
  fine; push freely. Next in Track B after the running whole-day proof: E11 narrowed, then E4
  rule by rule with measured value changes.
- **Decided by the owner, 2026-11-11 (to build in track B):** E11 as proposed: an absent provider
  input gives its assets the new provider-limited reason `PROVIDER_INPUT_MISSING`, not counted in the
  10% engine-fault hold; FIELD follows h5-field's existing rules for a missing member; the day is held
  for a retry when more than 6 of 64 assets are withheld. C-026 as proposed: the engine adds its
  coordinates projection to the day's bundle, and core stores and serves it.
- Not built in A7, on purpose: core's own Intrinio fetch for the universe (no caller until the daily
  chain), and Kubernetes manifests with KEDA's query on `core.claimable_job` (milestone 3).
