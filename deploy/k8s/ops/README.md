# Cluster operations

The scripts and Job templates used to run workers on the DigitalOcean Kubernetes cluster. Moved here
from a local scratch folder on 2026-11-21 so how the system runs is versioned with the code.

Nothing secret lives here. What the scripts need, from the environment:
- `KUBECONFIG`: the cluster's kubeconfig (download it from DigitalOcean; never commit it).
- `VYREON_DOKS_CLUSTER`: the cluster id.
- `VYREON_DO_TOKEN_FILE`: a DigitalOcean API token file (default `do-full-token.txt` at the
  repository root, which is git-ignored).

The worker secret `vyreon-worker` is made by hand (`kubectl create secret generic`, from files, never
from literals on a command line). Its keys: `worker-token` (core's worker token), `intrinio-key`,
`fred-key` (D-105's last curve fallback), `ordered-pass-pg` (the ordered pass's database login, mounted
only by that Job).

| File | Use |
|---|---|
| `do.sh METHOD /v2/path [json]` | one DigitalOcean API call |
| `scale-pool.sh POOL_ID NAME COUNT` | set a worker pool's node count (costs money: the spend ceiling in docs/MILESTONES.md applies) |
| `health.sh` | one read-only screen of a run's health; oddities flagged with !!; run at every check |
| `delete-idle.sh` | delete every `day*` node with no running, pending or starting worker pod; run after every run ends |
| `job-day-block.yaml` | day-block worker Job on the phase-1 image; set `metadata.name`, `completions`, `parallelism`, and the node pool |
| `job-tape.yaml` | tape worker Job on the phase-1 image; same fields |
| `backup-now.yaml` | an on-demand base backup of `vyreon-pg` (`kubectl create -f`; C-067) |
| `restore-drill.yaml` | the temporary cluster of the restore drill (`../README.md`, "Backups and the volume") |
| `drill-counts.sql` | the drill's comparison: per month, day revisions by state and measurement rows with a digest; diff source against restore |

A Job only claims work core has scheduled (`vyreon-core schedule <tape|day-block|ordered-pass> ...`,
see `crates/vyreon-core/src/main.rs`); an image is registered and admitted in core first
(`vyreon-core build register|admit`). The other manifests (`../00-namespace.yaml` to
`../40-daily.yaml`) define the standing system.
