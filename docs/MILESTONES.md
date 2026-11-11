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
  None of it is in the tape reader's eight source files. Tests need a real day with one asset's
  real input left out (as `h5-field/tests/fault_members.rs` does), and the golden days must be
  unchanged when nothing is missing.
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
- **Decided by the owner, 2026-11-11 (to build in track B):** E11 as proposed: an absent provider
  input gives its assets the new provider-limited reason `PROVIDER_INPUT_MISSING`, not counted in the
  10% engine-fault hold; FIELD follows h5-field's existing rules for a missing member; the day is held
  for a retry when more than 6 of 64 assets are withheld. C-026 as proposed: the engine adds its
  coordinates projection to the day's bundle, and core stores and serves it.
- Not built in A7, on purpose: core's own Intrinio fetch for the universe (no caller until the daily
  chain), and Kubernetes manifests with KEDA's query on `core.claimable_job` (milestone 3).
