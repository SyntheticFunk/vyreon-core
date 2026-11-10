# Engine map (vyreon-engine `engine-v1`, a046bf54, read 2026-11-10)

What the engine does today, seen from the manager: how a day runs, what it keeps on disk, what it
reads from earlier days, what assumes the fixed 64-asset universe, and where it calls Intrinio.
Read-only survey; paths are relative to `/home/diggz/vyreon-engine`. Claims were read in the code
unless marked *(inferred)*. Line numbers drift; re-check before editing. The design built on this
is `docs/ARCHITECTURE.md`.

## 1. How a day runs

**`vyreon-days`** (`crates/vyreon-cli/src/bin/vyreon-days.rs`, `src/days.rs`): modes `daily`,
`rebuild`, `republish`; run dirs `h5r`/`h5f`/`h5c<YYYYMMDD>`. Per date (`days.rs:273-317`):
- `--fill` runs `vyreon-inputs fill` for date i+1 in a background thread while date i computes; a
  fill that reports something missing does not stop the run (the day's plan decides).
- `decide()` (`:149-164`) reads only the local run dir: `finish.log` → skip, staged + same build →
  resume at finish, else run. A fresh pod always recomputes.
- `--prune` (D-160) deletes inputs no later day reads (frontier = next session).
- Every day publishes base only (`skip_derive`, `days.rs:266`); one `derive_all` runs at the end.
- `rebuild` needs an explicit store, `--history-run`, and an empty store (`days.rs:508-521`).

**`vyreon-day T`** (`day.rs:1050-1065`), stages in order, each stamped into `acceptance.log`:
1. start: session flock (local), a previous run dir is set aside (never resumed), disk guard,
   build identity (needs a git checkout, see §10), clocks, `build.json`, `cutoff.json`.
2. plan (`plan.rs`): refuses the day if any input of any of the 64 symbols is missing (§7).
3. surfaces T and T−5 (`h5_surface`, ≤16 threads), cached per input identity in
   `h5-surface/<date>/<id16>/`.
4. intent A: endpoint history (Q01 + O02/O04/L01/OIJ01 per asset-session, §5), exec via
   `h3-asset` worker pool, history exports.
5. intent B: per-asset science; guards; EVC tape history linked from every prior session (§5).
6. exports (one `h3-asset export-*` per slot), company (`h5-company-science`), FIELD panels and
   `h5-field --threads 16`, FIELD options.
7. stage → verify → publish → provider-free readback → cleanup (§4).

Measured (droplet, 8 shared AMD vCPU, 2025-12-10): inputs present 2 h 28 min — surfaces 48 min,
intent A 35, intent B 52, FIELD 5, rest ~8; 654% CPU; peak RSS 1.2 GB; store +150 MB. From an empty
home: download 200 min (provider-bound, same on the laptop), compute 160 min, peak disk 44.7 GB.
Second consecutive day with a kept history: intent A 30 → 20 min (MILESTONES:761).

**Process model.** `h3-asset` spawns N workers over Unix sockets, each with a cleared environment
(`h3-asset/src/main.rs:34-128`). N = min(20 requested, CPUs incl. cgroup quota, (memory − 768 MiB) /
320 MiB, 32 GiB scratch / 2 GiB), capped 16 (`runtime_resources.rs:82-162`); disk kept free per wave
max(5 GiB, 20%). Other pools: surfaces ≤16 threads, FIELD fixed 16, M0 `VYREON_M0_THREADS` ≤8,
requote 8, capture batches 4. Rust's `available_parallelism` honours cgroup limits *(inferred)*.

**Runtime needs.** No Python or psql on the production path. Needs: `git` (build identity), all 19
`STAMPED_BINARIES` in `--bin` (`build_id.rs:12-16`), MKL/mpfr/gmp at the build-time rpath
(`h1-numerics/build.rs`), the SHA-pinned SQLite 3.45.1 at its rpath (`sqlite_index.rs:90-106`;
workers clear `LD_LIBRARY_PATH`), an AVX2 CPU (`h5-field/src/mkl.rs:83-96`).

## 2. Environment (production path)

| Variable | Effect |
|---|---|
| `VYREON_HOME` | home root (or `--home`) |
| `VYREON_PG` / `VYREON_PG_FILE` | store conninfo (owner role); fallback `~/.config/vyreon/pg` |
| `VYREON_HISTORY_RUN` | shared endpoint-history dir (HIST, §5) |
| `VYREON_TAPE_HISTORY_FROM` | EVC01 tape history start, default 2023-05-24 (D-159); all workers must share it |
| `VYREON_SKIP_DERIVE`, `VYREON_KEEP_TRANSIENT`, `VYREON_ALLOW_DIRTY`, `VYREON_ALLOW_NONEMPTY` | run switches |
| `VYREON_MIN_FREE_GB` (60), `VYREON_FREE_PATH` | disk guard; 60 GB refuses an 80 GB node |
| `INTRINIO_API_KEY` / `VYREON_KEY_FILE` | one key for every call, passed to child captures |
| `VYREON_H4_*` (memory, scratch budgets), `VYREON_M0_THREADS`, `REQUOTE_WORKERS` | sizing |
| `VYREON_ACQ`, `VYREON_SURFACE` | derive's archive and surface roots |
| `VYREON_WEBHOOK_FILE`, `VYREON_NOTIFY_STATE`, `VYREON_HEALTH_*`, `VYREON_PUBLISH_BY_UTC`, `VYREON_BACKUP_DIR` | notices and health |
| `VYREON_API_ADDR`, `VYREON_API_PG`, `VYREON_API_TOKEN_SHA256` | vyreon-api |

## 3. The home and its state

Class: **a** transient per day · **b** needed across consecutive days of a block · **c** long-lived,
belongs to the manager · **d** dev only.

| Path | Read by | Lifetime | Class |
|---|---|---|---|
| `h5-acq/<SYM>/options/<T>[-rN]/` (chain, population, `close-quotes.csv`) | plan, views, derive | 200-session window (D-160) | b |
| `h5-acq/<SYM>/daily/<T>` | views (380 sessions); **derive `load_bars` from 2020** | kept (~2 MB/session) | b |
| `h5-acq/<SYM>/consolidated/<T>/capture` (+ `derived/` cache ~70 KB/asset-session) | EVC01, intraday FIELD, VWAP | raw dropped once cached; cache kept from 2023-05-24 | **c** |
| `h5-acq/<SYM>/security.json` (`first_stock_price`) | fill, FIELD view | per home | c (universe) |
| `h4-account/attempts.sqlite3` | request ledger (§9) | forever, per home | a (budget → manager) |
| `h5-carry/<T>`, `h5-macro/<T>`, `h5-company-capture-<T>`, `h5-act01-auto/<T>` | surface, views, company, intent B | to T−5 | b |
| `h5-surface/<date>/<id16>/<SYM>.json` | intent B (T−5), **derive** | to T−5 | b, and derive needs T's |
| `h5-exq01-events/<T>/<SYM>` | intent B (T) | **perishable, kept forever** | **c** |
| HIST (`--history-run`): `index.sqlite3`, results, admissions, `t5-results/` | intent A reuse, T−5 hand-off | +0.7 GB/session | b at most |
| run dir `<prefix><YYYYMMDD>/` | the day | ~21–23 GB during the day; index + logs kept; `retain` prunes only `day-*` | a |
| `locks/`, `logs/` | flocks, logs | | a |
| notify state (`runner.json`, `summaries/`, outbox) | health, drop comparison | forever | c |

**Assumes one persistent home** (break on a disposable pod): skip/resume via local `finish.log`;
HIST reuse and the T−5 hand-off; surface reuse; EVC01 linking every prior tape cache; derive reading
archive files of every date it derives; attempt ordinals; notify summaries; local flocks (two pods
on one date would `discard-staged` each other).

## 4. Publish path (direct SQL)

All clients connect as the owner role with **`NoTls`** (`vyreon-store/src/main.rs:45`; also derive,
days, watch, vyreon-api). DigitalOcean managed Postgres requires TLS *(inferred: fails as written)*.
- `vyreon-store`: `discard-staged T` (deletes every STAGED revision of T) → `open` → `stage REV dirs`
  (reads export dirs from the local disk, checks digests, 200 dirs per transaction) → `verify` →
  `publish` (idempotent) → `read` (readback with a cleared environment).
- `publish_revision` (`db/schema.sql:103-167`): exact match with `expected_slot` (no missing, extra,
  or duplicate slot) and every committed stream present; a derived revision needs its base
  PUBLISHED; **a plain base publish supersedes every derived revision from its date on**; a
  republish base supersedes only its own date's.
- Derive (`derive/mod.rs:532-1000`): one REPEATABLE READ transaction per date; checks publications
  unchanged since load; inserts the DLM revision, `reader_scalar`, `unusual_score`.
- Republish (D-104, `days.rs:737-872`): one RUNNING `republish_job`, copy-forward rows, restricted
  intent B, compare, cascade by value via `vyreon_graph::history_readers`; no `--fill`: inputs must
  be on disk.
- **SQL reads during compute:** base (surfaces, intents, exports, FIELD) reads **nothing** from the
  store (`vyreon-graph/src/lib.rs:13`, test `:346`). Derive reads all published history (below).

## 5. Cross-day reads

Source: **R** raw captures re-read from the home · **C** the engine's own cache · **P** published
results in Postgres.

| Read | Family | Lookback | Src | Layer | Expanding | Where |
|---|---|---|---|---|---|---|
| Endpoint history (intent A) | ENDPOINT, QOV | ≤200 captured sessions, stops at ex-date / `CAPTURED_HISTORY_START` | R (+C) | base | no | `views.rs:235-283` |
| O02, L01 temporal context | ENDPOINT | scans until 126 eligible changes (132–200) | from A | base | no | `h3-o02/src/history.rs:187-226` |
| O04, OIJ01 temporal context | ENDPOINT | 132 | from A | base | no | `h3-o04/src/history.rs:86-116` |
| Close-quote markers / requote | input prep | 200 + T | R | base | no | `day_inputs.rs:44` |
| `last_adjusted` (D-081 frozen-OI drop) | QOV | **every daily bar on disk** | R | base | **yes** | `occ.rs:71-73,153-160` |
| Split history (D-120) | SURFACE | 400 days | R | base | no | `acq.rs:141-160` |
| P01, P07 | P | 131 + T (P01 at T−5 reaches T−136) | R | base | no | `views.rs:453-469,512` |
| PATH, W21, P03, PD01 | P | 153 | R | base | no | `views.rs:16,510` |
| FIELD daily panel | FIELD | 376 + T (DEP01/SUB01/RMT01 250; context 126/63) | R | base | no | `views.rs:500-507` |
| FIELD intraday panel | FIELD | 189 + T | C (tape cache) | base | no | `day.rs:26,623-627` |
| **EVC01 baseline** (q90 of every prior, ≥20) | REPLAY | **every session from 2023-05-24** | C (~70 KB/asset-session) | base | **yes** | `day_inputs.rs:16-27`; `h3-temporal/src/evc01.rs` |
| T−5 endpoints (IVSC, V03-MATCHED, M0-IV/GAMMA-CHANGE, O03) | QOV, M0, SURFACE | T−5 chain, P01 view, surface | R, C (`t5-results`, 6 sessions) | base | no | `views.rs:514-560` |
| M0 T−5 closure (bound to **T's** macro) | M0 | T−5, recomputed daily | R | base | no | `h4_run.rs:1213-1226` |
| ACT01 cohort, EXQ01 | ACT, EXQ | cohort at T−5; actions [T−5, T]; prints of T | R | base | no | `views.rs:196-221` |
| DLM PCR/volume levels | DLM-POSITIONING | 252 (21 mean) | P | derived | no | `h5-derived/src/lib.rs:448-490` |
| DLM-OI-CHANGE | DLM-POSITIONING | 1 | P | derived | no | |
| DLM-HV/IV-CONTEXT, EVENT-WINDOW, IV-CRUSH | DLM-REALIZED/IV/EVENTS | 252 (own IV30) | P | derived | no | `derive/mod.rs:189-300` |
| DLM-IV-TERM | DLM-IV | 5 (own) | P | derived | no | |
| 8-K first-seen, expected earnings | DLM-EVENTS | all published CDD-EVENTS01 | P | derived | **yes** | `derive/mod.rs:406-466` |
| Unusualness robust z; 95% lines pooled across assets (≥500) | DLM-UNUSUALNESS | 63 scalars; lines 252 | P | derived | no | `derive/mod.rs:586-597,844-899` |
| **DLM-TECHNICAL** (EMA, Wilder RSI, MACD, ADX, SAR, streaks, last SMA cross, `price_history_sessions`) | DLM-TECHNICAL | **every daily bar on disk from 2020**; restarts at a gap | R | derived | **yes** | `derive/mod.rs:313-325` |
| Derived ex-date cut (`action`) | DLM-POSITIONING | 260 close-quote headers | R | derived | no | `derive/archive.rs:97-112` |
| Chain, Greeks, gamma exposure, expected move | DLM-* | T's population, **surface file** (missing = hard error), VWAP cache | R, C | derived | no | `derive/mod.rs:334-364` |

**Intent A reuse.** 64 assets × ~197 sessions × 5 tasks ≈ 63.5k tasks (B1: 61,499 of 63,547 reused).
Key = sha256(scoped code digest, `SYM:date`, measurement, op version, pins, input digest, parents)
(`h4_asset_index.rs:61-87`); stored in HIST `index.sqlite3`; reuse commits cost ~23 of intent A's 39
min. A cold home recomputes from raw captures: with today's `OPTION_PRIOR` = 131 a fresh day has ~42k
tasks. A requote changes the source digest and every dependent key.

**Days computed before their history exists.** Base: not implemented: the plan refuses a gap in a
window; the only lawful short histories are the `CAPTURED_HISTORY_START` and `CORPORATE_ACTION`
terminals. Derived: withheld reasons exist (`PRIOR_SESSION_NOT_PUBLISHED`,
`INSUFFICIENT_LEVEL_HISTORY`, `INSUFFICIENT_HISTORY`, `LINE_UNCALIBRATED`), but nothing re-derives
when older days arrive (`derive_all` only derives days without a derived layer unless `redo`).

**Recompute scope** (`crates/vyreon-graph`): 22 families: 13 base (P, REPLAY, ENDPOINT, QOV, ACT,
EXQ, M0, SURFACE, FIELD, FIELD-OPTIONS, COMPANY, MACRO, INACTIVE) and 9 derived (DLM-POSITIONING,
-REALIZED, -CHAIN, -IV, -GREEKS, -EVENTS, -GAMMA, -TECHNICAL, -UNUSUALNESS). Recompute = seed + its
same-day dependants; later days via `history_readers` (longest window 252). Costs: a derived family
22 s/day *while T's inputs are on disk*; base P 58 min/day (about a whole day); surface ~23 min per
session plus T−5's; M0 ~20 min.

## 6. What makes a day depend on where its run started

| Mechanism | Effect | Fix direction |
|---|---|---|
| O02/L01 scan depth: 131 option sessions (fresh home) vs 200 (long-lived) | different references/percentiles for assets with withheld changes in the window; parity did not cover it (both fresh) | every worker fetches 200, or cap the scan |
| `last_adjusted` scans all daily bars on disk | older splits move markers → requote, ACT01 rebind, new keys; rarely values (frozen-OI drop) *(inferred)* | bound the scan to a window or provider split history |
| DLM-TECHNICAL reads bars from whatever start is on disk | EMA seeds, SAR, streaks, last cross, `price_history_sessions` | a fixed origin per asset |
| Derived ex-date scan 260 vs pruning at 200 | level percentiles via `volume_position` | store ex-date markers or shorten the scan |
| Derived reads `first_published` and whatever is published at derive time | out-of-order derive gives different, partly withheld values | derive strictly in date order |
| `VYREON_TAPE_HISTORY_FROM` | EVC01 baseline start | one value for all workers |

Fixed-window readers (P01/P07, PATH, FIELD, O04/OIJ01, O03/IVSC, split history) are pure functions of
T and the provider's data. Ledger cutoffs, attempt names and batch names are provenance only. A
re-download is new provider evidence (macro is served revised only, no vintages).

## 7. The fixed universe

**Defined in:** `specifications/science/PROFILE.json` (64 tickers, 17 companies, 7 groups,
`FIELD.CORE64`) and `OWNER_TRACEABILITY.json`; constant copies in `vyreon-sensors/src/lib.rs:9-20`
(`SECURITIES[64]`, `COMPANIES[17]`), `h5-field/src/profile.rs:4-28` (`OBJECT_COUNT` 64, `PAIR_COUNT`
2,016, `MINIMUM_CROSS_SECTION` 51, `GROUPS` as ordinal blocks), `h5-company/src/lib.rs:16`,
`h5-field/src/option_field.rs:29-38` (640 support keys, M0 minimum 48, `ETF_COUNT` 47 by position).
`h4_run.rs:467-468` refuses more than 64 assets.

**Slots:** security 43 base (42 traceability + VS-IVS01) + 28 derived = 71; company +5 base + 2
derived = 78; FIELD 16 → 2,853 base + 1,826 derived = 4,679. `Members::objects()`
(`vyreon-sensors/src/lib.rs:61-69`) is the one place a per-date member list plugs in.

**What refuses a smaller or different set:** the plan checks every input for all 64 (`plan.rs:184-343`,
`ok = !any refuses` at `:637-660`); the FIELD driver refuses a missing view (`day.rs:603-614`); the
FIELD panel length is the minimum over assets and errors under 2 captures (`h5_field_panel.rs:33-37`,
D-059): before IBIT (2024-01-11) and ETHA (2024-07-23) list, FIELD cannot run; `publish_revision`
needs the exact inventory; seeding refuses changed ordinals; readback, summary and the API
(`objects`, `rankings`, `window`) and derive `objects()` count the full set.

**Identity is the ticker everywhere:** archive paths, `security.json` (`/securities/{ticker}` returns
whoever holds it now; FB is now an ETF), option EOD `by_ticker`, snapshot `row_owner` roots
(`close_quotes.rs:249-259`: pre-2022-06-09 FB rows dropped for META), OCC detection (FB contracts
become `adjusted_pending`), company resolution (`/companies/{ticker}`; a delisted company fails),
tape filter, P01/Q03 challenge registries (`UND_FB_20220608` can never fire), `object_id`, result ids,
FIELD axes and pair ids, API paths. No dated alias table exists.

**Sources that do not exist yet on T:** option EOD before 2021-09-27 and snapshots/ACT01 before
2022-05-27 make the plan **refuse**; tape before 2023-05-24 is handled (`TAPE_UNAVAILABLE` after two
recorded absences, provider-limited); macro before 2025-09 uses fixed lags (stated limit); insider
before 2025-12-16 uses SEC acceptance times. P01, PATH and the consolidated history are not
listing-aware (the FIELD view and the fill are). Reason codes carry no cause: only `summary.rs:14`
maps `NOT_APPLICABLE` and `BEFORE_SERIES` to structural; there is no code for not listed or before
provider start.

**Backfill-readiness audit (MILESTONES, 2026-11-08):** (1) OCC adjusted roots looked up at the
archive's first session, specials before 2024 — open; (2) FB→META roots — open; (3) tape history
start — fixed (D-159); (4) challenge registry names two splits — open; (5) macro lags — stated limit;
(6) FIELD panel minimum — open; (7) retention — fixed (D-160; its "results-based history" next step
open).

## 8. FIELD owners and a variable member count

| Owner | Needs every member? | N-tied constants | Class |
|---|---|---|---|
| BR01, DIST01, XCONC01 | no, gate N ≥ 51 | 51 absolute (spec) | science: 51 as a share? context across N |
| SYNC01 (W21/W63) | **yes**: one missing cell in the window withholds | 2,016 pairs | science |
| DEP01 | no (pairs need ≥126 common sessions) | common-scale row N ≥ 51 | parameter; 51 science |
| GRP01 | no | fixed `GROUPS`, permutes all 64 labels | science |
| RMT01, DIM01 | **yes** (rows where all are supported) | nulls at dimension 64 | science |
| SUB01 | **yes**, leave-one-group-out | `GROUPS`, `OBJECT_COUNT` | science |
| SUP01 | exact ordered 64 inputs | 640 = 10 × N; `ETF_COUNT` by position | parameter + science (absent member's keys) |
| OPTBR01 | no, gate 51 | 51 | parameter; 51 science |
| M0-FIELD-ANALYTICS | no, minimum 48 per subgroup | 48 | parameter; 48 science |
| MHDEP01 | no (pair ≥80% of bins) | 2,016 | works (array sizing) |
| FDSYNC01, FDRMT01, FDDIM01 | **yes** (every security has a midpoint) | 64 | science |

## 9. Intrinio and other network calls

All Intrinio calls go to `api-v2.intrinio.com` with one key (`INTRINIO_API_KEY`, bearer header).

| Call site | Endpoint | Category | Pacing | 429/5xx |
|---|---|---|---|---|
| `h2-engine` `live-capture-batch` (`transport.rs:173`) | `/securities/{SYM}/prices` (2 requests per bar), `/options/prices/by_ticker/{SYM}/eod` | daily, options | ledger, 4 slots, ≥33.3 ms | **none**: a 429 fails the chain; fill retries the step 3× |
| `act01_auto_source` | `/options/interval/{contract}` (80 per cohort) | act01 | ledger, sequential | none; the ledger grant fails rather than waits |
| `exq01_cohort_capture` | `/options/{contract}/trades` (D-103 prints) | exq01 | ledger, sequential | 404 "no trades" = end |
| `vyreon-cli` `Provider` (`provider.rs:54,72`) | `/securities/{SYM}`, `/options/snapshots` listing, presigned snapshot file (~40–60 MB), `/prices/adjustments` | quotes, security | **not ledgered** | 5 tries; file GET no resume |
| `carry_inputs` | dividends, 5-min intervals; **Fed H.15 CSV** (D-105) | carry | **not ledgered** | 8 tries |
| `capture_source` | `/indices/economic/...` | macro | **not ledgered**, 100 ms | none |
| `h5-company-capture` | 13 company endpoints | company (17) | **own gate 480/min**, 4 in flight | 5 tries |
| `h3-replay` | `/securities/replay` locate + presigned ranged GETs (~13 GB/tape, streamed, never stored) | consolidated | **not ledgered** | 9–10 tries, resumes by range |
| `notify.rs` | Discord webhook | notices | | honours `retry_after` |

The ledger (`h2-unit-result/src/sqlite_index.rs:389-660`, `<home>/h4-account/attempts.sqlite3`) is
local: a hard-coded 1,800 per rolling minute and ≥33.3 ms spacing per home. N workers with N homes
would each take 1,800/min. Presigned file GETs go to another host *(inferred: not rate-limited)*. The
code cannot tell which access code (`api` or `edi_uscomp`) a call is billed to.

**Narrowest choke points for a shared budget:** `authorize_api_attempt` / `api_next_grant_at` (all
ledgered categories, most of the volume; act01/exq01 must learn to wait), plus five unledgered
helpers (`provider.rs:47`, `carry_inputs.rs:47`, `capture_source.rs:98`, company `Gate::admit`
`capture.rs:62`, replay `fetch.rs:47`), plus 429 handling in `h2-engine/src/transport.rs:163-180` and
macro. An egress proxy cannot meter (TLS tunnels) *(inferred)*.

**Requests per day.** Measured on the droplet ledger (ledgered categories only):

| Run (UTC) | daily | options | act01 | exq01 |
|---|---|---|---|---|
| 11-07: 12-10 from empty, 2025-only calendar | 30,208 | 8,495 | 4,720 | — |
| 11-08: 5-session catch-up + 12-10 refill with 2024 bars | 49,583 | 9,166 | 40,160 | 28,151 |
| 11-10: daily capture (one new session + recaptures) | 256 | 66 | 5,120 | 8,325 |

The "~39k a day" quoted earlier was daily + options of the first row. Estimated from code: a cold
day with today's 379-session daily window ≈ **65k** (daily 48.6k at 2 requests a bar, options 8.5–10k,
act01 4.7k, company 2.7k, the rest <1k); a warm day inside a block ≈ **8k** (company and act01
dominate); forward daily capture ≈ **16k** with prints. Daily bars as one ranged request per asset
would cut 48k to ~64 (changes the frozen binding `INTRINIO-CI-DAILY-BAR-1@1`: an engine decision).

**Not re-downloadable:** option trade prints only (Intrinio keeps ~5 sessions;
`PRINT_RETENTION_SESSIONS` = 5, `fill.rs:61`). On the droplet they sit in
`/srv/vyreon-home/h5-exq01-events` on the droplet's own disk (1.5 GB, 10-23 and 11-02…11-09 on
2026-11-10), not on the volume; the droplet has no backups or snapshots.

**Point-in-time endpoints:** used: `/securities/{ticker}` (`first_stock_price` only), the security id
on daily bars, company `/securities/{id}`, splits/dividends/adjustments. Not used:
`get_security_history_by_ticker`, `list_securities`, `list_options_tickers`.

## 10. Operations today

- Timers on the droplet: `vyreon-capture` 02:00/12:00 UTC (`capture-recent.sh`: every category but
  consolidated for the last 5 sessions), `vyreon-health` every 15 min (disk, memory, store, runner
  liveness via `/proc`, orphan republish job, publish deadline, backup age), `vyreon-heartbeat` daily.
- Discord: silent, no mentions; from `vyreon-days` and `fill`.
- Kill and rerun: a killed day recomputes from scratch; a killed finish resumes on the same home;
  STAGED revisions linger until the next `discard-staged`; captures are sealed and never re-taken.
- Re-running a published day publishes a new revision (the old stays readable) and supersedes later
  derived layers, which `derive_all` redoes.
- Derive refuses a build without a git code identity (`nogit`, `derive/mod.rs:1072-1082`).
- `--prune` with derive-at-end would delete surfaces the end-of-run derive reads (`retain.rs` vs
  `derive/mod.rs:350`) *(inferred; never run on a block over 5 sessions)*.

**vyreon-api** (axum, bearer SHA-256, gzip, NoTls, a new connection per request): contract, objects,
rankings, review, window (≤260), peers, slots, coordinates, streams. Keep its revision pinning,
coordinate projection and review; the manager adds the bulk `history`/`sessions` shapes, TLS and
pooling.

## 11. What breaks when the worker is a disposable pod

1. Publishing needs write SQL as the owner role, without TLS, from local export dirs.
2. Derive is an ordered stream over all published history and needs T's local archive (population,
   surface file, VWAP cache, daily bars back to 2020); a base publish supersedes later derived days.
