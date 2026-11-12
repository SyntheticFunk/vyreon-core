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

## C-023 The universe follows the provider's records (2026-11-10)
Read from the real responses for the 64 (fetched 2026-11-11 UTC, frozen in
`crates/test-support/fixtures/intrinio-universe`). Corrections to what we assumed:
- An Intrinio security id is **not** stable across a rename. A security record is one id with one
  ticker: Meta is `sec_gQ8B8y` (FB, to 2022-06-08, merged into `sec_XO82JO`) and `sec_XO82JO` (META),
  both dated from 2012. FIGI and composite FIGI are shared by the two. So a member is keyed by its
  Vyreon asset key and today's security id, and its ticker on a date comes from the chain of records.
- **IBIT was another security's ticker** (a Defiance ETF, 2022-09-08 to 2023-05-01), as META was the
  Roundhill ETF's to 2022-01-28. Both fall inside the option history: a request by ticker for those
  dates returns the other fund. It does not touch the first build (from 2025-03); the backfill must
  treat the member as not listed there (E7, E8).
- The history listings are incomplete: empty for DBA, SLV and UUP, undated for EEM's earlier CFEM
  records, and GOOGL's `previous_tickers` names GOOG with no chain record. The security record's own
  first price date is then the evidence.
Tables (migration 0004): `core.security` (every provider record: members, their earlier tickers, the
other users of their tickers; listing dates are columns, not a table), `core.member`,
`core.ticker_span` (derived, a date range per ticker, overlap excluded by constraint),
`core.source_coverage`, and `core.members_on(profile, date)`. Membership windows (`from`, `to`) are
left until members vary (E6). Core does not call Intrinio yet: `vyreon-core universe load` reads
saved responses; the fetch joins the budget work (A7).

## C-024 The publish contract and what core checks (2026-11-10)
`crates/core-contract` is the whole contract (VYREON-DAY-BUNDLE@1, now @2: C-030): one day of one profile as a gzip
tar holding a header (the day, the build, one entry per slot), the day's reader scalars and
unusualness scores, and each slot's body, receipt and event stream as the engine wrote them.
- **Core takes each slot's identity, state and versions from the header.** The engine's store dug
  them out of bodies and receipts, whose shapes differ by sensor family (four body shapes, three
  receipt shapes in one day) and are the engine's own; doing that in core would tie core to every
  engine version (C-012, C-019). The engine keeps its identity check before it bundles. Core checks
  what is generic: the exact slot list against the build's registered inventory, every digest, the
  stream commitments (root, events, length, line count), the scalars' slots, and the base binding.
- **Stage, verify and publish are one transaction.** A staged revision is never visible and a
  refused bundle leaves nothing. The readback is done inside that transaction, on what the
  database returns, before promotion.
- **A day's reader scalars and unusualness scores travel in its bundle.** They are the engine's
  projection of the bodies; core cannot derive them. Today the engine's derive writes the base
  day's scalars afterwards, which a single writer rules out: the worker's bundle carries its own
  (Track B, with E10).
- **A revision computed from a base names the base's profile**, and core binds it to the day's
  published revision of that profile; it may also name the revision id it read, and is refused if
  the day's base has been replaced since.
- **The supersession rules are the engine's** (`publish_revision`): the day's previous revision of
  the profile, and, on a base publish, every later day's revision computed from a base.
- **An inventory is written once.** Registering a build again is accepted only with the same rows.
- Left for the recompute work (milestone 5): copy-forward revisions (`carried_from_revision`,
  `republish_scope`).

Exit test (2026-11-11 UTC): the droplet's published 2025-12-10 (2,853 base and 1,826 derived slots,
320 streams of 724 MB, 13,723 scalars) exported read-only as two bundles (96 MB and 1.7 MB), published
through core into an empty PostgreSQL 16 in 17.6 s, 151 MB on disk. A listing of every slot, stream,
scalar and revision taken with psql from the droplet's store and from core's is identical (18,724
lines; digests, text, JSONB, receipts, versions, states, scalar values).

## C-025 The queue, the budget and the worker API (2026-11-10)
- **The queue is a PostgreSQL table** claimed with `FOR UPDATE SKIP LOCKED` (the documented way to
  run a queue with several consumers); no message broker (C-014). One view, `core.claimable_job`,
  says what may be claimed, and both the claim and the autoscaler's count read it.
- **Order:** a day block waits for every tape job up to its last session; the ordered pass takes one
  day at a time, behind the published days. Blocks themselves run in any order.
