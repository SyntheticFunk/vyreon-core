# Milestones

The one living status document. History is in git; decisions in `docs/DECISIONS.md`; the plan in
`docs/ARCHITECTURE.md`.

## Where things stand (2026-11-10)
- Engine: `engine-v1` tagged (vyreon-engine a046bf54). A fresh 2025-12-10 agrees laptop vs droplet to
  within 1e-11 (2026-11-09). The droplet is authoritative: Postgres, the daily capture, health checks.
- Architecture reviewed with the owner (`docs/ARCHITECTURE.md`); decided items in `docs/DECISIONS.md`.
- Kept inputs: the droplet archives prints, ACT01 pulls and company captures nightly to the volume and
  the Spaces bucket `vyreon-data` (C-004).
- Repo renamed from vyreon-manager (2026-11-10).
- The historical database build waits for the cluster (owner, 2026-11-08). Go-live is on the fixed 64.

## Plan (ARCHITECTURE §7)
1. **Core service** (this repo): Rust workspace; schema (the engine's store, the point-in-time
   universe, work, budget, coverage, artifacts); universe builder for the 64; worker API; budget;
   publish path; reader API; scheduler; notices.
2. **Engine as worker** (engine v1.x, E1–E5), alongside 1. Exit: the golden days through the worker
   path, and the block-start test.
3. **Cluster:** DOKS, CloudNativePG, KEDA; backup and restore drill; the store imported; a 2-node trial
   on a week of recent dates with the measurements (C-006); the daily chain moved over.
4. **Backfill, stage 1:** the recent fixed-universe range (~2025-03 onward).
5. **N assets:** point-in-time membership in the engine (E6–E8), then backfill to the provider starts
   (option EOD 2021-09-27). Its order against 4 is decided at phase 4 (ARCHITECTURE D5).
6. **Steady state;** the droplet retired.

Next: phase 1, the workspace and the schema.

## Open
- ARCHITECTURE §9 D1, D2, D5, D6, D7, D10: decided as their phases start.
- Intrinio: is the rate limit per access code or per account; do file downloads count.
