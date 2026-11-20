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
*Amended 2026-11-15 (audit 2026-11-14, core.md 17):* each worker pod mounts only the keys of the
`vyreon-worker` Secret it reads; tape and day-block pods mounted the whole Secret, the ordered
pass's database login included (C-020's one exception). Manifest edited, not applied.

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
  *Amended 2026-11-15 (audit 2026-11-14, core.md 3):* the transaction commits only after the
  bundle's reader says it reached the end. A reader that failed after the last slot (an entry the
  contract does not have, a damaged tail) closed its channel, which was taken for the end: the day
  was committed, then reported refused and the queue paused.
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
- *Added 2026-11-15 (audit 2026-11-14, core.md 12):* **a day is published only on a session of the
  calendar core holds** (C-025); any other date is refused, and with no calendar loaded the publish
  fails. A lease's day was always its job's session, but `vyreon-core publish` took a Saturday,
  which the history never lists and whose `/v1/sessions/{date}` is a 404. Loading the calendar
  therefore comes before publishing by hand as well.
- *Added 2026-11-15 (audit 2026-11-14, core.md 9):* **an entry of a bundle is at most 256 MiB
  uncompressed** (`core_contract::MAX_ENTRY_BYTES`), checked on its tar header before anything is
  allocated. The upload limit (2 GiB) bounds compressed bytes only, and the reader reserved
  whatever an entry's header claimed, so one damaged entry could exhaust the core node's memory
  and stop the service for every worker. The largest real entries are event streams, about 2.3 MB
  on average on 2025-12-10; the maximum is not measured. Left: the tar library reads GNU long-name
  and pax entries itself, unbounded; a worker's bundle is authenticated, so this is a fault guard,
  not a security boundary.
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
  *Amended 2026-11-15 (audit 2026-11-14, core.md 13):* a release reporting more than the lease
  granted is counted at what it reports (migration 0015 lifts the column's cap at the grant), so
  the overrun holds back the next grants until it ages out; the release still answers with an
  error naming the overrun. It was counted at the grant: 900 reported on 600 granted left 300
  requests outside the ceiling.
  *Amended 2026-11-15 (audit 2026-11-14, core.md 14):* core counts a lease from the clock when it
  is granted (not the transaction's start, which precedes the wait for the budget's lock) to 5 s
  after the validity the worker is told (`budget::TRANSIT_SECONDS`, a plain allowance for the
  answer's way back, not measured): the worker starts its 30 s on receipt, so core's window must
  not end before the worker's. A lease now counts for at most 95 s, not 90 s; sustained
  throughput with every lease held to its end is about 1,140 a minute instead of 1,200.
- **Tape caches are opaque and written once**: stored as the worker packed them; the same bytes
  again are accepted, other bytes refused. A session without a tape is a stated absence. A tape job
  is done only when every member listed on the session has one or the other.
  *Amended 2026-11-15 (audit 2026-11-14, core.md 11):* under one method version. A session redone
  under a new method was complete as soon as the old method's rows covered it, so its days could
  start and read neither caches nor absences of the new method. `tape-caches/{session}/done` takes
  `method_version`; without it (today's worker sends none) the method of the session's most
  recently stored cache is the one checked.
- **Sessions come from the engine's calendar**, loaded as a list; core does not compute trading days.
- **The claim carries the member manifest**: for each session, each member's key, security id,
  ticker in use (none when not listed), group and company flag, and the provider sources' first
  dates. The engine ignores it until E6.

- **Amended 2026-11-20 (audit 2026-11-20-pass1-round-cc, core.md, triage c.md, 12): requests outside the budget.** The budget covers the
  workers; a tool run by hand with the same key (`tools/external/datashop.py fetch`) is not drawn
  from it and takes the provider's margin above the ceiling (200 a minute at the default 1,800;
  the cluster runs 1,500). Such a tool runs one thread and at most one keyed call every 0.7 s
  (under 100 a minute, retries included) and is not run while a build holds the budget
  (`tools/external/README.md`). Ordinary day: no change (no worker path).

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
- *Amended 2026-11-15 (audit 2026-11-14, core.md 16):* a history's ETag is taken over every session
  it lists, not only the revisions it serves, so a session added to the calendar inside a range a
  reader holds changes it (RFC 9110 8.8.1: a strong validator changes with the representation).
  Every history ETag changes once with this release.

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
- *Amended 2026-11-15 (audit 2026-11-14, core.md 18):* the queue's pause and resume are written in
  one transaction with their notice by every caller (the refused-bundle path and `queue pause` /
  `resume` used two autocommitted statements, so a restart between them left the queue paused with
  no notice, and a second pause changed nothing). The lease sweep's own transactions are C-038.
- *Superseded in part by C-050 (2026-11-17):* the events above are logged, not posted; the channel
  carries problems (opened once, resolved once), the 09:00 digest and the Saturday check.

## C-028 A day sent twice is answered, not refused (2026-11-10)
A worker whose publish succeeded but whose answer was lost will send the day again. Core answers
"already published" (and writes nothing) when the bundle is of an earlier session of the lease's own
job, for the same profile and build, with every slot's body digest equal to the published
revision's. Anything else that is not the job's next day is still refused and pauses the queue. This
makes the publish call safe to retry, so the worker retries it on a transport failure.
*Amended 2026-11-15 (audit 2026-11-14, core.md 15):* "the same bundle" also means each slot once
and the same listed members as the stored revision. The recognition compared counts from a join,
so one matching slot repeated as many times as the revision has slots was answered "already
published" without the inventory or listed-members checks (C-024, C-036); such a bundle now goes
through the full checks.

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

## C-036 A member has slots only on the sessions it is listed (owner, 2026-11-14)
Nothing stored may depend on which assets are in the universe. An asset not listed on a session
(before its first listing, after a delisting) has no slots that session; the field object
`FIELD.CORE64` has them every session.
- **Core's universe decides who is listed**: `core.members_on(profile, day)` with a ticker. A day
  bundle states its listed set in the header (`listed_members`, asset keys in profile order; one
  constant, `core_contract::LISTED_MEMBERS`, names the field). A bundle without it, or whose set
  differs from core's, is refused: the worker and core disagree about membership, and neither
  copy is silently preferred. The expected slots are the build's inventory rows of the field and of
  the listed members; the inventory itself stays one template per build.
- **The set is recorded with the revision** (`vyreon.day_revision.listed_members`, migration 0013).
  What a stored day was expected to hold (the readback's count, the reader) comes from that record,
  never from today's universe, so loading the universe again never re-judges a stored day. Days
  published before it record every object of their inventory, which is what was expected of them.
- **The reader:** in an asset's history a published session on which the asset was not listed is
  `NOT_LISTED`, with no revisions: not a 404, not `NOT_PUBLISHED`. `/v1/sessions/{date}` gives each
  revision's `listed_members`. Other shapes are unchanged.
- **The claim** adds `tickers`: each member's every ticker span `{ticker, from, to}` (inclusive,
  `to` null while open), since a day's history windows cross ticker changes (Meta: FB to
  2022-06-08, META from 2022-06-09). `days` is unchanged.
- `vyreon-core engine-store export` states the inventory's objects as listed: the engine's store
  published a fixed member list on every day. Its own `expected_slot` joins read that store and are
  unaffected.
- Tests send the slice's real AAPL bodies of 2025-12-10 under 2024-07-01 (ETHA not listed, IBIT
  listed); core's checks never read a body's date. The field object's path has no database test:
  no frozen slice holds a field body.

## C-037 What a publish replaces takes down everything that read it, and the pass derives it again (2026-11-15)
Audit 2026-11-14 (core.md) 1, 2 and 10. Amends C-024's supersession rule and C-031.
- **Whatever kind of revision is published on day D**, every revision that read what it replaces
  or adds to is superseded in the same transaction: on later days every revision that reads
  history (its windows, or its own earlier values); on D itself, after a base every revision
  computed from the old base (as before), after a day-local revision the revisions of D that read
  history (DLM-UNUSUALNESS reads every reader value of the day). A history revision takes down
  only later days. Before, only a base cascaded, so a replaced day-local or history revision left
  dependants published on its old values.
- **What falls is derived again, not left to vanish.** In the same transaction the finished
  ordered-pass job of each day whose history revision fell is queued again with fresh attempts,
  and a pass running on an affected day is stopped: its lease ends (`CANCELLED`, the worker is
  told the lease is gone at its next call) and its job is queued again without counting that
  attempt. Before, the jobs stayed DONE while their measurements were gone. A revision no job will
  derive again (published by hand, or a day-local revision taken down by a base published outside
  its day block) is named in a notice. The notice is posted only when a pass is queued again or
  something is left with no job, so an ordinary day posts nothing new.
- **A pass is refused if a day it reads was published after its lease was granted** (any revision
  dated on or before its day): it may have read what was replaced. Publishes and ordered-pass
  claims take one advisory lock (`work::PUBLISH_LOCK`, taken before any row lock), and the grant
  and `published_at` are taken from the clock (`clock_timestamp()`), not the transaction's start,
  so the comparison follows the lock's order. With the stop above this refusal should never fire;
  it is the check that the stop happened. The lock replaces the per-day-and-profile one: C-025
  already published one bundle at a time; now that holds across processes too.
- **An ordinary day changes nothing:** a day block's base and day-local revision of D find no
  published revision that reads history on D or later (the pass waits for the block, C-039), so
  nothing is superseded beyond the day's own previous revision and no notice is posted.

## C-038 A job is done when its sessions are; finish, day_done and session_done may arrive twice or late (2026-11-15)
Audit 2026-11-14 (core.md) 6 and 5. Amends C-025.
- **A job whose every session is done is DONE when its lease ends**, by expiry or by `fail`,
  whatever became of its worker's `finish`. Before, a lost `finish` left it QUEUED with no next
  session: the claim panicked on it, and, being the earliest job of its kind, it stopped every
  claim of that kind; a day block out of attempts in that state failed a NOT NULL check inside the
  claim's transaction, so every claim failed. `retry-failed` makes such a FAILED job DONE.
- **Calls a worker sends once are safe twice or late.** `finish` on a lease whose job is DONE
  answers with the job, as the first one did; `days/{date}/done` and a tape session's `done` for a
  session already done answer and change nothing; a publish sent twice was already answered
  (C-028). A late call on a lease that expired with work left is told the lease is gone (410), and
  the next claim redoes the day.
- **The sweep ends each expired lease and moves its job in one transaction of its own** (before:
  the service's sweeper ended every expired lease in one autocommitted statement and moved the jobs
  in others, so a restart between them left jobs LEASED under ended leases that nothing recovers).
  A lease that cannot be ended is reported and does not keep the others from ending. The claim
  sweeps first, outside its own transaction, and ignores what the sweep could not do (the sweeper
  reports it every half minute), so one bad job cannot block every claim; the claim also skips a
  job with no next session rather than read it as one.

## C-039 The ordered pass waits until its own day's block has moved past it (2026-11-15)
Audit 2026-11-14 (core.md) 7. Amends C-025's order and migration 0005's claim rule (migration 0014
replaces the view; 0005 is not edited). A day block publishes its day's base, then its day-local
revision, then says the day is done (C-031). The rule compared the blocks' next session with `<`,
so the pass of day T was claimable between T's base and T's day-local revision; the engine refuses
a day without the latter, and three quick refusals left the pass FAILED, holding up every later
pass. Now a pass waits while any unfinished block of its profile stands at or before its day
(`<=`). The view also leaves out a job with no session left (C-038), and a constraint keeps any job
from being queued without one; the migration first makes DONE the jobs a lost `finish` left that
way. An ordinary day: the pass of T starts after the block's `day_done` of T instead of possibly
after its base alone.

## C-040 One worker pod's failure never stops the others (2026-11-15)
Audit 2026-11-14 (core.md) 8. Each worker Job of `deploy/k8s/30-workers.yaml` ran several pods
under `backoffLimit: 0`: one pod exiting unsuccessfully failed the whole Job, and Kubernetes then
terminates its running pods ("Job termination and cleanup", kubernetes.io/docs/concepts/workloads/
controllers/job). It happened on 2026-11-12, and it defeats C-035 (a failed date is set aside, the
build goes on). Now every worker Job is `completionMode: Indexed` with `backoffLimitPerIndex: 1`
(Kubernetes "Backoff limit per index", GA in 1.33; the cluster runs 1.36): a failed pod is
replaced once and its index then given up, the other pods run on, and the Job is marked failed
only when all of them have finished. No Job-wide `backoffLimit` and no `maxFailedIndexes`, since
either, once exceeded, terminates the running pods. The job a failed pod held returns to core's
queue as before (its `fail`, or its lease expiring); the replacement pod claims whatever is
claimable. The KEDA ScaledJob for the build, not yet written, must keep this property (README).
Edited as files only; nothing was applied.