- **A lease is a token.** The claim (made with the workers' shared token, of which core holds only
  the SHA-256) returns a random lease token; every other call carries it; core keeps its digest. A
  lease not renewed for `VYREON_CORE_LEASE_SECONDS` (300) expires and the job resumes at its first
  session not done. Three failed attempts leave a job FAILED and pause the queue (amended for day
  blocks by C-035); so does a refused bundle. Nothing is claimed while paused; a running worker learns of it at its heartbeat.
- **A worker's bundle must be its job's next day**, for its profile and build, and the day is
  published and the job moved on in one transaction. One bundle is published at a time.
- **The budget is a reservation, not a rate estimate.** A worker draws a lease of up to 600 requests
  valid 30 s; the lease counts against the ceiling (1,800 of Intrinio's 2,000 a minute) from its
  grant until 60 s after its end, at full size while open and at what was used once released. The
  requests of any rolling minute therefore cannot exceed the ceiling. Sustained throughput is about
  1,200 a minute if every lease is held to its end, more when workers release early.
- **Tape caches are opaque and written once**: stored as the worker packed them; the same bytes
  again are accepted, other bytes refused. A session without a tape is a stated absence. A tape job
  is done only when every member listed on the session has one or the other.
- **Sessions come from the engine's calendar**, loaded as a list; core does not compute trading days.
- **The claim carries the member manifest**: for each session, each member's key, security id,
  ticker in use (none when not listed), group and company flag, and the provider sources' first
  dates. The engine ignores it until E6.

## C-026 The reader API, and what it cannot serve yet (2026-11-10)
`crates/core-api/src/reader.rs` (VYREON-CORE-API@1), the shape of AGENTS.md and C-009: sessions from
the calendar with NOT_PUBLISHED explicit; an asset's history session by session, `detail=full` (each
complete body, the default) or `detail=headline` (each measurement's reader scalars); the exact body
and event stream by revision, as the engine's API serves them; `/v1/status`; hashed keys with
`read` and `admin` scopes. One response is one database snapshot, so it never mixes revisions. Full
history pages at 20 sessions, headline at 260. No Parquet yet (no reader has asked for it).
- **Not served: per-coordinate state, reason, unit and quality.** The engine's API offers a
  `coordinates` view that decodes each body family into role objects with those fields. That decoder
  is engine code over body shapes the engine owns; core holds none (C-012). Until decided, a reader
  gets the supported numbers from `headline` and everything else from the body. Proposal for the
  owner: the engine writes each slot's coordinates projection into the day's bundle, as it already
  writes the scalars, and core stores and serves it (roughly +30% store size). The alternative is
  Reporter decoding bodies with the engine's crate.
- **Dates are `YYYY-MM-DD` at every boundary** (`core_contract::is_date`): PostgreSQL also reads
  words such as "yesterday" as dates, which a test caught on the session route.
- The service reads through its one database pool; a separate login in the `vyreon_reader` role
  comes with the deployment.

## C-027 Notices are an outbox; automated admission waits for track B (2026-11-10)
- A notice is a row written in the same transaction as the event it reports, and the service posts
  it to the Discord channel afterwards (the transactional outbox pattern): no notice for an event
  that was rolled back, none lost when Discord or the service is down, and they arrive in order.
  Every post carries Discord's silent flag and an empty mention list (channel only, never a phone
  ping). The webhook URL comes from the file named by `VYREON_CORE_WEBHOOK_FILE` and is kept out of
  logs and stored errors. Without that setting notices wait unsent.
- Reported: the queue paused (with the reason that stopped it) and resumed; each day block done;
  the last tape or ordered-pass job done; a build admitted or withdrawn; the trading calendar within
  45 days of its last session (C-014f).
- **Automated build admission is not built in track A.** What admits a build is the engine's golden
  comparison (values equal except the declared changes and provenance) and the block-start test,
  run through the worker path; both are track B's proof harness, and comparing values is the
  engine's knowledge, not core's. Core's part exists: a build claims nothing until
  `vyreon-core build admit <build> <evidence.json>` records what it passed. Track B's harness will
  write that evidence file; admission stays one command by hand for the first build, as planned.

## C-028 A day sent twice is answered, not refused (2026-11-10)
A worker whose publish succeeded but whose answer was lost will send the day again. Core answers
"already published" (and writes nothing) when the bundle is of an earlier session of the lease's own
job, for the same profile and build, with every slot's body digest equal to the published
revision's. Anything else that is not the job's next day is still refused and pauses the queue. This
makes the publish call safe to retry, so the worker retries it on a transport failure.

