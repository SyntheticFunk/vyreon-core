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
Amended (owner, 2026-11-17 and 11-21): backups start as soon as the test block is stored, not at
acceptance; how, in C-067.

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
- **Amended 2026-11-22 (C-065): a claim leases a job whenever one is claimable and unlocked.** The
  locking scan itself requires `state = 'QUEUED'`, so a row leased by a concurrent claim fails
  PostgreSQL's re-check and the scan takes the next; a claim that still finds nothing while work is
  claimable looks again (3 times) and is kept as a miss for the watch.

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
- **Amended 2026-11-22 (C-068; audit opsurface ops.md 13): a lost node is not an attempt, twice a
  day.** The basis above ("each failed day has already cost three attempts") did not hold: the
  attempts are the job's, so a block that lost two leases on Monday, finished Monday on its third and
  lost its node on Tuesday set Tuesday aside on its first try. A lease that ends by expiring (its node
  lost, its pod killed or out of memory, its Job's `activeDeadlineSeconds`) and left no evidence (no
  stop the worker could explain, C-049) is not charged while the session it stopped on (recorded,
  `core.lease.stopped_on`) has lost at most `work::LOST_LEASES_FREE` (2) leases; from the third such
  loss on that session it is charged. So a session that kills every node still ends FAILED, after
  at most 2 + `max_attempts` (5) lost leases, and a stop with evidence is charged at once as before.
  Two is a plain declared default (one lost node is ordinary on a 13-node autoscaled pool, a second
  on the same day is not yet a pattern), not tuned. Applies to every kind (a tape or a pass whose
  pod is killed is the same case). Test `a_lost_node_never_sets_a_day_aside_on_its_first_loss`.
- **Amended 2026-11-22 (C-068 round 2, `round2-core.md` 9): a block's attempts are its next day's.**
  The free losses fixed the lost-node case, but failures charged on an earlier day still spent a
  later day's attempts (two charged failures on Monday, the third lease finishing Monday and failing
  on Tuesday set Tuesday aside at once). The lease that moves a block to its next day is that day's
  first attempt (`session_done` sets `attempts = 1`), so the basis above now holds: each day set aside
  has had its own three attempts (and its two free losses). Bounded: each reset needs a day done.
  Test `a_days_attempts_are_its_own`.

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
- **Amended 2026-11-22 (C-062):** the pass of a fallen day is queued under a build that runs (the
  publishing build when it publishes that profile, else the fallen revision's own build), never under
  one no worker claims; with neither, the day is named. A retirement takes down what read the retired
  days the same way.

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
- **Amended 2026-11-22 (C-062):** the property rests on a day whose block is not scheduled yet holding
  the later passes; C-062 keeps that (with only published and pending days holding, AtStart measured 18
  runs, not 12). A whole-range schedule after an earlier publish no longer meets "overlaps unfinished":
  it keeps the passes a publish queued under its build.

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
  *Amended 2026-11-22 (C-065):* neither is built; spend stays the operator's (no cloud token in
  core), idle nodes the autoscaler's. Two kinds added from core's rows: `STALLED` and `CLAIMS_EMPTY`.
  A second, optional channel carries progress (C-065); this channel is unchanged.
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

- **Amended 2026-11-20 (audit 2026-11-20-pass1-round-cc, core.md, triage c.md, 1): coverage from
  2019-05-27.** Engine D-120 as amended takes a ledger special out of every ex-date in its 1,096-day
  dividend window, so the database's first session (2022-05-27) reads distributions from
  2019-05-27; without its entry, ARKK's 2021-12-29 capital gains (0.7826 in Intrinio) recurred as a
  projected dividend (engine `a_due_unpaid_recurrence_is_not_projected`: 0.7826 at 2023-12-27 for
  T 2023-03-01). Coverage is now every OCC strike-offset special of the 64 with an ex-date from
  2019-05-27. Five entries added, each memo read from MIAX's copy and its figures quoted: ARKK
  2020-12-29 2.04431 (offset 2.04, #48070) and 2021-12-29 0.78257 (0.78, #49856), both capital
  gains; EEM 0.266326 (0.27, #46168), the older form like FXI's: cash in the deliverable on the
  ex-date 2019-12-16, strikes reduced 2019-12-17 (the entry's date; `special_ex_date` 2019-12-16);
  XLE 2019-12-30 1.791209 (1.79, #46261); XLV 2019-12-30 0.66554 (0.67, #46260). Nine strike
  offsets in all. **How found:** every MIAX Options corporate-action alert dated 2019-05-20 to
  2022-06-05 (691, all loaded; 8 without a PDF, none of the 64) and each attachment's name; Intrinio
  `prices/adjustments/dividends` for each asset over the window gave the candidates (62 calls). The
  only other alerts for the 64 were non-cash (the AAPL, TSLA, NVDA and AMZN splits, USO's reverse
  split, FB to META). **Not established, so not in the ledger** (Intrinio candidates with no OCC
  alert found; an entry is never written without its memo): ARKK 2019-12-27 0.19063; DBA
  2019-12-23 0.25717; UUP 2019-12-23 0.52612; URA 2021-12-30 1.308965; EFA 2021-12-30 0.163147
  and EWZ 2021-12-30 0.160947 (iShares EWG's like payment was adjusted, memo #49872); the large
  iShares semiannual payments of 2021-12-13 (INDA 2.879402, EWZ 1.983642, EWY 1.678518, EFA
  1.352231, EWJ 0.888134; EWT's was adjusted, memo #49743); XRT 2021-12-20 0.932859; XBI
  2020-12-21 0.176; TSM 2019-06-24 1.2887; the iShares bond funds' recurring December extras (TIP
  2021-12-16 0.7862; EMB, HYG, LQD 0.2564 to 0.4411; TLT 0.1639 to 0.2176). The absence of an alert
  is not OCC's statement that it did not adjust; only MIAX's Options listing was crawled (an alert
  posted only on Pearl or Emerald could be missing). Intrinio's gaps met on the way:
  `docs/PROVIDER_FAULTS.md` P-22 (EEM has no dividend record in the window; XLV two on 2019-12-30).
  `adjustments::check` has no date floor; the engine's `parse_ledger` reads any date (checked read
  only). Ordinary day: a claim of ARKK, EEM, XLE or XLV carries the new entries; the engine's
  surfaces whose dividend window holds them change (the special is no longer projected). A day
  already published is republished by asset under the next build (C-055: a ledger change is a
  scoped republish). Tests: `adjustments::tests` (the five specials and memos; nine offsets).
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

- **Amended 2026-11-21 (the digest after core 8a53f67): a cache is judged on its latest
  reader's verdict.** The verdict kept beside a cache is written once (the first), so the 51 FAIL
  verdicts of the reference reader in build 3b85b4f6 (two reference defects, a half-to-even tie
  rounding and an in-bin conditional-last count, fixed by engine D-370 of 2026-11-20) stayed
  "disagrees on 51 asset-sessions" after the corrected reader of cdf58be2 PASSED the same
  recomputed bytes in the sample (C-059: MATCH, its verdict not kept). Now a cache's verdicts are
  the kept one and every sample check's that recomputed it byte for byte; the current one is the
  latest reader's. **The reader is the build that ran it** (the verdict's `engine_build`: the
  reference suite ships in the worker image; the documents carry no reader version of their own,
  and `versions` names only the libraries), ordered by the build's admission in core
  (`core.engine_build.admitted_at`, its registration when never admitted, an unknown build
  earliest), never by when the verdict came; for one reader, the verdict stored last.
  REFCHECK_DISAGREES and `refcheck show <date>` read the current verdict
  (`refcheck::CURRENT_VERDICTS`); a later reader's FAIL reopens it, an earlier reader's FAIL sent
  late does not. The kept verdict is unchanged (evidence). **The day check reads the current
  verdict** (owner's decision, 2026-11-21): `GET /tape-caches` serves each cache's current verdict
  in `verdicts.json` (`tape::verdicts_of_session`, the same order), as its reader sent it, so the
  day checks of 2025-06-02, 08-01, 12-01 and the 2026 sampled sessions (inside phase 1) read the
  tape owners under the corrected reader's PASS instead of treating them as unverifiable under the
  old reader's FAIL. Test: `core-api/tests/worker_api.rs::a_day_worker_is_served_the_latest_readers_verdict`
  (real 748 then 1768: FAIL served, then PASS). This replaces C-059's round-cc 6 rule (a FAIL in a sample beside a kept
  non-FAIL): it is the same judgement, now with the reader's order. On the cluster (read-only,
  2026-11-21): all 1,276 judged caches have a current PASS, so the tape part of
  REFCHECK_DISAGREES clears and the problem is restated as the day's disagreement alone ("1
  published day: 254 comparisons failed unexplained, the oldest 2026-10-28"), no post. Ordinary
  day: no change (one verdict per cache). Tests: `core-db/tests/tape_check.rs::
  an_old_readers_fail_is_superseded_by_a_fixed_readers_pass_on_the_same_bytes` (the real FAIL 748
  and PASS 1768, the two builds at their recorded admission times), and the round-cc steps of
  `a_sample_check_keeps_the_verdict_only_beside_an_equal_cache_and_a_difference_is_a_problem`.
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

## C-060 Phase 1 starts where today's values are final: 2024-07-09..2026-11-20, a warm-up chain of 596 sessions (owner, 2026-11-21)
*Standard:* the warm-up period of history-dependent calculations: TA-Lib gives every function a
lookback (bars before its first valid output) and an unstable period for those that carry memory,
whose outputs depend on where the data starts and are discarded. Product intent: Reporter's window is
"at least 252 accepted trading sessions" (free pages plot each sensor over time); the paid issue
states each flag's robust z and own-history percentile beside the flag count of ordinary weeks
(D-067).
- **Criterion (owner):** at the live date every value of the current week, base and derived, is
  final: building further back later (to the provider's starts, 2022-05-27 for snapshots) changes no
  value from the live date on. The 252 sessions behind it are populated; their oldest points may move
  on a deeper build.
- **Measured** (code inventory 2026-11-21, engine `worker` 24713351, core dbe5d0e; checked at the
  cited lines): base sensors take their history from the provider (option captures 223 sessions,
  daily 379, `h3-o04` `history_production.rs`, `day_inputs.rs`), so store depth does not reach them;
  tape caches are fetched again when missing and start at the earliest TAPE job (each day reads 252
  tape sessions, D-310). The longest chain through the store is the flag line: pool 252 sessions of
  every security's z (`unusual.rs` LINE_WINDOW, `ordered.rs:188`) + z window 63 + DLM-PCR-HISTORY
  `sma_30` 29 (`h5-derived/src/lib.rs:367`) = **344**. The 126/252 level percentiles are not scored
  (`unusual::scored` excludes "percentile"), so they end at 252.
- **Weekly baseline (owner):** the D-067 baseline of ordinary weeks had no bound (engine
  `vyreon-api/src/main.rs:790-798`: every earlier week back to the first stored) and so could never
  be final. It becomes a **fixed window of 252 sessions**, the horizon of the flag line's pool and of
  Reporter's window (control-chart practice asks at least 20–25 subgroups, Montgomery, *Introduction
  to Statistical Quality Control*; 252 sessions is ~50 weeks). The engine records it with the code
  change (wherever the review is computed for Reporter, gap-scan item 7).
- **Range:** 344 + 252 = 596 sessions before the current week: start **2024-07-09**, end 2026-11-20,
  597 sessions; tapes from **2023-07-07** (252 before the start). Snapshot windows of the first day
  reach 2023-12-27 (the 20-column layout, D-252, read before on the cluster); the 2023 snapshot gap is
  not reached. D-359's partial snapshots (2023-10-30, 2023-11-17) are past sessions before the start,
  skipped and stated in windows since D-404 (not the 200-session withholding D-359's text describes).
- **Replaces** the 2025-04-10 start ("every sensor at full window on the newest week", a weaker
  test) and the per-tranche gate's history parts: the ordered passes run once for the range after the
  blocks (C-046), so the history reference check and the flag-agreement test judge the whole store
  after them. Per day as each publishes: the in-worker reference check, the Cboe bands, EDGAR.
- **Order:** a test block 2024-07-09..07-22 (with its tape history) first; then the rest; daily runs
  (C-051) switched on with the build, from 2026-11-23, deadlines from the live date.
- Graph declarations that disagree with the code (judged by the code here; to fix after the build):
  DLM-PCR-HISTORY declared 252 (reads 30), DLM-EVENT-WINDOW 252 (reads T), DLM-IV-CRUSH 252 (no bound:
  the latest confirmed event in the store; checked after the build that every company's event lies
  inside it). A later deepening re-runs every later ordered pass (C-037) and changes their provenance
  bytes (`history_sha256`), not their values.

## C-061 A failed day is a missing date: stated with its cause, never a block on later history (owner, 2026-11-21; built before the test block)
Owner: "failed should count as failed such as missing or whatever. same as a missing date. If the
problem is on the provider side it should be clear to us so we can just omit that date."
- **Already so in the engine:** history windows walk the calendar; a session with no published day is
  `NOT_PUBLISHED`, ineligible, one fewer against D-404's 75% floor (`h5-derived/src/lib.rs:141-156`;
  unusualness `prior_values`).
- **Not so in core:** a FAILED day block holds back every later ordered pass, and its own pass (never
  claimable, its base unpublished) holds back the rest (`core.claimable_job`, 0029). C-037 already
  re-derives every later pass when a day publishes later, so the wait buys nothing.
- **Change:** a FAILED or omitted day block, and its unpublished day's pass, no longer hold back later
  passes; a day queued again holds them as any pending day does. `GET /v1/sessions` states each
  NOT_PUBLISHED session's cause: PROVIDER_INPUT_MISSING (the provider-input hold, with its counts),
  ENGINE_FAULT (the fault hold), FAILED (the stop's first line), NOT_COMPUTED_YET (queued or running),
  OMITTED (with the operator's reason); omitting a date is one command with a reason; the digest lists
  each set-aside day with its cause. An engine fault is fixed and recomputed, never omitted.
- **Built (2026-11-21, migration 0031):** `core.claimable_job`: a FAILED day block no longer holds
  later passes, and an earlier pass whose day has no published base while its block is FAILED (or
  the day omitted), with no day job pending for it, no longer holds the passes after it; a day
  queued again holds them again; DAY_BLOCK and TAPE claims unchanged. `vyreon-core omit <profile>
  <session> <reason...>` (`work::omit`) is refused unless the session's day job stands FAILED with
  nothing of the day published, and refused for the engine-fault hold; it records
  `core.omitted_session` (session, profile, reason, job, time), cancels the day job and the
  session's queued pass, and logs a notice. The cause (`core.session_cause`, from the last
  attempt's evidence message, else its lease's failure; the engine's held text is found after
  `held: `, since the worker sends `stop: <date> not published: held: ...`) is on every
  NOT_PUBLISHED session of `GET /v1/sessions`, `/v1/sessions/{date}` and an asset's history (in its
  ETag too), `{code, detail}` in this order: NOT_COMPUTED_YET (a queued or running day job), OMITTED,
  PROVIDER_INPUT_MISSING / ENGINE_FAULT (the hold's counts, without its slot list), FAILED (the first
  line), NOT_SCHEDULED. The digest has a line per day set aside and per date omitted in the last 24
  hours, with its code (at most 10), and counts an omission as handled. Tests:
  `core-db/tests/work.rs` `a_failed_day_holds_no_later_pass_and_a_retried_one_holds_them_again`,
  `a_date_is_omitted_only_when_its_day_failed_for_the_provider` (the digest lines),
  `core-api/tests/reader_api.rs` `each_unpublished_session_states_its_cause`.
- **Engine side, read (worker branch, 2026-11-21; not changed):** the ordered pass computes a day
  whose window holds a session with no published day without error: the level and history windows
  read it as `Position::Absent` (NOT_PUBLISHED; `h5-derived/src/lib.rs:141-156`, `:659-671`,
  `:896`), DLM-OI-CHANGE on the next day withholds PRIOR_SESSION_NOT_PUBLISHED (`:422-499`),
  DLM-IV-TERM `change_5` NO_PRIOR_SESSION_VALUE (`:950-966`), DLM-IV-CRUSH IV30_UNAVAILABLE_PRE/POST
  (`:1123-1155`); `inputs_digest` lists only what is published (`vyreon-cli/src/derive/ordered.rs:77-95`);
  only T's own base and day-local revision are required (`ordered.rs:54`, `:63`, which the claim
  guarantees). **Not one fewer for readings in option contracts or over the strike partition:** a
  session with no published day has no VS-O04/VS-OIJ01 evidence, so its OCC adjustment status is
  Unknown (`ordered.rs:220`), and D-170 restarts the contract and partition basis at it for every
  object for the next 252 sessions (`h5-derived/src/basis.rs:64-100`): DLM-OPTIONS-VOLUME's windows
  count every earlier session BEFORE_SERIES (`lib.rs:507-521`; its 252-session percentile needs 189
  sessions after the gap, D-404), and DLM-UNUSUALNESS withholds contract-unit and partition readings
  INSUFFICIENT_HISTORY until 48 sessions after it (`unusual.rs:217-260`). D-404 part 2's walk
  evidence rescues a session whose own bodies are withheld for PROVIDER_INPUT_MISSING
  (`vyreon-cli/src/derive/mod.rs:242-308`) but not one with no published day, though T's own walk
  crossed it from the provider's bars. For the engine (owner's call): treat a session absent from
  the store like an undelivered one there. Also: a gap at the first published session moves
  `first_published` (`lib.rs:142`, `:511`, `:660`, `:896`), so it reads BEFORE_SERIES, not
  NOT_PUBLISHED (the same count against the floor).
- **Amended 2026-11-22 (C-062):** the claim rule is generalized: a day missing for any reason (its
  blocks FAILED, cancelled by `omit` or by hand, or DONE with nothing published) holds no later pass.
- **Amended 2026-11-22 (C-063):** a day is published for the ordered pass only with its base AND a
  day-local revision on that base (the engine refuses a pass without it), in the own-day and the
  holding test alike: a FAILED block that left only its base is a missing day, never a pass that fails
  three times and then holds every later one. The executor's and the day-local derive's holds are
  ENGINE_FAULT (never omitted); an omission states its session only while it is current; `omit` takes
  the publish lock.

## C-062 Nothing waits on a pass no worker runs; a missing day holds no pass; earlier builds' days can be retired (2026-11-22)
Audit 2026-11-22 prebuild (`surface.md`), finding 6 (above the cut line: it stalls the build) and the
gap the triage found in C-061 (finding 5). Migrations 0032–0034.
- **Fault (6):** a publish that superseded later history revisions queued each fallen day's DONE pass
  again under its *old* build (`publish::rederive`). Workers claim only their own build's jobs, and
  the claim's earlier-pass check and `schedule`'s overlap check ignored the build: the job was
  claimable by no one, held every later pass, and refused the new build's whole-range schedule.
  Withdrawing the old build did not help: the claim then excluded the job, but it still held.
- **A fallen day's pass goes to a build that runs** (admitted, not withdrawn: core's only notion of
  "runs"): the publishing build when it runs and its inventory holds the fallen revision's profile
  revision; else the fallen revision's own build when it runs (as before: its DONE job is queued
  again); else none, and the day is named (notice and NOT_DERIVED), never queued where no worker
  claims. A pass under another build than the finished one is a new job; the finished one stays DONE.
  Why the inventory test: a base-only build cannot publish the history profile (the slice's real
  builds are split so); the cluster's image is one build for every profile, so there it is the
  publishing build while the history profile's revision is unchanged. If a new build changes that
  revision and the old build is still admitted, the pass goes to the old build: **withdraw a build
  when its image is replaced.**
- **Never two unfinished passes for one session and profile:** a unique index (0032; any existing pair
  is settled first: the LEASED one, else one whose build runs, else the newest is kept, the others
  cancelled with a logged notice). `schedule ordered-pass` keeps a session's unfinished pass of its own
  build, cancels and replaces one of a build that does not run (carrying its priority), and refuses
  one of another build that runs or under a live lease. DONE and CANCELLED passes of any build never
  stood in its way, and still do not.