3. EVC01 needs every prior tape cache from 2023-05-24: either streamed again (~30 GB a session) or
   served from central storage.
4. Prints live only on the capturing machine.
5. The disk guard's 60 GB default refuses an 80 GB node.
6. A git checkout at the build commit is required (build identity, derive).
7. Rebuild mode needs `--history-run`, an empty store and local skip state.
8. No shared request budget; five call sites unmetered; no 429 retry in the main capture path.
9. Locks are local flocks; concurrent staging of one date clobbers.
10. Notify and health assume one long-lived host.
11. The image must carry MKL, mpfr, gmp and the pinned SQLite at their rpaths; nodes need AVX2.
12. Day values depend on history depth on disk (§6).
13. The plan, FIELD and the store refuse any day where the 64 are not all present (§7).

## 12. Open questions

- Which option-history depth defines O02/L01 values (131 or 200), and how often it matters.
- How often `last_adjusted` and the 260-vs-200 scan change values.
- A fixed origin for DLM-TECHNICAL.
- Size of a day's derive inputs (populations, surfaces, VWAP caches) if stored centrally.
- EVC01's per-day cost as the tape history grows (re-reduces every prior session).
- Intrinio: which access code each endpoint is billed to; whether the 2,000/min is per code; whether
  presigned file GETs count; what `/options/prices/by_ticker/META/eod` returns for 2021 dates;
  whether `upcoming_earnings`, `daily_metrics` and `shares_outstanding` serve past dates.
- Tape materialization time per session (not measured separately from the 200-minute download).
- Whether the 200-session close-quote window makes days after 2022-05-27 refuse until it fills.