## C-029 Owner decisions on E11, coordinates and the security key (2026-11-11)
- E11: an absent provider input withholds its assets with a new provider-limited reason,
  `PROVIDER_INPUT_MISSING`, outside the 10% engine-fault hold; FIELD follows its existing
  missing-member rules; more than 6 of 64 withheld holds the day for a retry.
- C-026 is settled: the engine writes each slot's coordinates projection into the bundle; core
  stores and serves it. This is a contract change (a new bundle version) made with track B.
- The security key is Vyreon's own permanent asset key, the practice of reference databases (CRSP's
  PERMNO): every provider identifier can change or be reused, so each is stored against the key as
  dated evidence, none is the key. `AGENTS.md` corrected to say so.

## C-030 Bundle contract 2: coordinates views and the run report (2026-11-11)
Settles C-026 as the owner decided (C-029). `VYREON-DAY-BUNDLE@2` adds, per slot, an optional
`coordinates.json` with its digest in the header, and a `report` object in the header. Core checks
the digest and that both are JSON objects, stores them (`vyreon.measurement.coordinates`,
`vyreon.day_revision.report`, migration 0009) and serves the view with `detail=coordinates`. Core
reads only contract 2; nothing in production wrote contract 1.
- The view is the engine's own projection, built by the code its API uses; core cannot recompute
  it and takes it as stated, like a slot's state and versions.
- **Measured on 2025-12-10's retained real exports (448 slots):** the 320 slots projected from their
  bodies have 5.5 MB of views beside 8.3 MB of bodies. The 128 stream-backed M0 slots' views are
  their event streams over again (202 MB beside 201 MB of events), so the engine leaves them out
  and a reader takes those coordinates from the stream, which core already serves byte for byte.
  The earlier estimate of +30% store size was wrong in both directions: about +65% of body size for
  body slots, and it would have doubled stream storage. If Reporter needs M0 views pre-projected,
  that is the cost to weigh.
- `detail=full` stays the default until Reporter's needs are settled (AGENTS.md names coordinates as
  the default; a day published without views would then answer nulls).
- The report (E13) is the engine's to shape: today the day's stage seconds and the worker block's
  Intrinio requests by category so far (later days download during a day, so the count is the
  block's, not one day's).


## C-031 B3's shape: the day-local derived sensors are a second profile and a second bundle (2026-11-11)
ARCHITECTURE left "one profile or two" to when E10 starts. Two.
- **Why not join the base profile:** its slot set would change, so its revision string would, and
  every base body names that string: all 2,853 bodies of a day would differ from `engine-v1` byte
  for byte and the golden comparisons would lose their sharpest check for no gain in values.
- **So:** the engine's derived profile splits. The 18 day-local sensors get their own profile
  revision, computed in the day from the run's own exports and files and published by the day-block
  worker as a second bundle on the day's base (`base_profile_revision`). The 12 history sensors
  keep a profile of their own for the ordered pass.
- **Body identity changes, values must not.** A derived body today hashes the list of every
  published revision id into its `result_id` and names the base revision id in `parents[]`. A
  worker knows neither and a day-local sensor reads no history, so the 18 name their parents by
  result id and body digest only and carry no history digest. Proof is role equality with the
  droplet's published derived bodies.
