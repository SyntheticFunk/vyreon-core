# Decisions

Core engineering decisions: short, dated, with reasons. Order of authority (owner, 2026-11-10):
product intent and reasonableness first, then established standards (cited); at this stage a
science change that makes the system implementable is preferred over keeping science that cannot be
built, provided the results stay correct. Science decisions live in the engine's
`docs/DECISIONS.md`; open proposals in `docs/ARCHITECTURE.md` §9.

## C-001 Workers compute blocks of consecutive dates (2026-11-10)
Product intent: each day is computed from T alone and its transient data destroyed. Blocks keep that
property and only share the download: a cold day spends ~200 of its ~360 min downloading (parity run,
2026-11-09), and consecutive days share most of the 132-session quote window. Every day must be a pure
function of (T, the provider's data, the declared windows) whatever block or position computes it;
the block-start test proves it (a block's first day equals the same day computed mid-block).

## C-002 Orchestration: DOKS with an always-on data box (owner, 2026-11-10; ARCHITECTURE D3)
DigitalOcean Kubernetes in nyc1: one fixed core node (`s-4vcpu-8gb-amd`, $56/month) runs Postgres,
the vyreon-core service and KEDA; worker pools autoscale 0..N; one KEDA ScaledJob per job type reads
the claimable count from Postgres. The job table is the source of truth; Kubernetes only runs
containers. Rejected: self-hosted k3s (we would write the node autoscaler), a separate data-box
droplet beside a zero-node cluster (two systems to deploy), scripted droplets without Kubernetes.

## C-003 Postgres: CloudNativePG on the core node (owner, 2026-11-10; ARCHITECTURE D4)
The standard Postgres operator for Kubernetes: data on a DigitalOcean volume, continuous backup
(base backups + WAL) to Spaces, restore drilled. Accepted cost: minutes of downtime when the node is
replaced at an upgrade we schedule. DigitalOcean managed Postgres stays the fallback (more cost).

## C-004 Kept data: Intrinio is the store; Spaces holds the perishables (owner, 2026-11-10)
Data from Intrinio is treated as always obtainable from Intrinio. Only what it will not serve again,
or serves differently later (volatile: option prints, ACT01 pulls, company captures), is kept, plus
the outputs. No raw or intermediate copies of re-downloadable data (ARCHITECTURE D1 declined: we
cannot afford to store it). Bucket `vyreon-data` (nyc3) receives the droplet's nightly archive
(`deploy/droplet/archive-perishables.sh`; copy, never sync). Pre-launch losses are tolerated:
integrity is verifiable only from launch day.

## C-005 FIELD deferred until the backfill is complete (owner, 2026-11-10; ARCHITECTURE D8)
FIELD is a stated absence on days without the full 64 members; its membership rulings are made with
the whole history in hand, then a FIELD-scoped recompute.

## C-006 No separate measurement phase (owner, 2026-11-10)
The cold day on 8 vCPU is measured (parity, 2026-11-09: download 200 min, compute 160 min, peak disk
44.7 GB). The rest (a warm day inside a block, the block-start effect, tape cost per session, the size
of a day's derive inputs, a day on 4 vCPU) is measured on the cluster's first blocks, on the code
path that will actually run, leaving the droplet to its daily capture.

## C-007 vyreon-core is written in Rust (2026-11-10; ARCHITECTURE D9)
It reuses the engine's crates (sensor registry, coordinates, store logic) as pinned git dependencies
and the engine API's stack (axum, tokio-postgres), so the store's types and checks are shared, not
re-implemented. Rust is the default; Python only where necessary (owner, 2026-11-10).

## C-008 Daily bars as one ranged request per asset (owner, 2026-11-10; ARCHITECTURE D7)
Wherever the provider allows, a window is fetched as one ranged request per asset instead of one per
bar: daily bars alone go from ~48.6k requests per cold block to ~64. It changes the engine's frozen
binding `INTRINIO-CI-DAILY-BAR-1@1`; values must be identical, checked on the golden days.

## C-009 The reader API follows telemetry-backend's shape (owner, 2026-11-10)
Core stores the final measurements with their provenance and serves them to vyreon-reporter (and
later vyreon-web) by API. The prototype `/home/diggz/telemetry-backend` (Financial Telemetry V1) is a
guide to the shape, not a 1:1 template: it stores far less (24 assets x 15 scalars a day; core keeps
every measurement, ~4,679 slots a day, with coordinates, state, reason, unit and quality). Taken from
it: sessions (latest, by date), an asset's latest and its history (cursor-paginated, bounded),
a report context (a session with the previous one), ingest/admin status, `/health` and `/ready`;
hashed API keys with scopes; an ETag from the canonical revision's digest; corrections kept, one
canonical revision per day; the roster and versions in tables, not constants; checksummed migrations
under an advisory lock. The engine's own v1 routes (`vyreon-api`) are not ported: nothing uses them.

## C-010 Tape caches are kept, in Postgres (owner, 2026-11-10)
EVC01's baseline reads a compact cache (~70 KB per asset-session) of every tape session since
2023-05-24: ~4 GB in all, ~1.1 GB a year. Re-streaming the raw tapes instead (~30 GB a session, every
earlier session, every day) is infeasible, so the caches are kept; the raw tapes stay with Intrinio.
They live in the database (one row per asset-session) because objects this small are better served
by a database than by file or object storage (Sears, van Ingen, Gray, "To BLOB or Not To BLOB",
Microsoft Research 2006: under ~256 KB the database wins), and it keeps one store, one backup and no
storage keys on workers. Spaces keeps the perishable archives (files of MBs) and the database backups.

## C-011 A worker computes every measurement that reads its day's inputs (owner, 2026-11-10)
The owner's model of a worker: download what it can, compute its measurements, write them to the
permanent database, exit. Measurements that read the day's own downloaded inputs (today's derived
families DLM-CHAIN, -GREEKS, -GAMMA and the expected move, which read T's option population, surface
and VWAP cache) move into the worker's layer (engine change E10). The ordered derived pass then reads
only the database. Rejected: staging those inputs in Spaces until the ordered pass reaches the day (a
second, unreliable hand-off). Recomputing one of these families later re-downloads the day.

## C-012 Core is generic; engine code runs only from the engine's image (owner, 2026-11-10)
Workers, the publisher (stage, verify, publish, readback) and the ordered pass are the same engine
image at the same build. Core holds no engine code, so it is not rebuilt per engine version and
cannot drift from the workers; a rebuild attempt with a new engine is a new image. The publisher and
ordered pass run on the core node and are the only pods with database credentials.

## C-013 Database backups come after a database worth keeping (owner, 2026-11-10)
No base backups or WAL archiving during the first builds: a lost build is rebuilt. Backups to Spaces
and a restore drill are set up when a database is accepted. The perishable archive is unaffected.

## C-014 Robustness built in from the start (owner, 2026-11-10)
(a) Measurement tables partitioned by month (PostgreSQL declarative partitioning). (b) A build is
admitted to the queue only after its golden days and block-start test pass on the cluster. (c) A
missing input withholds the assets it touches, not the day (engine E11). (d) One declared list of
input windows for the plan and the readers (E12). (e) Per-stage timings and request counts with
every day (E13). (f) A health check on the frozen calendar's last session. Left out on purpose: a
high-availability database, a message queue, several regions, several core services, stored
intermediates.

## C-015 Derived sensors split between the worker and the ordered pass (owner, 2026-11-10)
The 18 day-local derived sensors are computed by the worker and published with the base layer in one
all-or-nothing step; the 12 history sensors are published by the ordered pass, which reads only the
database (ARCHITECTURE §2.1; engine E10). Measurement ids do not change. No value may change; the
golden days check it.

## C-016 Workers publish over HTTP (owner, 2026-11-10)
A worker sends each finished day to core as one compressed bundle; the publisher job checks and writes
it. Workers hold no database or storage credentials and there is no shared storage between them.

## C-017 Stable Vyreon asset keys (owner, 2026-11-10; ARCHITECTURE D2)
Each asset has a permanent key; core maps it to the Intrinio security id and the dated tickers.
Tickers are reused (FB, META), so history is never keyed by ticker. The 64's keys are today's tickers.

## C-018 A rebuild attempt is a new database (owner, 2026-11-10)
An attempt fills a fresh database beside the current one; readers switch when the owner accepts it.
Tape caches are copied. There is no database yet: the first attempt is simply the first one.

## C-019 Core is the authoritative database; the engine is a stateless worker (owner, 2026-11-10)
Revises C-012 and E15's ownership note. Core owns the whole database: every table, the migrations,
the partitioning, and the only write path. The engine owns the science and nothing that lasts. The
boundary is a versioned **publish contract**: a day's bundle (bodies, receipts, streams, digests, the
build and its slot inventory) that the engine produces and core checks and writes with its own code
(digests, exact inventory, all-or-nothing publish, readback). No engine process holds write
credentials. The ordered pass is an engine job that reads history and publishes its day through the
same contract; for the first build it reads with a read-only database role, later through core's
reader API. The engine's `db/schema.sql` and `vyreon-store` become the starting point of core's
schema and publish code and are retired from the engine once the worker path is proven.

## C-020 The ordered pass reads core's database directly; the reader API is built early (owner, 2026-11-10)
The ordered pass reads history with a read-only database role (accepted, not a stopgap to hurry away
from). Core's reader API is built in milestone 1's tail rather than after the first database: it is
small, vyreon-reporter needs it anyway, and engines can read through it too. Moving the ordered
pass's reads onto it is an engine change made when it pays for itself.

## C-021 The store schema: what changed from the engine's (2026-11-10)
Migration 0002 starts from the engine's `db/schema.sql` at engine-v1 and changes this:
(a) `measurement`, `measurement_stream`, `reader_scalar` and `unusual_score` carry the represented
date in their primary keys and are range-partitioned by month (C-014a; a PostgreSQL primary key on a
partitioned table must include the partition key). (b) A composite foreign key
`(revision_id, represented_date)` to `day_revision` ties each row's date to its revision's, and a
derived revision's base must be a revision of the same day, so the date added in (a) cannot disagree
with the revision. (c) No default partition: a row for a month without partitions is refused, not
parked. `vyreon.ensure_months(first, last)` creates partitions by create-then-attach, which leaves
the parent readable and writable (PostgreSQL documentation, "Table Partitioning", 5.12.2.2). It is
called when work is scheduled, never inside a publish: attaching briefly locks `day_revision`, and
two publishing transactions doing it would deadlock. (d) Identity columns instead of `bigserial`
(the SQL standard form). Core assigns revision ids; a derived body records its parents' revision
ids, so the ordered pass must read them from core. (e) Left out: the engine's `republish_job` (a
recompute is a job in core's queue), `revision_progress`, the D-080 backfill, and the `vyreon_api`
login (logins and passwords belong to the deployment; the migration creates only the `vyreon_reader`
group role where the deployment has not, and grants it the four reader views). (f) The slot
inventory is keyed by engine build as well as profile (migration 0003), and every revision names a
registered build. `publish_revision` is rewritten with the publish path (A6).
A body's digest is the SHA-256 of its canonical text plus the final newline the engine writes (the
engine hashes the body file and stores the text without the newline).

## C-022 A table lands with the step that first uses it (2026-11-10)
The plan's A4 listed every core table in one step. Instead each group gets its migration with the
code and real data that exercise it: the universe with A5 (shaped by Intrinio's real responses),
coverage with publish (A6; it may be a view over `measurement`), work, budget and tape caches with
the worker API (A7), API keys with the reader API (A8). Reason: tables designed ahead of their code
are guesses, and an applied migration is never edited. Engine builds and inventories are in now
because publish needs them next.
