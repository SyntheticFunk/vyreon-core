# The cluster: first run (two worker nodes, two golden days)

What this sets up, and nothing more: a DigitalOcean Kubernetes cluster with one core node
(PostgreSQL under CloudNativePG, `vyreon-core`) and a pool of two worker nodes that each compute
one day from an empty disk. KEDA and node autoscaling come with the build, after this run shows the
pieces work (see "After this run").

**Nothing here is created without the owner's word. Every command that costs money is marked $.**

## Cost (DigitalOcean prices read from the API, 2026-11-11)

| Item | Size | Price | For this run |
|---|---|---|---|
| Control plane | DOKS, not highly available | free | $0 |
| Core node | 1 x `s-4vcpu-8gb-amd` (160 GB disk) | $0.083 an hour ($56 a month) | about $1 for 12 hours |
| Worker nodes | 2 x `s-8vcpu-16gb-amd` (320 GB disk) | $0.167 an hour each | about $3 for 2 x 9 hours |
| Database volume | 60 GB block storage | $0.10 a GB-month | about $0.20 a day |
| Container registry | Basic plan (the worker image is 1.5 GB; the free plan holds 0.5 GB) | $5 a month | $5 |

About $10 if everything is deleted afterwards; the registry's $5 is most of it and is monthly.
Left running, the core node, volume and registry are about $67 a month; the worker pool is
deleted after every run.

## Before anything is created (free)

1. Build both images on the laptop and note their tags:
   - `docker build -f deploy/core/Dockerfile -t vyreon-core:$(git rev-parse --short HEAD) .`
   - in the engine checkout, branch `worker`: `deploy/worker/build.sh`
2. The worker build's identity and slot inventory, from the image itself:
   - `docker run --rm --entrypoint /opt/vyreon/bin/vyreon-worker <worker image> build-identity /opt/vyreon/bin /opt/vyreon/bin`
   - `docker run --rm --entrypoint /opt/vyreon/bin/vyreon-worker <worker image> inventory <that build> > inventory.json`
3. Install `doctl` and `helm` on the laptop (`kubectl` is there).

## Create ($)

```sh
# $ the registry, and the images in it
doctl registry create vyreon --subscription-tier basic --region nyc3
doctl registry login
for i in vyreon-core:<tag> vyreon-worker:<tag>; do
  docker tag $i registry.digitalocean.com/vyreon/$i && docker push registry.digitalocean.com/vyreon/$i
done

# $ the cluster with its core pool; then let it pull from the registry
doctl kubernetes cluster create vyreon --region nyc1 --version 1.36.5-do.3 \
  --node-pool "name=core;size=s-4vcpu-8gb-amd;count=1;label=vyreon=core"
# the registry's pull secret, for every pod of the namespace (after 00-namespace.yaml); read-only is
# the command's default (doctl has no --read-only flag: C-068, audit opsurface ops.md 5)
doctl registry docker-config > pull-config.json
kubectl -n vyreon create secret generic regcred --type=kubernetes.io/dockerconfigjson --from-file=.dockerconfigjson=pull-config.json
kubectl -n vyreon patch serviceaccount default -p '{"imagePullSecrets":[{"name":"regcred"}]}'

# the operator and the database (the volume is $)
kubectl apply --server-side -f \
  https://raw.githubusercontent.com/cloudnative-pg/cloudnative-pg/release-1.30/releases/cnpg-1.30.1.yaml
kubectl rollout status deployment -n cnpg-system cnpg-controller-manager
kubectl apply -f deploy/k8s/00-namespace.yaml -f deploy/k8s/10-postgres.yaml
```

Secrets, from files that are never printed (`$(tr -d '\n' < file)`):

```sh
openssl rand -hex 24 > worker-token && chmod 600 worker-token
kubectl -n vyreon create secret generic vyreon-core \
  --from-literal=worker-token-sha256="$(tr -d '\n' < worker-token | sha256sum | cut -d' ' -f1)"
```

Core, with its image tag in place of `VYREON_CORE_IMAGE`:

```sh
sed "s#VYREON_CORE_IMAGE#registry.digitalocean.com/vyreon/vyreon-core:<tag>#" deploy/k8s/20-core.yaml | kubectl apply -f -
kubectl -n vyreon rollout status deployment vyreon-core
```

## Load and schedule (free)

Inside the core pod (`kubectl -n vyreon exec deploy/vyreon-core -- vyreon-core ...`), in order:

```
universe load /opt/vyreon-core/seed/engine-v1-PROFILE.json /opt/vyreon-core/seed/intrinio-universe
calendar load /opt/vyreon-core/seed/engine-v1-sessions.json
build register <inventory.json copied in with kubectl cp>
build admit <build> <evidence.json: what the build has passed>
schedule tape FREE_REPORT_64 <build> 2025-12-10 2025-12-11
schedule day-block FREE_REPORT_64 <build> 2025-12-10 2025-12-11 1      # blocks of one day: two jobs
schedule ordered-pass FREE_REPORT_64 <build> 2025-12-10 2025-12-11
```

A build newest first schedules its day blocks in tranches, newest first (the claim takes the
earliest claimable block), but its ordered passes **once, for the whole range, before the first
tranche** (or after the last): passes scheduled with each tranche run every later pass again
after each earlier tranche (C-046).

The ordered pass's read-only login (C-020), with a password made on the spot, and the workers'
secret:

```sh
openssl rand -hex 24 > pass-password && chmod 600 pass-password
kubectl -n vyreon exec vyreon-pg-1 -- psql -d vyreon -c \
  "CREATE ROLE vyreon_pass LOGIN IN ROLE vyreon_ordered_pass PASSWORD '$(tr -d '\n' < pass-password)'"
printf 'host=vyreon-pg-rw port=5432 dbname=vyreon user=vyreon_pass password=%s' "$(tr -d '\n' < pass-password)" > ordered-pass-pg
kubectl -n vyreon create secret generic vyreon-worker \
  --from-file=worker-token=worker-token --from-file=intrinio-key=<the Intrinio key file> \
  --from-file=fred-key=<the FRED key file> --from-file=ordered-pass-pg=ordered-pass-pg
```

## Run ($ while the worker pool exists)

```sh
# $ two worker nodes, kept for the engine by a taint
doctl kubernetes cluster node-pool create vyreon --name day --size s-8vcpu-16gb-amd --count 2 \
  --label vyreon=worker --taint "vyreon=worker:NoSchedule"
```

