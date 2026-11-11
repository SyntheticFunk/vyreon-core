# Vyreon Core

The data platform around the Vyreon engine: the PostgreSQL database of sensor measurements, the API
that Reporter and plain Python read, the job queue that hands date blocks to engine workers, the
Intrinio request budget, the point-in-time security universe, and the cluster that runs the workers.
The engine computes; core schedules, stores and serves. The measurements are the product.

## Authority
- `docs/PRODUCT_INTENT.md` is the ultimate guide (a copy of the engine's; the engine's is canonical).
- Science belongs to the engine: `/home/diggz/vyreon-engine` (tag `engine-v1`), its
  `docs/DECISIONS.md` ("Rules in force" at the top) and `AGENTS.md`. Core never computes,
  fills, smooths or alters a measurement; it stores and serves exactly what the engine publishes,
  including every absence with its state and reason.
- Core engineering decisions go in `docs/DECISIONS.md` here (short, dated, with reasons). When
  implementation shows a better way than the plan, record it and proceed; conflicts with product
  intent go to the owner.
- Intent decides what and why; established standards decide how (cite them). Prefer boring, proven
  infrastructure; flag over-building.

## Design (agreed with the owner, 2026-11-08 to 11-10)
- **Workers are stateless engine runs.** Each node takes a block of consecutive dates, downloads from
  Intrinio, computes, publishes to core, deletes its raw data and exits. Consecutive days share
  most of a 132-session quote window, so a block downloads it once; `vyreon-days --fill` already
  computes day T while the next date downloads.
- **Measured per day** (2025-12-10, laptop–droplet parity, 2026-11-09): download ~200 min
  (provider-bound), compute 110–160 min on 8 vCPU, peak disk 44.7 GB, ~39k Intrinio requests.
- **Budget:** at most 10 droplets on the account, less a web server and a database, so 8 worker nodes.
  Candidate node: 8 vCPU shared, 80 GB disk, $0.17/h. Anything that costs money is the owner's call.
- **Intrinio rate limit: 2,000 requests a minute, owned by core.** Workers draw from a shared
  budget (with margin) and report what they use; core tracks usage over time.
- **Point-in-time universe, built into the database.** Key securities by a permanent Vyreon asset key, never by
  ticker or by a provider id alone (C-023: an Intrinio security id changes on a rename, Meta is two
  records); provider ids and FIGIs are stored against the key as evidence. Tickers are reused: tickers are reused (FB is now a ProShares ETF; META was a Roundhill ETF to
  2022-01-28). Store dated ticker spans (`get_security_history_by_ticker`), listing and delisting
  dates, merges, and per-source provider start dates. Option EOD is keyed by the ticker in use on that
  date. Delisted securities keep prices and options history (TWTR, SIVB checked). Provider starts:
  option EOD 2021-09-27, option snapshots 2022-05-27, replay tapes 2023-05-24.
- **Field completeness is discovered by the backfill**, reported per day by the workers, not
  pre-measured.
- **API** (shape agreed; Reporter's exact needs decided later):
  `GET /v1/assets/{symbol}/history?from=A&to=B[&detail=headline][&format=json|parquet]` and
  `GET /v1/sessions/{date}`. Default is every measurement with its coordinates, state, reason, unit and
  quality; every session is listed, NOT_PUBLISHED explicit; `headline` returns the reader scalars.
  The engine's own API contract is `vyreon-engine/docs/API.md` (VYREON-ENGINE-API@1, chatty: one body
  per request); its store schema is `vyreon-engine/db/schema.sql`.
- **Storage:** Intrinio is the default store; keep only perishable inputs (option prints) and outputs.
  Full and per-sensor rebuilds are a recurring workload, not a one-off.
- Orchestration (DigitalOcean Kubernetes + KEDA, or a job table plus scripted droplets) is chosen
  after core exists and with measured numbers.

## Rules
- Real data only (engine D-102): tests and validation use real Intrinio data or frozen slices of it;
  no synthetic data.
- Secrets are never printed: the Intrinio key (`api.txt`), `VYREON_API_PG`, the Discord webhook. Read
  them with `$(tr -d '\n' < file)`; keep them out of logs, commits and command lines that echo.
- Preview and ask before deleting anything significant; never edit or replace a script, checkout or
  binary that a running job uses; ask before persistent system changes on the droplet.
- Stopping remote jobs: bracket patterns (`pgrep -f "[v]yreon-day"`), kill whole process trees,
  verify zero survivors before restarting.
- Discord notices go to the channel only, never phone pings.
- Production code hard-codes no machine paths.
- One status document, `docs/MILESTONES.md`, kept short; git is the history. Commit each landed step
  with a short summary; separate commits for code, tests, docs and deploy.
- Report delivered capability, not documents or test counts. Times for the owner in Mountain Time.

## Machines
- Droplet `ssh vyreon-engine` (authoritative today): DigitalOcean Premium AMD, 8 shared vCPU / 16 GB,
  Postgres 16 on the volume `/mnt/vyreon_engine_v1`, settings in `/etc/vyreon.env`, daily capture and
  health timers running. Engine checkout `/root/vyreon-engine`.
- The laptop is a development machine only: no Vyreon compute or data.
