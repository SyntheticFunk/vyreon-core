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

## Plan (ARCHITECTURE §8)
1. **Core service** (this repo): schema, universe for the 64, worker API, publish through core,
   scheduler, budget. Exit: a droplet-published day goes through core and reads back identical.
2. **Engine as worker** (engine v1.x: E1–E5, ranged bars, E10 day-local derived), alongside 1. Exit:
   golden days through the worker path; the block-start test.
3. **Cluster:** DOKS, CloudNativePG, KEDA; backup restore drill; a 2-node trial on a recent week with
   the measurements (C-006).
4. **First build:** the tape job for every session since 2023-05-24, day blocks from ~2025-03 on the
   fixed 64, the ordered pass behind them.
5. **Rebuilds and recomputes** exercised for real.
6. **After:** the reader API for vyreon-reporter; N assets (E6–E9) and the backfill to 2021-09-27;
   the droplet retired.

Next: the owner reviews ARCHITECTURE (design pass 2, §10 lists what to confirm); then milestone 1.

## Open
- ARCHITECTURE §10: seven proposals to confirm; E4's rules; company inputs on historical dates.
- Intrinio (owner to ask support): is the rate limit per access code or per account; do file
  downloads count.
