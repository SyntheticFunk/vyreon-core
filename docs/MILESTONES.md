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
   reads back identical.
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

Now: A8, the reader API (sessions, an asset's history, report context, status), then A9 (Discord
notices, automated admission). Then track B on an engine branch.

Owner (2026-11-10): tracks A and B are both completed before any new DigitalOcean spend, then we
evaluate together. Spend is approved in principle (be mindful of costs), but nothing is created until
that evaluation. Track B's golden proofs use the droplet we already pay for.

## Open
- Before the first build (ARCHITECTURE §8 step 0): E4's rules; stated absences on historical dates
  and expected coverage.
- Intrinio: do presigned file downloads count against the 2,000 a minute (per account).
- `unusual_score` is untested with real rows: the droplet has none yet (one derived day).
- Publish memory, measured on the full day: the core process peaks at ~0.7 GB and its database
  backend at ~0.6 GB. Not yet bounded; A7 runs one publish at a time (a day takes ~18 s).
- For track B: the worker's bundle carries the day's reader scalars (C-024); derived bodies record
  parent revision ids, which the ordered pass must read from core.
- For A8: the droplet's database and a stock PostgreSQL sort text differently (collation); the
  reader API orders with an explicit collation.
- For track B: the engine's `list_sessions` cannot print its whole calendar (core's copy was
  assembled from its `before` and `session` outputs); add an `all` mode with open and close times.
- Not built in A7, on purpose: core's own Intrinio fetch for the universe (no caller until the daily
  chain), and Kubernetes manifests with KEDA's query on `core.claimable_job` (milestone 3).
- For track B: the tape cache's packing and `method_version` are the engine's to define (the test
  used a tar of the four cache files).

