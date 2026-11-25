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
| `release.sh [--dry-run] [--from N] --evidence <passed.json> <engine-image-tag> "<reason>"` | the whole worker-image release (C-073): identity and inventory from the image, push, register, pause, hand-over (preview, then its token), evidence and admit, ScaledJobs applied, daily build and core's manifest (with the running core image), the old image's Jobs deleted, resume, the new pods' first lease, then the old build's draining blocks (C-077: they finish their day; their Jobs deleted as each ends); stops at the first refusal and prints the evidence. `--core <core-image-tag>` deploys a core image the same safe way. Needs `KUBECONFIG`, `VYREON_DO_TOKEN_FILE` |
| `kubeconfig.sh [--if-needed \| --check] [file]` | refresh the operator's DOKS kubeconfig (7 days; mode 600, never printed; its end recorded in `<file>.expires`); `--if-needed` only within 2 days of its end or when the cluster refuses it (release.sh and health.sh call it first); `--check` says what it would do |
| `do.sh METHOD /v2/path [json]` | one DigitalOcean API call; any answer but 2xx is an error (exit 1, C-068) |
| `scale-pool.sh POOL_ID NAME COUNT` | set a worker pool's node count (costs money: the spend ceiling in docs/MILESTONES.md applies) |
| `health.sh` | one read-only screen of a run's health; oddities flagged with !!; run at every check |
| `delete-idle.sh` | delete the `day*` nodes with no unfinished worker pod (fails closed: an unreadable pod list deletes nothing; a node with a finished worker pod is drained; C-068); the autoscaler normally does it |
| `job-day-block.yaml` | day-block worker Job on the phase-1 image; set `metadata.name`, `completions`, `parallelism`, and the node pool |
| `job-tape.yaml` | tape worker Job on the phase-1 image; same fields |
| `backup-now.yaml` | an on-demand base backup of `vyreon-pg` (`kubectl create -f`; C-067) |
| `restore-drill.yaml` | the temporary cluster of the restore drill (`../README.md`, "Backups and the volume") |
| `recover-production.yaml` | the production recovery after the volume is lost: Cluster `vyreon-pg2`, archiving under its own name (`../README.md`, "Backups and the volume", D; C-068) |
| `drill-counts.sql` | the drill's comparison: per month, day revisions by state and measurement rows with a digest; diff source against restore |

A Job only claims work core has scheduled (`vyreon-core schedule <tape|day-block|ordered-pass> ...`,
see `crates/vyreon-core/src/main.rs`); an image is registered and admitted in core first
(`vyreon-core build register|admit`). The other manifests (`../00-namespace.yaml` to
`../40-daily.yaml`) define the standing system.
