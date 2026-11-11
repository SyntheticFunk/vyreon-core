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
  window. Ranged bars from a fixed origin (B2) supply them, so **B2 comes before B5**.
  Later, for daily operation: a recent session's bar is re-captured for three sessions and its cache
  re-derived, which core's write-once rule does not allow yet.
- B2 (ranged daily bars) is studied, not started. Facts: today each bar is its own capture made by
  two requests to `/securities/{SYM}/prices` with `start_date = end_date` and `page_size=1` (the row,
  then an empty terminal page), and every capture proves it: the verified decode
  (`intrinio-daily` `reopen_day_from_read`) requires the single-day URL, `page_size == 1`, the token
  chain and exactly one row. So a ranged response cannot be split into today's per-session
  directories without inventing provenance. The honest design: a new binding (for example
  `INTRINIO-CI-DAILY-RANGE-1@1`), one range capture per asset holding the raw pages, a per-session
  view that names it by digest and row, a new schema branch in the verified decode, and the five
  raw-body scanners and the history-file count taught the new layout. Bar values stay equal; what
  must change and be declared: the binding and its receipt checksum, every daily manifest digest,
  and two body fields the golden comparison does not yet treat as provenance
  (`current_daily_capture_sha256`, `source_input_set_sha256`). One real risk to measure first on
  real responses: P01 falls back to the provider's adjusted prices when any bar lacks `factor`, and
  those depend on when the bar was fetched.
- An alternative for B5 that does not wait for B2, for whoever picks it up: a worker that did not
  derive a cache could accept it on core's digest and its recorded daily-bar identity instead of
  re-checking the local daily bar. That is a change to the cache's validity rule, so it is recorded
  as a decision before it is coded.
- Remaining in track B, each a multi-day change proven on golden days: B2, B3 (the 18 day-local
  derived sensors into the day), B4 (the budget gate), B5, B7 (the image), E11 and E4 (owner
  decisions first), E13 (timings in the bundle).

## Open
- Before the first build (ARCHITECTURE §8 step 0): E4's rules; stated absences on historical dates
  and expected coverage.
- Intrinio: do presigned file downloads count against the 2,000 a minute (per account).
- `unusual_score` is untested with real rows: the droplet has none yet (one derived day).
- Publish memory, measured on the full day: the core process peaks at ~0.7 GB and its database
  backend at ~0.6 GB. Not yet bounded; A7 runs one publish at a time (a day takes ~18 s).
- **Owner decisions for E11:** (1) a new provider-limited slot reason for an absent input (proposed
  `PROVIDER_INPUT_MISSING`, not counted in the 10% engine-fault hold); (2) FIELD on such a day follows
  h5-field's existing rules for a missing member; (3) how many assets may be withheld before the day
  is held for a retry instead (proposed: more than 6 of 64, the same tenth as the fault hold).
- **Owner decision, with track B:** how Reporter gets each coordinate's state, reason, unit and
  quality (C-026). Proposed: the engine adds its coordinates projection to the day's bundle.
- Not built in A7, on purpose: core's own Intrinio fetch for the universe (no caller until the daily
  chain), and Kubernetes manifests with KEDA's query on `core.claimable_job` (milestone 3).
