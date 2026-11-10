# Architecture and implementation plan (2026-11-10)

Status: reviewed with the owner on 2026-11-10. Decided: the orchestration and the data box (D3, D4),
FIELD deferred until after the backfill (D8), Spaces for the data we keep (bucket live). Every other
recommendation is evaluated when its phase starts. Decisions go to `docs/DECISIONS.md`; status goes to
`docs/MILESTONES.md`. The engine
facts it rests on are in `docs/ENGINE_MAP.md` (cited as MAP §n).

## 1. What we are building

A data platform that runs the engine as stateless batch workers and owns everything that must last:
the measurements, the point-in-time universe, the job queue, the Intrinio budget, the perishable
option prints, and the API that Reporter and plain Python read.

```text
  DOKS cluster, nyc1 (control plane free, managed by DigitalOcean)
  ┌───────────────────────────────────────────────────────────────────────────────┐
  │ core pool: 1 node, always on (the data box)   worker pools: 0..N, autoscaled   │
  │  Postgres 16 (CloudNativePG, DO volume)  ◄──►  engine Jobs (one per claim)     │
  │  vyreon-manager: reader API · worker API  HTTP  started by KEDA ScaledJobs     │
  │   · scheduler · budget · notices                (one per job type)             │
  │  KEDA · daily capture (CronJob)                                                │
  └───────────────┬──────────────────────────────────────────┬────────────────────┘
                  │ S3 API (backups, artifacts)              │ S3 API (presigned URLs)
          ┌───────▼──────────────────────────────────────────▼──┐
          │ Object storage (DO Spaces, nyc3, bucket vyreon-data): │
          │ prints, tape caches, derive inputs, DB backups        │
          └───────────────────────────────────────────────────────┘
  Web droplet (nyc3) ── authenticated HTTPS ──► reader API          Intrinio ◄── workers
```

**Principles:**
- The **job table** in Postgres is the source of truth for work. Kubernetes only runs containers.
- **Workers hold no credentials to the store.** They publish through the manager, which keeps
  today's atomic, inventory-checked publish.
- **The store keeps outputs; object storage keeps the few things that cannot or should not be
  re-downloaded.** Everything else comes from Intrinio again when needed.