## C-041 The audit loop lives in the repository; the cut line for the build (2026-11-15)
The Codex audit loop (briefs, scopes, the runner, the fix rules) is in `tools/audit/`, with the
steps in its README. It had lived in a session's scratch directory and was nearly lost when WSL's
disk filled on 2026-11-15.
The engine is ready for the build when an audit of the candidate leaves no verified finding that
would put a wrong number in the database or that only a full rebuild could correct. Other verified
findings are listed and fixed after, by a scoped republish. This replaces "two consecutive clean
audits" (never an owner rule; every round so far has found something, partly in the code the
round before added). Each merged batch of fixes runs a real day before the next batch starts.
Order of authority: a published standard, then product intent, then the owner.
Reason: owner, 2026-11-15; rebuilds and republishes are a planned workload, and the build is the
correctness experiment.
Amended 2026-11-15 (owner): every verified finding is fixed, without asking, on either side of the
line; fixing and gating are separate. Only a finding above the line sends the candidate round
again before the build. The audits have done their job when their findings converge on low
meaning, which an auditor's report never being empty does not contradict. The owner and Claude
judge rounds g and h (fixes 8 and 9) converged: no defect in the audited changes, the rest
latent and unreached on real captures. Fix 10 (`pass-fx10`) is the last batch before the build;
its audit runs as usual and, with nothing above the line, its findings are fixed during or after
the build. The remaining gate is the candidate's real days.

## C-042 By hand, core takes a based revision only if it can check its base, or on the operator's word (2026-11-15)
Audit 2026-11-15 (fix round A, core.md) 1 and 9. Amends C-024 and C-037.
- **`vyreon-core publish` has no lease**, so C-037's check (a day the bundle read was published
  after the lease was granted) cannot run, and `bind_base` bound a bundle naming no `base_revision`
  to whatever base is published now. A history revision of T computed before an earlier day was
  replaced, or a revision computed on a replaced base, was published as it stood: a wrong number.
  Workers cannot reach this path. Now, by hand: a revision computed from a base that reads history
  is refused (the ordered pass publishes those), and one that reads its day only must name the
  base revision it read. A base needs neither.
- **`publish --vouch` is the operator's word** that the bundles were computed from what is
  published now; core then takes them as before and checks nothing more. One flag covers both
  refusals, not the history one alone as the audit proposed: `engine-store export` writes derived
  bundles that cannot name core's base revision (the store's ids are not core's), so a rule with
  no way round it would end publishing an exported day (C-024's exit test).
- **A bundle's gzip stream is read to its end**, where its CRC-32 and length are checked.
  `bundle.json` and `scalars.json` (the reader scalars `detail=headline` serves) carry no digest,
  and the reader stopped at the tar's end, before the trailer. A mismatch refuses the bundle
  before anything is committed (C-024: nothing is written until the reader reaches the end).

Note, 2026-11-15 (fix round C, corepublish.md). Two small corrections: the drain to the gzip
trailer is bounded at 1 MiB of uncompressed tail (`MAX_TRAILING_BYTES`; the engine's writer leaves
1,024 bytes) and a bundle with more is refused, since an unbounded tail was decompressed in full
while a publish waited; and `publish` reads its whole command line first, so `--vouch` counts
wherever it stands and a bad line publishes nothing. One thing is open and not changed here: a
worker's day-local bundle names no `base_revision`, so on the leased path it binds to whatever
base is published now, and a base of the same date published by hand between the worker's base and
its day-local bundle would leave values derived from the old base stored on the new one (the day
block's lease is not ended, as an ordered pass's is). Until that is decided, the operating rule:
during a build, `vyreon-core queue pause`, wait until no day block is leased (a pause stops new
claims, not a lease already held), then publish the base by hand, then resume. (Corrected the same
day after the audit of this note: pausing alone left a live lease free to publish.) The trailer
bound counts bytes after the tar's end-of-archive blocks.

## C-043 The inputs the provider restates are kept, in the database (owner, 2026-11-16)
Owner's ruling, 2026-11-16 ("do it before phase 1"): bulk raw inputs stay with Intrinio (option
chains, snapshots, tapes: tens of GB a day), but the small inputs the provider restates in place
are kept, because a past session's restated input cannot be fetched again as it was: the company
capture (filings, share reports, float, standardized and as-reported fundamentals, the
daily-metrics row), the macro capture and the carry capture (dividends, spot, rates). Uses: a
sensor computed again later on the inputs it was first computed from, consistent with the rest of
its day (the planned "compute just this sensor" route); restatements seen by comparing a later
fetch with the kept one; macro first prints. Engine D-306 is the worker's side.
- **Where: the database** (`core.input_capture`, migration 0016), by C-010's reasoning: one store,
  one backup (C-013, when backups start), and no storage key on any pod (C-016). Spaces would put
  a key in core's Secret and add a second store with its own retention and restore. The archives
  are larger than C-010's 256 KB line (the company capture about 4.2 MB gzipped), where file
  storage reads faster, but they are written once a day and read rarely. Size: measured on a
  cluster worker (2026-11-16) about 44 MB a day raw, 4.2 MB gzipped for the company capture, 0.8
  MB for the carry and 0.2 MB for the macro (27 KB packed on 2025-12-03); about 4.5 MB a day
  stored, about 5 GB for every session from 2022-05-27, beside about 150 MB a published day
  (C-024's exit test). Stored `STORAGE EXTERNAL`, as the tape caches: already compressed.
  Amends C-004 for the cluster: the company captures it names go to core, not to Spaces.
- **Key and versions.** A capture is (session, kind, identity), the identity being the SHA-256 of
  the capture's `manifest.json`, the digest the engine's receipts already bind. Written once: the
  same identity again is answered `new: false`; the same identity with other bytes is refused and
  the first kept; another identity for the same session and kind is kept beside the first, with
  `captured_at` (when the worker's capture wrote its manifest, as the worker states it; the
  provider's own retrieval clocks are in the manifest) and `stored_at`, never in its place. That is
  how a restatement becomes visible. A capture a worker could not keep is recorded with its reason
  (an absence), each time it is reported.
- **What core checks** (generic, `core_db::capture::check`): the archive is a gzip tar read to its
  end (CRC and length, as C-042), its first entry and only that is `manifest.json` and hashes to
  the identity, every entry is a file with a plain relative name, at most 64 MiB as sent (413
  above) and 512 MiB unpacked. Core never unpacks it to disk or reads its contents otherwise.
- **Who may write: a day block's lease**, for a session on or before the latest day its job has
  published (a base from its own build), so a day that was refused keeps nothing, and a day keeps
  what it read up to its own session (T−5's macro and carry with T's three).
- **Routes.** Worker API (lease token): `PUT /input-captures/{session}/{kind}/{identity}?captured_at=T`
  (the archive), `POST /input-captures/{session}/{kind}/absence {reason}`,
  `GET /input-captures/{session}` (every version and absence) and
  `GET /input-captures/{session}/{kind}/{identity}` (the archive as sent). Reader API with the
  `admin` scope, the same two reads under `/v1/input-captures/`: these are the provider's raw
  responses, not measurements, so a `read` key does not see them.
- **Deploying:** the core image (migration 0016 is applied by the Deployment's `migrate` init
  container) and the engine's worker image; no new secret, setting or service.

## C-046 A newest-first build runs each ordered pass once when its passes are scheduled whole (2026-11-16)
The question before the build (MILESTONES step 5, from a verifier): by C-037 each earlier day's
publication takes down every later day's history revisions and queues their passes again;
newest first over 402 sessions (2025-04-10 to 2026-11-13) that could be some 80,000 pass runs.
- **The mechanism as it is.** A publish of day D (its base or its day-local revision) supersedes
  every published history revision dated after D and queues again the pass of each such day that
  is DONE (`publish::rederive`); a pass running on such a day is stopped and queued again. A pass
  QUEUED or never run is left as it is: nothing of it is published. So a pass runs again only if
  it ran before an earlier day's publish. The claim (`core.claimable_job`, 0005 as replaced by
  0014) lets the pass of S run only when S's base is published, no earlier pass of the profile is
  unfinished and no unfinished day block stands at or before S. When every session's pass exists
  before the range's first publish, the earliest pass waits for the earliest day, which a
  newest-first build publishes last, and every later pass waits behind it: no pass runs before
  every earlier day of the range is published and its block past it, and each runs once, in date
  order, after the blocks. Scheduled after the blocks: the same.
- **Measured** with a pass worker that runs every claimable pass after every publish and every
  day done (a pass runs as early as the claim allows), the real calendar and the slice's bodies
  under each date; each wave of blocks is scheduled newest first, since the claim takes the
  earliest claimable block. Over the 402 sessions, blocks of 5, 8 nodes (run once): passes
  scheduled at the start **402 runs**, after the blocks **402**; with each wave's blocks (40
  sessions) **2,602**, every later pass running again after each earlier wave. With each block
  of 5 it would be 5·(1 + … + 80) + 402 = 16,602, day by day 402·403/2 = 81,003: quadratic only
  in that shape. The test (`tests/work.rs` `a_newest_first_build_runs_each_ordered_pass_once`,
  12 sessions to 2025-12-12 in blocks of 3): 12 at the start and 12 after the blocks on 2
  nodes, 3 + 6 + 9 + 12 = 30 with each block's passes.
- **Rule (operating):** a build schedules its ordered passes once, for its whole range, before
  its first day block publishes (or after its last), never with each tranche of blocks
  (`deploy/k8s/README.md`). The code then runs each pass once; nothing in it changes. Adding an
  earlier range later (phase 1b after the two-week sample, the deeper history after phase 1),
  its passes scheduled whole the same way, runs each later pass once more, which is right: their
  windows gain the earlier days. The mistake
  shows at once: each publish posts "the ordered pass derives N days again" (C-037).
- **Not enforced, and why:** core cannot tell a range scheduled whole from one scheduled in
  pieces; a refusal to schedule passes before passes that have run would also refuse the two
  legitimate cases above, so it would need an override, and the notice already names the error
  on the first publish.

## C-049 A failed attempt leaves its evidence in core before the worker exits (owner-approved process addition, 2026-11-16)
Twice on 2026-11-16 a refused day's decisive error was lost when its pod exited: it was in the
day's fill log, which no report carried (`fail` sends the last 64 KiB of the day run's logs).
Engine D-351 is the worker's side.
- **Where: the database** (`core.job_evidence`, migration 0017), as C-043's captures: one store, no
  new secret or service; the document is stored as sent (`STORAGE EXTERNAL`, already gzipped).
  One record per lease, that is per attempt: job, attempt (the lease's place among the job's
  leases: it keeps counting after `retry-failed`, which sets `core.job.attempts` back to 0), kind,
  the session the attempt stopped on (NULL before its first), the job's sessions then, build,
  worker, stage, stop message, the input plan's refusing entries (jsonb) and the document. Written
  once: the same lease again is answered `new: false` and the first kept. The lease's own
  `failed_stage`, `failure` and `failure_log` stay as they are.
- **The document:** a gzip whose text is one JSON line (`VYREON-JOB-EVIDENCE@1`: stage, message,
  session, refusals) and the log excerpt after it (D-351: the last 4 MiB of the fill log and 1 MiB
  of each other log, and every earlier line naming an error, whole). At most 16 MiB as sent (413
  above), 64 MiB unpacked, read to its end (CRC and length, as C-042).