All four keys (C-068: the tape and day-block pods mount `fred-key`, D-105's last curve fallback;
without it they never start, stuck in `CreateContainerConfigError`, and core sees no claim: the
kind's `STALLED`). Check: `kubectl -n vyreon get secret vyreon-worker -o jsonpath='{.data}' | python3 -c
'import json,sys; print(sorted(json.load(sys.stdin)))'` lists `fred-key`, `intrinio-key`,
`ordered-pass-pg`, `worker-token` (names only, no value).

Apply the three Jobs of `30-workers.yaml` one at a time, each after the one before has completed
(a worker that finds nothing to claim exits at once): `tape`, then `day-block`, then `ordered-pass`,
with the worker image in place of `VYREON_WORKER_IMAGE`. Watch with
`kubectl -n vyreon logs -f job/<name>` and `vyreon-core queue`.

What the run must show, against the droplet's own published days: both days published by different
nodes; the sensors E4 did not touch equal to the droplet's; the two nodes sharing the Intrinio
budget without a refused request; the ordered pass on the second day reading the first.

## Take down

```sh
doctl kubernetes cluster node-pool delete vyreon day          # stops the worker cost; always after a run
doctl kubernetes cluster delete vyreon --dangerous            # everything, with its volume (the database)
doctl registry delete                                         # the images
```

Deleting the cluster deletes the database: export what is to be kept first.

## After this run

- **KEDA and node autoscaling:** written (C-066): "Workers from the queue" below.
- Backups to Spaces (C-013) once a database is worth keeping.

## Daily runs (C-051)

**Replaced by KEDA once it runs the daily build (C-066):** the ScaledJobs of `50-keda.yaml` start
the daily session's tape, block and pass from the queue, so CronJob `daily` is suspended then
("Workers from the queue", 9). What follows is the CronJob as it stands until that switch.

From the session the owner names, each trading session is computed and published with nobody
starting anything, and worker nodes exist only while there is work:

- **Core queues** session T at **07:30 New York on the calendar day after T** (a Friday's on
  Saturday; weekends and holidays are not sessions): one tape job, one day block and one ordered
  pass, claimed before any backfill work, each only if not already queued by hand, none if T is
  already published, never twice (`core.daily_session`). Settings on core (`20-core.yaml`):
  `VYREON_CORE_DAILY_FROM` (the first session queued by core) and `VYREON_CORE_DAILY_BUILD` (an
  admitted build). Unset: nothing is queued. A build not admitted or withdrawn is the problem
  `DAILY_STOPPED` on the channel. The publishing deadline (C-050) then starts at the same session,
  each due by **20:00 MT** the next calendar day (`VYREON_CORE_DEADLINE_HOUR`).
- **The cluster starts the worker:** CronJob `daily` (`40-daily.yaml`) at 07:45 New York, Tuesday
  to Saturday, starts the worker Job itself (worker image, the `default` service account and the
  `regcred` pull secret as the other worker Jobs, `concurrencyPolicy: Forbid`, at most three pods
  one after another, stopped after 23 h). Its pod runs the tape, the day and the ordered pass in
  order. Every worker pod carries `cluster-autoscaler.kubernetes.io/safe-to-evict: "false"`, so
  the autoscaler never removes a node under a running day. A run still going at the next 07:45 is
  never joined by a second; that start is made once it ends, within 6 h
  (`startingDeadlineSeconds`; Kubernetes, "CronJob").
- **Accepted cost (the owner, 2026-11-17, retiring round v's gate):** on a weekday morning with
  nothing queued (the morning after an exchange holiday) a node still starts, the first pod's tape
  container asks again every minute for 30 minutes (a late queue, audit round x, 6), the
  containers find nothing to claim and exit, and the node goes: about $0.25 a time, about nine
  times a year. The CronJob's containers claim only the daily runs' jobs (`VYREON_WORKER_DAILY`,
  round x, 13), never a backfill block.
- **The pool scales:** pool `day` autoscaled with min 0; the worker Job's pending pod brings up one
  node; the node is removed after it has been unneeded for the autoscaler's unneeded time
  (10 minutes by the upstream default).

Cost (the owner's): about one `s-8vcpu-16gb-amd` node for 6 to 7 hours a session, about $1.10 a
session at $0.167 an hour, about $24 a month; more for a retried day.

### What DigitalOcean's documentation says about scaling to zero (read 2026-11-17)

- Autoscaler page (https://docs.digitalocean.com/products/kubernetes/how-to/autoscale/): "The
  Cluster Autoscaler supports scaling a given node pool down to zero"; `min-nodes` "must be greater
  than or equal to `0`"; it recommends "at least one fixed node pool of the smallest size with one
  node" (our pool `core`) and, for larger pools, autoscaling "and set the minimum number of nodes
  to zero".
- Contradiction: the `doctl ... node-pool create` reference
  (https://docs.digitalocean.com/reference/doctl/reference/kubernetes/cluster/node-pool/create/)
  says of `--min-nodes` "Scale-to-zero is not supported"; the `node-pool update` reference says
  only "Default: `0`". The API is what decides: the GET after the PUT below must show
  `"min_nodes": 0`. If the API refuses 0, the smallest alternative is min 1 (one idle node, about
  $120 a month) or keeping today's practice: a script or the owner creates the pool for a run and
  deletes it after.
- The API (https://docs.digitalocean.com/reference/api/digitalocean/#tag/Kubernetes/operation/kubernetes_update_nodePool):
  `PUT /v2/kubernetes/clusters/{cluster_id}/node_pools/{node_pool_id}` requires `name` and `count`;
  `auto_scale`, `min_nodes`, `max_nodes`, `labels`, `taints` are optional. Send the labels and the
  taint again: an update may clear what it leaves out (doctl states this for taints).
- Scale-down: DigitalOcean publishes no default timings; the cluster's
  `cluster_autoscaler_configuration` (`scale_down_unneeded_time`, `scale_down_utilization_threshold`)
  can set them. Upstream (https://github.com/kubernetes/autoscaler/blob/master/cluster-autoscaler/FAQ.md,
  "What types of pods can prevent CA from removing a node?"): a pod annotated
  `cluster-autoscaler.kubernetes.io/safe-to-evict: "false"` keeps its node; "If a node is
  unneeded for more than 10 minutes, it will be terminated." Pods of completed Jobs (Succeeded or
  Failed with `restartPolicy: Never`) do not keep a node: no documentation page says so; the
  autoscaler's source treats such terminal pods as drainable, before it reads the annotation.
  Check it on the first run: the node goes within about 10 minutes of the run's end.
- Scale-up from zero: the upstream DigitalOcean provider builds an empty pool's node template from
  the pool's own labels and taints (`cloudprovider/digitalocean/digitalocean_node_group.go`), so
  the pods' `nodeSelector: {vyreon: worker}` and toleration bring up a node. Not stated by
  DigitalOcean: check it on the first run.

### Apply, in order (the owner)

1. **Core image** with migration 0019 (build, push, `sed ... 20-core.yaml | kubectl apply -f -`
   as above), the daily settings still commented out: nothing is queued. Check:
   `kubectl -n vyreon exec deploy/vyreon-core -- vyreon-core daily` says "daily runs are off".
2. **Annotate the running Jobs' successors:** `30-workers.yaml` now carries the annotation on all
   three Jobs; it applies to the next Jobs created from it (a running Job's pods keep theirs).
3. **Pool `day` to autoscaling ($ while nodes run).** With no worker running, read the pool, then
   set it (DigitalOcean token from a file, never printed; `count` = the pool's current node count,
   so the call itself removes nothing; `max_nodes` is the owner's ceiling):
   ```sh
   C=5f56e6e0-1c74-4dc9-bfdb-962ee55ebff3 P=5b683697-6614-4a62-9fb9-315a10a840a3
   curl -sS -H "Authorization: Bearer $(tr -d '\n' < <token file>)" \
     https://api.digitalocean.com/v2/kubernetes/clusters/$C/node_pools/$P
   curl -sS -X PUT -H "Content-Type: application/json" -H "Authorization: Bearer $(tr -d '\n' < <token file>)" \
     -d '{"name":"day","count":<current>,"auto_scale":true,"min_nodes":0,"max_nodes":<max>,
          "labels":{"vyreon":"worker"},"taints":[{"key":"vyreon","value":"worker","effect":"NoSchedule"}]}' \
     https://api.digitalocean.com/v2/kubernetes/clusters/$C/node_pools/$P
   ```
   The answer must show `"auto_scale": true, "min_nodes": 0`, the label and the taint. Idle nodes
   then go after about 10 minutes. (If the pool was deleted, create it with
   `doctl kubernetes cluster node-pool create vyreon --name day --size s-8vcpu-16gb-amd --count 0
   --auto-scale --min-nodes 0 --max-nodes <max> --label vyreon=worker --taint "vyreon=worker:NoSchedule"`
   and use the new id.)
4. **The CronJob (superseded by KEDA, C-066; C-068):** under KEDA skip this step: `40-daily.yaml` is
   now applied suspended (`suspend: true`) and stays so ("Workers from the queue", 9); it is the
   fallback only. What follows is how it was applied before KEDA. If round v's gate was applied, remove it first: `kubectl -n vyreon delete
   cronjob daily daily-worker --ignore-not-found` and `kubectl -n vyreon delete serviceaccount,role,rolebinding
   daily-gate --ignore-not-found`. Then
   `sed -e "s#VYREON_WORKER_IMAGE#registry.digitalocean.com/vyreon/vyreon-worker:<tag>#" deploy/k8s/40-daily.yaml | kubectl apply -f -`.
   The worker image must be the build named in step 5. Until step 5 nothing is queued, so each
   morning's pod finds nothing to claim and exits (a node for about 40 minutes, about $0.25); apply
   it with step 5, or suspend it until then
   (`kubectl -n vyreon patch cronjob daily -p '{"spec":{"suspend":true}}'`).
5. **Switch on** (the owner's word that the system is ready): the build admitted
   (`vyreon-core build admit`), then in `20-core.yaml` uncomment `VYREON_CORE_DAILY_FROM` (the
   first session to compute by itself: normally the session after the last one scheduled by hand)
   and `VYREON_CORE_DAILY_BUILD`, and apply. Check: `vyreon-core daily` shows the settings, "due
   now" and the next session's time; it queues within a minute of 07:30 New York.
6. **First morning:** `kubectl -n vyreon get cronjob,jobs,pods -o wide` (a Job `daily-…`; a node
   comes up, the pod runs `tape`, `day-block`, `ordered-pass`),
   `vyreon-core queue`, the 09:00 digest's "Daily runs" line; after the run, the node is gone
   within about 10 minutes (`kubectl get nodes -l vyreon=worker`). To run at once instead of
   waiting: `kubectl -n vyreon create job daily-now --from=cronjob/daily` (a Job made by hand is
   not held back by `Forbid`: check `kubectl -n vyreon get jobs -l work=daily` first).

**Off:** comment out the two daily settings and apply (core queues nothing more), and
`kubectl -n vyreon patch cronjob daily -p '{"spec":{"suspend":true}}'`.

## Workers from the queue (C-066)

KEDA starts a worker Job for each job core can hand out, and the pool `day` autoscales, so work
scheduled in core runs with nobody applying Jobs, counting completions or deleting idle nodes.
Manifest `50-keda.yaml` (its header explains every choice); the database side is migration 0046.
**Written, not applied. Nothing here is applied without the owner's word; $ marks what costs money.**

How it works: every 30 seconds each ScaledJob asks core's database, as the read-only login
`vyreon_keda`, `SELECT core.claimable_count('<KIND>', '<build>'[, daily])`: the jobs a worker of
that build could claim now (`core.claimable_job`). A claimed job leaves the count. KEDA (strategy
`accurate`) creates one Job per counted job, less the Jobs whose pod is not running yet, up to the
ScaledJob's cap. A pod that does not fit waits Pending; the cluster autoscaler adds a `day` node for
it (a few minutes) and removes a node that has had no worker pod for about 10 minutes. Each Job is
one pod, no retry: a failed job goes back to core's queue and the next poll starts a new pod for it.

| ScaledJob | Counts | Cap | Why |
|---|---|---|---|
| `tape` | TAPE, every priority | 24 | 3 a node (2 CPUs each), 8 nodes: the test block ran 24 tape pods on 8 nodes |
| `day-block` | DAY_BLOCK, not daily | 12 | one a node; the request budget (below) |
| `day-block-daily` | DAY_BLOCK, daily (priority 1) | 1 | the daily session never waits behind the backfill's 12 |
| `ordered-pass` | ORDERED_PASS, every priority | 1 | date order (C-046) |

### 1. Install KEDA (free; once)

KEDA 2.21 is tested on Kubernetes 1.34 to 1.36 (https://keda.sh/docs/2.21/operate/cluster/,
"Kubernetes Compatibility"; the cluster runs 1.36.5). The release manifest, pinned to 2.21.1
(2026-11-15), applied as KEDA's YAML install says (https://keda.sh/docs/2.21/deploy/, "Deploying
using the deployment YAML files"), no Helm:

```sh
curl -sSLO https://github.com/kedacore/keda/releases/download/v2.21.1/keda-2.21.1.yaml
echo "a84d4e6a53df3ad88f6a69e3288b6cbbd699746f9deaea8c7237cbc345061d58  keda-2.21.1.yaml" | sha256sum -c
kubectl apply --server-side -f keda-2.21.1.yaml
kubectl -n keda rollout status deployment keda-operator
kubectl -n keda get pods        # keda-operator, keda-metrics-apiserver, keda-admission: Running
```

It makes the namespace `keda` with three Deployments (operator, metrics API server, admission
webhooks that check ScaledJobs on apply), each requesting 100m CPU and 100 MiB and limited to 1 CPU
and 1000 MiB. The worker pools are tainted, so all three run on the core node
(`s-4vcpu-8gb-amd`: 3.89 CPUs and 6.3 GiB allocatable, 1.21 CPUs and 1.8 GiB requested on
2026-11-22, before KEDA; Postgres declares no request): +0.3 CPU and +300 MiB requested. Their real
use was not measured (the cluster has no metrics API; the control panel's node graphs show memory).

### 2. The trigger's login (free; once; nothing printed)

Migration 0046 needs the group role `vyreon_scaler`, which only a superuser can create (the
database's owner cannot; a fresh cluster gets it from `10-postgres.yaml`'s `postInitSQL`). On this
cluster, **before** deploying a core image that carries 0046 (its migrate step stops otherwise,
saying so):

```sh
kubectl -n vyreon exec vyreon-pg-1 -c postgres -- psql -d vyreon -c "CREATE ROLE vyreon_scaler NOLOGIN"
```

Then deploy core as usual (`sed ... 20-core.yaml | kubectl apply -f -`), and make the login and its
Secret. The Secret lives in `vyreon`: a namespaced TriggerAuthentication reads its Secret from its
own namespace (KEDA 2.21 `pkg/scaling/resolver`, `getTriggerAuthSpec`; only a
ClusterTriggerAuthentication reads from `keda`).

```sh
openssl rand -hex 24 > keda-pg-password && chmod 600 keda-pg-password
# the password goes through stdin (printf is a shell builtin): it is on no command line
printf "CREATE ROLE vyreon_keda LOGIN IN ROLE vyreon_scaler PASSWORD '%s'" "$(tr -d '\n' < keda-pg-password)" \
  | kubectl -n vyreon exec -i vyreon-pg-1 -c postgres -- psql -d vyreon -v ON_ERROR_STOP=1 -q