- **Core must follow:** publishing a base supersedes every later day's revision that was built on
  a base, because the ordered pass reads history. A day-local revision of a later day reads none
  and must stay. The bundle header says so (`day_local`, an optional field of contract 2 that
  defaults to today's behaviour; migration 0010 keeps it with the revision), and core supersedes a
  later day's based revision only when it is not day-local. A republished base still supersedes
  its own day's day-local revision.
- The engine's direct-to-Postgres derive is not kept working for the 18 on the `worker` branch; it
  becomes the ordered pass.
- **A day block says when its day is done** (2026-11-11). Publishing the base used to move the
  block to its next day, so the day's second bundle would have been refused. A day-block publish
  no longer moves the job; the worker posts `days/{date}/done` after its bundles, and core checks
  the day's base from that build is the published one. A bundle of the current day sent again is
  answered, not written twice, as for earlier days. A day block may publish a base or a day-local
  revision, the ordered pass only a based revision that reads history. Consequence, accepted: the
  day's base is readable for the seconds before its day-local revision lands (C-015's "one
  all-or-nothing step" is two transactions); a worker that dies in between repeats the day.

## C-032 Owner rulings, 2026-11-11 (E4, E11's scope, two-step days, pushing)
- **E4 goes ahead as planned.** A day's values must not depend on what a disk holds, or stateless
  workers cannot work: 200 option sessions everywhere (the corporate-action lookback with it), a
  fixed bar origin for the technical sensors, `last_adjusted` bounded, one tape-history start, and
  the oldest tape sessions gaining their volume reconciliation. Each change is still measured on
  real days and declared with before-and-after values.
- **E11 narrows.** A missing input withholds the sensors that read it and whatever depends on
  them downstream, not the whole asset. The as-built whole-asset withholding (proven 2026-11-11)
  is the starting point; UNG's price and tape sensors should publish when only its chain is
  missing, and the cross-asset sensors should lose a member only where they read what is missing.
- **A day in two publishes is accepted** (C-031): nothing reads the database live; the product is
  a weekly publication.
- **Pushing:** both repositories may be pushed whenever there is something to push.

## C-033 E4: `last_adjusted` needs a short, fixed lookback; its length is not settled (2026-11-11)
The marker depended on how many daily sessions a disk held. Two findings, same day:
- **A long lookback buys nothing at the far end.** On the droplet's real chains the overlay's
  frozen rule had no row to drop for any adjustment 25 sessions old or older (MILESTONES, E4).
- **"Scan the 200-session quote window" is not deterministic either** (my first version of this
  entry said it was; it was wrong). A day rebuilds the quote file of every session d in
  [T-200, T], and d's marker looks back from d: for the oldest files that reaches before T-200,
  where a worker has no quote files, so new-root and strike-shift adjustments cannot be detected
  there and the same file gets a different marker depending on where T is. The stale rows are
  large when it matters (XLE: 1,137 rows, 2.1 million contracts of open interest).
- **So:** the marker of session d reads a fixed lookback [d-L, d), and the quote window a worker
  downloads grows by L+1 sessions so every file in the day's window has what its marker needs.
  L must outlast the stale rows: ETHA's were gone 5 sessions after its adjustment, the sector
  funds' were still there at 6 and are unmeasured until 200. L is chosen when that is measured
  (about 20 provider requests); until then nothing is built.

## C-034 An asset's option history starts at its first chain with open interest (2026-11-12)
The two-node run stopped twice on ETHA's open-interest history. Intrinio's EOD chain is empty for
ETHA until 2025-04-11, and that first chain has 168 contracts, every one with zero open interest
(open interest is the prior session's settled count, so a listing session has none). The engine's
open-interest node faults on a chain with no open interest, and a faulted history node stops the
day's export. The 223-session option window (E4) reaches back to 2025-01-22; engine-v1 never met
this because its disk held 131 chains, all after the listing.
- **Rule (engine 60692393, 2fccd1ba):** walking back from T, a chain that is empty or reports no
  open interest, with no chain with contracts before it in the window, ends the history, with the
  terminal the engine already uses for "no capture before this" (`CAPTURED_HISTORY_START`, reason
  NO_PRIOR_HISTORY). No new state. All four endpoint histories share the one start.
- **Correction:** the first version of this rule (empty chains only) was written on a wrong
  diagnosis; the failure log of the next attempt showed the zero-open-interest chain was the cause.
  The empty chains are still excluded, since a session before the listing is not history.
- **Not covered:** an empty or zero-open-interest chain with contracts before it (a gap inside
  the history) still stops the day; so does any other faulted history node. That belongs with
  E11's narrowing after the build. An asset whose options are not listed at T itself is the E11
  case (its option sensors withheld, input missing).
- Tested on ETHA's real captures of 2025-04-09 to 04-14 (fixture `etha-first-chain`).

## C-035 A day that fails for the last time is set aside; the build goes on (2026-11-12)

A day block out of attempts used to stay FAILED whole and pause the queue, so one bad date stopped
every node. Now the job keeps only the day it stopped on (FAILED, with its leases and logs), the
days before it become a DONE job, the days after it a new QUEUED block, and a notice is posted.
The queue pauses when `FAILED_DAYS_PAUSE` (3) day jobs stand FAILED: several at once look like the
engine, not the day, and each failed day has already cost three attempts of node time. Three is a
plain declared default, not tuned. A tape job still pauses the queue on its last failure, because
the day blocks wait on it. An ordered pass out of attempts stays FAILED without a pause: later
passes wait for it by the claim rule, the day blocks do not read it, and a pass takes seconds to
run again once its cause is fixed. `queue retry-failed` puts the set-aside days back.
Reason: the owner's rule that a stopped day never holds up other dates (2026-11-12), with the
engine's sensor-level withholding as the first line (a fault withholds its sensors, not the day).