- **Every day is a pure function of (T, the provider's data, the declared windows)**, whatever block
  or machine computes it. This is tested, not assumed.

## 2. The key finding: three kinds of work, two of them parallel

The map (MAP §5) shows the engine's cross-day reads split cleanly:

| Stage | What | Reads from earlier days | Parallel? | Cost |
|---|---|---|---|---|
| **T: tape** | turn each session's consolidated tape into its compact cache (~70 KB per asset-session) | nothing | fully, per session | streaming ~30 GB per session; network-bound (to measure) |
| **A: base** | the base layer of day T (2,853 slots), published without derive | raw input windows (≤200 option, 379 daily, T−5) downloaded per block; **tape caches of every earlier session** (EVC01, intraday FIELD) from storage | yes, in blocks of consecutive dates | ~2–2.5 h a day on 8 vCPU; cold start ~3 h per block |
| **B: derived** | the derived layer (1,826 slots) | published history (≤252, plus expanding event tables and pooled unusualness lines) and T's own derive inputs | **no: strictly in date order** | ~22 s a day + unusualness ~4.6 min a day |

So the backfill is: stage T for every session (any order, cheap nodes), stage A in parallel blocks,
and stage B as one ordered job trailing behind A. Stage T also saves stage A from streaming each
day's ~30 GB of tape again: a block reads caches instead.

Two changes make A and B work on disposable machines:
- **Tape caches live in object storage** (64 × 70 KB × ~870 sessions since 2023-05-24 ≈ 4 GB; grows
  ~1.1 GB a year).
- **Each day's derive inputs live in object storage**: the option population, the surface file and
  the VWAP cache that the derived layer reads for T (MAP §5, last row). Stage B, and any later
  derived recompute, then takes seconds per day instead of a re-download. Size to measure.

**This touches product intent** ("production stores final values … nothing intermediate"; "raw
captures are transient"). These are compact per-session results, not raw captures, and they are
exactly what D-160's "next step" (windows read compact per-session results) points to. Owner
decision D1 below.

## 3. Components

### 3.1 Data box (the cluster's always-on core node, nyc1)
One DOKS node pool of one fixed node holds everything that runs all the time (owner's design,
2026-11-10). DigitalOcean recommends keeping one such node anyway so its own cluster components
(networking, DNS) stay available; using it as the data box means the always-on droplet does double
duty. Worker Jobs are kept off it by node selectors (and a taint), and core pods are kept on it.
- **Postgres 16** run by **CloudNativePG** (the standard Postgres operator for Kubernetes), data on a
  DigitalOcean volume through the cluster's storage driver (a PersistentVolumeClaim, resizable),
  continuous backup (base backups + WAL) to Spaces with CloudNativePG's built-in tooling, restore
  tested. When DigitalOcean replaces the node (cluster upgrades, which we schedule), Postgres
  restarts on the new node and its volume reattaches: minutes of downtime, during which workers wait
  on their leases. The engine's store schema moves here unchanged at first
  (`measurement`, `day_revision`, `publish_revision`, streams, `reader_scalar`, `unusual_score`,
  `republish_job`), then gains:
  - **universe:** `security` (Intrinio security id, FIGI, composite FIGI, company id, code), `ticker_span`
    (security, ticker, from, to), `listing` (first/last price date, delisted, merged into), `profile_member`
    (profile, object key, security, from, to, group, asset class), `source_coverage` (source, from;
    option EOD 2021-09-27, snapshots 2022-05-27, tapes 2023-05-24, prints 2026-10-23);
  - **per-date inventory:** `expected_slot` becomes a template (sensor × member kind) and
    `expected_slots(profile, date)` expands it over the members present on that date; `publish_revision`
    compares against it;
  - **work:** `job` (type, dates, scope, engine build, priority, state, attempts), `lease` (job, worker,
    expires, last published date), `queue_control` (paused, reason);
  - **budget:** `budget_grant` and `request_usage` (worker, category, count, window);
  - **coverage:** per day, per sensor, per asset: state counts and absences by cause (drives the
    pause checks and the 80% coverage report);
  - **artifacts:** an index of what is in object storage (key, kind, session, asset, digest, bytes).
- **vyreon-manager**, one Rust service (axum + tokio-postgres, like `vyreon-api`), reusing the
  engine's crates as pinned git dependencies (sensor registry, coordinates, store logic):
  - **reader API:** the agreed `GET /v1/assets/{symbol}/history` and `GET /v1/sessions/{date}` (JSON and
    Parquet), plus the engine's existing v1 routes ported over (window, rankings, peers, review,
    slots, coordinates, streams);
  - **worker API** (per-worker token, private network only): claim, heartbeat, publish a day, budget
    lease, presigned artifact URLs, usage, completion/failure;
  - **scheduler:** turns requests ("backfill 2024-08-01…2026-11-10", "recompute family X over dates")
    into jobs; decides what is claimable (block order, one date-order pass at a time, budget, pause);
    expires dead leases; pauses on failed checks. It does not talk to Kubernetes: KEDA reads the
    claimable count (§3.3);
  - **budget:** the account's Intrinio window (§5);
  - **notices and health:** Discord (channel only, silent), absorbing `vyreon-notify`'s checks.
- **Daily capture** (prints and the other perishable inputs) as a Kubernetes CronJob, once the cluster
  has matched the droplet's captures.
- Size: small (2–4 vCPU, 4–8 GB; DigitalOcean's cluster components take ~1 GB; measure). Postgres
  needs disk, not CPU: ~150 MB of store a day, ~190 GB for 2021-09-27 onward plus indexes.
- **Transition:** today's engine droplet keeps Postgres, the capture and the archive running until
  the cluster has matched them; the store moves into CloudNativePG by its import from an existing
  Postgres (or dump/restore), and the droplet is retired after.

### 3.2 Object storage (DigitalOcean Spaces)
S3-compatible, $5/month for 250 GB (+$0.02/GB). Bucket `vyreon-data` in nyc3 (created 2026-11-10;
the droplet's nightly archive of captured inputs already writes to it). Holds: option prints, ACT01
pulls and company captures (zstd, ~22 MB a day), tape caches (~4 GB + 1.1 GB a year), per-day derive
inputs (size to measure), publish bundles (transient), and Postgres backups (CloudNativePG base
backups + WAL; restore tested). Workers get presigned URLs from the manager, so they hold no storage
keys. nyc3 is the same metro as the cluster; traffic is modest.

### 3.3 Compute (DOKS, nyc1)
- **Core pool:** the data box (§3.1), one fixed node.
- **Worker pools, autoscaling 0…N** (N ≤ droplet limit − 2, for the core node and the web droplet).
  Backfill on 8 vCPU nodes (today's measured shape, 320 GB disk); tape and daily work on 4 vCPU
  (size by measurement). DigitalOcean's cluster autoscaler adds a node when a Job pod is waiting for
  room and removes idle nodes; worker pools may scale to zero beside the core pool (docs,
  2026-11-10; confirm with a small test).
- **KEDA ScaledJobs, one per job type** (tape, base block, date-order derive with at most one
  replica, daily, recompute), each polling a queue-depth query on Postgres (`claimable jobs of this
  type`) and starting that many Jobs, minus those running. The scheduler decides what is claimable;
  KEDA only turns the count into Jobs, so the manager needs no Kubernetes credentials.
- Each Job pod: one claim → run → publish each day → release → exit. Annotated
  `cluster-autoscaler.kubernetes.io/safe-to-evict: "false"` so scale-down never picks a busy node;
  scratch on the node's local disk (`emptyDir`) with a disk request sized from the measured peak.
- Image: the engine's 19 binaries with MKL/mpfr/gmp and the pinned SQLite at their rpaths, build
  identity baked in (no git at runtime). Registry: DOCR Basic ($5/month; not offered in nyc1, pulled
  from nyc3) or GitHub's.

### 3.4 Web
Unchanged on its own droplet (nyc3); reads the reader API over authenticated HTTPS (a DigitalOcean
load balancer, ~$12/month, or an ingress on the core node's address behind a firewall that admits
only the web droplet; or Tailscale, as the engine API planned). Chosen in phase 4.

## 4. How a job runs

1. The scheduler queues jobs: `tape(session)`, `base(block of dates)`, `derive(dates, ordered)`,
   `capture(T)` (daily perishables), `recompute(scope, dates)`, `universe(refresh)`.
2. It creates a Kubernetes Job for each claimable job up to the per-type caps (derive: exactly 1).
3. The pod claims a job: it gets the dates, the engine build, the **membership manifest** (for each
   date: members present, the provider ticker in use per source, provider availability) and a
   budget lease.
4. It fills inputs from Intrinio (paced by the budget), pulls tape caches and T−5 artifacts it needs
   from storage, computes each day, uploads the day's export bundle and derive inputs, and asks the
   manager to publish. The manager stages, verifies, publishes (the existing `publish_revision`) and
   reads back. The lease records the last published day.
5. On a crash, eviction or a bug, the lease expires; the job is reclaimed and resumes from the next
   unpublished day (one block warm-up lost, ~3 h).
6. After every published day the manager updates coverage and runs the checks; a failed check pauses
   the queue and posts to Discord.

**Blocks:** ~30–40 sessions (5–8% warm-up overhead, small enough that a straggler or a loss is
cheap). The first blocks of a backfill sit at era boundaries (2021-09, 2022-05/06, 2023-05, 2024-01,
2024-07, and one recent block compared with the droplet's own runs).

**Daily operation:** one `capture(T)` + `base(T)` + `derive(T)` chain after the close; the prints for
the last 5 sessions are captured every day whether or not compute succeeds (they cannot be fetched
later). ~4–5 h of one 4 vCPU node, ~$11/month.

**Recomputes** are ordinary jobs with a scope (families or measurements) and a date range. The
engine's D-104 machinery decides the same-day closure and the later days a change reaches. Derived
recomputes read stored derive inputs (cheap); base recomputes re-download (about a day per day).
Superseded revisions are pruned by a manager job (retention rule: replaced values may be deleted).

## 5. The Intrinio budget

- The manager holds the account window: 2,000 requests a minute, used at most 1,800 (10% margin).
  *To confirm with Intrinio:* whether the limit is per access code (`api`, `edi_uscomp`) or per
  account, and whether presigned file downloads count.
- Workers take **leases of K requests** (e.g. 500, valid 2 minutes) and spend them locally; usage by
  category returns with each renewal. One round trip per 500 requests, no per-request call.
- Engine changes: the ledger's grant (`authorize_api_attempt`) draws from the lease instead of the
  hard-coded 1,800/min; the five unledgered helpers call the same gate; act01/exq01 wait instead of
  failing; 429 is retried with backoff in the main capture transport.
- Measured demand: a cold block start ~65k requests (~36 minutes of the whole account), a warm day
  ~8k, the daily capture ~16k. Cold starts are staggered. Fetching daily bars as one ranged request
  per asset would remove ~48k of the 65k (an engine binding change; decision D7).

## 6. Engine changes (vyreon-engine, v1.x)

| # | Change | Kind | Size |
|---|---|---|---|
| E1 | `vyreon-worker` entrypoint: claim loop around today's day/fill code; per-day publish through the manager; lease heartbeat | engineering | M |
| E2 | Storage adapters: tape caches, prints, derive inputs and T−5 artifacts read from and written to object storage | engineering | M |
| E3 | Budget gate, 429 handling, usage reporting (§5) | engineering | M |
| E4 | Determinism: every worker fetches 200 option sessions (or the scan is capped); `last_adjusted` bounded; the derived ex-date scan matches retention; a fixed DLM-TECHNICAL origin; one `VYREON_TAPE_HISTORY_FROM` | values change (declared) | S–M |
| E5 | Packaging: build identity at build time, container image, disk guard by env, TLS for the derive job's store connection | engineering | S |
| E6 | Membership-aware day: members present on T drive the plan, views, intents, surfaces; P01/PATH/tape windows listing-aware; no 64 cap | engineering | L |
| E7 | Provider ticker per date (FB → META roots), challenge registry from split/ticker history, adjusted roots at their own origin (audit items 1, 2, 4) | engineering | M |
| E8 | Absences with causes: `NOT_LISTED`, `BEFORE_PROVIDER_START` (codes to name); a code→cause table (D-094); sources not yet existing become stated absences instead of refusals | science + engineering | M |
| E9 | FIELD on days whose membership differs from 64: published as a stated absence (FIELD pending its membership rulings) until the FIELD science is revisited on the complete database after the backfill (owner, 2026-11-10); then FIELD over the members present (MAP §8) and a FIELD-scoped recompute | S now; science + L later | S → L |

The store side of E6 (per-date inventory, seeding, readback, API objects/rankings/window, derive
`objects()`) is manager work (§3.1). **Identity:** `object_id` stays a stable Vyreon asset key
(today's tickers: META covers Meta Platforms back to 2012, including its FB years); the manager maps
key ↔ Intrinio security id ↔ dated tickers, and the engine asks for "the provider ticker of this
object on T". This avoids rewriting every body's identity while staying correct; a future member
that reuses a ticker gets a distinct key. Decision D2.

**Proof:** after E1–E5, the golden days through the worker path equal `engine-v1` except the declared
E4 changes and provenance (the existing golden comparison); plus the **block-start test**: a day
computed as the first day of a block equals the same day computed mid-block.

## 7. Implementation plan

Effort figures are rough guesses for planning, to be replaced by measured pace after phase 2.

**Phase 0: a place to keep data while we build (~half a day).**
Database integrity is only verifiable from launch day forward, so pre-launch perishable data is kept
on a best-effort basis, not treated as an emergency (owner, 2026-11-10). Create the Spaces bucket
early and sync the droplet's prints (`/srv/vyreon-home/h5-exq01-events`, 1.5 GB for 7 sessions on the
root disk) to it nightly, compressed. Ask DigitalOcean to raise the droplet limit (to ~25–30).
*Owner: approve the bucket ($5/month + $0.02/GB) and the nightly sync on the droplet.*

**Phase 1: measurements (current droplet; ~3–5 days, mostly waiting on runs).**
(a) A warm chain of 5–10 consecutive days with a kept history: per-day time and peak disk of a
running block. (b) Tape materialization time per session (stage T's cost). (c) The block-start
effect: one day computed fresh (131 sessions) vs inside the chain (200). (d) The size of a day's
derive inputs. (e) One timed day on a 4 vCPU node (*owner approves a temporary droplet, <$1*). (f)
Intrinio: access codes and file downloads (ask support or test with the usage endpoint).

**Phase 2: manager core (this repo; ~2 weeks).**
Rust workspace; schema and migrations (store moved from the engine, universe, work, budget, coverage,
artifacts); universe builder from Intrinio (security records, ticker history, listings, provider
starts) for the 64 at first; worker API; budget; publish path; reader API (new endpoints + ported
v1); scheduler with Kubernetes Jobs (a fake runner first); notices. Tests on real data (frozen slices,
D-102).

**Phase 3: engine as worker (engine repo, v1.x branch; ~2 weeks, overlaps phase 2).**
E1–E5. Exit: golden days through the worker path pass, and the block-start test passes.

**Phase 4: cluster (~1 week).**
*Owner approves the spend.* DOKS in nyc1 with the core pool (CloudNativePG, KEDA, the manager),
registry, secrets; size the core node; a worker pool scaling up from zero and back; a backup and
restore of Postgres through Spaces; the store imported from the droplet; a 2-node trial on one week of recent dates compared with the droplet's runs; then the daily chain in
parallel with the droplet's capture for several sessions, compared, and the capture moved over (the
droplet keeps capturing until the cluster has matched it).

**Phase 5: backfill, stage 1, the recent fixed-universe range (~1–2 weeks of runs).**
Today's engine can only run days where all 64 exist with full windows: ETHA lists 2024-07-23 and
its P01 and PATH windows need 131 and 153 sessions after listing, so roughly **2025-03 onward** (~420
sessions) needs no E6–E9.
Stage T from 2023-05-24, then stage A in blocks, era checks first, then stage B. This delivers the
recent history readers see most (the 252-session context) and proves the system at scale.

**Phase 6: point-in-time universe (engine + manager; ~2–3 weeks).**
E6–E8 and E9's stated absence; validated on era-boundary days (2021-09, 2022-05/06 with FB→META,
2023-05, 2024-01 IBIT, 2024-07 ETHA). No FIELD science rulings here.

**Phase 7: backfill, stage 2, back to 2021-09-27.**
Stage A for the older days (independent of stage 1's days), then stage B re-derives everything in
order (~5 min a day; the derived layer of stage 1's days gains its older history). Because base days
are independent and the ordered derive is cheap, **the start date is no longer fixed once**: older
years can be added later at the cost of a re-derive.

**Phase 7b: FIELD science on the complete database (owner).** FIELD's rulings for a changing
membership (E9) are made with the whole history in hand, then a FIELD-scoped recompute fills the
older days and re-derives FIELD's readers (BETA reads FIELD-DEP01). It is cheap because FIELD's inputs
are daily bars (re-downloaded, ~64 requests with D7) and the tape caches kept in object storage. Stage
1's FIELD values need no recompute if the rulings leave N = 64 unchanged.

**Phase 8: steady state.** Daily chain, recompute jobs, pruning, backup restore drill, coverage
report per sensor (≥80% target, misses diagnosed), the current droplet retired.

## 8. Costs (monthly unless stated)

| Item | Cost |
|---|---|
| Core node = data box (2–4 vCPU) | ~$24–48 (today's droplet $112 until it is retired) |
| Postgres volume (~250 GB) | ~$25 |
| Spaces | $5 (+$0.02/GB over 250 GB) |
| Registry | $0–5 |
| DOKS control plane | free (HA $40, not needed) |
| Daily worker (~4–5 h/day, 4 vCPU) | ~$11 |
| Reader API exposure (if a load balancer) | ~$12 |
| Backfill stage A, full 2021-09-27 → now | ~$450–550 compute per pass (+ stage T, to measure) |
| Web droplet | unchanged |

## 9. Decisions for the review

Proposals with a recommendation; each is evaluated when its phase starts (owner, 2026-11-10).

- **D1 Object storage for compact intermediates** (tape caches, per-day derive inputs) beside the
  prints. Recommend yes: it makes stage B and derived recomputes cheap and EVC01 possible on
  disposable machines; it is D-160's planned next step.
- **D2 Identity:** stable Vyreon asset keys mapped to Intrinio ids in the manager (recommend) vs
  rewriting the engine to security ids.
- **D3 Orchestration — decided (owner, 2026-11-10):** DOKS with the always-on core node as the data
  box; KEDA ScaledJobs per job type; worker pools 0..N. Rejected: self-hosted k3s (we would write
  the node autoscaler ourselves; DigitalOcean's cluster autoscaler only drives DOKS pools); a
  separate data-box droplet with the cluster at zero nodes (same droplet count, but against
  DigitalOcean's recommendation and two systems to deploy); scripted droplets without Kubernetes.
- **D4 Postgres — decided with D3:** CloudNativePG on the core node, volume-backed, continuous backup
  to Spaces. Accepted cost: minutes of downtime when the node is replaced at an upgrade we schedule.
  DigitalOcean managed Postgres stays an option if that proves a problem (more cost).
- **D5 Staged backfill:** stage 1 (~2025-03 → now) before the universe work (recommend) vs one
  backfill after it.
- **D6 Determinism rules (E4):** 200 option sessions everywhere; a fixed DLM-TECHNICAL origin (e.g.
  the asset's first session or the database start minus 379 sessions); these change some values
  against the golden days (declared).
- **D7 Daily bars as one ranged request per asset** (−48k requests per cold block; binding change).
- **D8 FIELD rulings (E9):** deferred by the owner (2026-11-10) until the backfill is complete;
  FIELD is a stated absence on days without the full 64 until then (phase 7b).
- **D9 Manager language:** Rust (recommend: shares the engine's crates and types) vs Python.
- **D10 Company capture on historical dates** (~2.7k requests a day): fundamentals are not served as
  of a past date; confirm what the company owners should read in the backfill.