- **An earlier pass holds later ones only while** its build runs and its day is still to come: a base
  published on it, or a QUEUED or LEASED day block covering it, or no day block ever scheduled for it
  and not omitted (0033). So a day whose blocks all ended FAILED, CANCELLED (by `omit` or by hand) or
  DONE with nothing published (retired), none pending, is a missing date (the owner's "a failed day
  just means -1 day"); 0031's FAILED-or-omitted test is its special case. A pass still never runs
  before a day that will publish: any QUEUED or LEASED block at or before it holds it, unchanged.
  **Not as asked:** a never-scheduled day still holds. A build newest first schedules its blocks in
  tranches and its passes once, before the first (C-046); if never-scheduled days held nothing, each
  tranche's passes would run before the earlier tranches' days and again after each: measured in
  C-046's test, AtStart 18 pass runs instead of 12 (the quadratic shape, 2,602 for 402 sessions in
  C-046's measure). A day never scheduled is resolved by scheduling its block, or by cancelling its
  pass. Also unchanged: a QUEUED day block of a withdrawn build still holds the later passes (cancel it).
- **`vyreon-core days retire [--preview] <reason> <date>...`** (0034): every PUBLISHED revision of
  each date (base, day-local and history, every profile; a day-local revision without its base would
  be incoherent) becomes SUPERSEDED, every row kept; `core.retired_revision` names each with its
  `core.retirement` (reason, sessions, the jobs cancelled), and the log has a line. It cancels the
  unfinished day blocks and passes of those dates **whose build published a revision of that day**
  (a job of another build, which will compute the day afresh, stays), and refuses the whole
  retirement when such a block also covers a date not named. The reader states each date
  NOT_PUBLISHED with the cause RETIRED and the reason (after a pending or failed day job's cause).
  `--preview` does all of it in a transaction that is rolled back and prints the report.
  **It triggers C-037's rederive, and must:** every later day's history revision read the retired
  day (its windows, its own earlier values, and the store's first published session: `load_store` has
  no upper bound, so phase 1's windows label pre-start sessions by the 2022-06 days, triage "missed by
  the auditor"). Left published, those revisions would stand on a basis the store no longer holds,
  which is what C-037 forbids for a replaced day; a removed day is the same case. The later history
  revisions are superseded and their passes queued (no publishing build, so the fallen revision's
  build when it runs, else named). Retiring all 24 dates together before phase 1's passes costs
  nothing more: the only later history revisions are theirs. **Implemented, not run:** whether to run
  it on the 24 dates core holds from 8 earlier builds (2022-06-06..15 and the 16 in phase 1's range)
  is the owner's decision; run `--preview` first.
- **Phase 1's days from earlier builds** (2025-04-11, 2025-12-10, 2025-12-11, 2026-10-28..11-13),
  when the test block publishes 2024-07-09 (every PUBLISHED history revision dated later, of any
  build, falls with it, as before):
  - *Passes scheduled for the whole range before the test block* (C-046's first option, the triage's
    fix 1): each of those days has a QUEUED pass of the phase-1 build, so nothing is queued under an
    old build and the old DONE passes stay DONE. The old bases and day-local revisions stay PUBLISHED
    until the phase-1 blocks replace them (step 2); the phase-1 passes run after the blocks, once, in
    date order. Between the test block and step 2 none of them runs: the never-scheduled days from
    2024-07-23 hold them. If a phase-1 block fails on one of those days, its old base stays published,
    so the day counts as published: its pass holds the later ones and runs on the old build's base.
    Retiring the day makes it a missing date instead.
  - *Passes scheduled after the blocks* (MILESTONES step 3 as written): each of those days with a
    PUBLISHED history revision and a DONE pass job in core gets a pass queued under the phase-1 build
    (if its inventory holds that profile revision; else under the old build while admitted, so withdraw
    the old builds first; else named). Before step 2 schedules the blocks, such a pass (2025-04-11
    first) is claimable: its day has a published base, no block stands at or before it, no earlier
    pass exists. It runs on the old build's base and runs again after the phase-1 block replaces it:
    wasted runs and a revision built on an old base, published in the meantime. Step 3's schedule
    keeps those passes.
  - Either way a day whose history revision has no pass job in core (published by hand, from the
    droplet's store) is named in a notice and a NOT_DERIVED problem, resolved when its phase-1 pass
    publishes. The 2022-06 days lie before 2024-07-09 and are never superseded by phase 1; retired
    after phase 1's passes they would take down every phase-1 history revision and run every pass
    again, so retire them, if at all, before the passes.
  - **Recommended order:** withdraw the old builds, retire the 24 dates (preview first, owner's
    call), schedule the phase-1 passes for the whole range, then the test block.
- **Amended 2026-11-22 (C-063):** a block makes its day tried (missing, holding nothing) only when its
  build runs, and a DONE block only while something of its day is published; an earlier build's
  cancelled or retired block leaves the day still to come. `days retire` executes only with the token
  its preview printed (`--expect`). Withdrawing a build ends its live leases. A fallen day's pass under
  a build that does not run is cancelled and replaced. In the paragraph above on a phase-1 block that
  fails on a day of an earlier build: the old base counts as published only with a day-local revision
  on it (pre-C-031 builds published none), else the day is missing.
- **An ordinary day changes nothing:** no history revision falls (C-037), so `rederive` queues
  nothing; the claim differs only for a pass behind a missing day or a pass of a build that does not
  run; `schedule` differs only where an unfinished pass already stands; daily runs are unchanged.
- Tests (`core-db/tests/work.rs`): `a_superseded_pass_is_queued_under_the_publishing_build_and_its_schedule_keeps_it`,
  `a_pass_under_a_build_that_does_not_run_holds_nothing_and_is_replaced`,
  `a_day_cancelled_by_hand_holds_no_later_pass_and_one_still_to_come_does`,
  `published_days_of_an_earlier_build_are_retired_kept_and_stated`; C-046's
  `a_newest_first_build_runs_each_ordered_pass_once` unchanged (12, 12, 30). A test build named
  `slice-base-and-derived` carries both real inventories: core's checks never read which build
  computed a body.

## C-063 Queue fixes from the prebuild audit of C-061 and C-062 (2026-11-22)
Audit 2026-11-22 prebuild (`changes-core.md`), its triage and the triage's own findings; migrations
0035–0038. Every verified finding is fixed (C-041); #3, #5, #14, #15, #17 and #19 (the flag gate) were
fixed on main, #4 and #16 are by decision, #20 is not real.
- **A day is published for the ordered pass only with its day-local revision (#2, above the cut
  line).** A block publishes a day in two bundles, the base then the day-local revision on it
  (C-031), and the engine's pass refuses a day without the second (`derive/ordered.rs`: "no published
  day-local derived revision on base revision N"). A block whose last attempt published the base and
  failed before the day-local upload (a 422, core restarting) stood FAILED with its pass claimable:
  refused three times, the FAILED pass on a published base then held every later pass. 0035:
  `core.day_published` (base, and a PUBLISHED day-local revision whose `base_revision` is that base)
  in both places `core.claimable_job` asked "a base is published"; such a day is a missing day (C-061)
  until `queue retry-failed` computes it. The set-aside notice, the pass's last-failure notice and the
  digest's set-aside line name the base left without its day-local revision. A base republished by
  hand is missing for the passes until its day-local revision follows (send both in one `publish`);
  a later pass run in between is derived again (C-037).
- **No queue conflict is a refused bundle (#1).** `schedule ordered-pass` took only the schedule lock,
  a publish's re-derive only the publish lock: both could insert a session's unfinished pass, and the
  unique index's violation was read as a refused bundle (422, the queue paused). `schedule` takes the
  publish lock first for ordered passes (the order `omit` and `retire` use), and a publish reads a
  class-23 violation of a core table as its own failure (the worker retries), never a refusal: the
  bundle writes only the store.
- **A block makes its day tried only when its build runs (#6).** 0033 read any block that ever covered
  a day as an ended attempt; under whole-range passes (C-046) an earlier build's cancelled or retired
  DONE block made a missing day of one whose new block was still to come. 0038: a block of a build that
  does not run, or a DONE block of a day with nothing published, does not make it tried; such a day
  holds the later passes as a day never scheduled does. A FAILED or CANCELLED block of a build that
  runs still makes it missing.
- **A fallen day's pass is under a build that runs (#7).** `rederive` took any unfinished pass as
  covering a fallen day; one under a withdrawn build kept the publishing build from the day and the day
  out of NOT_DERIVED. It is cancelled (named in the notice) and the day targeted as C-062 says; a pass
  the cascade stops is queued again only under a build that runs.
- **Withdrawing a build stops its running jobs (#8, and the triage's: a withdrawn build's live lease
  could still publish).** `build withdraw` ends every live lease of the build's jobs (CANCELLED) and
  queues each job again under it, the attempt not counted, in one transaction. Ending the leases, not
  refusing the publish, because it is safe mid-upload: a publish holds its lease's row from its start,
  so the withdrawal waits for it to commit (the day was published before the withdrawal) or the
  publish finds the lease ended and writes nothing; a worker is refused at its next call. A claim
  share-locks its build's row, so no lease is granted under a build being withdrawn.
- **Every engine-fault hold is ENGINE_FAULT (#9).** The executor's exit-1 hold ("the executor held the
  day (too many faulted slots; ...)", `day.rs`) and the day-local derive's ("N of M objects' day-local
  derived records faulted (more than X)", `derive/local.rs`, D-103) read FAILED and `omit` took them.
  0036 classifies both ENGINE_FAULT (detail without the log's path or the object list).
- **A stop on a day already done does not set the next aside (#10).** After `day_done(T)` a stop
  before T+1 began (the engine's `stop: prune after T`) on the last attempt set T+1 aside untried with
  T's cause. When the attempt's evidence (C-049) names a day before the block's next session, the
  block is queued again and the attempt not counted; each such attempt moved the block on, so it cannot
  repeat without end. Without evidence, as before.
- **An omission is the cause only while current (#11).** 0037: OMITTED only while its day job is the
  latest day job covering the session and no retirement of it came later; else the later causes.
- **`omit` takes the publish lock (#12)**, so a base published by hand cannot land between its
  empty-base check and its commit; `retire`'s comment now names the lock order truly.
- **A retirement takes down only what its preview showed (#13).** `retire` chose its revisions and
  jobs when it ran. The preview prints a token (SHA-256 of the revisions and the jobs it cancels,
  16 hex digits); `days retire --expect <token> <reason> <date>...` is refused whole when the dates
  hold anything else. Executing without a token is refused: preview first.
- **A post cuts its lines before its open problems (#18):** room is kept for the problems' heading and
  count; lines that do not fit are counted.
- **On an ordinary day nothing changes:** a block publishes the base, the day-local revision, then
  says the day is done; a pass waited for `day_done` already, so it is claimed exactly when it was.
  No measurement, body or byte changes (core computes nothing). Notices, the digest and the reader's
  causes change only in the cases above.
- **Not verified on real data:** none of this has run on the cluster. The tests use the real slice of
  2025-12-10 (its DLM-MAX-PAIN slot as the day-local profile, under a profile name made for the test)
  and the engine's messages as its source writes them; no held day's real message is kept where core
  can read it.
- **Amended 2026-11-22 (C-064):** a block that ends for good on a base without its day-local revision
  supersedes that base; a pass counts a day published only in its own build's profile revisions; a
  refund needs a day its own lease did; the retirement token covers the later history; `build
  withdraw` takes the publish lock.

## C-064 Prebuild audit round 2 of C-061 to C-063: a missing day is missing everywhere (2026-11-22)
Audit 2026-11-22 prebuild round 2 (`round2/core-r2.md`, bf808f3...4e784e1), 11 findings, each
re-verified against the code before it was fixed (no separate triage this round); all 11 are real and
fixed. Migrations 0039–0041.
- **A base left without its day-local revision is superseded when its block ends (1, above the cut
  line: a wrong number).** C-063 made such a day missing for the claim, but the engine's ordered pass
  reads every PUBLISHED base directly (`vyreon-cli/src/derive/mod.rs` `load_with`), so the day still fed
  later days' history (DLM-OI-CHANGE `raw_change_1` from its VS-O02 and VS-O04). Now when a day block is
  set aside FAILED (`work::requeue`) or cancelled (`work::cancel_in`) on a day whose base is PUBLISHED
  with no PUBLISHED day-local revision on it, core supersedes the base in the same transaction
  (`publish::drop_base_without_day_local`; the row is kept and named in `core.dropped_base`) and C-037's
  rederive takes down what read it (normally nothing: the block held every later pass while it ran).
  The notice says so ("its base revision N was published but no day-local derived revision on it: the
  base is superseded, so no later day's history reads D"), and so does the session's cause: a FAILED
  day's detail opens "base revision N superseded, no day-local revision was published on it; " (also on
  the digest's set-aside line), a cancelled one reads NOT_SCHEDULED "its day job J was cancelled; base
  revision N superseded, …". A later attempt (`queue retry-failed`) publishes a new base and its
  day-local revision as any day; the passes after it are derived again (C-037). Why here and not "a base
  counts only once its day-local revision is published": the day-local revision binds to a published
  base (`bind_base`, FOR SHARE) and the engine's day-local derive reads it, so the base must be
  published first, and what counts for the engine's pass is a revision's state, which only core sets;
  while the block runs it holds every later pass (`core.claimable_job`), so the only exposure is a block
  that stops for good, which is where core now acts. `fail`, a lease's expiry and `cancel` take the
  publish lock first (they may now supersede and re-derive).
- **A pass counts a day published only in its build's profile revisions (2, above the cut line: a
  stall).** `core.day_published(day, profile, build)` (0040) requires the base and the day-local
  revision to be of profile revisions in the pass's build's registered inventory (the profiles it
  publishes; the engine's pass reads exactly its own `BASE_PROFILE` and `DAY_DERIVED_PROFILE`), for the
  pass's own day and, for an earlier pass, with that pass's build. A mixed-revision day is missing for
  that pass, never a FAILED pass that holds the later ones. On the cluster (one build for every profile)
  nothing changes; the tests' derived build now registers the profiles its pass reads
  (`test_support::slice::reading`). Not changed: a base whose day-local revision is of another profile
  revision is still PUBLISHED and read by the engine's later passes (a coherent published day; only a
  change of the day-local profile revision makes one, and then the days are recomputed).
- **The claim reads an omission only while it is current (3),** the reader's rule (0037), through one
  function, `core.omission_current` (0040).
- **A refund needs a day the same lease did (4, 10).** C-063 #10 took the progress on trust: a worker
  naming a stale day on each new lease was refunded without end, and the supplied worker names its
  claim's first day for any stop without a date (`vyreon-worker` `stopped_on`), so a disk guard on the
  day after a done day refunded a real failure. Now a lease records the session its attempt began at
  (`core.lease.start_session`, set by the database when the lease is granted, 0041), the day is read from
  the evidence's message as the engine writes it (`stop: <date> ...`, or `stop: prune after <date>:`,
  `days.rs`), never from its session field alone, and an attempt is refunded only when that day lies from
  the lease's start to before the block's next session: each refund moved the block on by a day of its
  own lease, so refunds are bounded by the block's length. A message naming no day is counted. Leases
  granted before 0041 have no start: counted.
- **The retirement token covers the later history it takes down (5):** the later days' PUBLISHED
  history revisions that fall with the retirement are locked and part of the token.
- **A refused upload from an ended lease is the lease gone (6).** Some refusals come before the lease is
  checked (a bundle that ends before its scalars, or cannot be read to its end); `publish_for_lease`
  checks the lease again before answering a refusal: a withdrawn, cancelled or expired lease's upload is
  a 410 (no pause), a live one's still a 422 that pauses the queue.
- **One lock order (7, 8).** `build withdraw` takes the publish lock first. Every holder of the publish
  lock takes it before any row lock (publish, the claim of an ordered pass, `schedule ordered-pass`,
  `omit`, `retire`, `cancel`, `fail`, a lease's expiry, `withdraw`), then the schedule lock where it
  takes one, then rows, leases before their jobs (`cancel_in` and `withdraw` now end the leases first, in
  their own statement; `rederive` already did). So no two of them interleave (the race of 7: a
  withdrawal between two statements of a publish's re-derive), and a cycle needs a transaction without
  the publish lock: a worker's lease call (`held`: its lease, then its job, in one statement), a claim
  of a day block or tape (the build's row shared, then a job SKIP LOCKED), evidence (its job), `admit`
  (the build's row). None of these waits for a row a holder of the publish lock takes before a row it
  holds: holders lock a job only after its lease, and the worker locks the lease first; a claim skips
  locked jobs and only shares the build row, which only `withdraw` updates, after the publish lock and
  before any lease. The cycle of 8 (retire: job then lease; withdraw: lease then job) is gone both ways:
  they serialize on the publish lock, and both lock leases first. `admit` takes no lock: it only makes a
  build run, which no statement of a re-derive can be caught out by.
- **`retire` records only the jobs it cancelled (9):** `cancel_in` returns them; a job whose worker
  finished in between is neither cancelled nor named.
- **A withdrawal is stamped with the clock (11):** `withdrawn_at`, its leases' `ended_at` and its jobs'
  `finished_at` use `clock_timestamp()`, taken after the locks it waited for, as a publish and a claim
  stamp theirs.
- **An ordinary day changes nothing in the measurements:** core computes nothing; no base is left
  without its day-local revision, so nothing is superseded; on the cluster every pass's build registers
  the profiles it reads, so every pass is claimed when it was; a lease records where it began; a
  `fail`, an expiry, a `cancel` and a withdrawal may wait for a publish under way (seconds to a minute)
  before they run. Notices, the digest and the reader's causes change only in the cases above.
- **Not verified on real data:** none of this has run on the cluster. The tests use the real slice of
  2025-12-10 and the engine's messages as its source writes them (`stop: prune after <date>: …`,
  `diskguard: stop, … GB free (minimum … GB)`); the day-local profile revision of an earlier build in
  test 2 is a name made for the test. Tests (`core-db/tests/work.rs`):
  `a_day_whose_block_failed_after_its_base_is_missing_for_the_passes` (amended),
  `a_day_block_cancelled_after_its_base_leaves_no_base_to_read`,
  `a_day_local_revision_the_pass_does_not_read_makes_a_missing_day`,
  `an_omission_no_longer_current_leaves_a_day_still_to_come`, `a_stop_that_names_no_day_is_counted`,
  `a_refund_needs_a_day_done_under_the_same_lease`, `a_retirement_token_covers_the_later_history_it_takes_down`,
  `a_refused_upload_from_an_ended_lease_is_the_lease_gone`,
  `a_withdrawal_waits_for_the_publish_lock_and_a_retirement_beside_it_ends`,
  `a_retirement_names_only_the_jobs_it_cancelled`, `a_withdrawal_is_stamped_when_it_ends_its_leases`.
- **The engine side of 10** (the worker's fallback day) is being fixed separately; core no longer
  depends on it.
- **Amended 2026-11-22 (audit prebuild round 2 of core-watch, `round2/core-watch.md`, 1 and 3).**
  *A lease is granted and expires on one clock (1).* C-064's stamps put `granted_at` on
  `clock_timestamp()` but `expires_at` stayed `now() +` the lease (the transaction's start), so an
  ordered pass's claim that waited for the publish lock longer than the lease handed the worker a
  lease already expired (PostgreSQL 16 documentation, "Current Date/Time": `now()` is the transaction's start,
  `clock_timestamp()` the moment of the call); its attempt was spent for nothing and repeated waits
  could leave the pass FAILED, holding later passes. Both are now one `clock_timestamp()` read when
  the lease is written; a heartbeat renews from the clock too (it may wait for the lease's lock
  behind a publish). *A claim holds the queue's control row (3, C-025).* The claimable view reads
  `paused` from each statement's snapshot, so a pause committed after the claim's statement began
  still let it lease, stamped after the pause. The claim now takes `core.queue_control` FOR SHARE
  (after the publish lock for an ordered pass, before the build's row) and hands out nothing if
  paused; `pause` locks the row FOR UPDATE before it stamps `changed_at` on the clock (an UPDATE
  computes its new row before it waits for a locker), so every lease is granted before the pause or
  refused by it. **Lock order** (7, 8 above): holders of the publish lock take it first, as before;
  a claim then takes the control row shared, then the build's row shared, then a job SKIP LOCKED. A
  pause runs in its own transaction (the API's refusal, `queue pause`), holding nothing a claim
  waits for, so it only waits. Ordinary day: no change (a lease is granted and expires as it was,
  to the millisecond; the queue is not paused). Tests: `core-db/tests/work.rs`
  `a_claim_that_waited_for_the_publish_lock_gets_a_live_lease` (3 s behind the lock, a 2 s lease is
  live when it comes back), `a_pause_during_a_claim_hands_out_no_lease_after_it`.

## C-065 The watch takes over the operator's health checks; the claim no longer loses its race; a progress channel (owner, 2026-11-22; MILESTONES "Before the full build" items 1 and 3)
The test block's tapes needed an operator watching `deploy/k8s/ops/health.sh`. Three of its checks
move into core; the fourth (spend) cannot. Migration 0042.
- **The claim race (item 1).** With 24 tape pods claiming at once about half got "no job to claim"
  while 195 were claimable. The claim was `UPDATE … WHERE job_id = (SELECT … LIMIT 1 FOR UPDATE SKIP
  LOCKED) AND state = 'QUEUED'`. Under READ COMMITTED a row another claim leased and committed after
  this statement's snapshot is locked at its newest version and re-checked against the locking scan's
  own conditions (PostgreSQL's EvalPlanQual, "UPDATE, DELETE, SELECT FOR UPDATE … the second updater
  … re-evaluate its WHERE clause using the updated version of the row", docs §13.2.1, as known; not
  re-read online). The inner scan's only conditions were the job id's membership in the view (read
  from the snapshot) and the priority, so the leased row passed, and the outer `state = 'QUEUED'`
  dropped it: the claim returned nothing. **Fix:** `j.state = 'QUEUED'` in the inner scan, so the
  re-check fails and the scan goes on to the next claimable row. Then, if nothing was leased while the
  view still shows work, the claim looks again with a fresh statement (`work::CLAIM_TRIES`, 3: a row
  that became claimable after the snapshot, a claim rolled back); work still claimable after that is
  kept as a miss (`core.claim_miss`: kind, build, worker, daily, how many claimable). **Measured:**
  `claims_at_once_each_lease_a_job_while_any_is_claimable` (1,759 real calendar sessions, one tape job
  each; 8 rounds of 32 claims at once, then 32 claims over the last 10): before the fix 3 of 3 runs
  failed in round 0 or 1 (30 or 31 of 32 leased); after, 4 of 4 with the inner condition alone, every
  claim leasing a distinct job, exactly 10 of the last 10, no job twice. The test needed the long
  queue: over 147 jobs the statement was too quick for the race to show. Lock order unchanged (C-064).
- **Watch problems (item 3, C-050's kinds; one alert, one "Resolved", never a ping):**
  - `STALLED` (key `queue`): claimable work (`core.claimable_job`, so not while paused or with no
    admitted build) and no live lease for **15 minutes** (the owner's number: a node's start and a
    pod's claim take minutes, a Job out of completions or KEDA not starting pods never ends). The
    clock is the later of when the watch first saw it (`core.stall_seen`, one row) and the last lease
    granted or ended, so pods that claim and die keep restarting it (their failures are
    `JOB_FAILED`'s). **30 minutes when every claimable job is a daily run's:** CronJob `daily` starts
    at 07:45 New York, 15 minutes after the 07:30 queueing (C-051), and its node has to start; 15
    would alert every morning until KEDA replaces the CronJob. Resolves when a lease is live.
    **Amended 2026-11-22 (C-068; audit opsurface ops.md 8 and 5): per kind, and the chain.** One
    live lease of any kind cleared the clock, so a day block renewing its lease for a day hid tapes
    or passes with no pod (Pending for a node, or failing before they claim: a Secret key missing, a
    login refused), and work all waiting on something nothing would do was no stall. Now one problem
    per kind (key `TAPE`, `DAY_BLOCK`, `ORDERED_PASS`): claimable work of the kind and no live lease
    of the kind, the clock per kind (`core.stall_seen` keyed by kind, migration 0049). And the chain
    (key `chain`): nothing claimable, no live lease, the queue not paused and jobs of a running build
    queued, for 15 minutes; not while a pass of a running build stands FAILED (it holds the later
    passes and is `JOB_FAILED`, one alert for one cause). The claimability view is now read every
    minute unless every kind holds a lease (it was skipped whenever any did); KEDA reads it four
    times every 30 s already.
  - `CLAIMS_EMPTY` (key `claims`): **at least 3 misses in 10 minutes** while work of their kind and
    build (and priority, for a daily worker) is **still claimable at the check**, open while any
    recurs with the work still there. The race made about 12 a minute. A miss whose work others were
    leasing at that instant (counted claimable by the claim, leased a moment later) is not claimable
    at the check, so it opens nothing; after the fix a real one means pods leave while work waits.
    Misses are kept 7 days.
  - **Spend against the ceiling: not in core.** Core's own data (leases) cannot see a node that runs
    without a worker, the costliest failure (8 idle nodes on 2026-11-22), nor the core node, the
    volume or Spaces; DigitalOcean's month-to-date usage needs a token, which core does not hold
    (owner's rule). It stays the operator's (`health.sh`) and item 4's hard caps.
  - Not built here, as asked: idle worker nodes (the autoscaler's, item 2). Left in `health.sh`:
    pod errors' last lines and day-worker memory (worker logs, which core does not hold; a failure
    reaches core as `fail`/evidence, an OOM kill as an expired lease).
- **The progress channel (owner, 2026-11-22).** `VYREON_CORE_PROGRESS_WEBHOOK_FILE` (optional;
  Secret `vyreon-core-progress-webhook`, key `webhook`, mounted as the alerts webhook is); unset or
  absent, nothing is composed or posted for it. Every **30 minutes** (`progress::WINDOW_MINUTES`) core
  composes the window since the last from its own rows and queues the posts with the window
  (`core.progress_window`), in one transaction: each day published (its day-local revision, which
  completes a block's day): slots, with a value, withheld whole by reason (provider input, engine
  fault: C-050's convention, `revision_tally`), its reference check if one has come (else "not in
  yet"; checks arriving later get their own line), the minutes the day took in its block (from the
  previous day's publication under the lease, or the lease's grant), the block and its worker; each
  block started (lease granted) with its worker and attempt; **every line core logged** in the window
  (blocks done, now with the block's minutes from its first lease; days set aside with cause; dates
  omitted; builds admitted or withdrawn; retirements; pauses); the queue's counts by kind (done in the
  window, done of all, running, waiting, failed; builds that run; tape verify jobs left out). A window
  with nothing done posts nothing (recorded, so the cadence holds). **Cap:** each post at most 1,900
  characters, at most 3 a window, the rest counted with `vyreon-core log 100`. `vyreon-core progress`
  prints the last 30 minutes as the channel would. Not included: worker memory (worker logs), spend.
- **Delivery.** `core.notice.channel` (`alerts` | `progress`); each channel has its own sender task,
  webhook and order (`notice::next_on`, `send_unsent_on`): a progress post that fails stops only the
  progress sender and never holds back or repeats an alert; the same silent payload (`flags: 4096`,
  `allowed_mentions: {parse: []}`), `wait=true`, unknown-outcome window and retry as C-050's. Progress
  posts not delivered within **6 hours** are marked not posted (a backlog after an outage is history
  the log keeps). Composing runs in its own task, so counting a window's bodies never delays the watch.
- **An ordinary day:** the claim leases the same job it did (the earliest claimable); with one pod,
  nothing differs. Nothing new is posted to the alerts channel unless work stalls or pods leave while
  it waits; the block-done log line gains " in N min". With the progress webhook set, about one post
  each half hour while blocks run (a day block's two days, its start and finish, the counts), nothing
  while idle. Measurements, bodies and the reader API are unchanged (core computes nothing).
- **Not verified on the cluster:** the race fix on 24 real pods, KEDA's interaction with STALLED, the
  progress channel's real posts (its webhook's silent test post answered 204, MILESTONES).
- Tests: `core-db/tests/work.rs` `claims_at_once_each_lease_a_job_while_any_is_claimable`,
  `a_claim_that_finds_only_locked_work_is_kept_as_a_miss`,
  `the_progress_channel_says_what_moved_every_half_hour` (the real slice's 2025-12-10 published under a
  block's lease); `core-db/tests/watch.rs` `claimable_work_with_no_worker_for_a_quarter_hour_is_a_stall`,
  `workers_leaving_while_work_waits_are_a_problem`; `core-api/tests/notices.rs`
  `progress_posts_go_their_own_way_and_never_hold_an_alert`; unit `progress::tests` (the day line on the
  B0 census report's real counts, the split and cap). `a_last_failure_and_a_provider_burst_…` now has a
  worker claim the retried blocks (else they would be a stall, correctly).
- **Amended 2026-11-22: prebuild audit round 2 of this branch** (`round2/core-watch.md`, a486c53, 16
  findings, a retention note, a suspicion; each re-verified against the code before it was fixed,
  all real; 1 and 3 are C-064's amendment above). Migrations 0043–0045.
  - *A claim misses only work no other claim holds (6, 8).* The claim counted rows other claims held
    as claimable and kept a miss after its tries, relying on the watch's later count; that count
    could read an uncommitted winner's job as queued, or a different job queued since, and alert on
    healthy concurrency (6); and a row released by a rolled-back claim between the last try and the
    count made a miss and a 204 while the job waited (8). Now, after a try leases nothing, the claim
    looks for a claimable job FOR KEY SHARE SKIP LOCKED (which skips exactly the rows claims hold
    FOR UPDATE): none, and the answer is "nothing", true as of that look, with no miss; one, and it
    tries again holding it. Only when a try fails again while it holds such a job, after
    `CLAIM_TRIES`, is a miss kept, naming the job (`core.claim_miss.job_id`, 0043). The watch counts
    a miss only while that same job is claimable. A miss now means a pod left while work no claim
    was taking waited. (The residual: a claim in flight that rolls back after another claim's
    look; that worker answered truly and exits; the job is the next claim's, and STALLED if none
    comes.)
  - *The alert's text is the same for the same rows (16):* misses ordered by count, then kind,
    build and daily.
  - *The stall's clock starts no earlier than the work became claimable (7):* the later of the
    watch's first sight, the last lease granted or ended, the queue's last resume
    (`queue_control.changed_at`, now stamped on the clock), the oldest claimable job's queueing and
    the earliest admission of a build with claimable work, so a watch restart over an overnight
    pause does not alert on the resumed queue at once. *The suspicion:* a live lease is checked
    first, and the claimability view is read only when none is live (the check holds the problem
    lock a publish opening NOT_DERIVED waits for). Its duration at phase-1 size is not measured.
  - *Progress binds a reference check to both revisions it checked (2, C-054 amended 2026-11-20)*
    and states reference errors beside the verdict, never as agreement (10): "reference check agrees
    (91,066 comparisons; 9 reference errors not judged)". A report that comes in for revisions no
    longer published says so ("(revisions B and L, since replaced)").
  - *What published a day is recorded (11):* a revision published under a lease records it in the
    same transaction (`core.lease_revision`, 0044; none by hand). A day line names a block and its
    minutes only from that lease (from its previous day-local revision, or its grant); a day
    published by hand has neither. Revisions published before 0044 have no row.
  - *Done in the window (12):* a job DONE now and finished in the window, or one whose lease ended
    DONE in it (an ordered pass queued again by a later publish has no `finished_at`).
  - *An event committed after its window is in the next (9):* events are stamped inside their
    transactions (a pause notice with its transaction's start; a publish before its commit), so one
    could commit after the window covering its time was composed, and the next left it out by its
    time. A window now records what it stated (`core.progress_shown`, 0045) and takes every event not
    yet stated from `STALE_HOURS` (6) before its start, never before the first window's start.
  - *Retention:* windows, what they stated, and progress posts done with (delivered or given up)
    are kept 7 days (`progress::KEEP_DAYS`, as claim misses); the newest window stays whatever its
    age (the next starts at its end).
  - *Secrets (4, 5).* A setting that cannot be read is named, never printed: the webhook files, the
    connection string's file, any numeric or date setting (the webhook URL put in the setting itself
    by mistake was printed whole). A claim takes a worker name only as a host name: 1 to 63 letters,
    digits, '.', '_' or '-', beginning with a letter or digit (an RFC 1123 label's length; the
    engine's worker names itself `VYREON_WORKER_NAME` or its host name, the cluster's pods
    `v-block-1102-0-wqlv9`); else 422, the name not repeated. So no URL, `key=value`, header or
    spaced text reaches the lease, a notice, Discord or `vyreon-core progress`.
  - *Delivery (13, 14, 15).* The sender takes a pooled connection for each database step, never
    across a post or a rate limit's wait, so a slow webhook holds no connection the other sender or
    the watch needs. The six-hour cutoff is checked before each post, not once a batch (a post can
    still go out up to one post's time, 20 s plus a rate-limit wait, after it). A line longer than a
    post is cut to fit with the cut counted ("… (N characters cut)"), no post is the header alone,
    and the sender's own cut is counted the same way.
  - **An ordinary day:** measurements, bodies and the reader API unchanged (core computes nothing). A
    lease's expiry is the same to the millisecond. Claims lease the same jobs; a claim that finds only
    work other claims are taking answers at once instead of after three looks, and keeps no miss.
    The watch reads the claimability view only when no lease is live. Progress lines read as before,
    except a day line's check gains "; N reference errors not judged" when the suite had any, and a
    day published by hand names no block. Each window writes a few rows to `core.progress_shown`;
    rows older than a week go.
  - **Not verified on the cluster:** none of this has run there; the claim's look under 24 real
    pods; the watch's duration at phase-1 size. Tests: `core-db/tests/work.rs`
    `a_claim_misses_only_work_no_other_claim_holds` (was `…_kept_as_a_miss`),
    `progress_states_a_check_only_of_the_revisions_it_checked` (the B0 account, with 9 reference
    errors as build 48d87154's suite had on 11-02), `progress_credits_a_day_only_to_the_lease_that_published_it`,
    `a_pass_done_then_queued_again_counts_as_done_in_its_window`,
    `an_event_committed_after_its_window_is_in_the_next_and_windows_are_kept_a_week`;
    `core-db/tests/watch.rs` `workers_leaving_while_work_waits_are_a_problem` (amended),
    `a_stall_counts_from_when_the_queue_was_resumed`; `core-api/tests/notices.rs`
    `a_slow_progress_webhook_holds_no_connection_and_posts_nothing_past_its_deadline`;
    `core-api/tests/worker_api.rs` (a URL as a worker's name: 422); unit `work::tests`,
    `progress::tests::a_line_longer_than_a_post_is_cut_and_counted`, `vyreon-core` `tests::a_setting_that_cannot_be_read_is_named_never_printed`.

## C-066 KEDA starts the workers from the queue; pool `day` autoscales; caps instead of judgement (2026-11-22; MILESTONES "Before the full build" items 2 and 4)
The test block's tapes needed an operator to size Jobs, count completions and delete idle nodes
(8 idle nodes on 2026-11-22). Written, not applied: `deploy/k8s/50-keda.yaml`, migration 0046,
runbook `deploy/k8s/README.md` "Workers from the queue".
- **One ScaledJob per kind, and the daily block apart.** `tape`, `day-block`, `ordered-pass`, and
  `day-block-daily`: a backfill of 12 blocks of about 35 hours each can hold every slot for a day
  and more, and the CronJob gave the daily session a node of its own; its pod claims daily work
  only (`VYREON_WORKER_DAILY` "0"), and `day-block` counts only the other blocks, so the two never
  start a pod each for one job. Tapes and passes count every priority (the daily tape is minutes;
  the daily pass waits for the backfill's passes by the claim's rule, C-051).
- **The trigger counts what a worker of the build can claim:** `core.claimable_count(kind, build,
  daily)` over `core.claimable_job` with the claim's own filters (`work::claim_as`). Per build,
  because workers claim only their own build's jobs: another admitted build's claimable work (today
  914ffed7's ordered passes) would start pods that find nothing. A function, SECURITY DEFINER with a
  fixed search_path, executable only by the group `vyreon_scaler`: the view calls invoker-rights
  functions, which PostgreSQL checks as the caller (CREATE VIEW, Notes), so SELECT on the view would
  need SELECT on the store's tables. The existing `vyreon_ordered_pass` login reads the published
  store and not `core`; it is not reused. Its own login `vyreon_keda`; Secret `vyreon-scaler-pg` in
  `vyreon` (a namespaced TriggerAuthentication reads its own namespace: KEDA 2.21
  `resolver.getTriggerAuthSpec`). The owner of the migrations cannot create roles: the bootstrap
  does (`10-postgres.yaml`), and on this cluster a superuser once, before the image with 0046.
- **One pod per claimable job.** `targetQueryValue` 1, `scalingStrategy: accurate` (KEDA 2.21
  `scale_jobs.go`: min(count - pending Jobs, cap - unfinished Jobs)): a claimed job leaves the count,
  and a Job is pending until its pod runs, so a pod waiting minutes for a node holds back every
  later poll. The residual: a poll in the moment between a pod starting and its claim (0.9 s on
  2026-11-22's day pod) starts one extra pod, which finds nothing (no miss, C-065) and exits.
  `pollingInterval` 30 s.
- **Each Job one pod, `backoffLimit: 0`:** the job goes back to core's queue on failure and the next
  poll starts a pod for it; core's attempts are the retry (C-035), and a Kubernetes retry beside
  them would start a second pod for one job. C-040 holds: no Job-wide limit over several pods.
  `activeDeadlineSeconds` (tape 2 h, block 72 h, daily block 23 h, pass 6 h) so a stuck pod that still
  heartbeats cannot hold a node for ever; a stopped block resumes at its next session.
- **`rollout.strategy: gradual`:** KEDA's default deletes running Jobs when a ScaledJob changes.
  Jobs are owned by their ScaledJob: deleting one (or KEDA's CRDs) deletes its running Jobs (runbook).
- **Caps (item 4):** tapes 24 (3 a node, 8 nodes: the test block's tape run); blocks 12 (the owner's
  rule, nodes only while the request budget has headroom: one node averaged 101 requests a minute
  with bursts of 500-760, core grants at most 1,900, so 12 average about 64% and their bursts wait in
  core's budget); daily block 1; pass 1 (C-046). Pool `day`: autoscaled, min 0, max 13: the hard
  cost cap, $2.17 an hour. Pods name pool `day` (pool `day-intel` carries the same label and taint; amended 2026-11-22: `day-std` replaced, its size no longer offered in nyc1).
- **Scale to zero:** DigitalOcean's autoscaling page (updated 2026-10-29) supports a pool at 0; the
  doctl `node-pool create` reference still says "Scale-to-zero is not supported". The API's answer
  and the first run decide (runbook). The autoscaler's template for an empty `day` pool (read from
  the API) carries the pool's label, node-pool label and taint, so a pending worker pod brings up a
  node.
- **CronJob `daily` (C-051) is superseded** once KEDA runs the daily build: both would start pods for
  the same session (and the CronJob's first pod waits 30 minutes for nothing); KEDA starts nothing on
  an empty morning. Kept as the fallback. C-065's 30-minute STALLED allowance for daily-only work
  was the CronJob's gap; 15 would do under KEDA (not changed).
- **KEDA:** 2.21.1, the release manifest applied with `kubectl apply --server-side` (no Helm),
  pinned by its SHA-256; tested on Kubernetes 1.34-1.36 (KEDA's compatibility table; the cluster
  runs 1.36.5); three Deployments of 100m CPU and 100 MiB requested each on the core node.
- **Not verified:** everything on the cluster (the runbook's step 10 is the proof: the test block
  recomputed by KEDA alone); `sslmode=require` against CloudNativePG's server certificate (the
  workers connect without TLS today); KEDA's real memory on the core node; Postgres's cost of the
  four counts every 30 s (the view's per-pass functions over ~600 queued passes).
- **Amended 2026-11-22 (C-068; audit opsurface ops.md 5, 6, 15 and the pass suspicion):** pods
  prefer pool `day` and may run on `day-std` (the Intel fallback, node affinity); the daily block
  has PriorityClass `vyreon-daily` (preempts) above `vyreon-backfill-block` (preempts nothing) above
  tapes and passes; the ordered pass has a 6Gi memory limit and a 2 h deadline; KEDA 2.21 has no
  limit on a ScaledJob's failed Jobs, so a pod failing before it claims is seen as its kind's
  `STALLED`; CronJob `daily` is applied suspended. See C-068.
- Tests: `core-db/tests/work.rs` `the_scaler_counts_what_a_worker_of_the_build_can_claim_and_reads_nothing_else`
  (the count equals `work::status`'s claimable per kind at each step on the real slice, per build,
  daily apart, zero when paused; the login cannot read `core.claimable_job`, `core.job`,
  `vyreon.day_revision` or call `core.build_runs`). The manifest checked against KEDA 2.21.1's CRD
  schemas (no unknown fields).

## C-067 Backups by CloudNativePG's Barman Cloud plugin to a bucket of their own; the volume to 250 GiB (owner, 2026-11-17, 11-21 and 11-22)
Owner: "given that recomputation has a known time and money cost its actually WORTH backing shit
up" (11-17); as soon as the test block is stored (11-21); with the KEDA rollout, before the full
build, and the volume grown 120 -> 250 GB (11-22). Written, not applied: `10-postgres.yaml`,
`15-backup.yaml`, `ops/backup-now.yaml`, `ops/restore-drill.yaml`, `ops/drill-counts.sql`, runbook
`deploy/k8s/README.md` "Backups and the volume".
- **The plugin, not the in-tree `barmanObjectStore`.** The cluster runs CloudNativePG 1.30.1; its
  docs deprecate native Barman Cloud since 1.26 and remove it from 1.32 ("You must migrate to the
  plugin before upgrading to 1.32"; https://cloudnative-pg.io/docs/1.30/appendixes/backup_barmanobjectstore).
  Plugin v0.16.0 needs CloudNativePG >= 1.30 and cert-manager (v1.21.3, Kubernetes 1.33-1.36),
  both pinned by SHA-256. The cost: cert-manager on the core node, kept for the plugin's 90-day
  certificates. The in-tree path would work until 1.31 and then need a migration, a second
  Postgres restart.
- **What:** continuous WAL archiving (gzip) and a full base backup each Sunday 09:00 UTC (02:00 MT,
  no daily session queued), retention a 30-day recovery window (about six fulls held), to
  `s3://vyreon-pg-backup/` at `https://nyc3.digitaloceanspaces.com`, path-style as core's archive
  (C-052). A bucket used for nothing else, a limited Read/Write/Delete key for it alone (the
  retention deletes); Secret `vyreon-backup-spaces` made by hand from files. Not `vyreon-data`:
  core never deletes there (C-052), the backup key must delete, and a key that can delete backups
  should not reach the perishables (nor core's key the backups). The
  boto3 checksum variables (`when_required`) the plugin documents for S3-compatible stores are set.
- **The volume:** 250 GiB, `VYREON_CORE_VOLUME_GIB` with it. Online: StorageClass
  `do-block-storage` allows expansion, `resizeInUseVolumes` is on, and 60 -> 120 GiB on 2026-11-16
  ran without restarting Postgres. Applied with a patch of the size alone, before the plugin.
- **One restart:** adding the plugin recreates the Postgres pod (a rolling update; one instance,
  so a minute or two down). Done with the queue paused and no worker running, best inside the KEDA
  rollout. Core is not restarted by it (readiness, C-053).
- **The drill:** a temporary cluster `vyreon-pg-drill` in the same namespace, recovered from the
  latest base backup and all WAL (no archiving of its own, so it never writes to the bucket); per
  month day revisions by state and measurement rows with an XOR digest of their body hashes,
  diffed against the source frozen by `queue pause`; then deleted. About $0 and 30-45 minutes at
  today's ~8 GB; a `day` node and an hour or two at ~150 GB.
- **Cost:** volume +$13 a month. Spaces: the $5 subscription (250 GiB, 1 TiB out) is already paid
  for `vyreon-data` and shared; today's backups (~25-50 GB) fit; after phase 1 (~150 GB, ~0.5-1.2
  TB held) about $5-20 a month beyond it at $0.02 a GiB. Measured after the first week.
- **Not verified:** everything on the cluster (the drill is the proof); Spaces' answers to
  barman-cloud through boto3 (the checksum variables are the documented workaround, not tested
  here); the sidecar's CPU during the weekly gzip of ~150 GB on the 4-vCPU core node; the backup
  sizes above. **Open:** a failing archive keeps WAL in `pg_wal`, which `DATABASE_FULL`
  (`pg_database_size`) does not count; until the watch reads `pg_stat_archiver`, the runbook's
  check is manual.
- **Amended 2026-11-22 (C-068; audit opsurface ops.md 2): the watch reads the WAL and the backups.**
  The open item above is closed: `DATABASE_FULL` counts every database and `pg_wal`; `WAL_ARCHIVING`
  and `BACKUP_STALE` are new problems; core's login holds `pg_monitor` (granted once by a
  superuser, README B0). See C-068.

## C-068 The operational failure-surface audit (2026-11-22; audit `opsurface/ops.md`, core 550e76e, engine 090d7aea)
The owner's audit of the build's operation before the recompute (MILESTONES "Before the recompute"):
scale, KEDA and the autoscaler, external dependencies, ordering over the full range, recovery by the
owner from the runbooks. Every real finding fixed; finding 14 is the engine's. Migration 0047.
- **1 and 4: a build's unfinished work is handed to its successor, once (`build hand-over`).** The
  rollout withdraws 914ffed7, which holds the 597 ordered passes of the whole range and, after the
  test block, any queued tape or block. Withdrawn, its queued passes were claimed by no worker (KEDA
  counts the successor's jobs only, C-066), and its queued tapes and blocks held the successor for
  ever: a block claims only when no earlier tape is unfinished, a pass only when no day block before
  it is unfinished, and a schedule over them is refused as an overlap. `vyreon-core build hand-over
  (--preview | --expect <token>) <from> <to> <reason>`: with `from` withdrawn (or never admitted) and
  `to` admitted, every unfinished job of `from` (QUEUED or FAILED, not a tape verify job, which
  holds nothing back) is cancelled and the same work queued under `to` with fresh attempts and the
  old priority and scope: a tape and a pass for their session, a block from its next session (the
  days it did become a DONE job of `from`, as `requeue` keeps them when a day is set aside). A
  FAILED job is carried too: 0038 makes a withdrawn build's ended block a day still to come, so a
  set-aside day left under the old build would hold the successor's passes; the new image is its
  next attempt. One transaction under the publish and scheduler locks; `--preview` rolls it back and
  prints every job with a token over the old jobs, their states and sessions; the hand-over takes
  `--expect <token>` and is refused if anything moved since (C-063's rule for `days retire`). Once:
  afterwards `from` holds nothing unfinished and a second hand-over is refused. Recorded in
  `core.hand_over` and `core.hand_over_job` (old job, new job) and the log. C-062's replacement of a
  not-running build's pass on schedule is unchanged and agrees with it (an ordered pass is one per
  session, 0032). **The claim's gates are not relaxed for withdrawn builds:** a block must not run
  before every tape it reads (an old build's unfinished tape is a tape cache not yet stored), and a
  pass must not run before the block that computes an earlier day; ignoring the old jobs would
  publish history that skipped days still to come (C-037 would derive it again, at the cost of
  every later pass) or withhold for a missing tape. The hand-over makes the successor own the work
  explicitly instead. Test `a_withdrawn_builds_unfinished_work_is_handed_to_its_successor_once`;
  the command line `the_hand_over_command_line_is_read_whole_before_anything_moves`.
- **13:** C-035 amended (above): a lost lease is free twice per session.
- **8, and 5's third part:** C-065 amended (`STALLED` per kind, and the chain). A pod that fails
  before it claims never charges an attempt, so its kind's stall is how the channel sees it. Test
  `a_stall_is_per_kind_and_a_waiting_chain_is_one`.
- **9: the key alarm clears only on work that calls the provider.** `PROVIDER_AUTH` (a 401 or 403 in
  a stop's evidence) cleared on any revision published or any job finished after it, an ordered pass
  among them (no provider call) and a revision of a worker started before the refusal (which proves
  nothing of the key now). It now clears only when a tape or day-block lease granted after the
  refusal (its worker read the key file after it) finishes, or publishes a day's base under that
  lease (`core.lease_revision`, 0044). Test `a_refused_key_is_cleared_only_by_work_that_calls_the_provider`.
- **10: `JOB_FAILED` names the final lease's cause**, its evidence or the lease's own failure, as the
  reader does (`core.job_failure_cause`): the newest evidence of any attempt named an earlier
  attempt's provider error when the final worker stopped without evidence. Test
  `a_final_failure_is_named_by_its_final_lease`.
- **2: WAL archiving, the WAL's size and the base backups are watched** (C-067 amended; migration
  0050; `watch::storage`, `watch::storage_found`). With the volume set (production): `DATABASE_FULL`
  is every database plus `pg_wal` against the volume (a failing archive grows `pg_wal`, which
  `pg_database_size` never counted). `WAL_ARCHIVING` (key `archiver`) when `pg_stat_archiver`'s last
  failure is after its last success and nothing was archived for `ARCHIVE_STALE_MINUTES` (15), or a
  finished WAL file (`.ready` in `pg_ls_archive_statusdir`) has waited 30 minutes; "no success for N
  minutes" alone would alert on an idle database, which closes a segment only when it writes. The
  15: CloudNativePG's `archive_timeout` is 5 minutes, so three segments missed; a plain default.
  `BACKUP_STALE` when the newest base backup is older than `VYREON_CORE_BACKUP_MAX_DAYS` (8: the
  weekly schedule plus a day) or none is recorded. Read from the server itself, no cloud credential
  in core: `pg_backup_stop` writes a backup history file (`<segment>.<offset>.backup`) into `pg_wal`
  and removes the older ones once archived (PostgreSQL 16 `do_pg_backup_stop`, `CleanupBackupHistory`,
  as known; not re-read online), so the newest one's time is the newest base backup's end; that the
  plugin's backups (barman-cloud-backup over the backup API) leave one is to be seen on the first
  backup (README B8). The volume's free space as the filesystem says is not readable from SQL
  without a superuser program; the sum of every database and the WAL is what the volume holds, less
  the filesystem's own overhead, which the 20% margin covers. `pg_ls_waldir` and
  `pg_ls_archive_statusdir` need `pg_monitor`: granted to `vyreon` by the bootstrap
  (`10-postgres.yaml`) and once by hand on this cluster (README B0); without it the watch says so
  (`WAL_ARCHIVING` key `unreadable`) and judges the volume on the data alone, never silently. Tests
  `a_failing_archive_and_a_stale_backup_are_problems_before_the_volume_fills` (the logic on the 250 GiB
  volume) and `the_watch_reads_the_wal_the_archiver_and_the_base_backups` (the readings on a real
  server). Not verified: the plugin's backup history file and the archiver's statistics under the
  plugin (both at B7/B8).
- **12: the request ledger states its limits and the grants never released.** `vyreon-core budget
  [hours]` (`budget::ledger`, `budget::unreleased`): the busiest minutes with the requests reported
  (booked at release) and the requests of leases granted then and never released (a dead worker, a
  lost release), and the count of such leases. The runbook's scaling criterion (README "The caps")
  now reads the two together as an upper bound, beside Intrinio's own usage. The budget's rule is
  unchanged (an unreleased lease is held at its full size). Test `the_ledger_shows_the_grants_never_released`.
- **Suspicion (the core node under 12 publishers): an upload holds no connection while it spools.**
  `POST /worker/v1/days` took a pool connection before its body arrived and kept it through the
  upload (minutes for ~200 MB) and the wait for the bundle before it: twelve publishing nodes held
  twelve of the pool's sixteen. It now takes one to name its caller, returns it, and takes one again
  only to publish (one at a time, under the publish mutex) or to pause on a refused bundle. The pool
  is 32 (PostgreSQL's limit is CloudNativePG's default 100): 24 tape pods, 13 blocks and a pass
  heartbeat and draw from the budget, each a statement or two. Test
  `an_upload_holds_no_connection_while_its_body_arrives` (a pool of one; fails on the old handler).
  Not measured: the core node's memory with Postgres, KEDA and core under twelve publishers (the
  recompute on two nodes, then the full build, are the measurement).
- **3 and 11: the operator's scripts fail closed.** `ops/do.sh` treats any answer but 2xx as an error
  (exit 1, the status and the start of DigitalOcean's message on stderr; the token goes to curl as a
  header file, not its command line). `ops/delete-idle.sh` deletes nothing and exits 1 when the worker
  pods or DigitalOcean's node list cannot be read (an expired kubeconfig no longer reads as "no busy
  node"); it never deletes a node with an unfinished worker pod, drains a node with only finished
  ones, skips the drain only on a node with no worker pod, leaves a node under 15 minutes old or not
  running (it may be coming up for a pending pod), and reports every refused deletion (exit 1).
  `ops/health.sh` says "spend: unreadable" on an error answer or an answer without the figure, never
  $0.00, and says when the pods or core cannot be read. Checked with stand-in `curl`, `kubectl` and
  DigitalOcean answers (a 401, a pool list of four nodes, one refused deletion), not on the account.
- **5: startup failures.** The runbook's `vyreon-worker` Secret has `fred-key` (the tape and day
  pods mount it), with a check of the keys' names; `doctl registry docker-config` loses the
  `--read-only` flag doctl does not have (read-only is its default). KEDA 2.21.1's ScaledJob has no
  backoff and no limit on failed Jobs (its CRD's spec fields read from the release manifest:
  pollingInterval, min/maxReplicaCount, the two history limits, rollout, scalingStrategy, triggers,
  envSourceContainerName, jobTargetRef), and a Job already runs one pod with no retry, so a pod that
  fails before it claims is replaced every poll while its job is claimable, bounded by the cap and
  the nodes. It is seen as that kind's `STALLED` (8), on `ops/health.sh` (failed Jobs per ScaledJob
  in the last hour, pods stuck before running) and in `failedJobsHistoryLimit`'s 30 kept Jobs;
  stopped by pausing the ScaledJob (runbook). A missing Secret key leaves the pod in
  `CreateContainerConfigError`, pending, which KEDA counts, so that case makes no loop.
- **6: the AMD stockout and the daily node.** Pods prefer pool `day` and allow `day-std` (required
  affinity on the pool label, preferred `day`). The autoscaler backs a pool off after a failed
  scale-up and grows another that fits; which one it grows when both can is its expander's, which
  DigitalOcean does not document: checked on the first runs. `day-std` autoscaled 0..13 in the
  runbook; nodes follow KEDA's capped pods, so about 13 between the pools in a build, and 26 only if
  KEDA's caps failed (a lower `day-std` maximum is the owner's call). The daily block: a
  PriorityClass, not a cap of 11 backfill blocks, because the cap reserves a Job slot and not the
  6 CPUs (tapes of 2 and passes of 1 could still fill the room); `vyreon-daily` (1000) preempts the
  lowest pods on one node (a tape or a pass, whose lease then expires, free of charge), a backfill
  block (`vyreon-backfill-block`, 100, preempts nothing) only when nothing lower stands anywhere.
  The daily tape is claimed ahead of the backfill's (priority 1); the daily block still waits for
  every unfinished earlier tape, rightly (it reads the tape history before its day): at most the
  backfill's tape phase, under 2 hours at the test block's rate.
- **7: production recovery is in the runbook** (README "Backups and the volume" D,
  `ops/recover-production.yaml`): the workers and core stopped, a new Cluster `vyreon-pg2` recovered
  from `vyreon-pg`'s backups and archiving under its own server name from the start (the plugin will
  not archive into a path holding another server's WAL, and the old history stays recoverable),
  core, KEDA and the pass's login repointed to `vyreon-pg2-rw`, the schedule and a base backup on the
  new cluster, a check before the ScaledJobs resume, then the new names committed. The drill stays
  a drill. Not verified: CloudNativePG's handling of the recovered owner's password (the runbook sets
  it if core cannot log in), and the whole procedure (a second drill run as D, on a copy, is the way
  to prove it: the owner's call, about an hour and a 250 GiB volume for it).
- **15: CronJob `daily` is applied suspended** (`suspend: true` in `40-daily.yaml`): applied as it
  was, it started a second worker beside KEDA's every morning. MILESTONES' launch line and the
  runbook's daily step say so.
- **The ordered pass's memory and time (suspicion):** a 6Gi limit (a pass beside a day block is
  killed by its own limit, not the block by the node's OOM killer; six times the ~1 GB estimate) and
  a 2 h deadline (seconds on the test block's 10 days), with the measurement plan in README "The
  caps": `memory.peak` and duration on the recompute and at ~100, 300 and 597 sessions; raise before
  going on if the peak passes 4 GiB or a pass 30 minutes. Five lost leases on one session leave it
  FAILED (C-035 amended), and later passes wait.

**Amended 2026-11-22 (audit round 2 of C-068, `opsurface/round2-core.md`).**
- **1: the successor is admitted after the hand-over, never before.** Between a withdrawal and a
  hand-over, an admitted successor's pod could claim a later day's pass (its own, under the claim's
  rule a withdrawn build's earlier pass holds nothing) and publish that day's history one prior
  reading short (`eligible_63 = 62`), visible to readers until the earlier day's pass republished
  it. Structurally safe order: `build hand-over` refuses a successor that is admitted or holds a
  live lease, and withdraws the old build itself, in its own transaction, when it still runs; the
  successor is admitted after it. No worker of either build runs between the withdrawal and the
  moment every moved job is the successor's. Runbook "Workers from the queue" 4 states the order.
- **6: a pass is carried only where the successor can run it** (C-064). A day published (by any
  build) in profile revisions the successor does not read is a day its pass cannot claim and that
  holds every later pass; the hand-over refuses while such a day has no day block (the old build's
  carried, or the successor's) to compute it again, naming the days. Test
  `a_hand_over_carries_no_pass_its_successor_cannot_run`.
- **11: the old build's tape verify jobs are cancelled, not carried** (recorded with no new job): a
  sample of the old build's caches, which held a schedule of the successor's tapes refused as an
  overlap.
- **7: the daily runs read their build again under the scheduler's lock**, which a hand-over holds:
  a tick that read the build admitted before a hand-over no longer queues its sessions under the
  withdrawn build (and marks them handled). Test
  `a_tick_waiting_on_a_hand_over_queues_nothing_under_the_withdrawn_build`.
- **13: a daily session's blocks are a stall group of their own** (`DAILY_BLOCK`, 30 minutes as
  before): they have their own ScaledJob, and twelve healthy backfill blocks' leases hid a broken
  `day-block-daily` for hours. Migration 0049's clock admits the group.
- **14: the chain counts only work that waits on something.** An ordered pass whose day is neither
  published for its build nor covered by an unfinished day block (a missing day whose block was
  cancelled, which holds nothing under C-062, or a day not scheduled yet) is not counted; a failed
  pass of a running build drops the passes from the count (they are `JOB_FAILED`'s) but no longer
  hides a day block or tape stuck behind a withdrawn build's tape. Test
  `a_stall_is_per_kind_and_a_waiting_chain_is_one` (both cases, and the daily group).
- **15: a tape clears the key alarm only if it stored a cache.** A replacement tape worker whose
  session's caches were all held already finishes without a request; its `DONE` lease no longer
  counts. A tape lease must have stored a tape cache (a body or the provider's absence) between its
  grant and its end. Test `a_refused_key_is_cleared_only_by_work_that_calls_the_provider`.
- **2, 3 and 16: the backups are judged on the plugin's own record, and nothing resolves without
  evidence** (replaces the backup history file above; migration 0051, `core.backup_evidence`;
  `core_api::backups`). Two of PostgreSQL's signs were false evidence: CloudNativePG 1.30.1 answers
  every WAL file archived when no archiver is configured (so `archived_count` grows and `.ready`
  clears with WAL going nowhere), and `pg_backup_stop` writes its history file before Barman has
  uploaded the label and written `backup.info` (a failed backup looked fresh). Core now reads,
  every 5 minutes, the database's Cluster (`spec.plugins`: a Barman Cloud WAL archiver naming an
  ObjectStore) and that ObjectStore's `status.serverRecoveryWindow.<server>`
  (`firstRecoverabilityPoint`, `lastSuccessfulBackupTime`, `lastFailedBackupTime`: the plugin's
  record of what is recoverable from the bucket; field names from plugin-barman-cloud v0.16.0's CRD)
  through the Kubernetes API, as its own ServiceAccount whose Role may only `get` those two kinds
  (`20-core.yaml`). The watch alerts on the absence of evidence: no record, a failed read or one
  older than 15 minutes is `BACKUP_STALE` "Backups unverified" (with why); no archiver is
  `WAL_ARCHIVING` "archiving is off"; no recovery window or a last success older than
  `VYREON_CORE_BACKUP_MAX_DAYS` is `BACKUP_STALE`. While the record cannot be read, an open "archiving
  is off" stays open; while the WAL cannot be read (no `pg_monitor`), an open `DATABASE_FULL` stays
  open ("cannot be judged"). `pg_stat_archiver`'s failures and waiting files still raise
  `WAL_ARCHIVING` (they are real failures when the plugin archives). The runbook's B7 proof is the
  WAL file in the bucket, not PostgreSQL's count. Tests
  `the_backups_are_judged_on_the_plugins_record_alone`, `the_backup_evidence_is_the_plugins_record`,
  `a_failing_archive_is_a_problem_before_the_volume_fills`. Not verified: the plugin keys its
  window by the server name (the B8 check), and the API read on the cluster.
- **17: the ledger's bound is a real one.** Usage booked at release (`core.request_usage`) splits one
  minute's requests across two when releases straddle its end, so "reported plus unreleased per
  minute" was no upper bound. `vyreon-core budget` now books each lease by its grant minute (used if
  released, its full size if not) and states "at most" for a minute as its and the previous minute's
  leases: a lease's requests are made within 35 s of its grant. Test
  `the_ledger_shows_the_grants_never_released`.
- **4: `delete-idle.sh` never deletes a node with a worker pod still to finish, and when in doubt
  deletes nothing.** A node is a candidate only if this kubeconfig's cluster shows it with the droplet
  id DigitalOcean records for it (another cluster's kubeconfig matches none: nothing deleted, exit 1).
  A candidate is cordoned before its pods are listed again, so no worker can be scheduled onto it
  between the check and the deletion; a pod not finished, or a listing that fails, uncordons it and
  leaves it; drain is skipped only with no worker pod at all. Checked with stand-in `kubectl` and
  DigitalOcean answers (a running pod, a finished one, none, a young node, a pod arriving at the
  cordon, another cluster's node list), not on the account.
- **19: `health.sh` says when the node list cannot be read**, never "0 worker, 0 other".
- **8: the recompute's command names the build the ScaledJobs run** (README "Workers from the queue"
  10), and `vyreon-core queue` lists jobs by kind, state **and build** (`work::status`), so who owns
  what is checked before KEDA is expected to start anything.
- **5 and 12: recovery.** A schedule made after the last archived WAL is lost without trace in the
  store: D5 compares `vyreon-core queue` (by build) with the schedules MILESTONES records and makes
  again what is missing. `ops/recover-production.yaml` takes the new Cluster's name and the lost
  server's on the command line (placeholders), and D repoints from the current names, so a second
  loss recovers from the current server's history, not the first one's.
- **10: the daily block never evicts a backfill block.** `vyreon-backfill-block` now has the daily
  class's value (1000) and preempts nothing; preemption takes only lower priorities, so the daily
  pod evicts tapes or passes where that makes room and otherwise waits for the autoscaler's node.
  In a stockout it waits (the deadline alert says so); the 6-CPU argument for a class over a cap of
  11 stands.
- **18: the cost bound is the pools' maxima.** With every cap working, tapes of later dates beside
  12 earlier blocks can hold up to 21 nodes (a tape does not fit beside a block), so README states
  `day` 13 plus `day-std`'s maximum as the cap that binds (13: $4.34 an hour; 6: $3.17; 0: no
  fallback).
- **Suspicion 20: the pass's memory request equals its limit, 3Gi.** A block (10Gi request) and a
  pass then fit the node's 13.3 GiB allocatable as scheduled, and under memory pressure the kubelet
  evicts pods above their requests first, never the pass; the block's own 14Gi limit above its
  request is as before. The measurement plan raises it at a 2 GiB peak; above 3.2Gi a pass no longer
  fits beside a block and takes a node of its own (a cost).


## C-069 The watch's failing part is a problem on the channel, and stops only itself; `/live` needs a completed tick (2026-11-23; audit `2026-11-23-core-gemini-trial`, Codex 8, Gemini 7, BLOCKS)
A failing `watch::tick` still refreshed `/live` (stamped before the tick, whatever its outcome) and
returned before the digest, so one persistent error (a read on `core.daily_session`, say) stopped the
daily queueing, every alert and the digest together, with the error in the service's log alone.
Migration 0052. Commits d46602f (code), 86b8f06 (tests).
- **Each part of the check runs on its own.** The check's thirteen parts (deadlines, the queue's
  pause, failed jobs, the provider, stalls, empty claims, the daily runs, the archive, reference
  checks, the volume, backups, the calendar's end, days not derived) each run in a savepoint of the
  check's one transaction (PostgreSQL 16 manual, SAVEPOINT / ROLLBACK TO SAVEPOINT). A part that
  fails is rolled back alone and becomes a `WATCH_FAILING` problem keyed by the part, its summary
  the error (one alert, restated each minute without a post, resolved when the part runs again).
  The open problems of the part's own kinds stay as they were: a part that could not look resolves
  nothing. The other parts run as before.
- **The tick's own parts likewise:** the check as a whole (its transaction, the problem lock, the
  open list), the digest and the weekly check each run whatever another's outcome and are recorded
  the same way, the check's outcome before the digest is composed (so a digest says what holds
  after it). A check that could not run still posts no digest (round w, 13) and is itself on the
  channel. A failed daily queueing is said in `DAILY_STOPPED` with its error, no longer "the
  service's log says why"; if the daily runs' check fails too, its `WATCH_FAILING` carries both
  errors.
- **`/live` is stamped after a tick completes**, never before: its checks ran and every failure is
  recorded. A watch that hangs, cannot run its checks (the check's transaction or the recording
  fails), or has ended (a panic) leaves `/live` behind and Kubernetes restarts core after
  `LIVE_WITHIN` (15 min) plus the probe's three failures. A failing part does not fail `/live`: a
  restart cures no failing query and would stop serving the workers, and the part is on the channel.
  Amends C-053 (round x, 5): a database down for longer than 15 minutes now restarts core too (the
  watch cannot tick without it); the new pod waits in its migrate step until the database answers,
  and nothing could be served meanwhile. A Postgres restart (a minute or two) stays the readiness
  probe's.
- **Not done here (Gemini 7's other two parts, below the brief):** the six background tasks' join
  handles are still dropped (the watch's own end is now caught by `/live`; the sender's, the sweeper's
  and the backups reader's are not), and one post Discord refuses with a 4xx is retried every 15 s
  ahead of every later post. Both are open for the next batch.
- Ordinary day: no change (no part fails; the same posts). Tests:
  `watch.rs::a_failing_part_of_the_check_stops_only_itself`, `a_failed_check_posts_no_digest`
  (now: the failed check is posted); `daily.rs::a_daily_table_that_cannot_be_read_is_said_and_the_other_checks_still_run`,
  `a_failed_queueing_is_a_problem_before_the_digest`;
  `notices.rs::a_core_whose_watch_cannot_check_is_not_live`.

## C-070 One PUBLISHED revision per layer of a day; a day block's day is done with both its bundles (2026-11-23; audit `2026-11-23-core-gemini-trial`, Gemini 1, BLOCKS; Codex 1, Gemini 2 and 11, latent)
- **A revision replaces the day's revision of its layer whatever its profile revision (Gemini 1).**
  Supersession was keyed on (date, profile revision), and 0002's unique index too: a build that
  bumped the base, day-local or history profile revision left the old revision PUBLISHED beside the
  new, and readers, the external checks, the digest's tallies and `core.base_without_day_local`
  read both. Not hit in production yet (phase 1 runs one set of profile revisions), but a derived
  recompute under a new history profile (MILESTONES Terms, "One pass") would hit it on every day.
  `publish::promote` now supersedes, in the publish's transaction, the day's PUBLISHED revision of
  the same profile and layer (base: `base_revision IS NULL`; day-local; history), as well as the
  same profile revision as before; what read it falls as after any republish (C-037). Migration
  0053 checks the store (a day with two PUBLISHED revisions of one layer stops it, naming at most
  20, since core does not choose which to keep), then adds the partial unique index
  `day_revision_one_published_layer (represented_date, profile_id, (base_revision IS NULL),
  day_local) WHERE state = 'PUBLISHED'`. 0002's index stays. Readers filter `state = 'PUBLISHED'`,
  so they serve one revision per layer with no change of their own. Commits 3c016c3, cfadcf1.
- **A day block's day is done only with its day-local revision (Codex 1, Gemini 2 and 11).**
  `work::day_done` checked the block's base only, and retry recognition (`published_before`) checked
  the job's last session but not its first: a stale bundle of another job's day of the same build
  was answered `already_published`, and a day could be done on a bare base that later passes read
  (C-064's error). `day_done` now refuses until a PUBLISHED day-local revision stands on the block's
  base ("published its base but no day-local revision on it"), and a bundle dated before the lease's
  first session goes to the full checks and is refused. The deployed worker already sends base,
  day-local, then done (vyreon-cli `day.rs` `publish_day` refuses any other order), so it is never
  refused by this. Commits 69c4a39, 7fc6818 (the archive, archive API and reference-check tests'
  blocks now publish their day-local revision before `day_done`, as the worker does).
- Tests: `publish.rs::a_bumped_profile_revision_replaces_its_layer_and_readers_serve_one`,
  `the_migration_names_a_day_with_two_published_revisions_of_one_layer`;
  `work.rs::the_ordered_pass_follows_the_published_days_one_at_a_time` (done refused on a base alone),
  `a_day_before_the_job_is_not_answered_as_already_published`.

### Rollout of C-069 and C-070 on a live core (phase 1 running)
1. **Check first (reads only):** `kubectl -n vyreon exec -i vyreon-pg-1 -c postgres -- psql -d vyreon
   -v ON_ERROR_STOP=1 < deploy/k8s/ops/precheck-c070.sql`. Check 1 (two PUBLISHED revisions of one
   layer) must print no row: 0053 stops on any, and core's migrate step then keeps core from
   starting (Recreate: the old pod is already gone), which stops the build. A row: retire that day
   and compute it again before deploying. Check 2 (bases without a day-local revision) should list
   only days a leased block is on now (`block_on_it` true); a row with `false` is a day already done
   on its base alone: retire it and compute it again.
2. **Deploy** the image with both containers (`kubectl set image ... core= migrate=`) as for
   C-065. Migrations apply in order, each in its own transaction: 0052 (problem kind, instant), then
   0053 (the check, then the index over `vyreon.day_revision`, a few thousand rows: milliseconds).
   Nothing else changes in the store; no worker or engine change is needed.
3. **Mid-build safety:** running workers are unaffected by the new rules (they send both bundles
   before done, and never a day before their job). Core's restart is the risk, as for any core
   deploy: a publish in flight is rolled back and retried by the worker after 15 s and 30 s (45 s in
   all); `day_done` is retried for about 75 s. Keep the restart short (the image pushed and its
   pull checked first) and avoid a moment when a block's upload is due; a publish that outlasts the
   restart can fail that day's attempt, which the queue retries (hours of a block's compute).
4. **After:** `/live` answers 200 within a minute; `vyreon-core problems` shows no `WATCH_FAILING`.
   **No image rollback** past 0052/0053 without undoing them by hand (an older binary refuses a
   database that holds a migration it lacks).

## C-071 The digest counts revisions in committed batches, a minute's budget at a time (2026-11-23; triage `2026-11-23-triage-r2`, C4 = G1, BLOCKS the ordered passes)
The 09:00 digest counted every revision of the last 24 hours not counted yet in one statement
(`INSERT … SELECT` over their bodies). Progress counts only bases and day-local revisions, so the
ordered passes' history revisions (one per session, ~597 in phase 1, within hours) would all land
there. A statement outliving `/live`'s 15 minutes (C-069) restarts core; the statement rolls back,
and the next minute starts it again: a restart loop. Pre-existing (not a C-069 regression); its
duration at phase-1 size was not measured.
- **Batches.** `watch::tally_batches` counts `TALLY_BATCH` (25, the triage's figure) revisions per
  statement, each committed on its own (the watch's client is outside a transaction). A batch is
  whole or absent (`ON CONFLICT DO NOTHING` on `revision_tally`'s key), so a kill loses at most the
  batch running, nothing is counted twice, and the next call goes on from there. Every layer is
  counted alike (base, day-local, history).
- **A minute's budget.** The digest and the weekly check count (`watch::tally_untallied`) for at
  most `TALLY_BUDGET` (5 minutes) a minute, one budget shared by both, stopping after the batch
  that ends past it; what is left is counted the next minute, which composes and queues the post
  once nothing is left. Composing counts only what was published since, within the same budget
  (`session_tallies` and the digest's own pass take it; past it, nothing is composed). The count
  runs after the open problems are read, as before, so a problem opened meanwhile still drops that
  composition (round v, 7; round w, 4).
- **Elsewhere:** `session_tallies` (digest and weekly check) first counts its sessions' uncounted
  revisions in batches, then reads in one statement as before (round x, 7); progress counts its
  days in the same batches (still inside its window's transaction, its own task). The CLI
  `vyreon-core digest` counts all, with no budget.
- **Ordinary day:** the same digest at 09:00 (a day's few revisions are one batch). On the passes'
  mornings the digest can come some minutes late (worst case one minute per batch: 597 revisions,
  24 batches); it never posts a partial count.
- **Round 3 (Codex 2026-11-23, `audits/2026-11-23-r3-core`, 4 items): bounded and visible.**
  1, 2. Every counting statement (the batches, their selection, and `session_tallies`' read) runs
  in its own transaction under `TALLY_LIMITS` (`SET LOCAL statement_timeout = '60s'`,
  `lock_timeout = '10s'`), well under `/live`'s 15 minutes: a batch that runs long or waits on a
  lock rolls back and is the digest's (or weekly check's) WATCH_FAILING problem (C-069), retried
  the next minute, instead of holding the tick until core is restarted. The worst tick is the
  budget plus one batch per counting call (each at most 60 s). 3. What is counted is what has no
  count (`tally_untallied`: every revision of the profile published by then, oldest first,
  `TALLY_BATCH` per transaction), not "the last 24 hours", so a later minute's moving cutoff never
  leaves one uncounted. 4. A post left for the next minute (budget spent before or during
  composing) is a part not run that minute: it neither opens nor resolves its problem; only a
  composed post resolves it. Not changed: the digest's lines still read the 24 hours to its
  minute, so a digest composed at 09:06 states nothing published 09:00 to 09:06 the day before
  (rare: only a late digest; its counts are kept).
- **Deploy:** code only, no migration, no setting. Takes effect with the next core image. The first
  digest after it counts every published revision with no count (bases and history revisions of
  any age, a bare base cheap), a few minutes a minute; how many wait:
  `SELECT count(*) FROM vyreon.day_revision d WHERE published_at IS NOT NULL AND NOT EXISTS (SELECT FROM core.revision_tally t WHERE t.revision_id = d.revision_id)`.
- Test: `watch.rs::the_digests_count_fails_within_its_limits_counts_what_is_left_and_resolves_only_on_a_digest`
  (round 3: a held count fails the tick's digest part in 10 s and the minute completes; a history
  revision published yesterday 09:02, outside the 09:30 minute's 24 hours, is counted; a minute
  out of budget leaves the problem open, the composing minute resolves it).
- Test: `watch.rs::the_digest_counts_the_ordered_passes_in_committed_batches_and_survives_a_kill`
  (597 sessions 2024-07-09..2026-11-20: the slice's real base and derived revisions of 2025-12-10
  with the engine's real faulted XLB body, re-dated as history revisions on bare bases; the count's
  connection terminated while its third batch waits, two batches kept, the rest counted once by the
  09:30 tick, each history revision 3 slots, 1 withheld for an engine fault).

## C-072 The owner's clock is fixed UTC−6; progress is terse (2026-11-24; owner's Discord; part 2, planned steps, retired the same day: see its last item)
(Amended by C-073's last item, "Both Discord channels at a glance": the progress post's shape and the alerts' effect and action lines.)
What the owner reads in Discord must answer "go look or running ok" at a glance (C-050, C-065).
Three things broke that during the hand-overs of 2026-11-23/24.
- **One owner zone, fixed UTC−6.** The owner's clock is UTC−6 all year (corrected 2026-11-11).
  Core rendered owner times and computed deadlines in tz-database `America/Edmonton`, whose meaning
  depends on the tz release a PostgreSQL image carries: older releases fall back to UTC−7 in winter
  (the cluster's "MT" ran one hour behind the owner's Discord on 2026-11-24; the laptop's tzdata
  does too), newer ones keep UTC−6 from 2026-11 (the test server's). Now `watch::ZONE` =
  `'Etc/GMT+6'`, the tz database's fixed UTC−6 zone (POSIX sign: "GMT+6" is six hours behind),
  the only definition: the watch's SQL, the progress windows, the CLI's printouts and
  `deploy/k8s/ops/health.sh` (`TZ=Etc/GMT+6`) read it; the label stays "MT". It works both ways in
  `AT TIME ZONE` (timestamptz → local for display; local → timestamptz for a deadline), checked on
  PostgreSQL 16 for January and July; a bare `'-06:00'` string does not (PostgreSQL reads it as a
  POSIX zone, UTC+6). Market times stay `America/New_York` (07:30 queueing, sessions).
  **Effect:** on a server whose tzdata says UTC−7 in winter, every deadline instant moves one hour
  earlier in winter than before (a Friday session's deadline, Saturday 20:00 MT, is 02:00 UTC
  Sunday, not 03:00), and the 09:00 digest is posted at 15:00 UTC all year. The daily deadlines start 2026-11-30.
- **Planned operator steps are progress, not alerts.** A hand-over posted 🔴 "Work queue paused"
  and 🔴 "Daily runs stopped: build … was withdrawn" to the alerts channel and resolved them minutes
  later: two alerts and two resolutions for one deliberate act. Now a pause by the operator
  (`vyreon-core queue pause`, recorded in `core.queue_control.by_operator`) and the daily runs stopped
  by a hand-over (their build withdrawn by handing its work over, or a hand-over's successor not yet
  admitted; the summary names the hand-over) are planned steps: their problem opens quiet
  (`core.problem.quiet_until` = the step's start + `PLANNED_GRACE_MINUTES`, 30), logged
  ("Planned step: … (alerted if still so at 04:58 MT)"), so the progress channel carries it, and
  resolves quietly ("Planned step: over after 12 min: …"). Still open when its grace ends, it is
  alerted once ("🔴 … (a planned step, still open after 31 min)") and resolves as any problem: a
  forgotten pause is a problem. A pause core makes itself (a job failed for the last time, a bundle
  refused) and every other problem alert at once, as before. A quiet problem is no reason to look:
  the digest says "🟡 Running; planned step under way: …", the weekly check is not red for it, the
  digest's list and `vyreon-core problems` mark it.
- **Terse progress.** A hand-over is one line: "Hand-over 26: fc36eefb → 2dde1132, 42 blocks + 593
  passes moved, 8 running jobs restarted — <reason cut to 80>", with its window's withdrawal (the
  stopped job ids are its "restarted"), its successor's admission, the operator's pause and resume
  and the planned-step lines folded in ("· queue paused 04:28 · 2dde1132 admitted 04:39 · queue
  resumed 04:41"). The blocks started are one line ("Started 8 blocks (2024): 07-09–07-12, 07-15,
  07-16–07-19 (from 07-17, attempt 2), …"; a resumed or retried block says so). Counts lines are
  kept; failures, days set aside, core's own pauses and everything else logged stay as lines.
- **Deploy:** migration 0054 (`queue_control.by_operator`, `problem.quiet_until`, progress_shown
  source `hand_over`); additive, a pause open at the migration is core's (alerts as before). Takes
  effect with the next core image; health.sh with the next checkout.
- Test: `work.rs::a_hand_over_is_progress_unless_it_overstays` (a hand-over with two running blocks:
  no alert, the digest yellow, one progress line with its steps folded; a forgotten operator pause
  quiet at 29 min, alerted at 31, once, resolved at 33; core's own pause alerted at once);
  `watch.rs::the_weeks_last_session_carries_sundays_report_and_the_owners_clock_stays_on_utc_minus_6`
  (a winter Saturday 10:00 MT deadline is 16:00 UTC); `progress.rs` unit tests of both lines.
- **Round 2 (Codex audit `2026-11-24-core-c072`, held the rollout at d728b04; all fixed before any
  deploy, so 0054 is amended in place: it was applied to no database but tests').**
  1. A bundle refused (or any pause of core's) during the operator's pause was swallowed by
     `pause`'s `WHERE NOT paused`. Now core's pause takes the operator's over: its reason first, the
     operator's after it, `by_operator` cleared, same pause (same `changed_at`, same problem), so
     the watch alerts it at once. During a pause of core's, a later reason of core's is logged
     ("Queue already paused; also: …"), never dropped.
  2. A successor left unadmitted was silent: the hand-over line keeps the instruction ("· build X
     is not admitted: admit it now (`vyreon-core build admit X <evidence.json>`)") while it does
     not run, and queued work of a hand-over's successor that does not run is a STALLED problem
     (key `successor <build>`) once `PLANNED_GRACE_MINUTES` have passed since the hand-over (the
     other stall checks count running builds' work only).
  3. Planned-ness is the hand-over's own: the daily runs' stop is planned only while the build's
     current withdrawal is that hand-over's (its reason "handed over to build <to>: <reason>",
     stamped in its transaction) or the build is its never-admitted successor; a re-admitted build
     withdrawn later for its own reason alerts at once. Progress folds only the hand-over's own
     withdrawal, its successor's admission, and the operator's pause it was made in (paused before
     its notice, not resumed before it; the pause's reason kept when it is not the hand-over's),
     that pause's resume and planned-step lines; any other withdrawal or pause keeps its line.
  4. A planned problem still quiet when resolved after its grace (open at its deadline, cleared
     before a check saw it) posts its overdue alert then its resolution; one overdue and not yet
     reconciled is red in the digest and weekly check. A failed check (C-069) holds a planned
     problem as it was: its stored grace decides, the failure never alerts it early.
  5. A daily queueing error other than the stopped build's own refusal (a table that cannot be
     read) is said in DAILY_STOPPED ("; queueing failed: …") and makes it no planned step (C-069).
  6. "Restarted" counts only moved jobs (a leased tape verify job, cancelled, is not); a resumed or
     retried block start is always named in the started line, whatever its place (only plain starts
     are capped at `STARTED_MAX`).
  7. 0054 sets `SET LOCAL lock_timeout = '10s'` and `statement_timeout = '60s'`: a lock wait fails
     it fast instead of holding the core pod in `migrate` past the workers' leases. The runner
     applies a migration and its ledger row in one transaction, so a timeout rolls it back whole;
     `vyreon-core migrate` exits non-zero, the init container fails and Kubernetes retries it with
     its back-off until the lock holder is gone (core does not serve meanwhile, as before; the
     failure is now visible in the pod's status and log instead of a silent wait).
  Tests: `work.rs::a_planned_step_is_the_hand_overs_own_and_hides_no_problem` (1–5 and 3's progress
  line), `work.rs::a_hand_over_carries_no_pass_its_successor_cannot_run` (6: a leased verify job is
  not restarted), `progress.rs` unit tests (2's line, 6's retried start at place 27).
- **Round 3 (Codex audit `2026-11-24-core-c072-r2`; owner's stopping rule: findings in ordinary
  operation block, rare ones are recorded LATER).** Fixed:
  1. (blocking) A bundle refused is alerted in the refusal's own transaction
     (`watch::refused`, called by the worker API after its `work::pause`), whatever then happens to
     the pause: the pause's problem opens there with its alert when none is open (the watch's
     next minute finds it open and posts nothing again); an open quiet one (the operator's pause,
     now core's) is alerted there; one alerted long before is followed by an alert of the refusal
     itself ("… (the queue was paused already)"). An operator's resume before the watch's next
     minute, or a pause whose grace alert cleared `quiet_until`, no longer silences it.
  6. A pause folded into a hand-over's progress line drops its reason only when it is exactly the
     hand-over's reason; any other reason is printed (cut to `REASON_MAX`).
  7. A planned pause is judged against its grace at its resume (`queue_control.resumed_at`,
     migration 0055, set by `work::resume`), not at the check that sees it over: resumed at 10:29:50
     with the check at 10:30:10 is quiet; resumed past the deadline is alerted though no check saw
     it overdue. (The daily runs' planned stop has no recorded end: judged at the check, as before.)
  **LATER** (recorded, not fixed now; none occurs in ordinary operation):
  - r2-2: a second hand-over to the same unadmitted successor restarts the successor-stall grace
    (`max(handed_at)`): the first hand-over's jobs are alerted up to 30 minutes late, and a stall
    alerted before can resolve while they still wait. Needs two hand-overs to one unadmitted build.
  - r2-3: the runner takes its blocking advisory lock and reads the ledger before 0054/0055 set
    their timeouts: a second migrator stuck holding that lock keeps the pod in `migrate`. Needs a
    stuck concurrent `vyreon-core migrate` (one replica, `Recreate`).
  - r2-4: progress says "build B is not admitted: admit it now" for a successor admitted and then
    withdrawn (or one that handed its work on), where admitting it would clear a deliberate
    withdrawal. Needs a successor withdrawn within its hand-over's progress window.
  - r2-5: a standalone withdrawal whose reason is literally "handed over to build B: <r>", followed
    within the hour by a hand-over A→B with reason <r>, reads as the hand-over's own (planned).
    Needs an operator to type a hand-over's withdrawal text by hand.
  - Pre-existing publish defects (C-024/C-028/C-031/C-070), each **needs a malformed or
    nondeterministic worker bundle; the engine is deterministic**: a day-local bundle that omits
    its base fields and `day_local` is accepted by a day block as a base (supersedes the real base);
    a resend with the same listed members and body digests but one changed reader scalar is
    acknowledged `already_published` and `detail=headline` keeps serving the first value.
  Tests: `work.rs::a_planned_step_is_the_hand_overs_own_and_hides_no_problem` (refusals alerted
  before any check, during the operator's pause, with none open and during an alerted pause; a
  pause resumed within its grace and checked after it is quiet, one resumed past it is alerted),
  `work.rs::a_hand_over_is_progress_unless_it_overstays` (a pause reason other than the hand-over's
  is printed).
- **Part 2 retired (2026-11-24, before any deploy; owner's rule: prefer retiring a rule to adding
  one).** Three audit rounds (Codex `2026-11-24-core-c072`, `-r2`, `-r3`; Gemini on r2) each found
  new ways the quiet planned-step mechanism lost or duplicated a real alert: a bundle refused, or a
  job failed for the last time, during the operator's pause swallowed by the pause or by an alert
  already posted; an operator's resume before the watch's next minute; a failed check firing the
  grace alert early; a grace judged at check time or by a resume read in a race; a transport retry
  posting a refusal twice; an alert write that rolled back the pause itself. Every pause and every
  daily stop now alerts as before C-072 (removed: `queue_control.by_operator` and `resumed_at`,
  `problem.quiet_until`, `PLANNED_GRACE_MINUTES`, the overdue and planned-step texts,
  `daily::handed_over`, the refusal's alert in its transaction, the successor stall). The noise a
  hand-over makes in the alerts channel will go with the release-script automation (pause →
  hand-over → admit → resume as one operator command, MILESTONES' automation debt), not with quiet
  alerts. The Round 2 and Round 3 items above and their LATER list are moot where they concern part
  2; the two publish defects stay LATER.
  **Kept:** part 1 (the owner's zone `Etc/GMT+6`) and part 3, now without folding: a hand-over's own
  notice is its one line ("Hand-over 26: fc36eefb → 2dde1132, 42 blocks + 593 passes moved, 8
  running jobs restarted — <reason ≤80>", "· build X is not admitted: admit it now (`vyreon-core
  build admit X <evidence.json>`)" while its successor has never been admitted); restarted counts
  only moved jobs (a cancelled tape verify job is not one); the blocks started are one line, every
  resumed or retried start named. No pause, withdrawal or admission line is folded or matched
  away: each keeps its line and reason. Migration 0054 was rewritten (never deployed; the live
  database is at 0053) to hold only part 3's `progress_shown` source `hand_over`, under
  `lock_timeout` 10 s and `statement_timeout` 60 s; 0055 was deleted.
  Tests: `work.rs::a_hand_over_is_one_progress_line_and_hides_no_reason` (the line, the pause's
  reason, the withdrawal and admission lines, the started line; the pause and the daily stop alert
  as before), `work.rs::a_hand_over_carries_no_pass_its_successor_cannot_run` (a leased verify job
  is not restarted; the admit instruction), `progress.rs` unit tests.

## C-073 The automation debts ride the next release: one release command, re-runnable reference checks, a budget-aware scaler, JOB_THRASHING (owner, 2026-11-24: "if we have to restart phase 1 or otherwise produce a new image, build the automation debt bits into the new release"; budget-aware KEDA approved 2026-11-23)
Four steps done by hand or noticed only by an agent looking (AGENTS.md: a step done by hand a second
time is missing automation; an observability gap becomes a notification), from MILESTONES' "Listed,
not fixed".
- **One release command.** `deploy/k8s/ops/release.sh <engine-image-tag> "<reason>"` runs the
  README's worker release (C-068's order), done by hand 9 times on 2026-11-22/23: build identity and
  inventory from the image; push; `build register` (inventory on stdin, no `kubectl cp`); `queue
  pause`; `build hand-over --preview` then `--expect <token>` from the old build (the live
  ScaledJobs' build, which must equal `VYREON_CORE_DAILY_BUILD` where set; an old build with nothing
  to hand over is withdrawn instead); the evidence JSON (build, image digest, replaced build,
  hand-over id and token, reason, `--notes` merged) kept locally and sent to `build admit`; 50-keda.yaml
  with its placeholders; `VYREON_CORE_DAILY_BUILD` set in 20-core.yaml when the daily runs were on
  the old build and 20-core.yaml applied **with the running core image** read from the live
  Deployment (never the raw file: that took core down once), its `kubectl diff` printed first; the
  unfinished worker Jobs of other images deleted (hand-over 5's debt: a withdrawn build's pods compute
  on with no lease and hold KEDA's slots); `queue resume`; then it waits (20 min) for a lease of the
  new build. It stops at the first refusal and says which steps were done and that the queue is left
  paused (the watch alerts that); `--from N` resumes after the cause is fixed; `--dry-run` prints
  every step and runs only reads. `--core <tag>` deploys a core image the same way (push, diff,
  apply with the image, rollout, core answers). The registry credential is the DigitalOcean token
  from `VYREON_DO_TOKEN_FILE` in a temporary docker config (the registry takes the token as user
  and password, so `docker login -u` would put it on a command line any process can read); never
  printed, removed on exit. No machine path: `KUBECONFIG`, `VYREON_REGISTRY`, `VYREON_RELEASE_DIR`
  from the environment. Rehearsed against stubbed `kubectl`/`docker` (dry run, a full release, a
  refused hand-over, a core deploy); not yet run on the cluster.
- **Re-runnable reference checks (job kind REFCHECK).** A day's check (C-054) runs in its worker
  beside the next day and dies with it: hand-over 18 stopped three such workers on 2026-11-23 and
  only REFCHECK_NOT_RUN said so (the days were recomputed to get their checks). Now core's watch, in
  its reference-check part, queues one REFCHECK job (one session; `scope`: the day's base and
  day-local revision) for each published day from `VYREON_CORE_REFCHECK_FROM` whose current
  revisions have no COMPLETED report, `RERUN_AFTER_HOURS` (6; `VYREON_CORE_REFCHECK_RERUN_HOURS`,
  `off` restores the old alert) after the day was published **and** once the lease that published it
  has ended (a live worker's check may still come: it runs beside the next day and is joined after
  it, up to its 600-minute deadline). It covers a day with no report and one whose check timed out,
  crashed or did not run. For the day's own build if it runs, else the running build admitted last;
  none runs: none is queued, and the day is not run. Once per (base, day-local) pair: a unique index
  (0055) refuses a second, so a re-run that ran is never queued again and two watches never queue one
  twice; a hand-over cancels and carries it like any job. `max_attempts` 2 (after C-035's free lost
  leases); out of attempts it pauses nothing, is no JOB_FAILED (one cause, one alert: the day's
  check not run), and `queue retry-failed` retries it. REFCHECK_NOT_RUN now counts a day only when no
  re-run is pending (one QUEUED or LEASED under a running build, or none yet while the day is not
  due): a day alerts only when its re-run also failed to complete (its report TIMEOUT, CRASHED or
  NOT_RUN, or none), or no build could take it. A re-run's report is stored in `core.refcheck_report`
  as any other, under its own lease: of its job's one session, bound to its job's revisions (or to
  none, which judges nothing), of whichever build published them; storing it marks the session done,
  and the worker then finishes. A re-run that finds a disagreement is REFCHECK_DISAGREES as usual.
  - **Why it is not "from the stored body alone".** The suite (engine `tools/reference`) recomputes
    the day from its raw inputs (`h5-acq`, the captures, `h5-act01-auto`) and the surfaces the day
    computed, and compares the published values with that; the published body is what is judged,
    not an input. No worker keeps the raws (C-004: Intrinio is the store) and core keeps only the
    restatable captures (C-043). So the re-run's worker fetches the day's inputs again and computes
    the day to its surfaces without publishing it; what it judges is the body core holds, read from
    core, never its own recomputation.
  - **Core serves the dump** the suite reads, so the judged values are exactly the stored ones:
    `GET /worker/v1/refcheck/{session}/dump` (a REFCHECK lease only) answers a tar of `store.jsonl`
    (one line per slot: `profile`, `measurement_id`, `object_id`, `revision_id`, `slot_state`,
    `method_version`, `science_version`, `body_sha256`, `body_text` (the body file with its final
    newline, so its SHA-256 is `body_sha256`), `receipt`, `stream` {`root_sha256`, `events_sha256`,
    `event_count`, `byte_length`} or null), `revisions.jsonl` (the base, then the day-local revision
    with `base_revision` the base; `date`, `profile`, `state` PUBLISHED, `engine_build`,
    `snapshot_cutoff`, `slots`) and `streams/<measurement>.<object>.<events_sha256>.jsonl.gz` (the
    events as core keeps them, gzip): the engine's `refcheck::write_dump` layout, read in one
    repeatable-read snapshot, written to core's spool row by row and sent from there in 1 MiB chunks
    (never held whole in memory; the file unlinked once open); refused (422) when a revision the job
    names is no longer published.
  - **The worker (engine change, ships in the same release):** `vyreon-worker run refcheck --home
    DIR [--bin DIR] [--repo DIR]`, claiming kind `refcheck` (`POST /claims {kind: "refcheck"}`; the
    claim's `sessions` is `[T]`, its `scope` `{base_revision, day_local_revision}`, its manifest as a
    day block's), heartbeating as any job:
    1. **Inputs.** Run the day block's fill and compute for T in its own home exactly as `run
       day-block` does for a one-session block, with three differences: (a) nothing is published:
       the publish callback writes the bundles to the run directory and returns, `day_done`,
       `/archives` and `PUT /input-captures` are never called (a re-run keeps nothing; core refuses
       them under a REFCHECK lease anyway); (b) the restatable captures core keeps for T (`GET
       /worker/v1/input-captures/{T}`, then for each kind the version whose identity the dump's
       receipts bind (the capture the published day read, C-043), else the newest; `GET
       .../{T}/{kind}/{identity}`,
       a gzip tar as `pack-capture` writes it) are unpacked where the fill puts them
       (`h5-company-capture-<T>`, `h5-macro`, `h5-carry/<T>`) and the fill takes them as captured
       (company, macro and carry are fetched again only where core keeps none, and the account says
       which: `"captures": {"company": "core" | "fetched", ...}`), since a later fetch of a restatable
       source is not what the day read; (c) the other inputs (`h5-acq`, `h5-act01-auto`) are fetched
       again from the provider, through core's request budget as every fill. A perishable input the
       provider no longer serves (option prints after about five sessions) makes the check NOT_RUN
       with that reason, never a check against a partial input.
    2. **Dump.** `GET /worker/v1/refcheck/{T}/dump`, unpacked to `<run>/refcheck/dump`; the phase
       uses it in place of `write_dump` (a `refcheck::Day` with the dump given, e.g. `dump:
       Option<PathBuf>`; `bundles` empty), so the suite judges the stored body. A 422 (its revisions
       not held, or no longer published) is a NOT_RUN account with core's message as `why`.
    3. **Suite.** `refcheck::phase` as for a day block's day, not beside another day (no overlap: the
       join is immediate), `VYREON_REFCHECK_MINUTES` (600, the ScaledJob's) as its deadline,
       `REFCHECK_OWN_BUILD` = the base revision's `engine_build` from `revisions.jsonl` (so D-404's
       rulings are judged by the day's build, as `base_build` reads it).
    4. **Report.** The account as today (`VYREON-REFCHECK@1`), `revisions` = `[base_revision,
       day_local_revision]` from the claim's scope (core refuses any other pair), plus `"rerun":
       {"job_id": <job>, "inputs_fetched_at": <UTC time the fill started>}` (core keeps the document
       as sent; a disagreement found by a re-run is then read knowing its inputs were fetched later);
       `PUT /worker/v1/refcheck/{T}`; then `POST /lease/finish` (core marked the session done when it
       kept the report). Any failure before a report: `fail` with its evidence (D-351), as a day
       block; core retries once, then the day stands not run.
    5. **Clean up** the home (inputs, dump, report), as a day block does.
    The pod (ScaledJob `refcheck`, `50-keda.yaml`) is a day block's: 6 CPUs, 10Gi, the Intrinio and
    FRED keys, the backfill block's priority, 24-hour deadline, at most one. Until the engine image
    carrying `run refcheck` is on the cluster, a queued re-run has no pod: `STALLED` (kind REFCHECK)
    after 15 minutes says so.
- **A budget-aware scaler.** The caps were tuned by hand from the request ledger (tapes 24 → 12,
  blocks 12 → 8, ACT01 in flight 2 on 2026-11-23). Now every draw core answers "wait" (the ceiling,
  `VYREON_CORE_INTRINIO_PER_MINUTE`, reached) is recorded by minute (`core.budget_wait`), and KEDA's
  triggers ask `core.scaler_count(kind, build, daily)` (0055, `vyreon_scaler`'s only other
  function): `claimable_count`'s rows, except that the work that calls the provider (tapes, backfill
  day blocks, re-runs) counts nothing while a draw waited within `budget::PRESSURE_MINUTES` (5, ten
  polls). A wait means the running workers already ask for more than the ceiling: another pod would
  only wait in the budget at node cost. The daily session's work (priority 1) and the ordered passes
  (no provider call) always count; running pods are never touched (KEDA only starts fewer); the
  claim is not gated; the caps stay the hard limit. The stall check agrees: a provider kind held back
  while the budget is pressed is not STALLED (its clock starts again when it is quiet).
  `vyreon-core budget` says whether the budget is pressed. Standard: KEDA's `postgresql` scaler counts
  what its query returns (KEDA 2.21 `pkg/scalers/postgresql_scaler.go`); the gate is in the query, as
  C-066's claimability is. The waits table is small (one row a minute at most) and not pruned.
- **JOB_THRASHING.** On 2026-11-23 11 tapes were OOM-killed 12 times in an hour; each kill was a lost
  lease re-queued quietly (C-035 forgives two) and nothing alerted. Now a job still to do (QUEUED or
  LEASED) whose leases that began on the session it is still on (`lease.start_session`) ended lost
  (EXPIRED) or failed `THRASH_LEASES` (3) times within `THRASH_WINDOW_HOURS` (24: a job retried after
  its cause was fixed starts clean within a day) is a problem, one per kind of work (key: the kind, the
  daily session's blocks apart, as STALLED): one alert however many jobs, naming the job that lost
  most (then the oldest), its session and its last error (its evidence, or the stop core recorded; a
  lease that expired with none is said as "expired with no stop recorded (the pod killed: out of
  memory, its deadline, or its node lost)"), the others counted, restated without a post as they
  change. A lease stopped by the provider's errors (an `HttpStatus(429|5xx)` in its text) is
  PROVIDER_ERRORS's and not counted here (one cause, one alert); a job set aside is JOB_FAILED's; a
  job that moves on or finishes resolves it.
- **Deploy:** migration 0055 (job kind REFCHECK in `job`, `job_evidence`, `claim_miss`,
  `stall_seen`; the re-run's scope check and unique index; problem kind JOB_THRASHING;
  `core.budget_wait`; `core.scaler_count` granted to `vyreon_scaler`; `core.build_profile` (backfilled);
  `job.retried_at`; `lease.job_first`, `job_last`
  and the grant trigger setting them; `hand_over_job.restarted`; `core.lease_step`; problem kind
  NO_PROGRESS), under 0054's timeouts. Order:
  `release.sh --core <tag>` first (the triggers need 0055), then `release.sh <engine-tag>` with the
  engine image carrying `run refcheck`, within 15 minutes (a re-run queued between them for the old
  build is carried by the hand-over). 20-core.yaml sets `VYREON_CORE_REFCHECK_RERUN_HOURS` 6;
  50-keda.yaml adds ScaledJob `refcheck` (pool `day` max 13 still holds 8 blocks, the daily block and
  a re-run). The old ScaledJobs' queries (`claimable_count`) keep working until 50-keda.yaml is
  applied again.
- Tests: `tests/automation.rs`: `a_check_that_never_reported_is_run_again_by_core_and_alerts_only_if_that_fails`
  (no re-run while the publishing lease lives; queued once at 7 h, never twice; scheduling by hand
  refused; a re-run out of attempts pauses nothing and is no JOB_FAILED, the day not run; retried, pending
  again; the dump's slots, digests, streams and revisions against the store; a report bound to other
  revisions refused, kept once, the session done, finish; a timed-out re-run is the day not run),
  `the_scaler_starts_no_provider_work_while_the_budget_made_a_draw_wait` (as `vyreon_scaler`: the
  backfill's tape held back, the daily one not, claimable unchanged, the claim not gated, counted again
  after 5 quiet minutes; no STALLED while pressed, its clock restarted after),
  `a_job_that_keeps_losing_its_lease_is_one_alert_with_its_last_error` (3 losses alert once, a
  worker's stated failure becomes the last error, a provider-stopped attempt not counted, restated
  without a post, resolved when the job finishes).
- **Workers say what they are doing; NO_PROGRESS (live investigation, 2026-11-24).** The daily run sat
  idle about an hour in a deliberate EXQ01 cutoff wait, heartbeating, and nothing could tell it from a
  stall. **Contract** (the engine's `eng-rel` branch calls it): `POST /worker/v1/lease/step` under the
  lease token, JSON `{"step": "<1-200 chars>", "waiting_until": "<RFC 3339 instant>"?, "reason":
  "<1-300 chars>"?}` (a wait states both its end and its reason; its end ahead of now by at most 24 h);
  answer `{"recorded": true}`; 410 the lease is gone; 422 a malformed report, or one a credential
  pattern matches (nothing stored). Core keeps the latest per lease (`core.lease_step`); a report whose
  step or wait differs from the last starts its clock (`step_since`), the same again only says the
  worker is alive. **The worker reports** at the start of each phase (download, each compute stage, the
  publish, the archive, the reference check), and declares a wait (`waiting_until` = its expected end)
  for any phase that makes no provider request for longer than 20 minutes: a cutoff wait, a long
  computation, the reference suite; a declared wait is cleared by the next report without one. **The
  watch:** a live lease whose worker has reported a step (an image before the endpoint is not judged:
  it could not declare a long computation) with no progress for `NO_PROGRESS_MINUTES` (20): no request
  drawn or reported (`core.budget_lease`, `core.request_usage`, by the lease's worker), no new step or
  wait, nothing published under the lease, no tape cache or reference check stored, since the latest of
  those and the grant. A declared wait moves the deadline to its end plus `WAIT_GRACE_MINUTES` (15), so
  one that overstays alerts too, naming the wait. One problem per job (key `job <id>`), alerted once,
  resolved when progress resumes or the lease ends; the command is the pod's log.
- **The C-072 audits' remaining items (r2 and r4, Codex and Gemini; fixed now, before any deploy).**
  1. A block set aside on the day it resumed was named in progress by its narrowed job ("07-10
     (attempt 3)"): a lease now records its job's sessions when granted (`lease.job_first`,
     `job_last`, set with `start_session`), and the started line names the block as it started, from
     where it resumed.
  2. A hand-over already stated by a core before 0054 (which recorded only its notice) was posted
     again on the upgrade, and one whose notice and row fell either side of a window's end was posted
     twice (its raw "Admit build … now" notice, then its line): a hand-over is stated when its own
     notice has been, and the hand-overs a window's logged notices name are taken whatever their
     `handed_at`.
  3. "Restarted" counted any earlier withdrawal's stopped leases: the hand-over now records which of
     the jobs it moved its own withdrawal stopped (`hand_over_job.restarted`; older rows as before).
  4. A successor admitted and withdrawn before the hand-over lost the instruction to admit it: the
     instruction is kept unless the successor was admitted and not withdrawn by the hand-over's time
     (one withdrawn after it, deliberately, still gets none: r2-4).
  5. A build's name went into the progress line unchecked: `build register` takes only the engine's
     kind of name (`build::build_name`: 1 to 64 letters, digits, `+ . _ -`), and a hand-over's whole
     line is checked for a credential pattern like any logged line.
  6. Many retried or resumed starts made one line the post splitter cut, their dates lost: the started
     line is cut into lines of at most `STARTED_LINE_MAX` (600), and a window records as stated only the
     events of the lines its posts carried; the rest come with the next window ("… and N more lines
     (the next window states them …)").
  7. `vyreon-core log` and `evidence list` printed UTC (a 20:00 MT pause read as 02:00 the next day):
     they print the owner's clock, "MT" (C-072's fixed UTC−6). 15-backup.yaml's comment: Sunday 09:00
     UTC is 03:00 MT.
  8. (Gemini, pre-existing; owner: such an event must alert) A bundle refused, or a job failed for the
     last time, while the queue was already paused was dropped: no log, no alert, lost at the resume.
     `work::pause` now adds the cause to the pause's reason (`\nalso: `, the first cause kept, at most
     5 later ones, the same cause again adds nothing) and alerts it at once in the same transaction
     ("🔴 Work queue already paused; also: …"), and logs it; the open QUEUE_PAUSED restates "…; also:
     <latest>" without a second post.
  9. (r2, pre-existing publish defects) A re-sent bundle was answered `already_published` from its
     header's body digests alone, so one with a changed reader scalar was acknowledged while the reader
     kept serving the first: it is now acknowledged only after every slot's files check against its
     header, the bundle ends where it says, and its reader scalars and unusual scores are bit for bit
     the stored ones; anything else is refused naming what differs. A day-local bundle stripped of its
     base fields and `day_local` was taken as a base and superseded the real one: under a lease, a
     profile revision of a build has one layer (fixed by any revision of it stored before), and a day
     block's day has one base (a second base of another profile on the same lease and day is refused).
     By hand (`publish --vouch` of old exports, a profile older than the split) only the old checks hold.
  10. (r2-3, r4) The migrator waited for another migrator's advisory lock without bound before any
     timeout applied: `migrate::apply` now waits at most `LOCK_WAIT` (60 s) for the lock and the ledger
     (`lock_timeout`, the statement's bound twice that), then fails, so the init container exits and
     is retried visibly; each migration runs without that bound.
- **Profile layers registered (owner, after the release audit's C13).** The engine's inventory
  declares each profile revision's layer: `"profiles": {"VYREON-RESEARCH-PROFILE-S64-3.0.0": "base",
  "VYREON-DLM-DAY-PROFILE-1.0.0": "day-local", "VYREON-DLM-HISTORY-PROFILE-1.0.0": "history"}` (engine
  change for eng-rel: `bundle::inventory` adds it from `vyreon_sensors::BASE_PROFILE`,
  `DAY_DERIVED_PROFILE`, `HISTORY_DERIVED_PROFILE`). `build register` keeps it in `core.build_profile`
  (every slot's profile declared, no other, at most one base and one day-local; a re-registration that
  declares another layer is refused); release.sh refuses an inventory without it. Under a lease a
  bundle presented as another layer than its profile's is refused before anything is stored. A build
  registered before the declaration has its layers from what it published (0055 backfills them; an
  undeclared profile's first publish fixes its layer). Test:
  `release_audit.rs::a_bundles_layer_is_its_registered_profiles` (a declared build's first day: the
  stripped day-local bundle refused with nothing published, then the real base and day-local accepted).
  The C-072 LATER items r2-2 (second hand-over to one successor) and r2-5 (a hand-typed withdrawal
  text) were part 2's and are moot since its retirement.
- **The release audit (2026-11-24, `audits/2026-11-24-release/core`, Codex 19 + 1 suspicion, Gemini 14 +
  2 suspicions; the only audit of this release, owner's rule: every real finding fixed and tested).**
  | Finding | Verdict |
  |---|---|
  | C1 release.sh resumes a pause it did not make | fixed: refuses to start over another's pause; resumes only if the pause is exactly its own (a cause added while it ran leaves it paused and stops) |
  | C2 admission without validation evidence | fixed: `--evidence` required, with non-empty `tests` and `checks`, kept under `validation` |
  | C3, G5.1 a superseded revision occupies the re-run pod | fixed: stale re-runs (revisions superseded, or a build that does not run) cancelled and the current pair queued; the dump refuses a revision no longer published |
  | C4, G1.1 budget pressure hides a stalled daily tape | fixed: under pressure only the backfill's work is set aside; the daily session's still stalls (DAILY_STALL_MINUTES) |
  | G1.2 pressure restarts the backfill's stall clock | not a defect: the backfill's provider work is held back by design while pressed (KEDA starts none); its stall is judged once the budget is quiet |
  | C5, C7, G14.2 `--from` loses state, partial ScaledJob apply | fixed: a run file per build (old build, hand-over id and token, its own pause), written after each step |
  | C6, G14.1 one lease hides a disabled ScaledJob | fixed: every ScaledJob checked (build, image, no `autoscaling.keda.sh/paused*`, Ready); step 11 waits for a claim of every kind `core.scaler_count` says KEDA should start |
  | C8, G4, G9.1 re-runs said as ordered passes, left out of hand-over lines | fixed: finished lines name their kind; hand-over lines count re-runs |
  | C9, G8.3 a dry run writes release records | fixed: a dry run writes to a temporary directory, removed; it diffs the daily-build change on a copy |
  | C10 the CLI's wait count includes older waits | fixed: counts only whole minutes in the window ("at least") |
  | C11 a changed body from the same lease supersedes the first | fixed: a lease publishes a day's revision of one profile once; a different one is refused |
  | C12, G13.2 a resend with other receipt, coordinates, state, stream or cutoff acknowledged | fixed: every stored field compared before `already_published` |
  | C13, G13.1 a stripped day-local bundle on another lease replaces the base | fixed generally (owner): a bundle's layer is what its registered profile is (`core.build_profile`: the inventory's declared `profiles`, or for a build registered before the declaration what each profile was first published as, backfilled by 0055), checked under every lease before anything is stored, a build's first day included; a second base of another profile for the day is refused too |
  | C14, G2.1 a re-run queued under a build being handed over | fixed: queueing takes the scheduler's lock (tried, never waited for) and a re-run under a build that does not run is cancelled and queued again |
  | G2.2 a re-run queued between a hand-over's preview and expect | fixed: nothing is queued while the queue is paused |
  | C15 empty hand-over fallback strands work queued meanwhile | fixed: after withdrawing, the preview runs again and hands over what appeared |
  | C16 a reused core tag reports a deployment that never ran | fixed: a tag the Deployment runs is refused; the pod's image digest must be the pushed one |
  | C17 a base-only completion covers a later day-local revision | fixed: completion is pair-exact |
  | C18, G10 another job's tape cache resets NO_PROGRESS; verify jobs not credited | fixed: tape caches and verdicts count for tape jobs only; tape checks (by lease), archives (by job and worker) and captures (day blocks) count; all bounded at now |
  | C19 `kubectl diff` errors ignored | fixed: exit 2 and above stops before any apply |
  | C20 (suspicion) re-run takes the newest capture | real in the contract's wording: the engine reads the capture version the dump's receipts bind, else the newest; C-073's contract now says exactly that |
  | G3.1 a lease that reported a non-completed check blocks the re-run for its whole block | fixed: a live lease holds the day back only until it has reported the day's check |
  | G3.2 pending keyed on the base alone, not bounded by `REFCHECK_FROM` | fixed: pair-keyed and bounded |
  | G3.3 a base without its day-local revision gets a re-run | fixed: only a complete day (C-070) is due |
  | G5.2 another build's re-run refuses a hand-over | fixed: re-runs are left out of the overlap refusal (one per revision pair already) |
  | G6 JOB_THRASHING after `retry-failed`, or of a withdrawn build | fixed: leases after `job.retried_at` only, running builds only |
  | G7 the registry credential left on INT/TERM/HUP | fixed: traps exit through the cleanup |
  | G8.1 step 9's delete races KEDA's cleanup | fixed: `--ignore-not-found` |
  | G8.2 a failure at step 10 omits the pause warning | fixed: the warning follows the run's own pause, whatever the step |
  | G9.2 day lines use the narrowed block | fixed: the lease's block span |
  | G9.3 kinds ordered nondeterministically | fixed: the kind breaks ties |
  | G11 -0 refused on an identical resend | fixed: values compared with -0 as 0 |
  | G12 step reports: errors as 422, loose instants, skew drops the step | fixed: strict RFC 3339 (never echoed), a past wait kept, database errors as 500 |
  | G suspicion 1 the dump in memory | real by construction: streamed through the spool (above) |
  | G suspicion 2 `/dev/stdin` under the pod's user | plausible: `vyreon-core build register` and `build admit` read `-` from fd 0, the script uses it |
  Tests: `tests/release_audit.rs` (5), the API test `a_reruns_dump_is_streamed_and_leaves_nothing`, the
  updated automation and worker API tests, `progress.rs` unit tests; release.sh rehearsed against
  stubbed `kubectl`/`docker` for each of its findings (another's pause, a cause added mid-run, a failed
  hand-over resumed with `--from 5`, a paused ScaledJob then `--from 7`, a diff error, a reused core
  tag, a dry run that keeps nothing, an interrupt during the push). The suite: 211 (193 + 18).
- Tests (added): `tests/automation.rs::a_worker_making_no_progress_is_one_alert_unless_it_declared_its_wait`;
  `tests/c072_later.rs` (a set-aside block named as it started; a hand-over stated once across a window's
  end and an upgrade, no restart from an earlier stop, the admit instruction kept; a cause during an
  open pause alerted once and the log on the owner's clock; a re-sent bundle with a changed scalar
  refused and a stripped day-local bundle refused as a base; an odd build name refused);
  `tests/migrate.rs::a_runner_waits_for_anothers_lock_only_so_long`; `progress.rs` unit test
  `many_retries_read_in_lines_a_post_can_hold`; the worker API test calls `/lease/step`. The suite:
  204 (193 + 11).
- **Both Discord channels at a glance (owner, 2026-11-24; display only, amends C-065, C-072).** The
  progress post is a headline ("🟢 Phase 1: 4 of 597 days published (0.7%) · 9 building · no failures":
  🟢 fine, 🟡 an engine fault, a retry, a check that disagrees or a failure to watch, 🔴 an alert open;
  days are the backfill's sessions with their base and day-local revision published; the label is
  `VYREON_CORE_PROGRESS_LABEL`), then "+N this half hour:" and one line per day published (date, share of
  readings with a value, engine faults ⚠, a disagreeing or incomplete reference check ⚠, hours; provider
  absences not in the line), then the other news as short lines (a retried or resumed block named; a
  pause, a resume, an admission, a withdrawal as what and why; a hand-over as C-072's one line, "· X not
  admitted yet" while its successor is not; a failure, a day set aside or a lost lease as logged), then
  the state in plain words ("History passes: 3 of 597 done · waiting for Jul 9 (they run in date order)",
  "Tapes: …", "Daily 2026-11-23: computing since 08:03"). One post a window: no "(continued)"; lines it
  cannot hold are counted and come next window. Dropped from the post, kept behind commands: pod names,
  job ids, plain block starts, a block's routine end, a check that agrees, the per-kind "0 done"
  counters. A half hour with nothing new is one line ("🟢 Phase 1: 4/597 · 9 building · nothing new");
  with no work left, no post. **Alerts** read what happened, then what it means for published numbers
  or absences, then "To do: … `command`" only when there is something to do (`watch::consequence`, one
  entry per problem kind; PROVIDER_ERRORS clears by itself and has none); a resolution keeps its
  duration. Never hidden: a failure, a stall, an engine fault. Tests: `tests/digest_format.rs` (a busy
  half hour then a quiet one, a failure with its alert, a hand-over), the progress tests of `work.rs`,
  `c072_later.rs` and the `progress.rs` unit tests rewritten to the new shape; the suite 214.
- **Hotfix: a download is its own wait; any new report is progress (live, 2026-11-24, core a6bb572,
  worker 1ae946f7).** NO_PROGRESS alerted seven day blocks and a re-run in their downloads ("no progress
  since 15:06, at step download …"): Intrinio's bulk option files (`h2-engine bulk-option-capture`) are
  no request in core's ledger, and a block's first download runs about three hours (later days about
  37 minutes). A step whose name begins with "download" or "fill" (the worker's `download <date>`, its
  mirrored `download <date>: fill <date>: …`, a fill's `fill <date>: …`) is now an implicit wait of
  `watch::DOWNLOAD_STEP_MAX_MINUTES` (240: the worst case and an hour's margin) from when it began;
  NO_PROGRESS alerts it only past that and the 15-minute grace, naming the bound. A lease lost is the
  queue's (its lease expires; STALLED, JOB_THRASHING), as before. **Contract amended:** `POST
  /worker/v1/lease/step` also takes `progress` (1-200 characters, e.g. "chains 41 of 64"); a step report
  may repeat, and any report whose step, wait, reason or progress differs from the last is progress
  (`step_since`); the same report again only says the worker is alive. Migration 0056
  (`lease_step.progress`). Test: `automation.rs::a_download_step_is_its_own_wait_and_any_new_report_is_progress`.

## C-074 Scoped recompute: a REPUBLISH job, copy-forward revisions core copies, a planner that chooses by measured cost (owner, 2026-11-24: "build scoped recompute now, while the cluster runs; we're gonna need it"; engine D-104, D-462)
A correction or a method change (MILESTONES' recompute rule: values or a method version change) used
to mean recomputing whole days. Engine D-104 recomputes only what a change touches: the seed
families or measurements and their transitive same-day dependants, every other slot of the day
copied forward byte for byte. This is that mechanism on the cluster.
- **Job kind REPUBLISH** (0057): one session, its scope the families, measurements and reason it was
  scheduled with and the recompute set (`recompute.base`, `.day_local`, `.history`, with the feeds,
  stages and input categories). One unfinished republish per day and profile (index), none over an
  unfinished day block of the day (`republish::schedule`). Claimed like any job (`vyreon-worker run
  republish`, engine D-462), heartbeating and reporting its steps and waits (C-073's NO_PROGRESS reads
  it like any lease), budget-aware: the scaler holds it back while the request budget is pressed, as
  the backfill (`core.scaler_count`, unchanged); STALLED, JOB_THRASHING and JOB_FAILED read it as any
  kind; a hand-over carries it with its recompute set composed again from the successor's plans
  (refused, naming the job, when the successor has none). ScaledJob `republish` (50-keda.yaml): a day
  block's pod, at most 4 at once in the same pools, whose maxima stay the cost cap (owner's to change);
  release.sh checks it as the others.
- **The plans** come from the build's inventory (`republish`, engine D-462: every family's and every
  measurement's recompute set, feeds, stages and categories from the engine's sensor graph, and its
  reference timings), kept in `core.build_republish` at `build register`. Core unites a scope's seeds
  exactly as the engine does (`republish::compose`; the engine pins the union to the united plan); the
  worker computes its own and refuses a difference. A build registered without plans cannot republish.
- **Copy-forward, core's side** (`publish`): only a REPUBLISH lease publishes a bundle with `republish`,
  and it publishes nothing else. The bundle carries the recomputed measurements' slots; core checks it
  against its job (the same families, measurements and reason; `recomputed` the job's set for the
  layer), against the day's published revision of the layer it `replaces` (the same profile revision,
  the same listed members, and holding exactly the build's inventory for every measurement it does not
  recompute: a measurement a new build added is recomputed, never copied from nothing) and against the
  versions (a recomputed slot whose science or method version differs from the replaced one must be
  stated in `value_changed`, engine D-273). It then writes the recomputed slots as any bundle's and
  copies every other row of the replaced revision in SQL, byte for byte: body text and JSONB, receipt,
  coordinates, state, versions, event stream, reader scalars and unusual scores, each with
  `carried_from_revision` (where it was first computed) and the revision's `republish_scope` (job,
  replaces, scope, recomputed, value_changed, reason, and the carried slots' cutoff and build). The read
  back then checks the whole revision as any day's.
- **A day is replaced whole or not at all.** A republish's base revision is written, read back and kept
  STAGED; its day-local revision (always sent after a new base: the old one read the old base) binds to
  it, and the two are published in the day-local's transaction. An attempt that ends before that (a
  failure, a lost lease) rejects its staged base (`work::requeue`; a later attempt also rejects what an
  ended lease left), so the day keeps the revisions it had. The day is done (`day_done`) only once the
  lease published the day-local revision; out of attempts, the day keeps its revisions, JOB_FAILED says
  so and nothing pauses.
- **Narrowed supersede, cascade by value** (D-104 items 3 and 4). When neither layer changed a value
  (the engine's `value_changed` empty, its versions checked by core), only the day's day-local revision
  falls with its base and is replaced in the same transaction: the day's and later days' history
  revisions stand (they read the same values). When a value changed, what read the day falls and the
  ordered pass derives it again, as after any republish (C-037). Narrower windows were considered and
  not built: DLM-UNUSUALNESS reads every reader scalar of 63 sessions and its own scores of 252, so a
  changed scalar reaches every later day within 63 sessions, and the pass's own publish supersedes
  every later history revision anyway; the passes read the store and fetch nothing. **The ordered pass
  waits for the republishes of its day and every earlier day** (0057's view, as it waits for day blocks),
  so after a range of republishes the passes run once.
- **The planner** (`vyreon-core republish <profile> <build> <scope> <first> <last> "<reason>" --plan`):
  the recompute set by layer and its share of the day's base and day-local slots, the feeds, the input
  categories and stages, the estimated node-time of the range scoped (one job a day: core's dump, the
  download less the categories not fetched where that was measured, the plan, the stages its work runs,
  intent B by the computed owners' share of its measured task-minutes, its bundles) and full (day blocks
  of `--block` days: a block's first day and its later days, whose download shares the window), and the
  choice. Measured figures: the medians of the latest 20 ordinary base revisions' run reports core
  keeps (stages, and from D-462 the task-minutes and the download by category), else the inventory's
  reference (cluster job 1423, 2026-11-21). **The rule is the measurement:** SCOPED when its estimate for
  the range is below FULL's; ORDERED PASSES when the set holds history sensors only; and two
  standing cases from priority 1 (no wrong numbers outranks cost): on a day whose EXQ01 prints the
  provider no longer serves (engine D-103), a scope that does not read them is SCOPED (a full recompute
  would publish EXQ01, M0-PRINT-SIZE and EXQ01-TEMPORAL withheld where the day holds readings; the scoped
  one carries them), and a scope that reads them has no faithful recompute: nothing is scheduled without
  `--full`. Without `--plan` the choice is scheduled (REPUBLISH jobs, day blocks, or ordered passes);
  `--scoped`/`--full` override it. Unpublished sessions in the range are named and left alone.
- **What the reference measurements give** (one day, 8 vCPU): a full day 3.8 node-hours as a block's
  first day, 2.8 as a later day. Scoped: a company sensor (CDD-FND01) 1.3 (its download counted whole
  but ACT01's until per-category download times are measured); a day-local family (DLM-CHAIN) 2.4 (its
  surfaces counted at intent A's time, their measured upper bound); M0-GAMMA-CHANGE 3.4; the price
  family P 3.7. So a day-local or company correction is SCOPED over any range; P (whose closure is
  nearly the whole day) is SCOPED alone and FULL over two days or more, where the blocks share their
  download.
- **Progress** (C-072): a republished day's line reads "Republished …" with its job, scope, reason, how
  many sensors it recomputed and which values changed; finished jobs say "republishes".
- **Owner decisions this raised** (coordinator's rulings below): (1) a republished day's current revisions have no
  completed reference check, so C-073's watch queues a REFCHECK re-run for each (a whole day's download
  and suite, about 4 to 6 hours a day); accept, or have the republish worker run the suite itself
  (it would then need the whole day's inputs); (2) the ScaledJob's 4 at once; (3) a copy-forward
  revision's `snapshot_cutoff` is its recompute's; the carried slots' own is in `republish_scope`
  (release-2 audit C7: none; each carried slot's are its `carried_from_revision`'s): the reader API states the revision's.
- **Deploy:** migration 0057 (the kind in `job`, `job_evidence`, `claim_miss`, `stall_seen`; the scope
  check and unique index; `core.build_republish`; the pass's claim waiting for republishes) before the
  engine image carrying `run republish` and the inventory's plans; `build register` of that image
  keeps its plans; 50-keda.yaml adds ScaledJob `republish`. Builds registered before keep working; they
  cannot republish.
- Tests (real slice of 2025-12-10, the engine's real plans `test_support::REPUBLISH_PLANS`):
  `republish.rs::a_scoped_recompute_is_the_full_days_slots_and_carries_every_other_byte_for_byte` (the
  plan from the engine's graph; the dump names the day's revisions; refusals: a measurement not
  recomputed, a set not the job's, another revision replaced, a version changed unsaid; the staged base
  holds every slot byte for byte with three carried; the day is not done before its day-local revision;
  both published at once; no value changed: the history revision stands and no pass is queued; a full
  recompute of the day sending the same bodies is answered `already_published` by the copy-forward
  revisions), `a_changed_value_cascades_and_a_failed_attempt_leaves_the_day_whole` (a failed attempt's
  staged base rejected, the day as it was; a value change supersedes the day's history revision; the
  pass waits for the republish; a history-only scope is ordered passes),
  `the_planner_chooses_by_the_measured_cost_of_the_range` (five scopes: SCOPED exactly when cheaper; a
  day-local family runs no intent; a company sensor fetches its capture and the daily bars; P alone and
  over three days; the prints' retention both ways; core's reports overlaying the reference), and
  `a_hand_over_carries_a_republish_on_the_successors_plans` (recomposed on the successor's plans; refused
  with nothing moved when it has none), and `progress.rs`'s republished line. 
- **Coordinator's additions (2026-11-24, same release).** (1) *The re-run of a republished day is scoped*
  (`refcheck::queue_reruns`, `SCOPED_MEASUREMENTS`): its REFCHECK job's scope names the measurements the
  republish recomputed (both layers' `recomputed`) when the revisions it replaced had a COMPLETED check
  (every carried slot was judged there); otherwise none, and the whole day is checked. The engine's suite
  then checks those measurements alone (D-462). (2) *Migration renumbered 0057* (main's 0056 is the
  NO_PROGRESS hotfix); the branch is rebased on it, and the republished day's line is written in the
  readable progress format ("… · republished (job …, [scope]: reason): N sensors recomputed, …").
  (3) *A download that reports progress is judged by it*: the hotfix's implicit 240-minute wait applies only
  to a download step with no `progress`; one that reports progress (engine D-462: at least every minute
  while it advances) alerts after 20 minutes without a new report. (4) *The first cluster run's plan*:
  `deploy/k8s/README.md` "11. The first scoped recompute" and `ops/scoped-check.sql` (validated on a
  migrated database). Tests: `republish.rs::a_republished_days_reference_rerun_checks_what_was_recomputed`,
  `a_download_reporting_its_progress_is_judged_by_it`. The suite: 223 (main's 215 + 8).
- **The release-2 audit, core half (2026-11-24, `audits/2026-11-24-release2/core`, Codex 17 + 3 rare, Gemini 16 +
  2 suspicions; the only audit of this release, owner's rule: every real finding fixed and tested).**
  | Finding | Verdict |
  |---|---|
  | C1 a requested history correction skipped | fixed: the scope's own history sensors (`seed_history`, named or a named family's) make the day's and later days' history derived again, whatever the worker's values |
  | C2, G6 prints retention only at planning; every pre-capture day "perished" | fixed: `core.prints_perished` (0057: from the prints' capture start, 2026-10-23, only); the claim FAILS a queued republish reading them once they are gone (retry and hand-over included), scheduling refuses it |
  | C3, G1 a scoped re-run clears a carried disagreement | fixed: scoped only when the replaced pair's check found nothing unexplained and no reference error; meanwhile a republished day carries its predecessors' latest check (walked back through successive republishes) into the problems |
  | C4, G2 an identical retry refused (422, pause); a worker ending after its day-local revision leased again | fixed: a republish's resent bundle (or a compare-only block's) is answered published/staged when it is that revision's recomputed part, every file and number; a job whose day-local revision is published is done on any requeue |
  | C5, G4 overlap depends on command order | fixed: a day block refuses an unfinished republish of its days (and the reverse, as before); a hand-over checks both kinds |
  | C6, G5 FULL and the passes schedule outside the plan | fixed: they schedule the plan's published sessions in runs of consecutive sessions (`Plan::runs`); FULL's cost counts those |
  | C7 carried slots' build and cutoff misstated after successive republishes | fixed: `republish_scope` states none; a carried slot's are those of `carried_from_revision`'s revision |
  | C8 request traffic hides a stopped progress counter | fixed: a download step that reports progress is judged by it alone |
  | C9 a tape-reading republish starts before its tapes | fixed: 0057's claim view holds a republish reading `consolidated` behind unfinished tape work, as a day block |
  | C10 consecutive corrections re-checked whole | fixed: the predecessor's base is the kept base when another job republished it |
  | C11, G7 a local-only correction inherits the base's age | fixed: a day is published at the later of its base and day-local revision; a day-local republish's open lease is checking |
  | C12, G11 parity SQL misses losses, rejects a correct copy | fixed: `ops/scoped-check.sql` compares both ways (full joins) per slot, stream and scalar; tested on the real slice (a correct copy and a changed recomputed stream pass, a lost scalar or slot is a row) |
  | C13 the runbook replaces the day before judging | fixed: `vyreon-core compare-day` queues a compare-only day block: its revisions staged, never published, nothing kept; the runbook compares against it |
  | C14 `--plan` omits the override | fixed: the plan prints the overridden choice; `--full` is honoured for history-only scopes, `--scoped` refused where impossible |
  | C15, G14 cancellation and withdrawal leave a staged base | fixed: `work::reject_staged` on cancel, withdrawal, retirement, as on requeue |
  | C16, G9 a failed republish said as an ordered pass | fixed: "Republish of D (its revisions stay as they were)", and JOB_FAILED's consequence says what each kind leaves |
  | C17 receipt counts mix sets | fixed: a copy-forward's slots and scalars are the whole revision's, as its streams |
  | C18 a malformed bundle binds a history revision as base | fixed: only a base-layer revision binds as a base (`bind_base`), for every bundle |
  | C19 scheduling under a withdrawn build | fixed: refused unless the build runs |
  | C20 a credential in a reason | fixed: refused before anything is stored or logged (scoped, FULL and passes) |
  | G3 profile revision changes not checked | fixed: the planner names days published in other profile revisions (FULL, a copy-forward cannot cross them); scheduling and hand-over refuse such republishes |
  | G8 retire leaves a republish | fixed: retirement cancels a republish of a retired day |
  | G10 release.sh checks 0055 only | fixed: it needs `core.build_republish` (0057) and an inventory with republish plans |
  | G12 hand-over lines omit republishes | fixed |
  | G13 measured costs: stale publish, mixed download days, replaced task-minutes | fixed: publish from the reports' stages; a category's download as its share of its reports' download times a first day's; task-minutes merged by measurement |
  | G15 a staged base of another profile or base | fixed: the staged base must be of the bundle's base profile and must have replaced the base the replaced day-local revision stood on |
  | G16 dump without a day-local revision; re-registration with other plans | fixed: the dump refuses a day without one; registration refuses other plans |
  | G S1 concurrent dumps fill the spool | partly real: dumps now build one at a time (`App::dumping`); their size is not measured (the first cluster run measures it) |
  | G S2 step names, staged `published_at` | not real: the engine's download steps begin with "download"; a staged base counts once published with its day-local revision |
  Tests: `tests/republish.rs` (`retries_overlaps_cancellation_and_counts`, `a_requested_history_correction_is_never_skipped`,
  `claims_bindings_and_scheduling_refusals`, `reference_reruns_of_republished_days`, `request_traffic_never_hides_a_stopped_progress_counter`,
  `the_scoped_check_catches_a_lost_carried_slot_and_passes_a_correct_copy`, `a_compare_only_day_block_publishes_nothing`,
  `planning_retirement_failures_and_measurement`, the planner test's prints coverage), `progress.rs`'s hand-over line,
  `digest_format.rs`'s JOB_FAILED consequence. release.sh: syntax checked, the two checks read. The suite: 231.
- **The engine half's plans (engine D-462 release-2 audit, 2026-11-24).** The engine's inventory changed, and
  core's plans with it (`test-support/fixtures/engine-republish-plans.json` regenerated from the engine
  binary): a scope reaching a day-local record that reads the option population or surface files (DLM-CHAIN,
  DLM-IV, DLM-GREEKS, DLM-EVENTS, DLM-GAMMA, and base families reaching them) now recomputes and republishes
  VS-O02, VS-O06 and VS-IVS01 with their dependants (a stateless worker holds none of the published files
  those bodies name), so such a scope costs about a day; a company-only scope fetches the quotes too;
  DLM-VWAP reads D111-TEMP01. The cheap day-local scopes are the daily-bar ones (DLM-TECHNICAL, its
  measurements). The tests use DLM-TECHNICAL as their day-local scope and check DLM-CHAIN's new base. The
  cluster test plan (deploy/k8s/README.md §11) should take DLM-TECHNICAL as its first scoped day. Suite: 231.

## C-075 A day the input plan refused is held and alerted at once (coordinator, 2026-11-24; AGENTS.md: an observability gap becomes a notification)
Live, 2024-07-23 was refused by the engine's input plan (ETHA's ACT01 gap; engine side on eng-scope's sibling
eng-etha) and core said nothing: the worker's `POST /lease/fail` (`stop: D not published: D refused by the
input plan (plan.json)`, its evidence with the plan's refusing entries stored just before) queued the job
again, and only its last attempt would have alerted, some 3 × 2-3 hours of node time later, each attempt
refused the same way.
- **Held at once.** `work::fail` records such a stop in `core.day_refusal` (migration 0058: day, job, lease,
  build, the refused inputs on one line from the evidence's `refusals`, transient, held). The job's attempts
  are spent, so it goes the way of a day out of attempts: FAILED, the day set aside, the rest of its block
  queued again, the build going on (and `FAILED_DAYS_PAUSE` as ever). A new build's hand-over (it carries
  FAILED jobs with fresh attempts) or `vyreon-core queue retry-failed` tries it again.
- **One quick retry for failed captures.** When every refused input is a capture that failed here
  retryably or was not attempted (the engine's `ENGINE_FAULT: capture failed here (<category>
  retryable|not_attempted)`, a fill that may succeed next time), the job is queued once more; the same
  day refused again by the same job is held. A provider's absence, a stale input, an engine rule or a
  fatal capture is held at once (`work::refusal_inputs`).
- **DAY_REFUSED** (watch part "refused days", alerts channel): one problem per held day, "Day D refused by
  the engine's input plan (job J, build B): <input (role session, status) assets or reason | …>", with
  what it means and what to do; resolved when a day-local revision of the day is published after the
  refusal. JOB_FAILED leaves a held refusal to it (one alert for one cause).
Tests: `tests/day_refusal.rs` (the real refused day's `close_quotes` refusal held and alerted, resolved
once the real slice of 2025-12-10 is published after `retry-failed`; a failed-capture refusal retried once
then held; the one-line inputs). `watch.rs`'s and `job_evidence.rs`'s failures that were refusals only by
their wording now use another failure (a refusal is DAY_REFUSED's). Suite: 234.

## C-076 The request ceiling holds over every rolling 60 s window, a release counted with its requests' way; a throttled carry refusal is transient (coordinator, 2026-11-24)
**Finding.** Phase 1's 2024-07-23 was refused after Intrinio answered HTTP 429 (2026-11-24 ~22:55 UTC: the
carry capture's `dividends/SPY`, company captures in the same minute; evidence 105) while the usage
ledger's calendar minutes held 1,589-1,845 of the 1,900 ceiling (Intrinio's limit 2,000 a minute); the
same on 2026-11-23 19:32 UTC. **Reading of the rule.** The budget is not a calendar-minute cap: it is a
sliding-window reservation (the sliding-log limiter, held per lease; `budget.rs`): a lease counts at its
full size from its grant, at what was used once released, until 60 s after its end, and a grant is made
only with that sum within the ceiling. That bounds every rolling 60 s window [a, a + 60 s]: the last
lease granted among those with requests in it was granted at t <= a + 60 s, and every other one ended at
or after a >= t - 60 s, so that grant counted it. The ledger's calendar minutes (`core.request_usage`,
booked at release) are no measure of it. **The gap closed.** A released lease ended at its release as
core received it, though a request its worker was permitted just before still travels to the provider:
it now ends `TRANSIT_SECONDS` (5 s) after its release, as an open lease's validity already allows. Cap
setting unchanged (`VYREON_CORE_INTRINIO_PER_MINUTE`, 1,900 deployed).
**Not established: why Intrinio answered 429 under it.** The guarantee is over the requests the workers
draw permits for. A request on the same key outside core's budget (another machine's captures, the
droplet's timers, an agent's provider calls) or one a capture sends without a permit would not be
counted. To find out: Intrinio's account usage for those minutes against the ledger's bound
(`vyreon-core budget ledger`), and an engine audit that every provider request draws a permit.
**Refusals (C-075 amended; engine D-464).** A refused day's day-wide carry entry now says how the fill left
the capture (`capture: "carry retryable"` after a throttled or failing provider); C-075 counts it transient
(one quick retry), as an asset's retryably failed capture. Tests: `budget_tape.rs`
`the_ceiling_holds_over_every_rolling_minute` (a full ceiling used in the previous calendar minute's last
seconds holds the next minute's draws until 65 s after its releases) and the in-flight end in
`the_budget_never_commits_more_than_its_ceiling`; `day_refusal.rs` (job 21818's carry refusal as the
engine now states it: transient; fatal, unfinished, or without the class: held).
**The bypass hunt (2026-11-24, engine D-464's table).** Every cluster path that sends to Intrinio's API
draws a permit; the file downloads (snapshot, bulk day, replay objects) are presigned links with no key;
the reference suite sends nothing. The worker image now refuses to send without the gate
(`VYREON_BUDGET_REQUIRED`). **Evidence of Intrinio's own count** (its account-usage report, read
2026-11-24 ~23:5x UTC through the Intrinio MCP server): a `one_min_call_limit` of 2,000 per access code
(`api` and `opra_historical_intervals` both counted), with `seconds_until_reset`, a fixed minute
window, which any rolling 60 s bound covers. That report came back for the same account as the
cluster's: **the agents' Intrinio MCP server shares the account and draws no permit** (core cannot see
it). It is the one sender left outside the budget; whether agents queried Intrinio near 22:55 UTC on
11-24 (or 19:32 UTC on 11-23) is for the session logs to say. Options, the owner's: lower the ceiling by
the agents' share, or give the MCP server its own key.

## C-077 A worker release drains the old build's running day blocks (owner, 2026-11-24: automation debt 1, "1-5 can all get built up")
**Before.** `build hand-over` withdrew the old build, which ended every live lease of it, and release.sh
step 9 deleted the old image's Jobs: a block on its first day (about 5 hours, 3 of them its download) lost
it all, 8 blocks at once on 2026-11-23/24.
**Standard followed.** Kubernetes' graceful termination and node drain (a terminating pod is sent no new
work and is given `terminationGracePeriodSeconds` to finish what it holds, then killed; `kubectl drain`
evicts within a timeout): in-flight work finishes within a bound, nothing new starts, past the bound it is
stopped as before. **Where it lives: core's leases, not release.sh.** The hand-over is the one place that
knows which job a pod holds, its day, and what the successor reads; release.sh only removes pods.
- **The drain (core).** In the hand-over (`build::hand_over`), a live lease of a DAY_BLOCK job of the old
  build (not compare-only) is kept, not ended, when the successor reads every base and day-local profile
  revision the old build publishes (or every profile revision, for a build that declares none): C-064's
  rule, so the successor's pass can read the day it publishes. Its job is cut to the day it is on (its
  next session); the days after it are carried to the successor at once (a `Carried` of state DRAINING,
  in the hand-over's token and notice); `core.drain` (migration 0059) records the lease, its day, the
  hand-over and its bound. The old build is withdrawn as before: it claims nothing; a draining lease may
  only publish the day it holds (C-070 unchanged: one published revision per layer; the successor does
  not hold that day). The successor's passes wait for it by the claim's own rule (an unfinished day block
  with `next_session` at or before their day).
- **Its end.** The day's `day_done` answers `last: true` (no session left); the lease stays live for the
  day's archive and reference check and ends with the check's report (`work::end_drained`: lease and job
  DONE), or with the worker's `finish`. A worker of an engine before D-465 goes on to its next day and is
  told at its next call that the lease is gone (D-465's worker stops by itself). **Its bound:**
  `work::DRAIN_MINUTES` = 360, the measured worst first day of a block on the cluster (4 h 55 min,
  2024-07-16 on 5f637983) plus its archive (15 min) and a typical check. Past it the sweep (every half
  minute and at each claim) stops the lease as a withdrawal does (`stop_drain`; its attempt not counted).
  **Settling:** a drain whose lease ended with its day not done (stopped, failed, expired) has its job
  carried to the build the hand-overs from the old build lead to (`settle_drain`: its done days a DONE job
  of their own, the rest a new job with fresh attempts, as a hand-over carries a job); a day done is
  logged ("finished D and stopped"). A later hand-over leaves a live drain to the sweep.
- **release.sh.** Preflight needs 0059. Step 9 deletes the unfinished Jobs of other images but those whose
  pods hold a live drain (`vyreon-core build drains`: lease, job, worker pod, day, bound); new step 12
  polls the drains each minute and deletes each one's Job once its lease has ended, until none is left;
  a drain standing 15 minutes past its bound stops the script (core should have stopped it). An interrupt
  leaves the drains to core; `--from 12` resumes the clean-up. The progress channel's hand-over line says
  "N running blocks finishing their day first".
- **Cost accepted.** A draining pod holds its node and its ScaledJob slot until its day ends (up to the
  bound), so the successor's carried days start on the free slots meanwhile; with `day-block` at its cap
  (8, phase 1's 8 blocks) they start as the drains end. Never slower than the old way: a slot finishes
  the day's remaining part instead of computing the whole day again. Step 11 therefore does not wait for
  a claim of a kind whose ScaledJob has draining Jobs (checked against live Job and pod labels, read-only). Other kinds (tapes, passes,
  re-runs, republishes) are stopped as before: passes are minutes; a republish or tape drain is a later
  extension if a release ever cuts one.
Tests: `tests/drain.rs` (two running blocks: one finishes its day, its check ends the lease, a further
publish is refused as gone, the pass of its day waits for it; the other is stopped at its bound and its day
carried; the successor runs the rest and every pass in date order; no drain where the successor cannot read
the day), the hand-over lines in `work.rs` and `digest_format.rs`, `progress.rs`'s line.

## C-078 The spend is watched against the owner's ceiling (owner, 2026-11-24: automation debt 3)
- **Figures.** DigitalOcean's own: `GET /v2/customers/my/balance` (`month_to_date_usage`, `generated_at`)
  and `GET /v2/customers/my/invoices` (finished months), read every hour by core (`core_api::spend`) and kept
  (`core.spend_reading`, 0060; a read that failed kept with why). The spend since the owner's start
  (`VYREON_CORE_SPEND_SINCE`) is each month from the start's: a finished month's invoice, or before it is
  issued the highest month-to-date figure read in it; the current month's month to date; less the start
  month's usage before the start (`VYREON_CORE_SPEND_BASELINE_USD`). It is the whole account's, as the
  owner's running total has counted it since 2026-11-21 06:09 UTC ($180.41 then; MILESTONES).
- **Said.** Each morning (09:00 MT, with the digest) one line on the progress channel: spent of the
  ceiling, the rate per day over the last 24 h (else since the start) and where that rate reaches the
  ceiling. `SPEND` alerts (alerts channel) at 80% and at 100% of `VYREON_CORE_SPEND_CEILING_USD` (no
  default: the owner's number, 500 in 20-core.yaml); a reading older than 3 h or failed opens and resolves
  nothing (the line says "not known" and why). `vyreon-core spend` says it at any time.
- **The token (decision).** Core in-cluster had no DigitalOcean token; it gets a read-only one: the owner
  makes a custom-scoped token with `billing:read` only (DigitalOcean's custom scopes) and puts it in the
  optional secret `vyreon-core-do-billing` (key `token`, from a file); it is read at each request, never
  printed or logged, and a refusal keeps only the status. No key was made by the builder. Without it the
  line states the absence; nothing fails (README "Spend").
Tests: `tests/spend.rs` (the real figures: the spend, its rate and projection, a stale reading, the
morning line once, 80% then 100% opened and kept while unread, a month boundary before and after its
invoice), `core-api/tests/spend.rs` (a simulated DigitalOcean: no setting, no secret, a 401 said without the
token, a reading with the bearer sent), the line's unit tests.

## C-079 Recompute after a method change is planned and scheduled by core (owner, 2026-11-24: automation debt 5)
- **A build's methods** are what it publishes: every row carries its measurement's science and method
  version (engine D-273); the engine keeps no list of them, and the inventory has none. So core reads a
  build's own labels per layer from its newest ordinary revision of each (a day block's base and
  day-local, a pass's history; never a republish's, whose carried rows keep older versions). A published
  day differs where a measurement of a layer it has published carries other labels, or none (a
  measurement the build added). Read-only check on the live database (2026-11-24, against the running
  build 1ae946f7's 2026-11-23): 11 of the published days differ, in two sets (the DLM day-local
  families; and those plus the ACT, EXQ, EVC, PCR-UPATH and M0-PRINT-SIZE versions on 2a8f8c4f's days); the
  comparison took about 1.5 s.
- **When.** Each minute the watch looks at the newest running build: once it has published both day layers
  (a whole day), its base and day-local layers are compared, once (`core.recompute_plan`, 0061); its
  history layer once its first pass has run. Not while a block of another build drains (C-077: it
  publishes under the old methods). Days with unfinished day blocks or republishes are left alone (being
  computed); so are days before `VYREON_CORE_RECOMPUTE_FROM` (phase 1's first session in 20-core.yaml).
- **What.** Days grouped by the measurements that differ; each group planned by the C-074 planner over
  exactly those days (`republish::plan_days`; runs never span a day not asked for): SCOPED (one job a day)
  when cheaper or on days past the prints' retention, FULL day blocks otherwise or across profile
  revisions, ordered passes for history-only differences. A group the planner says has no faithful
  recompute (its scope reads prints no longer served) or cannot plan is not scheduled: the owner's.
- **Within the ceiling.** The plan's measured node-hours at `VYREON_CORE_NODE_HOUR_USD` (0.167, the day
  pools' list price) added to the spend now (C-078) must stay within the ceiling; otherwise, or when the
  spend is not watched or not known, it is held: the progress line says what it would do and the alerts
  channel says why and what to do (`vyreon-core recompute <profile> <build> --plan`; the owner schedules
  by hand or raises the ceiling). Scheduled work runs through the existing machinery (REPUBLISH jobs, at
  most 4 at once, held back by the scaler while the request budget is pressed; day blocks; passes).
- **Said.** One progress post per decision: the measurements, days and span per group, scoped or full and
  its node-hours, the total and its dollars, days left alone. `vyreon-core recompute <profile> <build>
  --plan` prints it without scheduling; `--schedule` decides it as the watch does.
Tests: `tests/recompute.rs` (the real slice with M0-GAMMA-CHANGE's method moved on: nothing before the build
publishes a whole day; a busy day and a day before the start left alone; held without a watched spend and
past the ceiling, each said on both channels; scheduled within it, once).
**Questions for the coordinator (product behaviour, not decided here):** (1) a plan is held whenever the
spend is not known (no billing token yet), so until the owner makes the token every recompute waits for
his word: is that the intended default, or should a plan within a fixed allowance go ahead? (2) The
automatic plan republishes every published day whose versions differ, including days whose difference is
a version bump that changed no value (the republish then narrows itself: D-104's value-unchanged rule);
the owner may prefer to exclude named measurements (a list) rather than pay for them.
