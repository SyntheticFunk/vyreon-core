# Architecture and plan (design pass 2, 2026-11-10)

Status: reviewed and accepted by the owner (2026-11-10). It replaces the first draft of the same day and
folds in the decisions since (`docs/DECISIONS.md` C-001 to C-011). Engine facts are in
`docs/ENGINE_MAP.md` (MAP §n); new facts read or measured in this pass are marked **(pass 2)**.
What is still open or unmeasured is in §10.

## 1. What we are building

A data platform that runs the engine as disposable workers and owns everything that must last: the
measurements with their provenance, the point-in-time universe, the job queue, the Intrinio budget,
the tape caches, and the API that vyreon-reporter (later vyreon-web) reads.

```text
  DOKS cluster, nyc1 (control plane free, managed by DigitalOcean)
  ┌──────────────────────────────────────────────────────────────────────────────┐
  │ core pool: 1 node, always on (4 vCPU / 8 GB)   worker pools: 0..N, autoscaled │
  │  Postgres 16 (CloudNativePG, DO volume)         engine Jobs, one per claim:   │
  │  vyreon-core (Rust): worker API · publish  ◄──  download from Intrinio,       │
  │   · scheduler · budget · reader API        HTTP compute, publish through core,│
  │   · publish (the only writer)                   delete, exit                  │
  │  KEDA (starts Jobs from the claimable count)                                  │
  └───────────────┬──────────────────────────────────────────────────────────────┘
                  │ S3 API: the perishable archive (database backups later, C-013)
          ┌───────▼───────────────────────────┐
          │ DO Spaces, nyc3, bucket vyreon-data │        Intrinio ◄── workers
          └─────────────────────────────────────┘
  vyreon-reporter / vyreon-web ── authenticated HTTPS ──► reader API
```

**Principles**
- **A worker downloads what it can, computes its measurements, writes them to the permanent database
  and is done** (C-011). It keeps nothing and holds no database or storage credentials; it talks only
  to Intrinio and to core's worker API.
- **Only what is needed is stored** (C-004): the measurements, the perishable and volatile inputs
  Intrinio will not serve again, and the tape caches (C-010). Everything else is downloaded again.