kubectl -n vyreon create secret generic vyreon-scaler-pg --from-file=password=keda-pg-password
```

`vyreon_keda` can run `core.claimable_count` and nothing else: no table, view or other function
(test `the_scaler_counts_what_a_worker_of_the_build_can_claim_and_reads_nothing_else`). The
existing `vyreon_pass` (`ordered-pass-pg`) cannot read `core` at all and is not reused: it reads the
published store, which the trigger has no need of.

### 3. Pool `day` autoscaling ($ while nodes run; the owner)

With no worker running, read the pool, then set it. `count` is the pool's current node count, so
the call itself removes nothing; the labels and the taint are sent again because an update clears
what it leaves out (doctl `node-pool update`: "Existing labels are removed from the node pool if
they are not specified"; the same for taints). `max_nodes` 13 = the two day-block caps.

```sh
export VYREON_DOKS_CLUSTER=5f56e6e0-1c74-4dc9-bfdb-962ee55ebff3 P=5b683697-6614-4a62-9fb9-315a10a840a3
deploy/k8s/ops/do.sh GET /v2/kubernetes/clusters/$VYREON_DOKS_CLUSTER/node_pools/$P
deploy/k8s/ops/do.sh PUT /v2/kubernetes/clusters/$VYREON_DOKS_CLUSTER/node_pools/$P \
  '{"name":"day","count":<current>,"auto_scale":true,"min_nodes":0,"max_nodes":13,
    "labels":{"vyreon":"worker"},"taints":[{"key":"vyreon","value":"worker","effect":"NoSchedule"}]}'
```

The answer must show `"auto_scale": true, "min_nodes": 0, "max_nodes": 13`, the label and the taint.

What DigitalOcean says about 0 (read 2026-11-22): the autoscaling page
(https://docs.digitalocean.com/products/kubernetes/how-to/autoscale/, last updated 2026-10-29):
"The Cluster Autoscaler supports scaling a given node pool down to zero", the minimum "must be
greater than or equal to `0`", and for larger pools "enable autoscaling and set the minimum number
of nodes to zero" beside "at least one fixed node pool of the smallest size with one node" (pool
`core`). The doctl `node-pool create` reference still says of `--min-nodes` "Scale-to-zero is not
supported" (https://docs.digitalocean.com/reference/doctl/reference/kubernetes/cluster/node-pool/create/);
the `node-pool update` reference and the API's schema (`min_nodes`: "The value will be `0` if
`auto_scale` is set to `false`", digitalocean/openapi `kubernetes/models/node_pool.yml`) say
nothing either way. The newer page and the API's answer decide: the GET after the PUT, then the
pool reaching 0 nodes after the first run. If the API refuses 0: min 1 (one idle node, about $120
a month) or the pool created and deleted around each run as before.

Scale-up from zero: the autoscaler builds an empty pool's node from DigitalOcean's pool template
(`GET .../node_pools_template/day`, read 2026-11-22: labels `vyreon=worker` and
`doks.digitalocean.com/node-pool=day`, taint `vyreon=worker:NoSchedule`, allocatable 7378m CPU and
13577 MiB), so the pods' node selector and toleration match it. Scale-down: a node whose pods carry
`cluster-autoscaler.kubernetes.io/safe-to-evict: "false"` (every worker pod) is never removed while
they run; one with no such pod left is removed after 10 minutes unneeded (upstream default:
https://github.com/kubernetes/autoscaler/blob/master/cluster-autoscaler/FAQ.md; DigitalOcean
publishes no default; the cluster's `cluster_autoscaler_configuration.scale_down_unneeded_time` can
set it, `PUT /v2/kubernetes/clusters/{id}`).

**Pool `day-std`, the Intel fallback (C-068; $ only while nodes run; the owner).** nyc1 has refused
AMD capacity (MILESTONES "Layout"). Every worker pod now allows pool `day-std` (Intel
`s-8vcpu-16gb`, same label and taint) and prefers `day` (node affinity, `50-keda.yaml`). The
scheduler places a pod on an existing `day` node first; when no node fits, the autoscaler grows a
pool that fits, and after a failed scale-up (DigitalOcean out of AMD droplets) it backs that pool off
(upstream: `max-node-provision-time`, 15 minutes, then a backoff) and grows the other, so a stockout
no longer leaves pods Pending for ever. Which pool it grows when both can is the autoscaler's
expander's choice, which DigitalOcean does not document or let us set; the preference steers the
scheduler, not the scale-up. So Intel nodes may be chosen when AMD is available: slower (AMD runs
stages at 0.54-0.73 of Intel's time), the same caps. Set it autoscaled, as `day` (same PUT, the
pool's id from `GET .../node_pools`):
```sh
deploy/k8s/ops/do.sh PUT /v2/kubernetes/clusters/$VYREON_DOKS_CLUSTER/node_pools/<day-std id> \
  '{"name":"day-std","count":0,"auto_scale":true,"min_nodes":0,"max_nodes":13,
    "labels":{"vyreon":"worker"},"taints":[{"key":"vyreon","value":"worker","effect":"NoSchedule"}]}'
