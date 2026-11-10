# Milestones

The one living status document. History is in git; decisions in `docs/DECISIONS.md`.

## Where things stand (2026-11-10)
- Engine: `engine-v1` tagged (vyreon-engine a046bf54). Laptop and droplet agree on a fresh
  2025-12-10 to within 1e-11; the droplet is authoritative and captures perishable data daily.
- The historical database build waits for this repo (owner, 2026-11-08).

## Plan
1. **Schema:** measurements (from the engine's `db/schema.sql`), the point-in-time universe
   (security id, dated ticker spans, listings, delistings, merges, provider start dates), the job
   queue (date blocks, claims, retries), the Intrinio request budget and usage, per-day field
   completeness.
2. **API:** asset history and session endpoints (`AGENTS.md`), JSON and Parquet.
3. **Worker:** the engine packaged as a container that claims a block, draws on the request budget,
   publishes to the manager and exits; a measured block run on the droplet.
4. **Orchestration:** chosen with measured numbers; a small trial (one week of dates on 2 nodes).
5. **Backfill** to the provider starts, oldest first (later days' own-history readings depend on
   earlier days); the spend is the owner's call.

## Open questions
- Where the database lives (the current droplet, a managed Postgres, or a dedicated node) and backups.
- How the engine publishes: through the manager's API, or straight into Postgres with the manager
  reading.
