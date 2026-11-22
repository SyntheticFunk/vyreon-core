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
# the registry's read-only pull secret, for every pod of the namespace (after 00-namespace.yaml):
doctl registry docker-config --read-only > pull-config.json
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
  --from-file=worker-token=worker-token --from-file=intrinio-key=<the Intrinio key file> --from-file=ordered-pass-pg=ordered-pass-pg
```

## Run ($ while the worker pool exists)

```sh
# $ two worker nodes, kept for the engine by a taint
doctl kubernetes cluster node-pool create vyreon --name day --size s-8vcpu-16gb-amd --count 2 \
  --label vyreon=worker --taint "vyreon=worker:NoSchedule"
```

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
4. **The CronJob:** if round v's gate was applied, remove it first: `kubectl -n vyreon delete
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
set it, `PUT /v2/kubernetes/clusters/{id}`). Pool `day-std` (Intel, 0 nodes) is selected by
nothing now (every pod names pool `day`); leave it at 0 or delete it.

### 4. Apply the ScaledJobs (free until pods start)

With the worker image and its build (`build register` printed it; the build must be admitted):

```sh
IMG=registry.digitalocean.com/vyreon/vyreon-worker:<tag> BUILD=<commit>+<inventory>
sed -e "s#VYREON_WORKER_IMAGE#$IMG#" -e "s#VYREON_ENGINE_BUILD#$BUILD#" deploy/k8s/50-keda.yaml | kubectl apply -f -
kubectl -n vyreon get scaledjob      # READY True for all four; ACTIVE True where work is claimable
```

A new image: withdraw the old build in core (`build withdraw`, C-062), then apply again with the new
image and build. `rollout.strategy: gradual` keeps running Jobs (KEDA's default would delete them).
**Never `kubectl delete scaledjob` while a worker runs:** its Jobs are owned by it and deleted with
it (`--cascade=orphan` keeps them); deleting KEDA's manifest deletes its CRDs and with them every
ScaledJob and Job.

### 5. The caps, and where to change them

- **KEDA's caps:** `maxReplicaCount` of each ScaledJob in `50-keda.yaml`; apply again (running Jobs
  stay). Day blocks: raise from 12 only from core's request ledger, the owner's rule (nodes only
  while the request budget has headroom): one node averaged 101 requests a minute over its wall
  time with bursts of 500-760 while a day downloads (2026-11-22); core grants at most 1,900 a minute
  (`VYREON_CORE_INTRINIO_PER_MINUTE`), so more nodes than the budget holds only wait in it, at node
  cost. Check: `SELECT date_trunc('minute', minute) m, sum(requests), count(DISTINCT worker) FROM
  core.request_usage WHERE minute > now() - interval '6 hours' GROUP BY 1 ORDER BY 2 DESC LIMIT 10`.
- **The node cap** (the hard cost cap): pool `day` `max_nodes` (step 3's PUT with a new value).
  Keep it at least `day-block` + `day-block-daily`, else a block pod waits Pending (it holds no
  job, so nothing stalls, but it is not computing).

### 6. Cost guard ($)

At most `max_nodes` x $0.167 an hour: 13 nodes, $2.17 an hour, $52 a day if every slot is busy.
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
- The block has its own slot and its own node, as the CronJob gave it, so a backfill holding all 12
  slots for a day and more never delays the deadline (20:00 MT, C-050).
- What the CronJob did that KEDA does not: one pod for all three kinds on one node. Under KEDA the
  tape's node is idle for a poll when the tape ends and the block's pod takes it (6 + 2 CPUs do
  not fit together; the tape is minutes), and the pass (1 CPU) fits beside the block.
Keep `40-daily.yaml` for a fallback (KEDA down): resume the CronJob and pause the ScaledJobs.
C-065's 30-minute STALLED allowance for daily-only work was for the CronJob's 15-minute gap; with
KEDA 15 would do (a core change, not made here).

### 10. First run under KEDA: the recompute of the test block (the owner; $ about $5)

Two blocks of 5 sessions, 2024-07-09..07-22, on build 48461197+7361a31e2fda (or its successor),
started by KEDA alone. Before: run.sh has finished and deleted its node; steps 1 to 4 done (core
image with C-065 and 0046, the login, the pool autoscaled at 0 nodes, the ScaledJobs on image
48461197); 914ffed7 withdrawn; CronJob `daily` suspended or not applied.

```sh
kubectl -n vyreon exec deploy/vyreon-core -- vyreon-core schedule day-block FREE_REPORT_64 48461197+7361a31e2fda 2024-07-09 2024-07-22 5
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
