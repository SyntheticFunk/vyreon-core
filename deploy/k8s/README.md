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

- **KEDA** (`helm install keda kedacore/keda --namespace keda --create-namespace`, 2.21): one
  ScaledJob per job kind with the `postgresql` trigger counting `core.claimable_job`, so workers
  start when work is queued. To check when it is added: the trigger's Secret must sit in KEDA's
  own namespace by default, and KEDA's page on the PostgreSQL scaler shows it only under a
  ScaledObject. A ScaledJob starts one Kubernetes Job per unit of work; keep each of those Jobs at
  one pod, or use the per-index retry limit of `30-workers.yaml`: never a Job-wide `backoffLimit`
  over several pods, which terminates the healthy pods when one fails (C-040).
- **Node autoscaling to zero:** see "Daily runs" below (the pool `day`, min 0).
- Backups to Spaces (C-013) once a database is worth keeping.

## Daily runs (C-051)

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
- **The cluster starts the worker:** CronJob `daily` (`40-daily.yaml`) at 07:45 New York,
  Tuesday to Saturday: one pod runs the tape, the day and the ordered pass in order, up to three
  pods one after another. Every worker pod carries `cluster-autoscaler.kubernetes.io/safe-to-evict:
  "false"`, so the autoscaler never removes a node under a running day.
- **The pool scales:** pool `day` autoscaled with min 0; the CronJob's pending pod brings up one
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
4. **The CronJob:** `sed "s#VYREON_WORKER_IMAGE#registry.digitalocean.com/vyreon/vyreon-worker:<tag>#" deploy/k8s/40-daily.yaml | kubectl apply -f -`.
   Its image must be the build named in step 5. Nothing is queued yet, so its runs end in a minute.
5. **Switch on** (the owner's word that the system is ready): the build admitted
   (`vyreon-core build admit`), then in `20-core.yaml` uncomment `VYREON_CORE_DAILY_FROM` (the
   first session to compute by itself: normally the session after the last one scheduled by hand)
   and `VYREON_CORE_DAILY_BUILD`, and apply. Check: `vyreon-core daily` shows the settings, "due
   now" and the next session's time; it queues within a minute of 07:30 New York.
6. **First morning:** `kubectl -n vyreon get cronjob,jobs,pods -o wide` (a node comes up, the pod
   runs `tape`, `day-block`, `ordered-pass`), `vyreon-core queue`, the 09:00 digest's "Daily runs"
   line; after the run, the node is gone within about 10 minutes
   (`kubectl get nodes -l vyreon=worker`). To run at once instead of waiting:
   `kubectl -n vyreon create job daily-now --from=cronjob/daily`.

**Off:** comment out the two daily settings and apply (core queues nothing more), and
`kubectl -n vyreon patch cronjob daily -p '{"spec":{"suspend":true}}'`.

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