- **Secrets.** The worker redacts before sending (it alone can read the literal secrets: the
  Intrinio key, the workers' token, the lease token, a connection string). Core then refuses
  (422, nothing stored) a document in which a credential pattern survives
  (`core_db::evidence::unredacted`, the same list the engine redacts: `api_key=`, `apikey=`,
  `token=`, `password=`, `x-amz-signature=`, `x-amz-credential=`, `signature=`, `secret=` without
  regard to case, bearer tokens, a password in a URL, a Discord webhook's token), and the worker
  sends the account alone. A gap in the worker's redaction is then a refusal, never a stored
  secret; a key in a form no pattern names is caught only by the worker's literal match.
- **Who may write: any lease the token names, live or ended**, for a session of its job: a worker
  whose lease was lost or cancelled still says why it stopped. The token is the same secret as for
  every lease call (C-016). Route: `PUT /worker/v1/evidence` (body: the document).
- **Reading:** `vyreon-core evidence list [<job>]` (newest first, one line each) and
  `vyreon-core evidence show <job> [--attempt N]` (the latest attempt by default: the record,
  the message, the refusals and the excerpt, checked against its stored digest). No reader-API
  route: it is for the operator.
- **Ordinary day:** nothing changes; nothing is written unless an attempt fails, is refused or
  loses its lease.
- **Deploying:** the core image (migration 0017, applied by the `migrate` init container) and the
  engine's worker image; no new secret, setting or service.

- **Amended 2026-11-20 (audit 2026-11-20-pass1-round-cc, core.md, triage c.md, 11): no credential pattern is kept from a failure.** The
  evidence endpoint refused a document carrying an unredacted credential, but a lease's `fail`
  stored its stage, message and log as sent, logged the message in the requeue and pause notices,
  and `pause` kept any reason as given; `vyreon-core log` and `jobs` print them. Now each passes
  through `work::scrubbed` (`evidence::unredacted`'s patterns): withheld whole, "(not kept: it
  carried a credential pattern, an unredacted api_key=)", never cut around. The worker's own
  redaction stays the first line. Ordinary day: no change. Test:
  `core-db/tests/work.rs::a_failure_message_with_a_credential_is_never_kept`.

## C-050 The owner's channel carries problems, a daily verdict and a weekly check (owner-approved design, 2026-11-17)
The owner: "I want to open it and immediately know if i gotta go look at it or its running ok." The
droplet's alerts failed at that: normal states raised as alarms ("provider not published yet"), 64
tickers per alert, the same stop re-alerted every 12 hours for days, "all well" while nothing had
published for a week. Migration 0018; `crates/core-db/src/watch.rs`.
- **Problems, not events.** `core.problem` holds one row per problem: kind, a stable key, opened,
  a one-line summary in plain words, the command that shows more, resolved. A partial unique index
  keeps a (kind, key) from being open twice. The service's watch runs once a minute and finds
  every problem from core's own rows (state, not events), so a restart, a missed minute or a second
  service never alerts twice: an alert post when a problem opens, one "Resolved" post when it
  clears, nothing in between. The problem, its post and its resolution are one transaction (C-027's
  outbox). Every other notice is now only logged (`core.notice.post` false; `vyreon-core log`):
  blocks done, every scheduled job done, builds admitted or withdrawn, the queue paused or resumed,
  a day set aside, a republish's re-derivation. Notices written before 0018 stay as the log.
- **The kinds (v1), from what core holds:**
  (a) `SESSION_LATE`: each session from `VYREON_CORE_DEADLINES_FROM` must have its profile's base
  revision PUBLISHED by 10:00 America/Edmonton on the next calendar day (Friday's is due Saturday;
  a session the calendar lacks has no deadline). Unset, no deadline is checked and the digest says
  so: the backfill's historical sessions never alert. `REPORT_AT_RISK`: the week's last session
  (Friday, or Thursday before a Friday holiday) still missing at Saturday 12:00, raised only before
  that week's Sunday is over, open until it publishes.
  (b) `QUEUE_PAUSED`, with `core.queue_control`'s reason (keyed by when it paused).
  (c) `JOB_FAILED`: a job out of attempts, with its latest evidence's stop message (C-049; else its
  lease's failure) and `vyreon-core evidence show <job>`; it clears when retried or cancelled.
  (d) `PROVIDER_AUTH`: an evidence record of the last 7 days whose stop message or refusals carry
  the engine's provider transport error `HttpStatus(401)` or `HttpStatus(403)` (h2-engine
  `transport.rs`, the client every Intrinio request goes through) with nothing published or done
  since. `PROVIDER_ERRORS`: provider errors (`HttpStatus(429)` or 5xx) on **more than 2 attempts in
  30 minutes** (several at once look like the provider, not a day; the same reasoning as C-035's
  3); it clears after 2 hours without one. Only the decisive text is read, never the log excerpt,
  where a good attempt's retried requests also show. A 401 in a form other than the engine's
  transport error is not recognised.
  (e) `DATABASE_FULL`: `pg_database_size` above 80% of `VYREON_CORE_VOLUME_GIB` (clears below 78%).
  It is the database's size, not the volume's use (WAL and other files are on it too).
  Also: `CALENDAR_END` (the calendar within 45 days of its end, C-014f; was a weekly notice) and
  `NOT_DERIVED` (a republish left days no job derives again, C-037; opened by the publish, open
  until each day has its job).
  Not alerts: a session not yet published before its deadline, an attempt retried that later
  succeeds, a provider absence stated by the engine or a worker, a build admitted, a block done.
- **The digest, 09:00 America/Edmonton, always sent** (the heartbeat; once a day by its key). First
  line the verdict: `🟢 Running fine` (nothing open, nothing handled), `🟡 Running; handled: …`
  (provider gaps recorded or attempts retried in the last 24 hours, nothing open), `🔴 Needs you:
  <the oldest open problem>`. Then: the latest session stored, the share of its measurements with a
  value and the typical share (median of the 20 published sessions before it), its engine faults;
  sessions published in the last 24 hours; provider gaps recorded in the last 24 hours (tape and
  input-capture absences, and slots withheld whole for the provider, as sessions, never tickers);
  the work queue by kind; during a backfill, sessions done of total and an estimate from the last
  24 hours' rate; the database's size; "Open problems (n):" one line each with its age and command.
  Cut to 1,900 characters (Discord's limit is 2,000): the problems that do not fit are counted, with
  `vyreon-core problems`.
- **What "has a value" means, without science:** the engine's own whole-slot convention (R7.1,
  `exports::hold_final`): a slot withheld whole has the result id `<measurement>/<object>/<date>/
  engine-fault`; it is an engine fault when its body states `ENGINE_FAULT` or
  `PARENT_ENGINE_FAULT`, else a provider absence. Counted once per published revision
  (`core.revision_tally`). A derived record withheld for a dependency is not withheld whole by that
  convention, so it counts as a slot with a value; its fault is still counted.
- **The weekly check, Saturday 09:00:** the week's sessions, each published or not with its share
  and engine faults, and a verdict for Sunday's report (sessions before the first deadline are not
  judged). The week's unusual-reading flag count is left out: the flag line is calibrated by the
  engine's science (D-069), and core does not compute it. Reporter's dry run is a placeholder line.
- **v2 (seam in `watch.rs`):** spend against the budget (needs a DigitalOcean billing-read token)
  and idle worker nodes (needs Kubernetes read access): each is one more source of problems.
- **Deploying:** migration 0018; `deploy/k8s/20-core.yaml` mounts Secret `vyreon-core-webhook`
  (key `webhook`) read-only and sets `VYREON_CORE_WEBHOOK_FILE`; absent, the posts wait unsent. The
  watch runs either way. The channel starts clean by itself: every notice written before 0018 is
  log, never posted. Posts queued while the webhook was off (or any backlog) are cleared before it
  is switched on with `vyreon-core notices mark-sent`, or in SQL:
  `UPDATE core.notice SET sent_at = now(), last_error = 'marked sent without posting (C-050)' WHERE post AND sent_at IS NULL;`
  Times are the owner's (Mountain Time), computed by PostgreSQL's zone rules.