- **The job table in Postgres is the source of truth for work.** Kubernetes only runs containers.
- **Core is the authoritative database; the engine is a stateless worker** (C-019). Core owns every
  table, the migrations and the only write path. The engine owns the science and keeps nothing. They
  meet at a versioned publish contract (the day's bundle), so core is not rebuilt per engine version.
- **Every day is a pure function of (T, the provider's data, the declared windows)** whatever block
  or machine computes it (C-001); the block-start test proves it.

## 2. How the work splits

Three kinds of job. Two are parallel; the third is cheap and ordered.

| Job | What | Reads | Parallel? | Cost |
|---|---|---|---|---|
| **tape(session)** | stream the session's replay files, reduce to the per-asset cache, store it in core | Intrinio only | fully | **5–7 min a session** on the droplet for the 64 (fill logs of 2025-12-10/11/12, pass 2); ~870 sessions since 2023-05-24 ≈ 90 node-hours ≈ $15 |
| **day block(dates)** | for each date: the base layer (2,853 slots) and the day-local derived sensors (1,105 slots, below) | Intrinio windows downloaded once per block; tape caches from core | yes, in blocks of consecutive dates | cold start ~3.3 h, then ~2.5 h a day on 8 vCPU (parity run; the warm day is to measure) |
| **ordered pass** | the history-reading derived sensors (721 slots) | the database only | no: strictly in date order, one at a time | minutes a day (today ~22 s + ~4.6 min of unusualness); runs on the core node |

The tape job runs first for every session, because every day reads the caches of all earlier
sessions (EVC01's baseline, MAP §5). Workers then never stream a tape: a day reads T's cache from core.

### 2.1 The derived sensors, sorted (pass 2, read in `vyreon-cli/src/derive/mod.rs` and `h5-derived`)

Today all 30 derived sensors (28 per security, 2 more per company) run in one ordered pass that also
reads files from the machine that computed the base layer. That hand-off cannot exist with disposable
workers. Sorted by what each actually reads:

**Day-local: computed by the worker (E10).** They read only T's own inputs and T's base values.

| Sensors | Reads today | Change needed |
|---|---|---|
| DLM-MAX-PAIN, -ZERO-DTE, -UNUSUAL-CONTRACTS, -QUOTE-FRICTION, -OI-CENTROID, -PREMIUM-YIELD (6) | T's option population, P01 close | none: both are on the worker |
| DLM-EXPECTED-MOVE, DLM-GREEK-EXPOSURE, DLM-IMPLIED-PROBABILITY (3) | population, T's surface file, IV30 | IV30 read from VS-IVS01 (the same number DLM-IV30 republishes) |
| DLM-GAMMA-EXPOSURE (1) | M0-GAMMA's stream, the surface's spot | none: both are the worker's own output |
| DLM-TECHNICAL, -PRICE-LEVELS, -CANDLESTICK, -RELATIVE, -DRAWDOWN, -VWAP, -BETA (7) | every daily bar on disk from 2020, SPY's bars, FIELD-DEP01 of T, T's tape cache | bars fetched from a fixed origin (one ranged request per asset, C-008): removes a start-dependence (MAP §6) |
| DLM-EVENT-WINDOW (companies, 1) | population, surface, T's expected date, the 8-K ledger over all published days | the ledger read from T's own CDD-EVENTS01 body, which already carries every 8-K the day knew (D-126) |

**History: the ordered pass.** They read earlier published days.

| Sensors | Reads | Change needed |
|---|---|---|
| DLM-PCR-OI, -PCR-VOLUME, -PCR-HISTORY, -OI-CHANGE, -OPTIONS-VOLUME, -HV-CONTEXT (6) | base parents up to 252 sessions back; **the OCC ex-date of the last 260 sessions from close-quote files** | the ex-date published as a value in T's base layer, so the pass reads it from the database |
| DLM-IV30, -IV-CONTEXT, -IV-RV, -IV-TERM (4) | VS-IVS01 and their own history | none |
| DLM-IV-CRUSH (companies, 1) | expected dates and 8-Ks over published history, IV30 before and after | none |
| DLM-UNUSUALNESS (1) | every reader value of T and its prior 63 sessions; flag lines pooled over 252 | none |

After E10 the ordered pass reads nothing but Postgres, so it can run anywhere, at any later time, and a
recompute of any of its sensors needs no download at all.

## 3. Where every byte lives

| Data | Where | Size | Kept? |
|---|---|---|---|
| Measurements, receipts, event streams, reader scalars | Postgres | **135 MB a day** measured (droplet store, 3 days: streams 88 MB, bodies 45 MB); ~57 GB for the first build (~420 sessions), ~175 GB back to 2021-09-27 (~1,290) | yes: the product |
| Universe, jobs, leases, budget and usage, coverage | Postgres | MBs | yes |
| Tape caches (per asset-session: the 15-minute and 5-minute bodies and their manifest) | Postgres, one compressed row per asset-session (C-010) | **~82 KB per asset-session** uncompressed JSON measured, ~5 MB a session for the 64, ~4.6 GB for 870 sessions (less compressed), +1.3 GB a year | yes: EVC01 and intraday FIELD need every earlier session |
| Option prints, ACT01 pulls, company captures | Spaces (the droplet's nightly archive; the daily capture job later) | ~30–50 MB a session compressed | yes: Intrinio will not serve them again |
| Database backups (base backups + WAL) | Spaces, written by CloudNativePG | about the database's size per retained base backup, plus WAL | **later** (C-013): set up once a database is accepted as worth keeping; until then a lost database is rebuilt |
| A worker's downloads and intermediates (quote window, chains, surfaces, run directories) | the worker node's local disk | 44.7 GB peak measured on a cold day | no: deleted when the pod exits |
| A day's publish bundle in transit | core node's disk, while it is checked | ~200 MB a day uncompressed (100 MB bodies + 84 MB gzipped streams) | no |
| Raw Intrinio data of any kind | nowhere | | no (C-004) |

Notes:
- The Postgres volume starts at 250 GB (~$25 a month) and is resizable. A rebuild attempt into a
  second database (§5.4) doubles the store while both exist: fine for the first build (2 × 57 GB),
  a temporary resize for the full history.
- Possible saving, not now: each body is stored twice (canonical text and JSONB), and the M0 event
  streams are 65% of the store. Both are the engine's schema; they stay as they are for the first build.
- Adding an asset later means re-running the tape job for its sessions (the cache is per asset).

## 4. Components

### 4.1 Core node (always on, `s-4vcpu-8gb-amd`, $56 a month)
- **Postgres 16 under CloudNativePG** (C-003), data on a DigitalOcean volume. Backups to Spaces are
  switched on, and a restore drilled, once a database is worth keeping (C-013). The measurement,
  stream and scalar tables are **partitioned by month of the represented date** from the first
  migration (PostgreSQL declarative partitioning: bounded indexes and maintenance, cheap replacement
  of a date range). Schema: the engine's store unchanged
  (`db/schema.sql`: `measurement`, `day_revision`, `publish_revision`, streams, `reader_scalar`,
  `unusual_score`, `republish_job`), plus:
  - **universe:** `security` (Intrinio id, FIGI, composite FIGI, company id, code), `ticker_span`
    (security, ticker, from, to), `listing` (first and last price date, merged into), `member`
    (profile, Vyreon asset key, security, from, to, group, class), `source_coverage` (option EOD
    2021-09-27, snapshots 2022-05-27, tapes 2023-05-24, prints 2026-10-23). Filled for the 64 from
    Intrinio's ticker history (fetched 2026-11-10);
  - **work:** `job` (type, dates, scope, engine build, state, attempts), `lease` (job, worker, expires,
    last published date), `queue_control` (paused, reason);
  - **budget:** `budget_lease`, `request_usage` (worker, category, count, minute);
  - **coverage:** per day, sensor and asset: states and absences by cause;
  - **tape_cache:** (asset key, session, method version, digest, compressed body).
  Migrations are checksummed and applied under an advisory lock (telemetry-backend's pattern, C-009).
- **vyreon-core**, one Rust service (C-007; axum + tokio-postgres). It contains no engine code:
  - **worker API** (private network, per-worker token): claim a block, heartbeat, budget lease, read
    tape caches, publish a day, report usage, finish or fail;
  - **publish (the only writer):** takes each day's bundle (the publish contract: bodies, receipts,
    streams, digests, the build), checks digests and the exact slot inventory **registered for that
    build when it was admitted** (a worker never supplies its own checklist),
    writes it all-or-nothing and reads it back. Core's code, started from the engine's `vyreon-store`
    and `publish_revision` (MAP §4);
  - **scheduler:** turns a request ("days 2025-03-03 to 2026-11-10") into block jobs; decides what is
    claimable; expires dead leases; pauses the queue on a failed check; **admits a build** (an engine
    build may claim work only after its golden days and block-start test passed on the cluster);
  - **budget:** the account's Intrinio window (§6);
  - **reader API** (after the first database, C-009) and **notices** (Discord, channel only).
- **The ordered pass** is an engine job (the engine's derive), one at a time, in date order behind the
  published days; it may run on a worker node, so the core node's memory does not bound it. It reads history with a read-only database role for the first build (through the
  reader API later) and publishes its day through the same contract as every worker.
- **Health checks** include the engine's frozen trading calendar: a notice well before its last
  session, since daily runs stop when it runs out.
- **KEDA:** one ScaledJob per job type reads "claimable jobs of this type" from Postgres and starts
  that many Jobs. Core needs no Kubernetes credentials.

To measure in the trial: the core node's headroom with 8 workers publishing (each day is ~200 MB to
check and insert) and the ordered pass running. The ordered pass today loads its parents for every
published day into memory; it must be bounded to its windows (252 sessions) before the full history.

### 4.2 Spaces (`vyreon-data`, nyc3, $5 a month for 250 GB)
The perishable archive now; the database backups later (C-013). Nothing else. Workers never touch it.

### 4.3 Workers (DOKS pools, 0..N)
- 8 vCPU nodes for day blocks (the measured shape), smaller nodes for tape jobs (network-bound).
  N ≤ the droplet limit less the core node and the web droplet.
- Each Job pod: one claim → run → publish each day as it finishes → exit. Marked not safe to evict;
  scratch on the node's local disk.
- Image: the engine's binaries with MKL, mpfr, gmp and the pinned SQLite, build identity baked in.

## 5. How it runs

### 5.1 A day block
1. The pod claims a block and receives: the dates, the engine build, the member manifest (for each
   date: members, the provider ticker in use, provider availability), the tape-history start, a
   budget lease.
2. It downloads the windows once (cold start), pulls the tape caches it needs from core, then for
   each date: checks every declared input in the first minute (E12), computes the base layer and the
   day-local derived sensors, sends the day's bundle to core with its stage timings and request
   counts (E13), and continues while the next date's inputs download. An asset with a missing input
   is published as a stated absence with its reason; the day goes on (E11).
3. Core checks, publishes and reads back each day, updates coverage and runs
   the checks. A failed check pauses the queue and posts to Discord.
4. On a crash or eviction the lease expires; the block is reclaimed and resumes at the first
   unpublished date (one cold start lost).

Blocks of ~30–40 sessions (5–8% cold-start overhead). The first blocks are chosen to prove the
system: one overlapping the droplet's own published days, and the block-start test.

### 5.2 The ordered pass
One at a time, in date order, behind the contiguous run of published days. It never blocks workers.
When older days are added later, it re-runs from the first new day forward (minutes a day).

### 5.3 Recomputing one sensor
The engine's family republish (D-104) already recomputes a family, its same-day dependants and the
later days that read it. With E10 the cost depends only on what the sensor reads:

| Sensor class | Needs | Cost per day |
|---|---|---|
| Ordered-pass sensors (levels, IV context, IV crush, unusualness) | the database | seconds to minutes, no download |
| DLM-TECHNICAL family | one ranged bar request per asset | seconds |
| Chain sensors without the surface (max pain, 0DTE, …) | T's option chain | a small download |
| Surface readers (expected move, Greeks, event window) and the surface itself | T's chain and quotes, the surface rebuilt | ~25 min |
| M0 | T's inputs | ~20 min |
| P (price path) | its windows | about a whole day |

A base recompute is a block job with a scope. Any change cascades into DLM-UNUSUALNESS for the next
63 sessions (and its flag lines for 252), which the ordered pass redoes.

### 5.4 A rebuild attempt
A new attempt builds into a new database beside the current one; readers stay on the last good
database until the owner accepts the new one. Tape caches are copied, not recomputed, unless the tape
method changed.

### 5.5 Daily operation
One chain after the close: capture the perishables, tape(T), day(T), ordered pass. The droplet keeps
doing today's capture until the cluster has matched it for several sessions.

## 6. The Intrinio budget

- Core holds the account window: 2,000 requests a minute, used to 1,800. Intrinio's usage endpoint
  reports the limit per access code (`api`: `one_min_call_limit` 2,000; pass 2); whether other codes
  have their own window, and whether presigned file downloads count, is still to confirm.
- Workers take leases of K requests (e.g. 500, valid 2 minutes) and report usage by category.
- Demand after C-008 (ranged daily bars): a cold block start ~17–21k requests (was ~65k), a warm day
  ~8k. Eight workers average a few hundred a minute; only simultaneous cold starts need staggering.

## 7. Engine changes (vyreon-engine, v1.x)

| # | Change | Kind |
|---|---|---|
| E1 | `vyreon-worker`: a claim loop around today's day/fill code; each day's bundle sent to core; heartbeat | engineering |
| E2 | Tape caches read from and written to core (not the home); the tape job as its own entrypoint | engineering |
| E3 | Shared budget: the ledger's grant draws from a lease; the five unmetered call sites use the same gate; act01/exq01 wait; 429 retried | engineering |
| E4 | Determinism: 200 option sessions everywhere; `last_adjusted` bounded to the declared window; a fixed bar origin for DLM-TECHNICAL (the calendar's first session, 2020-01-02, or the listing); one tape-history start from the claim | values may change, each declared |
| E5 | Packaging: build identity at build time, container image, disk guard by env, no git at runtime | engineering |
| E7′ | Daily bars as one ranged request per asset (C-008); the binding `INTRINIO-CI-DAILY-BAR-1@1` changes, values must not | engineering |
| E10 | The 18 day-local derived sensors computed in the worker's day and published with it; the OCC ex-date published as a base value; the ordered pass reads only the database (C-011, §2.1) | engineering; values must not change |
| E11 | A missing or refused input withholds the assets it touches (a stated absence with its reason), not the day | engineering; the readiness check "one failed input withholds only what it touches" |
| E12 | One declared list of input windows, read by both the upfront plan and every reader, so a day fails in its first minute or not at all (the parity run failed ~75 min in on a window the plan did not check) | engineering |
| E13 | Per-stage timings and request counts reported with every day | engineering |
| E6 | Member-aware day: the members present on T drive the plan, views, surfaces; listing-aware windows; no 64 cap | after the first build |
| E7 | Provider ticker per date (FB → META), challenge registry from history | after the first build |
| E8 | Absences with causes (`NOT_LISTED`, `BEFORE_PROVIDER_START`) | after the first build |
| E9 | FIELD over a changing membership: stated absence until its rulings (C-005); the constants tied to 64 (pair counts, group positions, minimum cross-sections) follow the day's member list | after the first build |
| E15 | Monthly partitioning of the store (C-014a): `measurement`, `measurement_stream`, `reader_scalar` and `unusual_score` are keyed by revision only, so each gains the represented date in its key and the engine's inserts supply it. The schema is core's (C-019), started from the engine's `db/schema.sql`; the engine's bundle carries the date | core; in milestone 1 |
| E14 | The stored format slimmed (each body is stored twice; M0 streams are 65% of the store) | before a larger universe |

**Proof for E1–E5, E7′, E10–E13:** the golden days through the worker path equal `engine-v1` except the
declared E4 changes and provenance; a day computed first in a block equals the same day mid-block; a
day published through core reads back identical to the droplet's own.

## 8. Plan

Durations are guesses until the first blocks give a measured pace; the audit of 2026-11-10 judged the
first drafts low. Likely: core to the worker API ~2 weeks, the engine track 4–6 weeks, first database
January 2027, first build $200–350.

0. **Before the tracks finish:** E4's rules and the values they change; the sensors that are a stated
   absence on historical dates (prints exist only from 2026-10-23; fundamentals are not served as of a
   past date) with the coverage each can reach; a temporary 8 vCPU droplet for the engine's golden
   proofs (owner approves; the laptop computes nothing and the droplet runs the capture); the Intrinio
   rate-limit question (owner).

1. **Core service** (this repo, ~2 weeks): workspace, migrations, the schema of §4.1, the universe for
   the 64, publish through core, worker API (a failing worker sends its log), scheduler, budget;
   build admission by checklist at first. The reader API and notices follow and do not gate the
   cluster. *Exit:* a day already published on
   the droplet goes through core into an empty database and reads back identical.
2. **Engine as worker** (engine branch, ~2–3 weeks, alongside 1): E1–E5, E7′, E10. *Exit:* the proof
   in §7.
3. **Cluster** (~1 week; owner approves the spend): DOKS, CloudNativePG, KEDA, registry, secrets; a
   pool scaling from zero and back; build admission run for real; a 2-node trial on one recent week
   compared with the droplet; the measurements of C-006.
4. **First build** (~1–2 weeks of runs; owner approves the spend): the tape job for every session
   since 2023-05-24, then day blocks from ~2025-03 on the fixed 64, the ordered pass behind them.
5. **Rebuilds and recomputes** exercised for real: one family republish across the history; a second
   attempt if the first shows the need.
6. **After the first database:** backups to Spaces and a restore drill (C-013); the reader API for vyreon-reporter (C-009); N assets in the engine
   (E6–E9) and the backfill to 2021-09-27; core working out from two builds' sensor fingerprints
   which families a new build changes; the daily chain on the cluster and the droplet retired.

Why the fixed 64 first: from 2024-07-23 no member lists, delists or renames (checked against
Intrinio's ticker history, 2026-11-10: FB → META 2022-06-08, the META ticker held by a Roundhill ETF
to 2022-01-28, IBIT listed 2024-01-11, ETHA 2024-07-23), and ETHA's windows fill by ~2025-03. Older
days need E6–E8 and are independent of the newer ones; the ordered pass is re-run when they arrive.

## 9. Costs

| Item | Cost |
|---|---|
| Core node (4 vCPU / 8 GB AMD) | $56 a month (the current droplet, $112, until it is retired) |
| Postgres volume, 250 GB | ~$25 a month |
| Spaces | $5 a month (250 GB included) |
| Registry | $0–5 a month |
| Tape job, all ~870 sessions | ~90 node-hours, ~$15 once |
| First build, ~420 days | ~1,100 node-hours at $0.17 ≈ $190 if nothing is retried; **$200–350** allowed; about 6 days on 8 nodes |
| Full history, ~1,290 days (later) | ~3,300 node-hours ≈ $560 per pass |
| Daily worker | ~3 h a day ≈ $15 a month |

## 10. To confirm on review, and what is still unknown

**Confirmed by the owner (2026-11-10; C-012, C-013, C-015 to C-018):** E10's split of the derived
sensors (§2.1); the day-local group published with the worker's day, the history group by the ordered
pass, measurement ids unchanged (one profile or two is settled when E10 starts); workers publish by
sending the day's bundle to core over HTTP; stable Vyreon asset keys; a rebuild attempt is a new
database; core as the only writer behind a publish contract (C-019); backups later.

**Open, decided when their phase starts:** the order of N assets against the first build is now
settled by the ticker-history check (§8) unless the owner wants it otherwise; E4's exact rules and
their value changes; company inputs on historical dates (fundamentals are not served as of a past
date: which company sensors are a stated absence in the backfill).

**Still unknown, measured in the trial:** a warm day inside a block; the block-start effect; whether
several workers streaming tapes at once are throttled by Intrinio; the core node's headroom; EVC01's
per-day cost as the history grows (it reads ~78 numbers per asset from each earlier session, so the
tape job could store those instead of the worker re-reducing every cache: an engineering option, no
science change).

**For Intrinio support (owner):** is the 2,000-a-minute limit per access code or per account; do
presigned file downloads count against it.