```
**Cost (C-068 round 2, 18):** nodes follow the pods and the pods KEDA's caps: 12 backfill blocks and
the daily one a node each, 24 tapes three a node (a tape does not fit beside a block), passes beside
blocks. With every cap working that is up to 21 nodes, if tapes of later dates run while 12 earlier
blocks do (in a newest-first build blocks wait for every earlier tape, so the two rarely overlap,
but nothing forbids it). So the pools' maxima are the cap that binds: `day` 13 plus `day-std`'s.
`day-std` at 13: up to 26 nodes, $4.34 an hour; at 6: 19, $3.17; at 0 (no autoscaling): no fallback,
13, $2.17. The owner's call; a lower maximum is a partial fallback. **Check on
the first runs:** which pool the autoscaler grows (`kubectl get nodes -L doks.digitalocean.com/node-pool`);
if it grows `day-std` while `day` has room, say so in MILESTONES (a cost and speed question, not a
correctness one).

### 4. Apply the ScaledJobs (free until pods start)

With the worker image and its build (`build register` printed it; the build must be admitted):

```sh
IMG=registry.digitalocean.com/vyreon/vyreon-worker:<tag> BUILD=<commit>+<inventory>
sed -e "s#VYREON_WORKER_IMAGE#$IMG#" -e "s#VYREON_ENGINE_BUILD#$BUILD#" deploy/k8s/50-keda.yaml | kubectl apply -f -
kubectl -n vyreon get scaledjob      # READY True for all four; ACTIVE True where work is claimable
```

A new image (C-068), **in this order** (the order is what keeps history in date order):
1. `vyreon-core build register <inventory>` for the new build. **Do not admit it yet.** An admitted
   successor's pod could claim a later day's pass while an earlier day's pass still belongs to the
   old build, and publish that day's history one reading short; the hand-over refuses an admitted
   successor.
2. If the new build reads other profile revisions than the old one, schedule its day blocks over the
   days the old build published (the hand-over names them and refuses until they are covered: C-064,
   such days are recomputed).
3. `vyreon-core build hand-over --preview <old> <new> "<reason>"`: it withdraws the old build if it
   still runs (its live leases end) and lists every queued or failed tape, day block and ordered pass
   it moves, and the old tape verify jobs it cancels, with a token. A running old build changes its
   jobs as it goes: preview with the queue paused (`queue pause`) or expect to preview again.
   `vyreon-core build hand-over --expect <token> <old> <new> "<reason>"` does exactly that, in one
   transaction (refused if anything changed since the preview).
4. `vyreon-core build admit <new> <evidence>`; set `VYREON_CORE_DAILY_BUILD` to it if the daily runs
   are on; `queue resume` if paused.
5. Apply the ScaledJobs again with the new image and build.
Without the hand-over the old build's queued tapes and blocks hold the new build's blocks and passes
for ever and its passes are claimed by no pod (core's `STALLED` chain says so). `rollout.strategy: gradual` keeps running Jobs (KEDA's default would delete them).
**Never `kubectl delete scaledjob` while a worker runs:** its Jobs are owned by it and deleted with
it (`--cascade=orphan` keeps them); deleting KEDA's manifest deletes its CRDs and with them every
ScaledJob and Job.

### 5. The caps, and where to change them

- **KEDA's caps:** `maxReplicaCount` of each ScaledJob in `50-keda.yaml`; apply again (running Jobs
  stay). Day blocks: raise from 12 only from core's request ledger, the owner's rule (nodes only
  while the request budget has headroom): one node averaged 101 requests a minute over its wall
  time with bursts of 500-760 while a day downloads (2026-11-22); core grants at most 1,900 a minute
  (`VYREON_CORE_INTRINIO_PER_MINUTE`), so more nodes than the budget holds only wait in it, at node
  cost. Check: `vyreon-core budget [hours]` (C-068): what is committed now, the leases never
  released, and the busiest minutes **booked by the minute each lease was granted**: what its worker
  reported using, the full size of leases never released (a worker that died, or whose release never
  reached core), and "at most": the minute's and the previous minute's leases together, an upper
  bound on the requests made in the minute (a lease's requests fall within 35 s of its grant).
  `core.request_usage` alone is no measure of a minute: it books usage when a lease is released, so
  two releases either side of a minute's end split one minute's requests, and a dead worker's
  requests never arrive. Judge a raise on "at most" and on Intrinio's own account usage for the same
  hours (Intrinio MCP `get_account_usage`). Core's budget itself is not fooled: an unreleased lease
  is held at its full size until a minute after its validity.
- **The ordered pass's memory and time (C-068; measured, not known):** memory request = limit = 3Gi
  (so a pass and a block, 10Gi, fit a node's 13.3 GiB together and the pass is never evicted under
  pressure; above 3.2Gi a pass no longer fits beside a block and takes a node of its own), deadline 2 h
  (`50-keda.yaml`). Each pass reads every published date of its base measurements, so its memory and
  time grow with the store (engine audit finding 14). Measure on the recompute's 10 passes and at
  about 100, 300 and the last of the 597: near a pass's end, `kubectl -n vyreon exec <pass pod> --
  cat /sys/fs/cgroup/memory.peak` (bytes) and the Job's duration (`kubectl -n vyreon get jobs -l
  scaledjob.keda.sh/name=ordered-pass`). If the peak passes 2 GiB or a pass takes over 30 minutes,
  raise the request and limit together or the deadline before going on and record the numbers in
  MILESTONES. An `OOMKilled` pass is a lost lease: free twice, then charged (C-035 amended); five on
  one session leave it FAILED and the later passes wait (JOB_FAILED).
- **The node cap** (the hard cost cap): pool `day` `max_nodes` (step 3's PUT with a new value).
  Keep it at least `day-block` + `day-block-daily`, else a block pod waits Pending (it holds no
  job, so nothing stalls, but it is not computing).

### 6. Cost guard ($)

At most `max_nodes` x $0.167 an hour: 13 nodes, $2.17 an hour, $52 a day if every slot is busy, plus
`day-std`'s maximum (3: the pools' maxima are the cap that binds).
Nodes exist only while worker pods run, plus about 10 minutes each. `ops/health.sh` shows spend
against the ceiling (MILESTONES "Machines and money") and flags a worker node with no worker pod
(`!! idle node`); the autoscaler should leave none for longer than about 10 minutes. The account's
Droplet limit (30) is above 13 workers plus the other machines.

### 7. Seeing what KEDA does

```sh
kubectl -n vyreon get scaledjob                                   # READY, ACTIVE, TRIGGERS
kubectl -n vyreon describe scaledjob day-block                    # conditions and events
kubectl -n vyreon get jobs -l scaledjob.keda.sh/name=day-block    # the Jobs it started
kubectl -n keda logs deploy/keda-operator --since=10m | grep -E "Scaling Jobs|Creating jobs|error"
kubectl -n vyreon get events --field-selector reason=TriggeredScaleUp   # the autoscaler adding a node
kubectl get nodes -l doks.digitalocean.com/node-pool=day
kubectl -n vyreon exec deploy/vyreon-core -- vyreon-core queue    # what core says is claimable
deploy/k8s/ops/health.sh                                          # pods that found nothing, idle nodes
```

**A pod that fails before it claims** (C-068): a Secret key missing (pod stuck in
`CreateContainerConfigError`), a database login refused (the pass exits at once), an image that
cannot be pulled. Core never hears from it, so no attempt is charged, and KEDA 2.21 has no backoff or
limit on a ScaledJob's failed Jobs: a pod that exits is replaced every poll while its job is
claimable. You see it: on the channel, that kind's `STALLED` after 15 minutes (claimable, none
leased); on `ops/health.sh`, failed Jobs per ScaledJob in the last hour and pods stuck before running;
and directly with `kubectl -n vyreon get jobs -l scaledjob.keda.sh/name=<name>` and `kubectl -n vyreon
describe pod <pod>` (the events say why; `failedJobsHistoryLimit` keeps the last 30 failed Jobs and
their logs). Stop the loop by pausing that ScaledJob (8, step 2), fix the cause, unpause.

The operator logs "Number of running Jobs" and "Number of pending Jobs" every poll. A trigger that
cannot reach the database shows READY False ("Triggers defined in ScaledJob are not working
correctly") and starts nothing; core's watch then raises STALLED after 15 minutes (C-065).

### 8. Stopping everything

1. **No new work:** `vyreon-core queue pause "<reason>"`. Nothing is claimable, so KEDA starts
   nothing; running workers finish the job they hold. `queue resume` undoes it.
2. **No new pods, whatever core holds** (a ScaledJob cannot be set to 0 replicas; its pause is the
   way): `kubectl -n vyreon annotate scaledjob --all autoscaling.keda.sh/paused=true`
   (running Jobs keep running); `kubectl -n vyreon annotate scaledjob --all autoscaling.keda.sh/paused-`
   undoes it.
3. **Stop running workers too:** after 1 or 2,
   `kubectl -n vyreon delete jobs -l scaledjob.keda.sh/name=<tape|day-block|day-block-daily|ordered-pass>`
   (KEDA labels each Job with its ScaledJob). Their leases expire and the jobs are queued again
   (each an attempt). Or `vyreon-core build withdraw <build> <reason>`: it ends the build's live
   leases at once.
4. **No nodes:** the autoscaler removes idle nodes within about 10 minutes; to remove them at once,
   `deploy/k8s/ops/delete-idle.sh`. To forbid nodes entirely, step 3's PUT with
   `"auto_scale":false,"count":0` (this deletes every node of the pool, running or not).

### 9. The daily runs: KEDA replaces CronJob `daily`

Once core queues daily work (C-051) for the build in `50-keda.yaml`, the ScaledJobs run it: the
tape (`tape`, which counts every priority), the block (`day-block-daily`, outside the backfill's
cap) and the pass (`ordered-pass`). Suspend the CronJob then, and never run both:
`kubectl -n vyreon patch cronjob daily -p '{"spec":{"suspend":true}}'`. Why:
- Both would start: KEDA within 30 seconds of core's 07:30 queueing, the CronJob at 07:45. Whichever
  pod claims second finds nothing; the CronJob's first pod then waits 30 minutes for a late queue,
  holding a node (about $0.10 every morning).
- KEDA starts nothing on a morning with nothing queued (the CronJob's accepted $0.25, C-051) and
  starts the pass whenever it becomes claimable, which during the backfill is after the backfill's
  earlier passes (the CronJob's pod gave up on it the same morning).
- The block has its own slot, and its own node by priority (C-068; round 2, 10): PriorityClass
  `vyreon-daily` (1000, may preempt) above the tapes and passes (0), and level with
  `vyreon-backfill-block` (1000, preempts nothing), so it can never evict a backfill block. With 12
  backfill blocks on 12 nodes and tapes or a pass filling the 13th, the daily block evicts the tape
  or pass pods on one node (their leases expire, their jobs are claimed again, the attempt free);
  where that makes no room (a pass beside a block), it waits Pending and the autoscaler adds the
  13th node. In a stockout it waits (the deadline alert, 20:00 MT, says so). A cap of 11 backfill
  blocks would reserve a Job slot, not the 6 CPUs, since tapes (2 each) and passes (1) could still
  fill the room.
- **The daily tape is claimed ahead** of the backfill's (priority 1: a claim takes the daily runs'
  jobs first). **The daily block still waits for every unfinished earlier tape** (the claim's gate:
  a block reads the tape history before its day, from `tape_history_from`), so on a morning while the
  backfill's tapes are still running it waits for them (the test block's tapes: 240 in 42 minutes on
  24 pods, so the whole range's ~600 in under 2 hours). That gate is right and stays; the deadline's
  13 h 30 min cover it.
- What the CronJob did that KEDA does not: one pod for all three kinds on one node. Under KEDA the
  tape's node is idle for a poll when the tape ends and the block's pod takes it (6 + 2 CPUs do
  not fit together; the tape is minutes), and the pass (1 CPU) fits beside the block.
`40-daily.yaml` is applied suspended (C-068) and stays suspended; it is the fallback (KEDA down):
pause the ScaledJobs, then resume the CronJob.
C-065's 30-minute STALLED allowance for daily-only work was for the CronJob's 15-minute gap; with
KEDA 15 would do (a core change, not made here).

### 10. First run under KEDA: the recompute of the test block (the owner; $ about $5)

Two blocks of 5 sessions, 2024-07-09..07-22, on **the build the ScaledJobs run** (C-068 round 2, 8:
the rollout's image, 090d7aea's build as `build register` printed it; a block of any other build
starts no pod), started by KEDA alone. Before: run.sh has finished and deleted its node; steps 1 to 4
done (core image with C-065 and 0046, the login, the pool autoscaled at 0 nodes, the ScaledJobs on
that image); 914ffed7's work handed to that build and the build admitted (step 4's order); CronJob
`daily` suspended.

```sh
BUILD=<the build in 50-keda.yaml as applied: kubectl -n vyreon get scaledjob day-block -o jsonpath='{.spec.triggers[0].metadata.query}'>
kubectl -n vyreon exec deploy/vyreon-core -- vyreon-core schedule day-block FREE_REPORT_64 "$BUILD" 2024-07-09 2024-07-22 5
kubectl -n vyreon exec deploy/vyreon-core -- vyreon-core queue    # the two blocks QUEUED under that build ("build")
```

(The passes of these days are queued again under the new build as each day publishes, C-037/C-062;
if the build's tape method changed, schedule its tapes first and `tape` runs them.)

What to watch, and what passes:
1. **One pod per claimable job.** `kubectl -n vyreon get jobs -l scaledjob.keda.sh/name=day-block`
   shows exactly 2 Jobs within a minute of the schedule and no third while the nodes come up
   (5 to 10 polls); the operator's log says "Number of pending Jobs: 2", then "No need to create
   jobs". `health.sh`: 0 pods that found nothing to claim. Each pass: one `ordered-pass-…` Job at a
   time, started within a poll of its day publishing. Count any extra pods (a poll inside a pod's
   sub-second claim window); more than one in the run is a finding.
2. **Nodes added and removed.** Two `TriggeredScaleUp` events, two `day` nodes within about 5
   minutes, each block on its own node, the pass pods beside a block. After each block ends its
   node goes within about 15 minutes (unless a pass runs there); after the last pass, 0 nodes:
   `kubectl get nodes -l doks.digitalocean.com/node-pool=day` empty and the pool's GET `count: 0`.
   That settles the scale-to-zero question.
3. **Two nodes sharing the request budget.** Core's ledger by minute (query in 5): the busiest
   minutes at or under 1,900 with two workers; the workers' logs without a provider 429; Intrinio's
   own usage for the window (Intrinio MCP `get_account_usage`) against the ledger's sum.
4. **The work itself:** both blocks DONE, 10 days published, gate a on them
   (`vyreon-core refcheck list 2024-07-09 2024-07-22`), STALLED and CLAIMS_EMPTY not opened.
5. **Idle stays idle:** an hour after the last pass, no Job and no node was started.
Cost: two nodes for about 13 hours each (first day about 226 minutes, later days 100-134), about
$4.50, plus the passes.

## Perishable archive (C-052)

The perishable inputs a published day read (the cohort prints, which the provider keeps about
five sessions, and the ACT01 interval pulls) go from each worker straight to the bucket
`vyreon-data` under `perishables/<category>/<session>/<sha256>.tar.zst`. Workers hold no storage
key: for each object core signs a PUT URL valid ten minutes (AWS Signature Version 4, which Spaces
accepts: https://docs.digitalocean.com/reference/api/spaces/), the worker uploads, and core reads
the object back (size, MD5, SHA-256) before it counts it kept. `vyreon-core archive list [date]`
shows each object and what is missing; the 09:00 digest counts what is missing. The droplet's
nightly archive (`deploy/droplet/archive-perishables.sh`, prefix `droplet-capture/`) keeps running
until the cluster's archive has been seen working; retiring it is a separate step.

### Apply, in order (the owner)

1. **A key for the bucket only** (DigitalOcean control panel, Spaces Object Storage, Access Keys:
   https://docs.digitalocean.com/products/spaces/how-to/manage-access/): a *limited access* key
   for the bucket `vyreon-data` alone, with the narrowest permission that can write
   (the panel offers Read, or Read/Write/Delete; core only signs PUT and HEAD, never DELETE).
   Save its id and secret to two files without printing them (`key-id`, `secret`, mode 600).
2. **The Secret** (from the files, never printed):
   ```sh
   kubectl -n vyreon create secret generic vyreon-core-spaces --from-file=key-id=key-id --from-file=secret=secret
   ```
   Keep the two files where the other keys are kept: the panel shows the secret only once.
3. **Core image** with migration 0020 (`sed ... 20-core.yaml | kubectl apply -f -` as above), the
   archive's settings still commented out: workers that ask are told it is off. Check:
   `kubectl -n vyreon logs deploy/vyreon-core | grep -i "perishable archive"` says it is off.
4. **Worker image** with engine D-360 (it installs `zstd`; build as `deploy/worker/build.sh`),
   registered and admitted as usual; the CronJob and Jobs use it.
5. **Switch on:** uncomment `VYREON_CORE_SPACES_ENDPOINT`, `_REGION`, `_BUCKET`, `_KEY_ID_FILE`,
   `_SECRET_FILE` (and, if wanted, `VYREON_CORE_ARCHIVE_ALERT_DAYS`) in `20-core.yaml` and apply.
   The log says `the perishable archive goes to Spaces { endpoint: ..., key: (held, not shown) }`.
6. **First day:** the day's `finish.log` lines `perishable archive: act01-auto <T>: N bytes, kept in
   the bucket as perishables/...`; `vyreon-core archive list <T>` shows both categories KEPT. Not
   verified before this run: Spaces' answer to a path-style pre-signed PUT that signs
   `Content-Length`, `Content-MD5` and `x-amz-meta-sha256`, and its ETag being the MD5 of a
   single-part upload (S3's behaviour; the read-back refuses the object otherwise, and says why).

**Off:** comment out the five settings and apply; workers are told the archive is off and send
nothing. Nothing in the bucket is ever deleted by core.

## Backups and the volume (C-067)

The database volume grows from 120 to 250 GiB, and the database is backed up to Spaces: continuous
WAL archiving plus a weekly full base backup, recoverable to any moment of the last 30 days, in the
bucket `vyreon-pg-backup` with a key for that bucket only; then one restore drill. **Written, not
applied. Nothing here is applied without the owner's word; $ marks what costs money; RESTART marks
the one step that restarts Postgres.** Commands run from the repository root with `KUBECONFIG` set; none prints a key.

| Item | Price | Today (DB ~8 GB) | After phase 1 (DB ~150 GB) |
|---|---|---|---|
| Volume 120 → 250 GiB | $0.10 a GB-month | +$13 a month | same |
| Spaces (subscription already paid for `vyreon-data`: $5 a month, 250 GiB stored and 1 TiB out included, shared by every bucket) | $0.02 a GiB-month beyond 250 GiB; uploads free | ~25-50 GB: $0 extra, if `vyreon-data` leaves room | ~0.5-1.2 TB: about $5-20 a month extra |
| cert-manager, the plugin | free; on the core node | | |

The backup estimate: a 30-day window with weekly fulls holds about six fulls at once (the newest
one older than 30 days plus the five after it), each the database's size less gzip's saving (the
tape caches and streams are stored compressed already, so assume one half to the whole), plus 30
days of WAL. The build itself writes roughly its own size again in WAL, gone 30 days later. Measure
the real numbers after the first week (the bucket's size in the control panel) and correct this.

### Why the plugin (CloudNativePG 1.30.1)

The cluster runs CloudNativePG 1.30.1 (`kubectl get deploy -n cnpg-system cnpg-controller-manager`).
Its documentation deprecates the in-tree `.spec.backup.barmanObjectStore` since 1.26: "CloudNativePG
1.31 is the last minor release that ships with native Barman Cloud support", "removed starting from
1.32.0", "You must migrate to the plugin before upgrading to 1.32"
(https://cloudnative-pg.io/docs/1.30/appendixes/backup_barmanobjectstore). The replacement is the
Barman Cloud plugin, v0.16.0 (2026-11-09), which needs CloudNativePG 1.30 or newer and cert-manager,
installed in `cnpg-system` (https://cloudnative-pg.io/plugin-barman-cloud/docs/installation/). The
in-tree path would work today and need a migration (a second restart) before 1.32; the plugin
costs one more install now (cert-manager: three small Deployments on the core node; the plugin's
TLS certificates last 90 days and cert-manager renews them, so it stays installed). Manifests:
`15-backup.yaml` (ObjectStore `vyreon-backup`, ScheduledBackup `vyreon-pg-weekly`) and the
`plugins` section of `10-postgres.yaml`; other sources: plugin "Usage", "Object stores",
"Retention policies", "Migration" (https://cloudnative-pg.io/plugin-barman-cloud/docs/).

### A. The volume, 120 → 250 GiB ($; no restart)

Checked on the live objects (2026-11-22): StorageClass `do-block-storage` has
`allowVolumeExpansion: true`, provisioner `dobs.csi.digitalocean.com`; the Cluster has
`resizeInUseVolumes: true` (the default). CloudNativePG applies a new size to every PVC and, where
the StorageClass resizes online, "the change is immediately applied to the pods"
(https://cloudnative-pg.io/docs/1.30/storage, "Volume expansion"); DigitalOcean grows a volume when
the PVC's request grows, only upwards, to at most 10,000 GB
(https://docs.digitalocean.com/products/kubernetes/how-to/add-volumes/;
https://github.com/digitalocean/csi-digitalocean, "Volume Expansion"). This cluster already did it
once: 60 → 120 GiB on 2026-11-16 while `vyreon-pg-1` ran on (started 2026-11-11, 0 restarts).

1. Grow only the size (not the whole file: its `plugins` section restarts Postgres and needs the
   plugin installed first, B2):
   ```sh
   kubectl -n vyreon patch cluster vyreon-pg --type merge -p '{"spec":{"storage":{"size":"250Gi"}}}'
   ```
2. Verify, after a few minutes (the PVC's status lags the volume):
   ```sh
   kubectl -n vyreon get pvc vyreon-pg-1                     # CAPACITY 250Gi, no condition left
   kubectl -n vyreon exec vyreon-pg-1 -c postgres -- df -h /var/lib/postgresql/data   # Size ~246G
   deploy/k8s/ops/do.sh GET "/v2/volumes?name=$(kubectl -n vyreon get pvc vyreon-pg-1 -o jsonpath='{.spec.volumeName}')" \
     | python3 -c 'import json,sys; print([v["size_gigabytes"] for v in json.load(sys.stdin)["volumes"]])'   # [250]
   kubectl -n vyreon get pod vyreon-pg-1                     # same age, RESTARTS 0
   ```
   If the PVC shows `FileSystemResizePending`, the filesystem grows at the next mount (B6's restart).
3. Core's `VYREON_CORE_VOLUME_GIB` is `250` in `20-core.yaml`: deploy it with the next core image
   (KEDA's rollout, `sed ... 20-core.yaml | kubectl apply -f -`), **after** step 2 shows 250 (a
   threshold above the real volume would make `DATABASE_FULL` warn late). The core rollout restarts
   core, not Postgres.

### B. Backups

0. **The watch's view of the WAL** (free; once; before the core image with migration 0050, C-068).
   Core's watch reads `pg_stat_archiver`, the WAL directory (`pg_ls_waldir`: its size and the newest
   base backup's history file) and the archiver's queue (`pg_ls_archive_statusdir`); the last two
   need `pg_monitor`, which only a superuser grants (a fresh cluster gets it from `10-postgres.yaml`'s
   `postInitSQL`; a restored one keeps it, role memberships being in the backup):
   ```sh
   kubectl -n vyreon exec vyreon-pg-1 -c postgres -- psql -d vyreon -c "GRANT pg_monitor TO vyreon"
   ```
   Without it the channel says so (`WAL_ARCHIVING`, "the core login lacks pg_monitor") and the
   volume is judged on the data alone (an open `DATABASE_FULL` stays open).
   Core also reads the **backup plugin's own record** through the Kubernetes API (C-068 round 2):
   `20-core.yaml` makes its ServiceAccount `vyreon-core` with a Role that may only `get` the
   Cluster and the ObjectStore, and sets `VYREON_CORE_PG_CLUSTER=vyreon-pg`; it is applied with core.
   Check after the core rollout: `kubectl -n vyreon auth can-i get objectstores.barmancloud.cnpg.io
   --as=system:serviceaccount:vyreon:vyreon-core` says yes, and core's log has no "backups: the
   plugin's record cannot be read".
1. **cert-manager** (free; once), v1.21.3 (2026-10-23; supports Kubernetes 1.33-1.36,
   https://cert-manager.io/docs/releases/), pinned by its SHA-256:
   ```sh
   curl -sSLO https://github.com/cert-manager/cert-manager/releases/download/v1.21.3/cert-manager.yaml
   echo "c04a51295adda2b1a775146751f973e787190778c7f35180861d9678876e5c74  cert-manager.yaml" | sha256sum -c
   kubectl apply -f cert-manager.yaml
   kubectl -n cert-manager rollout status deploy/cert-manager-webhook
   ```
2. **The plugin** (free; once), v0.16.0, into `cnpg-system`:
   ```sh
   curl -sSL -o barman-cloud-v0.16.0.yaml https://github.com/cloudnative-pg/plugin-barman-cloud/releases/download/v0.16.0/manifest.yaml
   echo "aee614683892dd4b44ff9f7afaca55254f1c28de17abbaa0552bf2e1228f7155  barman-cloud-v0.16.0.yaml" | sha256sum -c
   kubectl apply -f barman-cloud-v0.16.0.yaml
   kubectl -n cnpg-system rollout status deploy/barman-cloud
   kubectl -n cnpg-system get certificate                    # barman-cloud-client, -server: READY True
   ```
3. **The bucket and its key** ($ only beyond the subscription; DigitalOcean control panel):
   - Spaces Object Storage, Create Bucket: region NYC3, name `vyreon-pg-backup` (if taken, use
     another and change `destinationPath` in `15-backup.yaml`), file listing restricted, no CDN.
     Nothing else goes in it.
   - Access Keys, Create Access Key, **Limited access**, the bucket `vyreon-pg-backup` only, with
     **Read/Write/Delete** (the retention deletes old backups; the panel offers Read or
     Read/Write/Delete: https://docs.digitalocean.com/products/spaces/how-to/manage-access/).
     The secret is shown once. Save both without echoing them and with no trailing newline (the
     Secret takes the files' bytes as they are):
     ```sh
     umask 077
     read -rs -p "key id: " v; printf '%s' "$v" > backup-spaces-key-id; unset v; echo
     read -rs -p "secret: " v; printf '%s' "$v" > backup-spaces-secret; unset v; echo
     ```
     Keep the two files where the other keys are kept (mode 600, git-ignored).
4. **The Secret** (from the files):
   ```sh
   kubectl -n vyreon create secret generic vyreon-backup-spaces \
     --from-file=ACCESS_KEY_ID=backup-spaces-key-id --from-file=ACCESS_SECRET_KEY=backup-spaces-secret
   ```
5. **The store and the schedule** (free; nothing runs until Sunday 09:00 UTC):
   `kubectl apply -f deploy/k8s/15-backup.yaml`
6. **RESTART. WAL archiving on: Postgres restarts.** Adding the plugin to the Cluster adds its sidecar
   to the Postgres pod, a rolling update (plugin docs, "Migration"); with one instance the
   database is down while `vyreon-pg-1` is recreated on the same volume (expected a minute or
   two). Core waits (its readiness probe fails, it is not restarted); a worker publishing in that
   window gets an error. So, with nothing in flight (best: in the KEDA rollout, before core's new
   image, when nothing runs):
   ```sh
   kubectl -n vyreon exec deploy/vyreon-core -- vyreon-core queue pause "plugin restart"
   kubectl -n vyreon get pods -l app=vyreon-worker           # none Running
   kubectl diff -f deploy/k8s/10-postgres.yaml               # expect: plugins added; postInitSQL
                                                             # added (read only at bootstrap); size 250Gi
                                                             # already; anything else: stop
   kubectl apply -f deploy/k8s/10-postgres.yaml
   kubectl -n vyreon get pods -w                             # vyreon-pg-1 recreated, Running, Ready
   kubectl -n vyreon get pod vyreon-pg-1 -o jsonpath='{.spec.initContainers[*].name} | {.spec.containers[*].name}{"\n"}'
                                                             # the plugin's sidecar is listed
   kubectl -n vyreon exec deploy/vyreon-core -- vyreon-core queue resume
   ```
7. **Verify WAL archiving.** The Cluster's `ContinuousArchiving` condition was already True before
   (with no store it archives to nowhere), so read Postgres itself:
   ```sh
   kubectl -n vyreon exec vyreon-pg-1 -c postgres -- psql -d vyreon -Atc "SELECT now(), pg_switch_wal()"
   # a minute later: archived_count grows, last_archived_time after the switch, failed_count 0
   kubectl -n vyreon exec vyreon-pg-1 -c postgres -- psql -d vyreon -Atc \
     "SELECT archived_count, last_archived_wal, last_archived_time, failed_count, last_failed_time FROM pg_stat_archiver"
   kubectl -n vyreon get cluster vyreon-pg -o jsonpath='{.status.conditions[?(@.type=="ContinuousArchiving")]}{"\n"}'
   ```
   On failure: the sidecar's log (`kubectl -n vyreon logs vyreon-pg-1 -c <sidecar name from step 6>`)
   names the cause (key, bucket, endpoint, checksum).
   **PostgreSQL's count is not proof** (C-068 round 2): with no archiver configured CloudNativePG
   still answers each WAL file archived, so `archived_count` grows anyway. The proof is the bucket:
   `SELECT pg_walfile_name(pg_switch_wal())` names the file just closed; it must appear in the
   sidecar's log as archived and under `vyreon-pg/wals/` in the bucket (Spaces panel). Core's watch
   checks the Cluster's `spec.plugins` (`WAL_ARCHIVING` "archiving is off" without the archiver).
8. **The first base backup, on demand:**
   ```sh
   kubectl create -f deploy/k8s/ops/backup-now.yaml
   kubectl -n vyreon get backup -w                           # PHASE completed (minutes at 8 GB)
   kubectl -n vyreon get objectstore vyreon-backup -o jsonpath='{.status}{"\n"}'
                                                             # vyreon-pg's firstRecoverabilityPoint
   ```
   The bucket now holds `vyreon-pg/base/<backup id>/` and `vyreon-pg/wals/`. Then the restore
   drill (C). After it passes: the laptop safety dump `~/claude-artifacts/db-backups/vyreon-20261116T*.dump`
   may go (MILESTONES), on the owner's word.
9. **Each week:** `kubectl -n vyreon get scheduledbackup,backup` (the last schedule time and a
   completed Backup each Sunday); the bucket's size in the panel.

**Watch (C-068):** if archiving fails, Postgres keeps every unarchived WAL file in `pg_wal` on the
database volume until it succeeds. Core's watch alerts on the channel, each one problem:
- `WAL_ARCHIVING` (key `archiver`): the archiver's last attempt failed and nothing was archived for
  15 minutes, or a finished WAL file has waited 30 minutes (stuck without failing). Look at the
  plugin's log: `kubectl -n vyreon logs vyreon-pg-1 --all-containers --since=1h`.
- `DATABASE_FULL`: every database **and the WAL** above 80% of `VYREON_CORE_VOLUME_GIB` (clears
  below 78%). The filesystem itself is a little smaller than the volume (`df` shows ~246G of 250);
  the 20% margin covers it.
- `BACKUP_STALE`: no evidence of a recoverable base backup newer than `VYREON_CORE_BACKUP_MAX_DAYS`
  (8) days, from the plugin's own record (the ObjectStore's `status.serverRecoveryWindow.<server>`:
  `firstRecoverabilityPoint` and `lastSuccessfulBackupTime`), or that record not read in 15
  minutes ("Backups unverified", with why). Until B8's first backup it is open, and true.
- `WAL_ARCHIVING` "archiving is off": the Cluster has no Barman Cloud WAL archiver in `spec.plugins`.
The 09:00 digest's "Database size" line adds the WAL and the backups as the plugin records them.
**Verify once after B8:** `kubectl -n vyreon get objectstore vyreon-backup -o
jsonpath='{.status.serverRecoveryWindow}{"\n"}'` shows `vyreon-pg` with both times, and within 5
minutes `BACKUP_STALE` resolves on the channel.

**Off:** remove the `plugins` section from `10-postgres.yaml` and apply (RESTART: Postgres restarts), and
`kubectl delete scheduledbackup -n vyreon vyreon-pg-weekly`. The bucket keeps what it holds;
deleting it is a separate, previewed step.

### C. The restore drill (once after B8; then after any change to backups)

A temporary cluster `vyreon-pg-drill` in the same Kubernetes cluster and namespace, restored from
the latest base backup and every archived WAL file, compared with `vyreon-pg`, then deleted. It
has no `plugins` section, so it never writes to the bucket (plugin docs, "Usage"). Its Services
(`vyreon-pg-drill-rw`, ...) do not touch core's.

**Cost and duration, today (~8 GB):** a 30 GiB volume for about an hour (under one cent);
downloading from Spaces NYC3 to the cluster's NYC1 nodes is free (Spaces pricing,
https://docs.digitalocean.com/products/spaces/details/pricing/) and within the included 1 TiB in
any case; the core node has room (Postgres under a GiB here). About 30-45 minutes of the
operator's time, the queue paused for about 20 of them. At ~150 GB: a 200 GiB volume and, better,
a `day` node (see `ops/restore-drill.yaml`; $0.17 an hour), an hour or two to restore.

1. **Freeze the source:** nothing may publish between its count and the restore's end.
   ```sh
   kubectl -n vyreon exec deploy/vyreon-core -- vyreon-core queue pause "restore drill"
   kubectl -n vyreon get pods -l app=vyreon-worker           # none Running
   kubectl -n vyreon exec vyreon-pg-1 -c postgres -- psql -d vyreon -Atc "SELECT now(), pg_switch_wal()"
   # wait until pg_stat_archiver's last_archived_time is after that now() (B7)
   ```
2. **Count the source:**
   ```sh
   kubectl -n vyreon exec -i vyreon-pg-1 -c postgres -- psql -d vyreon -At -v ON_ERROR_STOP=1 \
     < deploy/k8s/ops/drill-counts.sql > drill-source.txt
   ```
3. **Restore:**
   ```sh
   kubectl apply -f deploy/k8s/ops/restore-drill.yaml
   kubectl -n vyreon get cluster vyreon-pg-drill -w          # "Cluster in healthy state"; note the time taken
   ```
   Once it is healthy its contents are fixed: `queue resume` now.
4. **Count the restore and compare:**
   ```sh
   kubectl -n vyreon exec -i vyreon-pg-drill-1 -c postgres -- psql -d vyreon -At -v ON_ERROR_STOP=1 \
     < deploy/k8s/ops/drill-counts.sql > drill-restore.txt
   diff drill-source.txt drill-restore.txt && echo "restore matches"
   ```
   Per month: day revisions by state, measurement rows and a digest of their body hashes. Any
   difference fails the drill: keep the drill cluster, find why, and do not delete the laptop dump.
5. **Delete the drill** (it names `vyreon-pg-drill`; never `vyreon-pg`):
   ```sh
   kubectl -n vyreon delete cluster vyreon-pg-drill
   kubectl -n vyreon get pvc                                 # vyreon-pg-1 only
   deploy/k8s/ops/do.sh GET "/v2/volumes?region=nyc1" | python3 -c 'import json,sys; print([(v["name"],v["size_gigabytes"]) for v in json.load(sys.stdin)["volumes"]])'
                                                             # the drill's pvc-... volume gone (reclaim Delete)
   ```
6. Record in `docs/MILESTONES.md`: the date, the backup used, the restore's duration, the result.

The drill stays a drill: it archives nothing and is deleted. Recovering production is D.

### D. Recovery after the database volume is lost (production; $ the new volume; the owner)

When the database's volume is gone (deleted, corrupt, its pod unable to start on it) and the bucket
holds the backups (C-068, audit opsurface ops.md 7; round 2, 5 and 12). The recovered database is a
new Cluster (`ops/recover-production.yaml`), recovered from the latest base backup and every WAL file
the lost server archived, and archiving its own WAL from its first minute under its own name in the
same bucket: the plugin refuses to archive into a path that already holds a server's WAL, and the
lost server's path stays the recoverable history of what came before.

**What is lost:** everything written after the last WAL file archived (at most about 5 minutes,
CloudNativePG's `archive_timeout`, more if archiving was failing: `WAL_ARCHIVING`). Core's queue is
in the same database, so it comes back consistent with the store: a day published in that gap is
computed again, leases live then expire, free of charge. **But a schedule made in that gap is gone
with no trace in the store** (round 2, 5): step 5 compares the queue with the schedules recorded in
MILESTONES (each tranche, hand-over and retirement is recorded there with its time as it is made)
and makes again what is missing.

Set the names once (the first loss: `LOST=vyreon-pg LOST_SERVER=vyreon-pg NEW=vyreon-pg2`; after
that, the current Cluster's name for both and the next number for `NEW`; `LOST_SERVER` is the lost
Cluster's plugin `serverName`, `kubectl -n vyreon get cluster $LOST -o
jsonpath='{.spec.plugins[0].parameters.serverName}'`, else its name):
```sh
LOST=vyreon-pg LOST_SERVER=vyreon-pg NEW=vyreon-pg2
REPOINT=(-e "s#$LOST-rw#$NEW-rw#g" -e "s#$LOST-app#$NEW-app#g" -e "s#value: $LOST\$#value: $NEW#" -e "s#name: $LOST\$#name: $NEW#")
```
1. **Stop the workers and core** (nothing can publish to a database that is not there; nothing
   should start against the recovered one before it is checked):
   ```sh
   kubectl -n vyreon annotate scaledjob --all autoscaling.keda.sh/paused=true
   kubectl -n vyreon delete jobs -l scaledjob.keda.sh/name --ignore-not-found   # workers of a dead database
   kubectl -n vyreon scale deployment vyreon-core --replicas=0
   kubectl -n vyreon get cluster $LOST -o yaml > $LOST-lost.yaml                 # its record, for MILESTONES
   ```
   Keep the lost Cluster object until the recovery is verified (deleting it deletes its PVC: only
   once you are sure nothing on it is wanted). The bucket is never touched.
2. **Recover:**
   ```sh
   sed -e "s#VYREON_PG_NEW#$NEW#g" -e "s#VYREON_PG_LOST_SERVER#$LOST_SERVER#g" deploy/k8s/ops/recover-production.yaml | kubectl apply -f -
   kubectl -n vyreon get cluster $NEW -w         # "Cluster in healthy state"; note the time taken
   ```
3. **Point core, KEDA and the ordered pass at it.** Core and KEDA take the host on apply (core also
   the Cluster whose backup record it reads, `VYREON_CORE_PG_CLUSTER`); the pass's login is a Secret
   (its password came back with the roles):
   ```sh
   sed -e "s#VYREON_CORE_IMAGE#registry.digitalocean.com/vyreon/vyreon-core:<tag>#" "${REPOINT[@]}" deploy/k8s/20-core.yaml | kubectl apply -f -
   printf 'host=%s-rw port=5432 dbname=vyreon user=vyreon_pass password=%s' "$NEW" "$(tr -d '\n' < pass-password)" > ordered-pass-pg
   kubectl -n vyreon create secret generic vyreon-worker --dry-run=client -o yaml \
     --from-file=worker-token=worker-token --from-file=intrinio-key=<the Intrinio key file> \
     --from-file=fred-key=<the FRED key file> --from-file=ordered-pass-pg=ordered-pass-pg | kubectl apply -f -
   IMG=registry.digitalocean.com/vyreon/vyreon-worker:<tag> BUILD=<build>
   sed -e "s#VYREON_WORKER_IMAGE#$IMG#" -e "s#VYREON_ENGINE_BUILD#$BUILD#" "${REPOINT[@]}" deploy/k8s/50-keda.yaml | kubectl apply -f -
   kubectl -n vyreon rollout status deployment vyreon-core
   ```
   If core cannot log in (its log says so), set the `vyreon` role's password to the new Secret's, by
   stdin: `printf "ALTER ROLE vyreon PASSWORD '%s'" "$(kubectl -n vyreon get secret $NEW-app -o
   jsonpath='{.data.password}' | base64 -d)" | kubectl -n vyreon exec -i $NEW-1 -c postgres -- psql
   -d vyreon -v ON_ERROR_STOP=1 -q`.
4. **The backups follow it:**
   ```sh
   sed "${REPOINT[@]}" deploy/k8s/15-backup.yaml | kubectl apply -f -          # the schedule names $NEW
   sed "${REPOINT[@]}" deploy/k8s/ops/backup-now.yaml | kubectl create -f -    # a base backup now
   ```
   Then B7 on `$NEW-1` (the WAL file in the bucket under `$NEW/wals/`), and B8's checks: the
   ObjectStore's `serverRecoveryWindow.$NEW` with both times, and `BACKUP_STALE` resolving.
5. **Verify before anything runs:**
   - `vyreon-core problems` answers, and `vyreon-core queue` (jobs by kind, state and **build**)
     holds every schedule MILESTONES records: each tranche's day blocks and tapes, the ordered
     passes, the hand-overs. A range missing (made in the lost minutes) is scheduled again now
     (`vyreon-core schedule ...` as recorded; a range present is refused as an overlap, which
     confirms it).
   - `drill-counts.sql` on `$NEW-1` against the last counts you have (a drill's, or the lost
     database's if they were read); the newest published revision (`SELECT max(published_at) FROM
     vyreon.day_revision`) against the last WAL archived by the lost server (`$LOST_SERVER/wals/` in
     the bucket).
   - Then `kubectl -n vyreon annotate scaledjob --all autoscaling.keda.sh/paused-` and watch the first
     claims.
6. **Make it the standing system:** in the repository, the new names in `10-postgres.yaml` (the
   Cluster `$NEW`, its `plugins` `serverName: $NEW`), `20-core.yaml`, `50-keda.yaml`,
   `15-backup.yaml`, `ops/backup-now.yaml`, `ops/restore-drill.yaml` (`serverName: $NEW`) and this
   README's examples (`ops/recover-production.yaml` takes the names on the command line and needs no
   change); commit; record in MILESTONES the time of the loss, the recovery's duration, the last WAL
   recovered, and the names (the next loss starts from `LOST=$NEW`). Delete the lost Cluster object
   and its PVC only then, previewed (`kubectl -n vyreon get pvc`), on the owner's word.