- **Amended 2026-11-17 (audit round v, coreops 2, 4, 6, 7, 8; migration 0021).**
  - *The watch and the sender are separate tasks* (`notices::spawn_watch`, once a minute;
    `notices::spawn_sender`, every 15 s, only with a webhook). They shared one loop, so a backlog
    of posts (20 a pass, up to 20 s each) delayed the watch, which also queues the daily runs
    (C-051), by tens of minutes.
  - *Delivery is claimed (0021, `core.notice.sending_since`).* The sender claims the oldest
    unsent post under a row lock before posting it (`notice::claim_next`); a post another sender
    holds stops this one, so two services never post the same notice and none overtakes another;
    `sent` is recorded under the claim. A claim lapses after `CLAIM_SECONDS` (120 s, longer than a
    post's 20 s timeout plus a rate limit's 30 s wait), and the post is sent again. **Residual
    (at least once):** a process stopped between Discord accepting a post and core recording it
    posts it once more after 120 s; the window is that one round trip. Discord's Execute Webhook
    (`POST /webhooks/{id}/{token}`, https://discord.com/developers/docs/resources/webhook#execute-webhook)
    lists no idempotency key; the `nonce` with `enforce_nonce` of Create Message
    (https://discord.com/developers/docs/resources/message#create-message) needs a bot token,
    which core does not hold (read from the documentation as known; not re-read online this round).
    The earlier claim "a second service never alerts twice" held for opening problems, not for
    delivery; it now holds for both, but for that window.
  - *`NOT_DERIVED` resolves only when each day's revision of its kind (history, or day-local) is
    published again, or a QUEUED or LEASED job will derive it.* A job created after the problem
    counted whatever its state, so a cancelled repair job resolved it.
  - *The weekly check reads the open problems and the history pass.* Any open problem makes it
    🔴 ("Sunday's report is not cleared while a problem is open"); a session past its deadline
    with its base but without its history pass (a published derived revision, not day-local) makes
    it 🟡; 🟢 only with every watched session's base and history pass published or due. Each
    published session's line says whether its history pass is published. During the backfill
    (C-051: T's history sensors follow the backfill) the check is therefore 🟡, which is true.
  - *The digest and the weekly check are composed and queued in one transaction holding the
    problems' advisory lock*, which every problem opening (`open_one`, including a publish's
    `NOT_DERIVED`) now takes too: a post saying what is open is never queued while a problem is
    opening, so a green digest never follows a red alert of a problem still open. A publish that
    opens a problem waits for a digest being composed (a second at most), and the reverse.
  - Ordinary day: nothing posted changes but the weekly check's lines (" · history pass
    published") and its first line when a history pass is missing or a problem is open.
- **Amended 2026-11-17 (audit round w, 4, 6, 8, 9, 11, 12, 13, 16; C-053).** Round w found most
  defects in round v's own machinery; the fixes simplify where one core (C-053) allows.
  - *One sender, no claim lease (11; C-053).* Two senders cannot exist, so the row lock and the
    claim token are gone (`notice::next`, `sent`, `not_sent`, `unconfirmed`). `sending_since`
    (0021) now marks a send whose outcome is not known yet: a restart between a send and its
    record, or an answer lost after the request went, waits `UNCONFIRMED_SECONDS` (120 s) before
    the post goes again, never at once (11); a refused connection or an error status goes again at
    the next send.
  - *Delivered means Discord returned the saved message (6).* Posts are sent with `?wait=true`:
    Discord's Execute Webhook then "waits for server confirmation of message send before
    response, and returns the created message body", while with the default `false` "a message
    that is not saved does not return an error"
    (https://docs.discord.com/developers/resources/webhook#execute-webhook, read 2026-11-17). A
    2xx without a message (`id`) is an unknown outcome (above). **Residual (at least once):** a
    message Discord saved whose answer is lost is posted once more after 120 s; Execute Webhook
    takes no idempotency key.
  - *The digest is composed outside the problems' lock (4).* It counts measurement bodies (the
    tallies), which could hold the lock while a publish opening `NOT_DERIVED` waited behind it with
    the publish lock held. Now the post is composed with no lock; the lock is taken only to read
    the open problems again and queue the post if they are the ones it was composed from (by id);
    if a problem opened or resolved meanwhile, nothing is queued and the next minute composes it
    again. Round v's guarantee holds (no green digest after the alert of a problem still open).
  - *No post after a failed check (13).* The minute's digest and weekly check are composed only
    after that minute's check succeeded, never from a list the check could not bring up to date.
  - *The weekly headline agrees with its lines (12).* A session is complete only with its base and
    history pass published; one not complete before its deadline is "due later" (its line: "history
    pass not published yet, due by …"); any open problem is 🔴 whatever the deadlines, before the
    "no deadline yet" and "no session" lines.
  - *`NOT_DERIVED` resolves only when each of its days has a revision of its kind published again
    after it opened (8, 9).* A queued or leased job no longer resolves it: a block may be past the
    day already (it resumes at its next session), and a repair job cancelled after the problem
    resolved left it hidden. The problem now reads "open until each day is derived again".
  - *16 (an older tick resolving `ARCHIVE_MISSING`) needs two services' ticks: answered by C-053;*
    one service runs one watch tick at a time.
  - Ordinary day: posts are the same; the weekly first line on Saturday with Friday's history pass
    still due reads "🟢 … on time; 4 of 5 sessions published with their history pass, 1 due later".

- **Amended 2026-11-17 (audit round x, 11): a failed queueing is a problem before the digest.**
  The tick queues the daily runs (C-051) before it checks, and a queueing error (the store's
  partitions or a job insert refused, the build admitted) was only returned after the minute's
  digest, which could say 🟢 while the due session had no jobs. Now the check finds it from
  core's rows: a session due (`daily::due`) and not recorded as queued is `DAILY_STOPPED` ("Daily
  runs stopped: a session due is not queued (the oldest …): queueing failed, the service's log says
  why"), the same kind and key as a build that cannot work (whose summary is kept when both hold),
  so the digest composed after it is 🔴; it resolves once the session is queued. A failure that
  clears the next minute is one alert and one "Resolved". No new machinery: the state was already
  in `core.daily_session`. Ordinary day: no change (queueing succeeds, nothing is due unqueued at
  the check). Test: `core-db/tests/daily.rs::a_failed_queueing_is_a_problem_before_the_digest`.
- **Amended (audit round x 2, 7, 12, 19, 2026-11-18).** Each the simplest fix; no new machinery.
  - *A cancelled repair pass opens `NOT_DERIVED` (2).* A republish queues the finished pass of a
    day whose history fell, so no problem opens; cancelling that pass left the history silently
    absent. `work::cancel` now opens `NOT_DERIVED` ("Cancelling their ordered pass left N days …")
    in its transaction for each ordered pass it cancels whose day has a published base, no history
    revision of the profile published and no other pass to come; it resolves as before. Not
    reopened: a problem round v resolved falsely before migration 0023 (none known on the cluster).
  - *The digest's tallies are one statement (7):* the published revisions, their kept counts and
    the ones counted for the first time come from one snapshot (the new counts are kept by the same
    statement), so a revision replaced between two reads can no longer make a "no measurements,
    0 engine faults" line.
  - *A 5xx is an unknown outcome (12).* A gateway's 502/503/504 can come after Discord saved the
    post (RFC 9110, 15.6), so it keeps `sending_since` and waits the 120 s window like a lost
    answer; only a 4xx (a definite refusal, 429 included) or a refused connection goes again at once.
  - *One `wait=true` (19).* Any `wait` parameter in the configured webhook URL is removed before
    `wait=true` is added (`notices::waiting_url`); the URL is never printed.
  - Ordinary day: unchanged. Tests: `core-db/tests/publish.rs::a_cancelled_repair_pass_leaves_its_day_a_problem`,
    `core-db/tests/watch.rs::the_digest_leads_with_its_verdict` (tallies), `core-api/tests/notices.rs::a_gateway_timeout_waits_the_unknown_outcome_window`,
    `core-api` unit `the_webhook_url_carries_one_wait_true`.

- **Amended 2026-11-20 (the live digest of 10:00 MT, 2026-11-20: a false 🔴 and stale lines).**
  - *An open problem says what holds now.* Its summary (and command) was written once, when it
    opened, so the digest showed "did not run on 1 published day (1 crashed), the oldest
    2026-11-03" 25 h after that day was published again and checked, and "1982 comparisons
    failed" after a later day became the disagreement. Each minute's check now rewrites an open
    problem's summary and command when what it finds differs (`Change::restated`), posting
    nothing (one alert, one "Resolved", nothing in between, as before); it still resolves when
    its condition clears, and the "Resolved" post carries the latest text. "Open N h" stays the
    age of the problem (kind and key) continuously open, which may be older than the day it now
    names.
  - *"(and 1 more problem)".* The verdict's "(and 1 more)" after a summary naming days read as a
    further day; it counts problems and now says so (the weekly check's first line too).
  - *The backfill's estimate is measured on the running blocks' own days.* It was the remaining
    sessions times 24 h over the sessions published in the last 24 hours, so one day published by
    a block that had run six hours (job 1420, 2026-10-28) gave 48 h: "Sun 09:26". Now
    (`watch::pace`): a day after a block's first takes from the previous publication under the
    block's live lease to its own (the whole cycle: compute, reference check, the next download
    overlapping); a block's first day, from the lease to its publication, includes the block's
    download and is used only when no later day has published (stated: "a block's first day, with
    its download"); the median of those. Each running block finishes its remaining sessions after
    its last publication, never before now plus the sessions after the one computing; with
    sessions waiting in blocks not yet claimed, the whole remainder at the running blocks'
    combined pace ("with N blocks running"). Nothing published yet under a running lease:
    "finish unknown until a running block publishes a day"; none running: "no block running, so
    no estimate". On the recorded times: job 1420's first day took 6.18 h, so 2026-10-30 about
    Fri 20:41 MT; job 1383's second day took 4.43 h, which put 2026-11-04 at 08:47Z, 28 minutes
    before it published (09:15Z).
  - *🟡 also names what is not fully judged* (C-054 amended 2026-11-20): "🟡 Running; handled: …;
    not fully judged: reference checks of N published days".
  - Ordinary day: the backfill line's wording ("about 4.4 h a day · expected to finish about …");
    "(and N more problems)"; an open problem's text follows its counts. Tests:
    `core-db/tests/refcheck.rs::an_open_problem_says_what_holds_now_and_resolves_when_it_no_longer_holds`
    (the cluster's two problems as the earlier core stored them),
    `core-db/tests/watch.rs::a_last_failure_and_a_provider_burst_are_problems_from_the_evidence`,
    `watch::tests::the_backfill_finish_is_measured_on_the_running_blocks_days`,
    `watch::tests::the_verdict_says_first_whether_to_look`.

## C-051 Daily runs: core queues each session the morning after; a CronJob starts the worker; the pool scales to zero (owner-approved goal, 2026-11-17)
Goal (owner): from the day the system is declared ready, each new session is computed and
published with nobody starting anything; nodes exist only while there is work. Migration 0019;
`crates/core-db/src/daily.rs`; `deploy/k8s/40-daily.yaml`; runbook in `deploy/k8s/README.md`.
- **When a session's inputs are ready: 07:30 New York on the calendar day after T.** Evidence:
  (1) the closing option snapshot (engine `close_quotes::admit`, D-072: taken 15 to 2 minutes
  before the close) was not listed at 21:00 ET on T: the droplet's 02:00 UTC captures of
  2026-11-10, 11-11 and 11-12 (sessions 11-09 to 11-11) each got T−1's snapshot ("snapshot at <T−1> 21:05Z, not 15 to 2
  minutes before the close"), which is why engine e8aad4e0 left the newest session to the morning
  run; (2) the next 12:00 UTC (07:00 EST) runs, 2026-11-10 to 11-12, captured every input of those
  sessions ("still missing: nothing"; from 11-13 the evening run no longer takes T, and the
  morning run's only remainder is the daily bar's re-capture, planned before its first capture in
  the same run); (3) Intrinio: "OPRA finalizes open interest after 12:00 AM, EST.
  Intrinio pushes that out starting at 4:00 AM EST" (FAQ, engine D-026); EOD stock prices around
  17:00 ET, adjusted around 19:00, "if all of the prices are not present at 7pm ... again around
  9pm" (FAQ); (4) the engine takes T's daily bars as settled only when captured on a UTC day after
  T (`acquire::daily_settled`, B5), so nothing before 00:00 UTC on T+1; (5) replay files close at
  23:31 UTC (2026-11-04 reading). Last observed-ready point 07:00 ET; 30 minutes' margin. Only
  winter mornings were observed: summer's 07:30 EDT is one hour earlier in UTC than any observed
  run (to watch on the first summer mornings; the setting to move is `daily::READY_AT`).
  The cohort prints (EXQ01) are kept by the provider for five sessions: the morning after is
  inside that.
- **What is queued:** for each session from `VYREON_CORE_DAILY_FROM`, for
  `VYREON_CORE_DAILY_BUILD` (both or neither), one TAPE, one DAY_BLOCK and one ORDERED_PASS, not
  only the day block: without its tape job T's tape cache never reaches core and every later day
  would download T's tapes again (a day fetches 252 sessions' caches, D-310); without the pass the
  history-reading sensors (the paid issue's unusual readings) are never computed. A kind already
  covered by a job not cancelled is skipped; a session already published gets nothing;
  `core.daily_session` records the session in the same transaction, so it is never queued twice
  (a cancelled daily job stays cancelled). The service does it once a minute in the watch's tick;
  `vyreon-core daily [queue]` shows (or does) it by hand. A build not registered, not admitted or
  withdrawn queues nothing and is the problem `DAILY_STOPPED` until it can.
- **Daily jobs first (`core.job.priority`, 1 for a daily run):** the claim took the earliest
  session, so during the backfill a daily session would wait behind every older block, past its
  perishable prints' five sessions. The claim now orders by priority, then session. Two waits
  remain by the existing rules and are not changed here: a day block waits for every unfinished
  tape job up to its session (C-025: so the backfill's current tranche of tapes), and the ordered
  pass of T waits for every earlier pass and block (C-039: history order), so during the backfill
  T's base publishes on time and its history-reading sensors follow the backfill.
- **Deadline (C-050) consistent with readiness:** `VYREON_CORE_DEADLINE_HOUR`, default **20:00 MT**
  the next calendar day (was a fixed 10:00, which falls before a day queued at 05:30 MT could
  finish: one day measured 5 h 50 min from an empty disk on the cluster). 13 h 30 min (New
  York's winter, Alberta on UTC−6) to 14 h 30 min after the session is queued: the tape job and
  two attempts fit before a late session is alerted (C-050: a retried attempt that succeeds is not
  an alert). Sunday's report at risk moves with it (Saturday, deadline + 2 h, 22:00 MT). The
  deadlines start at `VYREON_CORE_DAILY_FROM` unless `VYREON_CORE_DEADLINES_FROM` is set. The
  Saturday 09:00 check now counts a session whose deadline is still ahead (Friday's) as due, not
  missing: it read every Friday as missing.
- **The cluster:** CronJob `daily`, 07:45 New York, Tuesday to Saturday, `concurrencyPolicy:
  Forbid`, Indexed, `completions: 3`, `parallelism: 1`, `backoffLimitPerIndex: 1`, 23 h
  `activeDeadlineSeconds`. One pod runs tape, day block and ordered pass in order (two init
  containers and the main one, each its own empty home and only the keys it reads). Parallelism
  cannot follow the queue without KEDA (README, "After this run"); a run takes up to three queued
  sessions one after another, and a longer catch-up is the existing Jobs by hand. Every worker pod
  (the CronJob's and the three Jobs of `30-workers.yaml`) carries
  `cluster-autoscaler.kubernetes.io/safe-to-evict: "false"`.
- **The pool:** `day` autoscaled with min 0 (the owner applies it; README). DigitalOcean's
  autoscaler page supports zero; its doctl `node-pool create` reference says it does not; the
  API's answer settles it. If 0 is refused, the smallest alternative is min 1 (one idle node,
  about $120 a month) or the pool created and deleted around each run as today.
- **Ordinary day:** about one worker node for 6 to 7 hours a session (about $1.10; the owner's
  spend).
- **Amended 2026-11-17 (audit round v, coreops 9): a worker node only when there is work.** The
  CronJob ran its worker pods on schedule whatever core held, so every empty morning (the Saturday
  after Good Friday; a session done by hand) brought up a worker node. Now CronJob `daily` (07:45
  New York, Tuesday to Saturday) runs a gate on the core node (the core image, no worker
  toleration): `vyreon-core daily start-worker` counts the daily jobs waiting
  (`daily::waiting`: priority 1, QUEUED, build admitted and not withdrawn, queue not paused) and,
  only when there is one and no Job labelled `work=daily` is unfinished, creates the worker Job
  from the template of the suspended CronJob `daily-worker` (as `kubectl create job --from`
  does: the template's spec, labels and annotations, `cronjob.kubernetes.io/instantiate: manual`;
  `ttlSecondsAfterFinished` a week, since a suspended CronJob's history limits do not apply).
  RBAC (`40-daily.yaml`, README "Daily runs"): ServiceAccount, Role and RoleBinding `daily-gate`
  in `vyreon` only: `get` on CronJob `daily-worker`, `list` and `create` on Jobs. The gate reads
  core's database with core's login. Ordinary day: the same worker run, started by the gate's
  pod (seconds on the core node) instead of directly; an empty morning starts nothing. Not
  verified: the Kubernetes API itself (the tests run the gate against a local stand-in) and the
  Role on the cluster; the first morning is the check (`kubectl -n vyreon logs job/<daily-…>`).
- **Amended 2026-11-17 (the owner; audit round w, 2, 5, 10, 14, 15): the gate is retired.** Round
  w found five defects in it (a Role able to create any Job in the namespace and the database
  owner's password for a count; no pull secret on its account; yesterday's finishing Job taking
  today's only start; a count that disagreed with what a worker can claim; two gates able to start
  two Jobs). The owner's decision: CronJob `daily` starts the worker Job directly again, as before
  round v (`40-daily.yaml`: the worker image, the `default` service account and, named in the pod,
  the `regcred` pull secret as the other worker Jobs; `safe-to-evict: "false"`; `completions: 3`,
  `activeDeadlineSeconds` 23 h; `concurrencyPolicy: Forbid`). `vyreon-core daily start-worker`,
  `core_api::gate`, `daily::waiting`, the ServiceAccount, Role and RoleBinding `daily-gate` and
  the suspended CronJob `daily-worker` are removed. **Accepted cost:** on a weekday morning with
  nothing queued (the morning after an exchange holiday) a node starts, the three containers find
  nothing to claim and exit in about a minute, and the autoscaler removes the node: at most about
  $0.17 a time, about nine times a year. What remains of each finding: 2, 5, 14 have no subject; 10
  (a run still going at 07:45): `Forbid` skips that start and counts it missed, and "a new Job can
  be created once the previous Job completes" within `startingDeadlineSeconds` (6 h)
  (https://kubernetes.io/docs/concepts/workloads/controllers/cron-jobs/, read 2026-11-17), so the
  session is started when the earlier run ends, not the next morning; 15: `Forbid` keeps
  scheduled runs from overlapping; Kubernetes states a CronJob may on rare occasions create two
  Jobs ("Jobs that you define should be idempotent", same page), and the worker is (core's leases:
  the second pod finds nothing or the next job), at the cost of a second node for minutes; a Job
  made by hand (`kubectl create job --from=cronjob/daily`) is not held back by `Forbid` (README).

- **Amended 2026-11-17 (audit round x, 6, 13): the daily run takes only daily work, and waits a
  little for it.** (13) During the backfill a daily pod with no daily job left claimed the
  backfill's earliest work of its kind (a multi-day block), held a node for hours and could be
  killed at its 23 h deadline, using an attempt. Now each container of CronJob `daily` sets
  `VYREON_WORKER_DAILY` and the worker claims with `daily: true` (`POST /worker/v1/claims`,
  `work::claim_as`): only a job of priority 1. The backfill's workers claim as before (daily jobs
  first, then any). (6) A watch delayed past 07:45 (core restarting, a stalled tick) let all three
  pods find nothing and succeed; the session queued a minute later waited for the next scheduled
  start (Saturday's: Tuesday). Now the run's first pod's tape container (`JOB_COMPLETION_INDEX`
  0), finding nothing (or core unreachable), asks again every minute for up to 30 minutes
  (`VYREON_WORKER_DAILY: "30"`); the queue inserts a session's three kinds in one transaction, so
  the day block and the pass follow it at once. The other pods and containers ask once (`"0"`),
  so an ordinary morning costs nothing more. **Cost:** a morning with nothing queued (after an
  exchange holiday) now holds the node about 30 minutes, about $0.25 instead of $0.17, about nine
  times a year. A queue later than 08:15 New York still waits for the next start (C-050's
  deadline alerts it). Engine side: D-360 amended (round x). Ordinary day: the same run; its
  claims carry `daily: true`. Tests: `core-db/tests/daily.rs`,
  `work_queued_or_published_by_hand_is_not_queued_again` (a daily-only claim takes the daily tape and then nothing, while the backfill's
  tapes remain), engine `core_client::tests::a_daily_worker_waits_a_bounded_time_for_its_job`.

- **Amended 2026-11-20:** a daily session's day block is counted in the digest's backfill line
  with the others, and its estimate is now the measured pace of C-050 (amended 2026-11-20), not
  the last 24 hours' count, so a lone daily block reads "finish unknown until a running block
  publishes a day" until its first day publishes.
## C-052 The cluster archives the perishable inputs to Spaces through upload URLs core signs (owner-approved design, 2026-11-17)
The owner (2026-11-17): workers stay stateless and hold no storage credential; core alone holds the
Spaces key and hands a worker a pre-signed upload URL for one named object, valid minutes; the
worker uploads straight to the bucket and reports it; core records what is kept per session. So
the droplet's nightly archive (`deploy/droplet/archive-perishables.sh`) can retire. Engine D-360
is the worker's side. Migration 0020; `crates/core-db/src/archive.rs`.
- **What:** per published day and category, the directory the day read, packed by the worker as
  one tar.zst (`zstd -10`, the droplet's level; entries `<T>/<path>` as the droplet lays them out,
  byte order, mode 0644, mtime 0, so the same files pack to the same bytes). Categories (C-004):
  `exq01-events` (the cohort prints, kept by the provider about five sessions) and `act01-auto`
  (the ACT01 interval pulls). **Not the company capture**, the droplet's third: C-043 already keeps
  it in core's database, whole (D-306 packs the same directory); a second copy in Spaces is the
  second store C-043 declined. Adding it is one line in each side's category list.
- **Key and versions:** `perishables/<category>/<session>/<sha256>.tar.zst` in the bucket
  (`droplet-capture/` stays the droplet's). Content-addressed: the same archive again is answered
  `kept`, with no URL; another archive of the same session and category is another object beside
  the first, never in its place; a KEPT object is never issued a URL again, so nothing kept is
  ever overwritten. The droplet's freeze: once a version is KEPT and the session is more than 14
  calendar days old (`FREEZE_DAYS`), another version is answered `frozen` (nothing sent): a late
  re-run, after the provider has dropped the prints, never adds a shrunken copy. Nothing is ever
  deleted by core.
- **The URL:** AWS Signature Version 4 in the query string (path-style `<endpoint>/<bucket>/<key>`),
  which Spaces accepts (https://docs.digitalocean.com/reference/api/spaces/, "Authentication"; the
  signing is AWS's, https://docs.aws.amazon.com/AmazonS3/latest/API/sigv4-query-string-auth.html),
  valid **600 s** from the database's clock, signing `Content-Length`, `Content-MD5` and
  `x-amz-meta-sha256`: the store refuses a body of another length or MD5 (S3's `BadDigest`), any
  other header value breaks the signature, and the SHA-256 travels with the object. **Signer:**
  the maintained crate `rusty-s3` 0.11.0 (sans-IO, made for pre-signed URLs on S3-compatible
  stores; default features off, `rustcrypto` on: it adds only `jiff`, `url`, `percent-encoding`,
  `zeroize` and the RustCrypto `hmac`/`sha2` already in the lock), not a signer of our own (the
  coordinator's ruling, 2026-11-17: proven infrastructure first; a hand-written signer was replaced
  before merge). Its action headers are signed, so the three headers above are in
  `X-Amz-SignedHeaders`; `UrlStyle::Path`. Its output reproduces AWS's published pre-signed
  example exactly (the unit test pins its signature, aeeed9bb…); `aws-sigv4` was the fallback.
- **Kept means read back:** on `done` core signs a HEAD (60 s) and keeps the object only if its
  size, its ETag (the MD5 of a single-part upload) and its `x-amz-meta-sha256` are those
  requested; otherwise FAILED with what it found. A worker's own failure is recorded with its
  reason. Core cannot reach the bucket: 503, the worker sends `done` again.
- **Secrets:** the key is read from two files (Secret `vyreon-core-spaces`, mounted read-only),
  never printed (`Spaces`' `Debug` hides it). The URL is a credential: never stored (only when it
  expires), never in a log or an error (core's HEAD errors are taken without their URL); a reason
  the worker reports is kept only if no credential pattern of C-049's list is in it (else
  "(the reason is not kept: …)"); the worker redacts it first (D-360).
- **Who may ask:** a day block's live lease, for a session its job has published (base, own
  build); `done` only for an archive its own job requested.
- **Routes:** `POST /worker/v1/archives {category, session, bytes, sha256, md5}` → `{state: off |
  kept | frozen | upload, archive_id, key, url, headers, expires_seconds}`;
  `POST /worker/v1/archives/{id}/done {failed: null | why}` → `{kept, why}`. CLI:
  `vyreon-core archive list [<date>]` (each object: state, size, key, job, worker; then what is
  missing).
- **Missing (C-050's digest):** a (session, category) with nothing KEPT that was requested, or, from
  `VYREON_CORE_ARCHIVE_FROM` (default the daily runs' first session), of a published day, an hour
  or more after the request or the publication. The digest always carries `Perishable archive: N
  missing (the oldest …)` while the archive is on; `ARCHIVE_MISSING` is a problem only past
  `VYREON_CORE_ARCHIVE_ALERT_DAYS` (unset: never). Backfilled days before the archive's first
  session are not expected (their prints were gone before they were computed).
- **Settings, all five or none (off by default):** `VYREON_CORE_SPACES_ENDPOINT`
  (`https://nyc3.digitaloceanspaces.com`), `_REGION` (`nyc3`), `_BUCKET` (`vyreon-data`),
  `_KEY_ID_FILE`, `_SECRET_FILE`. Off, or the Secret absent: every request is answered `off` and
  the worker sends nothing.
- **Ordinary day:** no published value changes. After `day_done` the worker packs and uploads two
  archives (about 170 MB of prints raw a day, some tens of MB packed; the ACT01 pulls are small),
  within a 15-minute deadline that never touches the day's outcome; finish.log gains two lines;
  core gains two rows and two signed HEADs.
- **Deploying (the owner, `deploy/k8s/README.md` "Perishable archive"):** a Spaces key limited to
  `vyreon-data`, the Secret, the core image (migration 0020) with the settings commented out, the
  worker image (D-360, with `zstd`), then the settings.
- **Not verified:** Spaces itself (no network here): a path-style pre-signed PUT signing those
  three headers, its `BadDigest` on a wrong `Content-MD5`, and its ETag as a single-part upload's
  MD5. The tests run the whole exchange against a simulated S3 endpoint that recomputes the
  signature with the bucket's key, checks the signed headers, the expiry and `Content-MD5`
  (`core-api/tests/archive_api.rs`), with the engine's packing of the real ACT01 pulls of
  2025-12-19 (`perishable-act01-auto-2025-12-19.tar.zst`). The first real day is the check.
- **Amended 2026-11-17 (audit round v, coreops 3 and 5; migration 0022).**
  - *An archive is read back only where its URL was signed for.* A row not kept, asked for again
    after the settings moved to another bucket, kept its old bucket while the URL and the HEAD
    used the new one, so it could be marked KEPT against a place never checked. Now the row holds
    the endpoint beside the bucket (0022); a request sent again moves a row not kept to the place
    its new URL is signed for; a `done` for a row whose place is not what core signs for now is
    recorded FAILED ("signed for … and core now signs for …: not read back"), never verified
    elsewhere. `archive list` shows the endpoint. Rows from before 0022 have none and count as
    another place (none are expected: the archive was off on the cluster).
  - *A missing archive's age counts from the first request.* A request sent again set
    `requested_at` to now, so with `VYREON_CORE_ARCHIVE_FROM` unset each retry took the archive
    out of the missing list for the grace hour and reset its age: `ARCHIVE_MISSING` resolved, and
    repeated retries kept it under the alert age for ever. `requested_at` is now the first
    request's (the latest URL's time is `url_expires_at` less 600 s; `urls_issued` counts them).
  - Ordinary day: unchanged (one request per archive; core gains the endpoint on each row).
  - Engine side: D-360 amended the same day (packing is inside the archive's deadline: a stalled
    compressor is killed, the archive is not made, the day's outcome unchanged).
- **Amended 2026-11-17 (audit round w, 1, 7, 17, 18; C-053; migration 0023).**
  - *1 (a retry through a second core configured for another bucket moving a row between its
    HEAD and its record) needs two cores at once: answered by C-053.* One core's settings change
    only with a restart, which round v's place check covers.
  - *A category not packed is counted missing (7).* The worker reports it
    (`POST /worker/v1/archives/unpacked {category, session, why}` → `{recorded: true}`, or
    `{state: off}`; the same lease rule as a request: a day block's, for a day its job published);
    core keeps it in `core.perishable_unpacked` (0023), and `missing` counts the (session,
    category) from the report, whatever `VYREON_CORE_ARCHIVE_FROM` says. `archive list` prints
    them ("not packed at …"). Engine side: D-360 amended (round w).
  - *A row KEPT before 0022 has no endpoint and is not counted as kept (17).* Migration 0023 sets
    such rows FAILED ("kept before migration 0022, where it was read back is not recorded"), so they
    count missing and a request for them is given a URL again; a CHECK keeps any row from being
    KEPT without its endpoint. None are expected on the cluster (the archive was off).
  - *`archive list` prints the endpoint (18):* each object as `<endpoint>/<bucket>/<key>`.
  - Ordinary day: unchanged (nothing is reported unless a category is not packed).

- **Amended 2026-11-17 (audit round x, 4; engine D-360 amended).** A category with an entry the
  worker cannot read (a broken link, a closed directory) is archived with its readable files and
  the skipped entries said in its outcome, or, with none readable, reported not packed (so core
  counts it missing); each category is named to the worker's parent as soon as its own walk ends.
  Core is unchanged. Ordinary day: unchanged.
- **Amended (audit round x 8, 9, 17, 2026-11-18; migration 0024).**
  - *A day its job published is one the job has marked done (9).* Revisions do not record their
    job; `published_by` now takes a day before the job's next session (`day_done` needs the day's
    base of the job's build published, and the worker sends it before its archive phase), never
    the next itself, so a block just leased on a day published by hand or by another job neither
    archives it nor reports it not packed.
  - *One "not packed" record per occurrence (17).* 0024 keeps one row per (job, worker, session,
    category) (earlier duplicates deleted, the first kept) and the insert is `ON CONFLICT DO
    NOTHING`: a report sent again after its answer was lost is the same record. Engine unchanged.
  - *A "not packed" report beside a KEPT archive of the same job and worker is not listed (8),* in
    either order (the child stopped after its upload, before its outcome line); `missing` already
    counted the category as kept.
  - Ordinary day: unchanged. Tests: `core-db/tests/perishable_archive.rs::a_block_archives_only_days_it_has_published`,
    `::a_not_packed_report_is_one_record_and_none_beside_a_kept_archive`.

## C-053 One core at a time, enforced by the database (2026-11-17; audit round w)
Core runs as one replica (`deploy/k8s/20-core.yaml`, strategy `Recreate`), and its watch, notice
sender and archive checks were written for one; round w's findings 1, 11 (two senders), 15 (two
gates, since retired, C-051) and 16 needed two instances at once, and round v had added machinery
(a claim lease on each post) only to survive that. Now it is an invariant, not a deployment
setting: `vyreon-core serve` takes a session-level PostgreSQL advisory lock
(`pg_try_advisory_lock(hashtextextended('vyreon-core serve', 0))`, `core_db::instance`) on a
connection it keeps for its life, before it starts anything, and refuses to serve when another
session holds it ("another vyreon-core serve holds the database's single-instance lock"; Kubernetes
restarts it until the old pod is gone). PostgreSQL releases a session-level advisory lock when its
session ends (PostgreSQL 16 manual, "Advisory Locks"), so a core that dies frees it. The service
checks every 15 s that its connection still holds the lock (10 s to answer) and stops when it does
not, or when that connection ends, so a core that lost its database cannot serve beside a new one.
The command-line tools (`publish`, `queue`, `digest`, …) do not take it: they are the operator's,
and their writes are safe beside the service (row and transaction locks). Machinery removed: the
notice claim lease (C-050 amended). One more database connection is held. Ordinary day: no change.
Test: `core-db/tests/instance.rs` (a second session refused while the first holds it; free once
the first session ends).
- **Amended 2026-11-17 (audit round x, 5): a frozen core is restarted, a lost one is let go.** A
  process frozen with its connection open kept the lock, and every replacement refused to start;
  the readiness probe takes a Pod out of service but never restarts it. Now `/live` answers 503
  once the watch has not started a tick for `LIVE_WITHIN` (15 min; it ticks every minute) and
  reads no database; `20-core.yaml` has a liveness probe on it (every 60 s, 10 s to answer, 3
  failures), so a stalled watch or a runtime that no longer answers is restarted within about 18
  minutes, which closes its connection and frees the lock. A database that is down stays the
  readiness probe's (a restart would not help it). For a node lost behind a partition, the lock's
  session sets the server's `tcp_keepalives_idle` 30 s, `_interval` 10 s, `_count` 3 (PostgreSQL 16
  manual, "Connection Settings"; the system default waits about two hours), so the server ends it
  and frees the lock within about a minute. Ordinary day: no change. Tests:
  `core-api/tests/notices.rs::a_core_whose_watch_stalled_is_not_live`, `core-db/tests/instance.rs`
  (the keepalive set on the lock's session).
- **Amended (audit round x 1, 2026-11-18): an archive is marked only where it was read back.** A
  core whose lock session the server ended, unaware for up to 25 s, could read an archive back in
  its bucket while the new core moved the row to another, then mark that row KEPT.
  `archive::record` now updates the row only if its endpoint, bucket and key are still those it
  read back (`archive::target`); otherwise the outcome is refused ("asked again in another place
  since it was read") and the row stays as the newer request left it. No fencing token: the
  place is the evidence. Ordinary day: unchanged. Test:
  `core-db/tests/perishable_archive.rs::an_outcome_read_in_the_old_place_is_not_recorded_on_an_archive_moved_since`.

## C-054 Every published day's reference check is kept per lease and session; a disagreement or a check not run is a problem, never an unpublished day (owner decision, 2026-11-18; engine D-370)
The owner (2026-11-18): the reference suite (`refcheck`: QuantLib, TA-Lib) recomputes each day from
its raw inputs, which workers delete once the day publishes, so it runs on the worker between
publishing and pruning and the report goes to core; the added node-hours are accepted. Phase 1's
tranche gate (a) is "refcheck in the worker on every day" (MILESTONES, 19:31 UTC). Engine D-370 is the
worker's side. Migration 0025; `crates/core-db/src/refcheck.rs`.
- **The path:** the failure evidence's (C-049): one gzip document, its first line the account
  (`VYREON-REFCHECK@1`: outcome COMPLETED | TIMEOUT | CRASHED | NOT_RUN, the reason when it did not
  complete, wall, suite and CPU seconds, jobs, deadline, the counts summed over the report's
  measurements, the per-measurement summary, the suite's versions), the suite's report as JSON after
  it. The worker redacts; core refuses a document in which a credential pattern survives
  (`evidence::unredacted`), and the worker then sends the account alone. Not the evidence table
  itself: that is one per lease and for failures; a check is one per day and mostly a success.
- **Kept:** `core.refcheck_report`, one row per (lease, session) (`PUT /worker/v1/refcheck/<T>`,
  under the live lease of the day block whose job published T and marked it done:
  `archive::published_by`, the archive's rule), with the job, attempt, worker, build and the day's
  published base revision of that build at that moment (what was checked); the counts, times and
  summary as columns (queryable); the document as sent (compressed; a real day's report is about
  42 MB of JSON, 3.5 MB gzipped; at most 32 MiB sent, 256 MiB unpacked). The same lease and day again
  is answered `new: false` with the first's id. A report outlives its revision (`ON DELETE SET
  NULL`); it is "current" while that revision is still the day's published base.
- **Never the day's outcome:** a disagreement does not withhold or unpublish anything; the day stays
  as published, and the problem says go look (C-050).
- **Problems (C-050: one alert when it opens, one "Resolved", counts never lists):** judged on each
  published day's latest current report.
  - `REFCHECK_DISAGREES` (key `refcheck`): while any such report completed with a failed comparison.
    "Reference check disagrees with N published days: F comparisons failed, the oldest D." It
    resolves when every such day's latest report agrees, which takes a recomputation (a fix, then the
    affected days published again and checked); bands are not moved after the fact.
  - `REFCHECK_NOT_RUN` (key `refcheck`), distinct: a latest report that TIMEOUT, CRASHED, NOT_RUN or
    completed with reference errors (the suite's own failure on a measurement), and, from
    `VYREON_CORE_REFCHECK_FROM` (a session; unset: not judged), a published base revision with no
    report `GRACE_HOURS` (4) after it was published (the worker's 2 h deadline after the archive
    phase, with room). "Reference check did not run on N published days (1 timed out, 2 with no
    report), the oldest D."
  - Both point to `vyreon-core refcheck list`; `vyreon-core refcheck show <date>` gives the day's
    record, the measurements with a failure and each failure class with its example and the objects
    it names (the tickers stay behind the command); `--report` prints the report.
- **Digest:** "Reference checks in the last 24 hours: N (a agree, d disagree, n did not run) · typical
  W min, C CPU min per day" (medians), shown when any check came in or a report is expected; the
  owner's cost number per phase-1 day.
- **Not judged here:** the history-derived profile (the ordered pass's twelve measurements) is left
  out on the worker (D-370); its check needs the built store's histories (MILESTONES step 5).
- **Deploy:** migration 0025 with the next core image; set `VYREON_CORE_REFCHECK_FROM` to phase 1's
  first session when its tranche is scheduled (unset, only reports workers send are judged).
- Tests: `core-db/tests/refcheck.rs` (a real report, the B0 census's of 2025-12-16, as the document:
  checked, every malformed account refused, a credential in the report refused; on the real slice of
  2025-12-10 published by a block: refused before the day is done, kept once, bound to the base,
  listed and shown; the day with no report opens REFCHECK_NOT_RUN past the grace, the report resolves
  it and opens REFCHECK_DISAGREES once, alerted once, counted in the digest; the day published again
  resolves it); `core-api/tests/worker_api.rs` (the same over HTTP: before done 422, kept, again,
  a credential 422, another day's account 422); `refcheck::tests` (the summaries count, never list).

- **Amended 2026-11-20 (the live digest of 10:00 MT, 2026-11-20; C-056).** Each published day is
  judged on the latest check of its current base revision only (as before: the crash on
  2026-11-03's superseded revision 51 judges nothing once revision 55 is checked); what changed is
  what counts against it.
  - *Reference errors are not a check that did not run.* A completed check with reference errors
    (comparisons the reference suite itself could not make: 9 a day on 11-02 to 11-04 by build
    48d87154's suite, 0 on 10-28 by 4944a182's) ran; counting it as "did not run" kept
    REFCHECK_NOT_RUN open and the digest's "3 did not run". REFCHECK_NOT_RUN is now TIMEOUT,
    CRASHED, NOT_RUN or no report past the grace. Reference errors are counted in the digest,
    never an alert, as a tape cache not verified is (C-058).
  - *A suite without rulings is not triaged, never "unexplained".* C-056 counted an account
    without `failed_unexplained` as every failure unexplained, so 11-02's 1,982 failures (1,915 of
    which match rulings, C-056) opened REFCHECK_DISAGREES. REFCHECK_DISAGREES now needs
    `failed_unexplained > 0`; an account without it is "not yet triaged against rulings". **Not
    🔴 alone (decision, notification design):** there is nothing to go and look at that the
    rulings would not say, and the condition clears only when the day is computed and checked again
    (the build ahead does it); a standing red would stop meaning "go look" (C-056's reason). It
    is 🟡 instead, with the counts.
  - *The digest.* "Reference checks in the last 24 hours: N (a agree, d disagree[, u not triaged
    against rulings], n did not run[; e reference errors]) · typical …": each report counted once
    (did not run = not completed; disagree = unexplained failures; not triaged = failures and no
    `failed_unexplained`; else agree), every report stored in the window (a crash later
    superseded included: it happened). And, while any published day's current check is not fully
    judged: "Reference checks not fully judged: N published days (u with F failed comparisons from
    a suite without rulings, not yet triaged against them; r with E reference errors), the oldest
    D", and the verdict's 🟡 says "not fully judged: reference checks of N published days" when
    nothing is open. `refcheck list` prints "(not triaged against rulings)" for such an account.
  - On the cluster at 10:00 MT (computed read-only with this code's queries): REFCHECK_NOT_RUN
    resolves at the first check; REFCHECK_DISAGREES reads "1 published day: 254 comparisons failed
    unexplained, the oldest 2026-10-28"; "4 (0 agree, 1 disagree, 3 not triaged against rulings, 0
    did not run; 27 reference errors)"; "not fully judged: 3 published days (3 with 6617 …; 3 with
    27 reference errors), the oldest 2026-11-02".
  - Ordinary day (a suite with rulings, no reference errors): the 24-hour line reads as before; a
    completed check with reference errors and no unexplained failure now counts as agreeing
    (stated beside it) instead of "did not run". Tests: `core-db/tests/refcheck.rs::
    the_clusters_checks_are_judged_on_each_days_current_check` (the cluster's five reports and
    revisions at their recorded times), `a_published_days_check_is_kept_once_and_judged_until_the_day_is_published_again`
    (the B0 account, without rulings: 🟡, no problem), `refcheck::tests::a_check_without_rulings_or_with_reference_errors_is_not_fully_judged_never_not_run`.
- **Amended 2026-11-20 (audit 2026-11-20-pass1-round-cc, core.md, triage c.md, 5): a report is bound to what the worker checked.** `put` bound
  a report to whichever base of the build was published when it arrived, so a day published again
  by the same build during the check (a hand publish, a scoped republish under C-055) got a report
  of a revision it never read, and the new revision's missing check was hidden; currentness looked
  at the base only. The worker's account already states `revisions` (the base, then the day-local
  revision: `[53, 54]` on 2026-11-02, `[59, 60]` on 10-28): no wire change. Core now requires
  them to be a base revision of the session, profile and lease's build and a day-local revision on
  it (published or since superseded), refuses others, and stores both (migration 0030,
  `day_local_revision`). A report is current, judged and listed as current, while both are
  published; an account that states no revision is kept bound to nothing (it judges no day, and
  from `VYREON_CORE_REFCHECK_FROM` the day counts as with no report). A report sent again by the
  same lease is answered with the first before any check, as before. Ordinary day: no change (the
  engine states the ids it published). Deploy: migration 0030 with this core image; reports already
  stored keep their base and have no day-local id (judged on the base, as before). Test:
  `core-db/tests/refcheck.rs::a_published_days_check_is_kept_once_and_judged_until_the_day_is_published_again`.

## C-055 Every claim carries the OCC adjustment ledger, committed data with an OCC memo behind each entry (2026-11-18; engine audit 9 finding 1, D-170, D-290..D-294)
The engine reads an optional per-asset ledger of dated OCC contract adjustments
(`<home>/h5-universe/adjustments.json`, `VYREON-ADJUSTMENTS@1`, engine `adjustments::parse_ledger`)
from a claim's `adjustments`, as it reads the dated tickers; core never sent one. Without it an
adjustment older than the engine's detector lookback (21 sessions) is invisible, while the provider
keeps listing the stale pre-adjustment contracts with frozen open interest: EWJ's open interest was
withheld PROVIDER_OI_INCOMPLETE on 100 sessions (2026-01-20..06-11) after OCC cut every strike by
$0.16 on 2025-12-16; with that one entry no session faults (reproduced). Phase 1 (from 2025-04-10)
meets the December 2025 adjustments.
- **What:** `crates/core-api/data/occ-adjustments.json`, compiled into the binary
  (`core_api::adjustments`). A claim's `adjustments` is the ledger's `assets` narrowed to the
  claim's members (`{}` when none has an entry). Only the engine's vocabulary: `split` with the
  provider's ratio (1/n, a whole forward split), `other` with `strike_offset` (every strike lowered
  by a special distribution) or `root` (positions moved to an adjusted root: reverse splits and
  cash added to the deliverable).
- **Why committed data, not a table:** the boring option. It is reference data that changes a few
  times a year, each change a reviewed commit with its source. No migration and no load step that a
  new database could miss (a forgotten `universe load`-style step would send no ledger, and the
  engine would carry on without one, silently); every claim one core build answers carries the
  same ledger. Cost: a new entry ships with a core image. The engine's own detector covers the
  first 21 sessions after an adjustment, so a new one needs its entry within about a month.
- **Core's rules beyond the engine's parser** (checked by `adjustments::check`, at first use and in
  the tests): every entry's evidence opens with its OCC Information Memo, number and URL
  (`https://infomemo.theocc.com/infomemos?number=N`); no entry without a source (engine D-102); a
  reverse or fractional split is never a `split` (the engine cannot place it) but the adjusted root
  OCC names; the engine's `unknown` (a possible adjustment) is never sent; an asset's entries in
  ex-date order without repeats; no field the engine does not read. A strike offset is the
  distribution rounded to the nearest cent, the amount every strike actually moved (OCC rounds each
  reduced strike to the penny: EWJ $0.157024 makes 35.00 into 34.84).
- **Coverage:** every OCC adjustment of the 64 with an ex-date from 2022-05-27 (the database's first
  session) to 2026-11-18: 21 entries, 13 forward splits (AMZN, GOOGL, TSLA, SMH, WMT, NVDA, AVGO,
  MSTR, and XLB, XLE, XLK, XLU, XLY on 2025-12-05), 4 strike offsets (FXI 0.15 2023-06-08, QQQ 0.22
  2023-12-27, EMB 0.25 2024-12-30, EWJ 0.16 2025-12-16) and 4 adjusted roots (UNG1 2024-01-24,
  BABA1 2024-06-13, BABA2 2025-06-12, ETHA1 2026-10-06). The roots agree with the engine's
  `assets/adjusted-roots.json`.
- **How they were found:** infomemo.theocc.com answers automated reads with a Cloudflare challenge
  (HTTP 403), so the memos were read from MIAX's copies (the PDFs OCC issued, attached to MIAX's
  corporate-action alerts). Every MIAX corporate-action alert from 2022-05 to 2026-11 and a MIAX
  search per ticker were read for the 64; Intrinio's `prices/adjustments` records (splits and
  dividends since 2022-05-27) gave the candidates, and each split and off-schedule distribution was
  matched to a memo or to none. Each entry also states Intrinio's record of its date.
- **FXI's 2023 distribution** (memo #52545) is the older form: on the ex-date (2023-06-07) the
  strikes stayed and the deliverable carried $14.90 for one day; the strikes were reduced the next
  session. The ledger dates the reduction (2023-06-08), the session the old strikes stop being
  listed.
- **Not covered:** adjustments before 2022-05-27 that are still within 39 months of the database's
  early sessions (to 2019-02); seen in passing: ARKK's 2021-12-29 capital-gains reduction of $0.78
  (memo #49856). The FB to META symbol change (memo #50536) is a ticker change, carried by the
  dated tickers. Which ledger a published day was computed with is not recorded (the engine build
  is); a ledger change that touches published days is a scoped republish of those assets.
- Tests: `core-api` `adjustments::tests` (the committed ledger passes, names only the 64 and holds
  EWJ's, EMB's, XLE's and UNG's entries; each rule refuses a real entry altered to break it; a claim
  carries only its members' entries); `core-api/tests/worker_api.rs` (a day-block claim over HTTP
  carries the ledger). The engine's own parser reads the file (checked from the engine at
  `pass-s3i`, read only: `parse_ledger`, `universe::write_claim`, `Universe::load`).
- **Amended 2026-11-20 (engine D-120 as amended, `pass-fx59`; refcheck triage of 2026-11-02,
  EWJ's 2026-12-18 forward):** every strike offset states the special behind it, OCC's exact
  amount per share from its memo, as `special_dividend` (EWJ 0.157024 #57919, FXI 0.148983 #52545,
  QQQ 0.21584 #53847, EMB 0.254515 #55816; each already quoted in its evidence), and FXI's
  `special_ex_date` 2023-06-07 (the cash went ex the session before the strike reduction). The
  engine's surface no longer projects the special as a recurring dividend: it projects the payment
  less OCC's special (EWJ's December: 3.0614 − 0.157024 = 2.904376), not less the penny offset,
  which is a rounding of OCC's figure. New rules in `check`: a strike offset without its special is
  refused; the offset must be the special rounded to the nearest penny; `special_ex_date` is a date
  on or before the entry's ex-date; neither field on an entry that is no strike offset. **Deploy
  order:** this core image before an engine image that contains D-120's amendment; such an engine
  faults the surface of an asset whose strike offset in its dividend window states no special
  (that is, every EWJ, FXI, QQQ and EMB surface since the special, against the current core).
  Tests: `adjustments::tests` (the four specials; each new rule refuses an altered real entry);
  `worker_api.rs` (the claim carries EWJ's special).

## C-056 A reference-check disagreement is a failure the rulings table does not explain (2026-11-20; engine D-370 amended)
The first complete worker check (2026-11-02, build 48d87154) failed 1,982 comparisons; 1,915 of them
match conventions the owner has ruled as the definition (the suite's rulings table names each), so
REFCHECK_DISAGREES would stay open every day and stop meaning "go look" (notification design: one
alert per problem). The worker's account now carries `failed_unexplained` (unruled, or ruled for an
engine fix) beside `failed`; core stores it (migration 0026) and judges REFCHECK_DISAGREES and the
digest's agree/disagree on `COALESCE(failed_unexplained, failed)`, so an older account still counts
every failure. `refcheck list/show` print both. The report itself is kept whole: a ruled failure is
still evidence, read when a ruling is revisited.

- **Amended 2026-11-20 (C-054 amended):** `COALESCE(failed_unexplained, failed)` is retired: an
  account without `failed_unexplained` is counted as not triaged against rulings, not as every
  failure unexplained, and is 🟡, never REFCHECK_DISAGREES.
## C-057 Core records each asset's first option session, with the provider's evidence, and every claim carries it (owner decision, 2026-11-20; engine D-396 amended 2026-11-20)
The bulk A/B of 2025-04-11 sent 181 ETHA and 126 IBIT chains to the Intrinio API, one request each,
for sessions before their options were listed: the day files hold no row of them, which D-396's
missing-rows rule hands to the API, whose answer is the empty chain. The engine knew no per-asset
start of option listings (D-158's is the stock's first trading session; `source_coverage` is per
source).
- **What:** `core.option_listing` (migration 0027): per asset key, the first session on which the
  provider's option EOD chain holds a contract, its evidence and when it was observed. Rows only for
  members whose options were listed after the provider's option EOD start (2021-09-27; a CHECK):
  ETHA 2025-04-11 and IBIT 2024-11-19. The other 62 members' stocks all traded before 2021-04-15
  (their security records), and the A/B left no other member's chain to the API for want of rows
  (its other 35 were 2024-05-22's, a file without closing quotes); they have no row and are planned
  as before.
- **The evidence** is the provider's own answer, frozen like the universe's responses
  (`crates/test-support/fixtures/intrinio-universe/option-first-chain`, README there):
  `/options/prices/by_ticker/{root}/eod?include_related_symbols=true` on the session before (no
  contract) and on the first session (contracts of the root, open interest 0 on every one: open
  interest is the prior session's settled count, so a listing session has none). ETHA: the engine's
  captures of 2025-04-10 and 04-11 (fixture `etha-first-chain`, 168 contracts), copied byte for
  byte; IBIT: two calls of 2026-11-20 (page size 2). The A/B counts agree exactly: 181 sessions
  2024-07-23..2025-04-10 and 126 sessions 2024-05-21..2024-11-18. Rejected: the first session a
  published chain had rows (a newest-first build reaches it last, after the waste), Intrinio's
  expirations list (the first expiry is not the first listing session), a date from the exchanges'
  announcements (not the provider's data, which is what the engine reads).
- **Why a migration's rows, like `source_coverage`:** two facts that change only when a member's
  options list after it joins; no load step a new database could miss. A later member gets its row
  in a new migration with its evidence beside the universe responses.
- **The claim** carries `first_option_sessions`, `{asset key: date}` for the claim's members with a
  row (`{}` when none). Not inside `sources`: the engine's parser refuses a `sources` value that is
  not a date, so an older engine would refuse every claim. An engine without D-396's amendment
  ignores the new field; a core without it sends none and the engine plans as before: no deploy
  order is forced. Deploy core first to get the saving.
- **What the engine does with it** (D-396 amended 2026-11-20): with `VYREON_OPTION_EOD_SOURCE=bulk`,
  a chain before the asset's first option session whose day file holds no line of its roots is
  captured from the file as the empty chain (no API request); readers see the empty chain they see
  today. The API path is unchanged.
- Tests: `core-db/tests/universe.rs` `each_first_option_session_is_the_providers_first_chain`
  (each row against its frozen answers and the engine's calendar; a row at or before the provider's
  start refused); `core-api/tests/worker_api.rs` (a day-block claim over HTTP carries ETHA's and
  IBIT's dates and no other member's).

## C-058 Core keeps the tape worker's verdict beside each tape cache and serves it with the cache; a failed verdict is a disagreement, none is a cache not verified (owner decision, 2026-11-20; engine D-370 amended 2026-11-20)
The owner: the tape owners (PCR-UACT-I01, PCR-UPATH-I01, PCR-UQTR-I01, DLM-VWAP, and INT-TEMP-EVC01's
tape-dependent readings) must be verified independently. A day worker holds only the tape cache
(the manifest and the engine's derived bins, B5) and never the raw tape (engine D-159), so its
reference check stated them unverifiable (C-054's reports, D-370 amended 2026-11-19). The tape worker
still holds the raw tape when it packs each cache: it runs the reference suite's own tape reader on it
and compares the reader's bins with the cache (engine D-370 amended 2026-11-20); core keeps that
verdict, and the day check reads the cached bins for the tape owners only under a passing one.
Migration 0028; `crates/core-db/src/tape.rs`, `refcheck.rs`.
- **Kept:** `core.tape_verdict`, one row per cache (session, asset key, method version; a foreign
  key to `core.tape_cache`), `PUT /worker/v1/tape-caches/<S>/<asset>/verdict?method_version=V`
  under the session's tape job's lease, as the cache itself. The document (`VYREON-TAPE-VERDICT@1`,
  at most 1 MiB, JSON) is checked: the schema, the same asset, session and method version, an
  outcome PASS | FAIL | WITHHELD | NOT_RUN, the packed cache's sha256, the reader's bins digest
  exactly when it compared (PASS, FAIL), a reason whenever it did not pass, no differences on a
  PASS, no unredacted credential (`evidence::unredacted`). It must judge the bytes core holds
  (`cache_sha256` = the cache's `body_sha256`; a stated absence has nothing to judge). Written
  once like the cache: the same document again is `new: false`, another is refused (a cache's raw
  tape is gone once the tape job ends, so nothing could judge it again). The outcome, the number
  of differences, the reader's digest and the reason are columns; the document is kept as sent.
- **Served:** `GET /tape-caches/<S>` adds `verdicts.json` (asset to its verdict) beside
  `absences.json`. An engine before D-370's amendment of 2026-11-20 never unpacks an entry that
  is not one of its members (`verdicts.json` is not), so no deploy order is forced by the tar.
- **Judged (C-050, C-056's logic: one alert per problem, "go look"):** a FAIL on the newest cache
  of an asset-session (a cache redone under a newer method version, with its own verdict,
  supersedes it) is a disagreement: REFCHECK_DISAGREES opens on it as on a day whose check
  failed unexplained ("Reference check: the tape reader disagrees with the engine's tape cache on
  N asset-sessions, the oldest S", appended to the days' summary when both). It resolves when the
  cache is redone under a new method version whose verdict is not FAIL (an engine reader fix); a
  reference reader defect behind a FAIL needs the session's tapes downloaded again: the owner's
  call. No verdict, or NOT_RUN, is a cache **not verified**: never an alert (every cache stored
  before the verdicts has none, and its raw tape is gone), stated in the digest ("Tape caches
  stored in the last 24 hours: N (p verified by the tape reader, f disagree, w withheld whole, n not
  verified)", when any cache was stored) and in `refcheck show <date>` (each outcome's count for
  the session's newest caches, and every cache not passed with its reason; the session's verdicts
  are shown even with no day report).
- **Not judged here:** whether a day's tape owners were checked; the day's report says so (its
  `unverifiable` counts and the intraday section's `tape_verdicts`), and C-054 stores it.
- **Deploy:** migration 0028 with this core image, before an engine image that sends verdicts (an
  older core answers the verdict's PUT 404: the engine logs it, the session is done, and the cache
  stays not verified).
- Tests: `core-db/tests/tape_verdict.rs` (the real AAPL cache of 2025-12-10 and the reference
  suite's own NOT_RUN on it: every malformed verdict refused before anything is stored; no cache or a
  stated absence refused; kept once; another document, and one of other bytes, refused; no problem
  for a NOT_RUN; a FAIL on a cache redone under a newer method opens REFCHECK_DISAGREES once, is
  counted in the digest and `show`, and resolves when the cache is redone again and passes);
  `core-api/tests/worker_api.rs` (over HTTP: kept once, another asset's refused, a stranger's
  lease refused, served in the session's tar); `refcheck::tests` (the summaries).

- **Amended 2026-11-20 (audit 2026-11-20-pass1-round-cc, core.md, triage c.md, 4): a cache or verdict is stored under its lease, locked.** The
  cache and verdict PUTs checked the lease without a lock and then stored in a separate statement,
  so a lease cancelled or expired between the two could still store (and a replacement worker's
  PUT be refused). The lease check (`work::held`, `FOR UPDATE`) and the insert are now one
  transaction; a cancel in progress makes the PUT wait and then answers it 410. Ordinary day: no
  change. Test: `core-api/tests/worker_api.rs::a_cache_sent_while_its_lease_is_cancelled_is_not_stored`.

## C-059 The tape caches stored before the verdicts are verified by a sample of their sessions, downloaded again and recomputed; a verdict is kept beside a stored cache only when the recomputation equals it (owner decision, 2026-11-20; engine D-370 amended 2026-11-20)
The caches stored before C-058 never get a verdict: their raw tapes are gone. The owner: verify a
sample instead, about twenty sessions spread over the tape history, each downloaded again and
recomputed with the current build. Tape jobs keep their default: they never recapture a cache core
holds. *No published standard covers sampling a derived cache; the rule is the owner's, the
mechanism the smallest the queue allows.* Migration 0029; `crates/core-db/src/tape.rs`, `work.rs`.
- **The job:** a verify job is a TAPE job whose scope is `{"verify": true}`, one per session,
  scheduled by `vyreon-core tape verify <profile> <build> <session>...` (`work::schedule_verify`:
  each a trading session where core holds a cache, with no unfinished tape job of the profile; all
  or none). It holds nothing back: no day block waits on it (`core.claimable_job`, 0029), the
  claim's `tape_history_from` and the daily runs' "is the session's tape job queued" ignore it, and
  one out of attempts is logged, never a queue pause (`work::requeue`). It stores nothing in place
  of a cache: a cache PUT or a verdict PUT under its lease is refused (422).
- **The check** (`PUT /worker/v1/tape-caches/<S>/<asset>/check?method_version=V`, under the job's
  lease, one per listed member; `VYREON-TAPE-CHECK@1`, at most 2 MiB, JSON, no unredacted
  credential): the worker's recomputed cache's packed digest (or none, with why: the provider
  refused the session, the member's capture failed), the fetch facts it took from the stored cache
  with both values (`stamped`), what still differs (`differences`), the tape reader's verdict on
  the recomputed cache (C-058's document, whose `cache_sha256` must be that digest), and the
  session's replay bytes downloaded. **Core compares the digest with the cache it holds:** MATCH
  when equal, and the verdict is then kept beside the stored cache (`tape::put_verdict`, C-058; a
  verdict already there stands); DIFFER when not, the verdict kept only in the check; NOT_COMPARED
  when either side has no cache (none stored under V, a stated absence, nothing recomputed). The
  stored cache is never touched. One check per job and member: another is answered `new: false`.
  The session is done when every member listed on it has its check (`tape::session_done`).
- **Equal, byte for byte, means this** (engine D-370 amended 2026-11-20, `tape_check::stamp`): a
  cache records when it was fetched and from which local daily-bar page (the manifest's request
  and retrieval times, each tape's retrieval time, the official bar's page digest and attempt name,
  D-268), which no recomputation repeats; the recomputed cache takes those from the stored one
  before it is packed, and every other byte is its own (bins, counts, tape identities and stream
  digests, the official volume, the round-lot basis). A recomputed digest equal to the stored one
  is therefore the stored cache recomputed from the tape by this build, and the reader's verdict
  names the stored cache's very files. A DIFFER's `differences` say which: a tape's ETag, size or
  stream digest means the provider's file changed since the cache was made (a provider change,
  `docs/PROVIDER_FAULTS.md`); the same tapes with other bins or counts mean the code changed.
- **Judged (C-050, C-056's logic):** the latest check of an asset-session that is DIFFER, while
  the cache it found is still the asset-session's newest, is a disagreement: REFCHECK_DISAGREES
  ("the tape sample recomputed a cache other than the stored one on N asset-sessions, the oldest
  S (vyreon-core tape checks)", beside the days' and the verdicts' parts). It resolves when a later
  check of the asset-session matches or a newer cache supersedes the one checked: the owner's call
  which. A FAIL verdict kept beside a matching cache opens it through C-058.
- **Presented:** the digest carries, once any check exists, "Tape cache code verified by sample
  (C-059): p of n sessions passed (a of b asset-sessions recomputed byte for byte with the tape
  reader agreeing; d differ from the stored cache, e recomputed equal, the tape reader not agreeing,
  x not compared)", the misses named only when there are any. A session passes when every listed
  member's latest check is MATCH with a PASS verdict (WITHHELD for a capture withheld whole).
  `vyreon-core tape checks` prints that line, a line per session (counts, replay GB, job, time) and
  every member not passed with why.
- **Cost per session (the engine's tape capture, D-089; not measured for a verify job):** the
  session's three replay files streamed whole and never stored, only the 64 securities' messages
  kept (AAPL's raw histories 5.2 MB): 30.1 GB on 2025-12-10 (CTA A 6.1, CTA B 13.2, UTP 10.8),
  33.6 GB 2026-09-18, 37.8 GB 2026-10-15, and about 2.4 times that from 2026-11-06 (CTA B 76 GB).
  Intrinio requests: the three replay listings (and one more each time a presigned URL expires
  mid-stream), up to 64 security records and each member's daily bars (the session and its
  round-lot evaluation month): about 130 to 200, read from the code; the job's log prints them by
  category. The file bytes are presigned object reads, not drawn from the request budget. Time: a
  tape pod took 6 to 9 minutes per late-2025 session; the reader about a minute more (C-058,
  estimated).
- **Deploy:** migration 0029 with this core image, before the engine image that sends checks; the
  verify jobs are scheduled for that engine build only (a job is claimed by its own build; an older
  build would fail its attempts harmlessly: its caches are refused and its session never done).
- Tests: `core-db/tests/tape_check.rs` (the real AAPL cache of 2025-12-10: malformed checks refused
  before anything is stored; nothing to verify without a cache, not over an unfinished tape job; no
  day block held back; a DIFFER keeps no verdict, leaves the cache untouched, opens the problem and
  states the sample in the digest; a MATCH keeps the verdict beside the cache and the problem
  resolves; a verdict already there stands; a verify job out of attempts pauses nothing);
  `core-api/tests/worker_api.rs` (over HTTP: the claim's scope and unmoved tape history, a cache and
  a verdict PUT refused, a check kept, the verdict served with the stored cache); `tape::tests`.
- **Amended 2026-11-20 (audit 2026-11-20-pass1-round-cc, core.md, triage c.md, 6, 7, 8, 10).**
  - *A recomputed-equal cache the reader FAILS is a disagreement (6; the decision's own rule
    changed).* A verdict already kept beside a cache stands (C-058's write-once), so a later MATCH
    whose reader verdict is FAIL left the kept PASS in place and raised nothing, against C-058's
    rule that a failed verdict is a disagreement. Now such a check, while the verdict kept beside
    that cache is not itself a FAIL (counted by C-058 already), opens REFCHECK_DISAGREES: "the tape
    reader disagrees with a cache the sample recomputed byte for byte on N asset-sessions, the
    oldest S". The kept verdict is not replaced (what the day workers read stays what they read);
    it resolves as below. Consistent with C-058 (FAIL on the newest cache is a disagreement) and
    with "go look": the reader and the stored bytes disagree.
  - *Only a comparison resolves a difference, and only the newest cache is judged (8).* The
    problem read each asset-session's latest check of any outcome, so a later NOT_COMPARED (the
    provider refused the session) resolved a DIFFER; and it judged supersession by the check's
    time, so a DIFFER of a superseded cache, checked after the newer cache existed, opened it. Now
    it reads the latest MATCH or DIFFER check of each cache (session, asset, method version), only
    while no cache of that asset-session was stored after it (C-058's supersession).
  - *A session passes only with every listed member checked (7).* The sample's pass counted the
    checks received, so a job stopped after one member passed its session "1 of 1". Now a session
    passes when every member listed on it (the checking job's profile, `members_on`) has a latest
    MATCH check with PASS (or WITHHELD); members without a check are "not checked", counted in the
    line and listed by `tape checks`, and the asset-sessions' denominator is the members listed.
  - *Verdict counts are of MATCH checks only (10).* A verdict on a recomputed cache that differs
    from the stored one names other bytes; counting it under PASS/FAIL made "MATCH − PASS −
    WITHHELD" (the equal caches the reader did not agree with) cancel out. `tape checks` prints
    "c of n listed members checked, m match (PASS, WITHHELD, FAIL, NOT_RUN, without a verdict)".
  - Ordinary day: none (no verify job runs); on the cluster's 20 sampled sessions the digest's line
    is unchanged if every listed member was checked and passed (20 of 20 sessions, 1,276 of 1,276
    asset-sessions on 2026-11-20). Tests: `core-db/tests/tape_check.rs` (three members of 64
    checked; a NOT_COMPARED after a DIFFER; a MATCH whose reader FAILS beside a kept PASS; a newer
    method version's cache superseding the checked one), `tape::tests`.
