# Vyreon Core

The data platform around the Vyreon engine: the PostgreSQL database of sensor measurements, the API
that Reporter and plain Python read, the job queue that hands date blocks to engine workers, the
Intrinio request budget, the point-in-time security universe, and the cluster that runs the workers.
The engine computes; core schedules, stores and serves. The measurements are the product.

## Priorities (owner, 2026-11-22; full authority: docs/PRODUCT_INTENT.md)
1. **No wrong numbers.** Withhold with an honest reason rather than fill, default or guess; this outranks
   coverage, speed and cost.
2. **Decision order: established standards > product intent > the owner's judgement.** Match the
   published standard or market convention and cite it; intent decides what the standards leave open;
   the owner decides the rest, brought with a recommendation and its reason.
3. **The product:** correct, explained, continuous measurements across the 64 for a weekly Sunday
   report; the paid layer rests on rare, real flags. Measurements, not forecasts.
4. **Generalizable sensors,** not one-off handling; prefer retiring a rule to adding one.
5. **Deterministic software:** no agent in the production path; agents build, audit and diagnose.
6. **The owner is told of every decision made autonomously** (in the next status update: what was
   decided, by which standard or rule, and what it changed).

## Working memory (owner, 2026-11-22)
Context is lost over time; the files are the memory. Sessions run for days on this method:
- **State lives in `docs/MILESTONES.md`**, not in a session's context or a handoff summary: its Terms,
  the step list, "Resume here" and the current section. Record each item there as it lands (with its
  commit), so a summarised session or a new one reads the truth. Trust the files and the code over any
  summary, including your own earlier messages.
- **A `/loop` pointed at MILESTONES and these rules** drives long work; each wake re-reads the files.
  The prompt stays short (`/loop follow docs/MILESTONES.md under the rules in AGENTS.md`): rules belong
  in the files or in code, not in the prompt.
- **A fresh session starts with an alignment discussion** with the owner (play back your understanding,
  propose a numbered plan), before acting. A term the files do not define gets defined there.
- **A step done by hand a second time** is flagged as missing automation, not repeated silently: the
  system must be runnable by the owner from these files, without an agent.

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
- **Point-in-time universe, built into the database.** Key securities by a permanent Vyreon asset
  key, never by ticker or by a provider id alone (C-023: an Intrinio security id changes on a rename,
  Meta is two records); provider ids and FIGIs are stored against the key as evidence. Tickers are
  reused (FB is now a ProShares ETF; META was a Roundhill ETF to
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
- **An observability gap becomes a notification** (owner, 2026-11-23): whenever something went wrong or stalled and only an
  agent looking noticed it, add a core watch check that posts it to the relevant Discord channel (problems to the alert
  channel, progress to the progress channel), so the next occurrence reaches the owner without anyone looking.
- Production code hard-codes no machine paths.
- Audits run without being asked: when a step or a merged batch lands (engine or core), start a
  read-only Codex audit **and a read-only Gemini 4 Neon audit** (`tools/audit/run-agy`, same scope) of what it touched,
  in the background, and bring the owner the triage. Wherever Codex is invoked, Gemini is invoked too when its
  quota allows (owner, 2026-11-23; 2026-11-26: Codex alone when Gemini's quota is out, recorded); both are treated
  alike. One audit per release. The loop is `tools/audit/` (README: audit, triage, fix, real day; no second audit
  round). Findings are
  claims to verify, from either reviewer; neither does code work.
- **Decided is not built (owner, 2026-11-27, after the trades-API reconciliation was found decided on 2026-11-04
  and never built).** Three states, never blurred, in the records and in every status report: decided; built (its
  commit and its test); verified on a real day (the date and where it shows in real output).
  - A decision entry carries its state and keeps it current.
  - A record that describes behaviour (`PROVIDER_FAULTS.md`'s handling, these rules, "Rules in force") is in the
    present tense only when the code does it; otherwise it says "decided, not built".
  - The decided-not-built sweep runs at every release, beside the audits (first run:
    `~/claude-artifacts/checks/decided-not-built-2026-11-27*.md`); its list goes to the owner.
  - To build: core reports a decision that has stayed "decided" too long, as it reports an open problem.
- One status document, `docs/MILESTONES.md`, kept short: mutable current state. Git is the
  history, so merge by default. Where a rebase or other clean-up of pushed history is really needed, first
  preserve the record (the old commits' hashes and subjects in a file under `docs/`, and a tag on the
  old tip), then do it. The commit before a prune of the status document is tagged
  (`milestones-<date>`). Commit each landed step
  with a short summary; separate commits for code, tests, docs and deploy.
- Provider faults go in `docs/PROVIDER_FAULTS.md` as they are found (evidence, dates, handling,
  reported or not); a rule that only compensates for one is judged against it in the rule census.
- Disk housekeeping on the laptop (owner, 2026-11-16): the WSL disk crash of 2026-11-15 was cargo
  build directories not deleted after their branches merged; WSL's virtual disk never shrinks on
  its own. Delete a worktree's `/home/diggz/wt/target-<name>` when its branch merges, and the
  worktree when it is no longer needed; check `du -sh /home/diggz/wt/target-*` at every merge.
- Decisions: a published standard first, then product intent, then the owner. Every verified audit
  finding is fixed (C-041). A status request gets the step list with where we are, what changed,
  decisions made and decisions waiting on the owner.
- Report delivered capability, not documents or test counts. Times for the owner in Mountain Time.

## Running the build (owner, 2026-11-26, after three days of restarts)
- **Canary:** every merge into the engine's release line computes one real day on one node before it can ship, and
  is compared with the last good build (engine faults, reference result, memory peak, time). Until the canary job
  exists, that real day is run by hand. No change reaches many nodes untested.
- **Science is frozen during production:** only blocker and operations fixes ship mid-run. Science changes wait on a
  branch for the fix pass, small ones included.
- **Scaling goes only through core:** the one-real-day gate, then the ramp the request budget allows. No slots are
  opened by hand.
- **Proven or hypothesis:** every finding brought to the owner is labelled. An agent's finding is verified, by me or
  a second reader, before it is stated as fact.
- **Fresh session at stage boundaries** (for example, once production runs). MILESTONES and the loop carry the state;
  the alignment talk comes first.
- **Prune:** MILESTONES holds current state only (tag before pruning). Memories and these rules are pruned after
  phase 1. The census covers the machinery (job kinds, automations that interact) as well as the science rules.
- **While the owner is away, pre-authorized** (each reported in the next status):
  - stop or pause the backfill on systematic breakage or runaway cost;
  - cancel, retry or reschedule blocks;
  - scale down;
  - release a fix for a blocker (an engine fault that stops publication, or that hits a whole measurement on every
    asset) through the gate, with evidence;
  - apply core settings through the scripts;
  - start read-only investigations.
- **Never without the owner:**
  - spend above the ceiling, or raise it;
  - change a science rule or method beyond such a blocker fix;
  - send anything to a provider or a customer;
  - delete data;
  - force-push;
  - create or change access keys.

## Machines
- Droplet `ssh vyreon-engine` (authoritative today): DigitalOcean Premium AMD, 8 shared vCPU / 16 GB,
  Postgres 16 on the volume `/mnt/vyreon_engine_v1`, settings in `/etc/vyreon.env`, daily capture and
  health timers running. Engine checkout `/root/vyreon-engine`.
- The laptop is a development machine. Vyreon compute there is allowed (owner, 2026-11-16) but
  keep its disk footprint small: WSL's virtual disk never shrinks, so prefer the cluster for
  anything that writes tens of gigabytes.
