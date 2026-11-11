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
  ScaledObject.
- **Node autoscaling to zero:** DigitalOcean's autoscaler page says a pool can scale to zero; its
  `doctl` reference says it cannot. Until that is settled on the live API, the worker pool is
  created for a run and deleted after it.
- Backups to Spaces (C-013) once a database is worth keeping.
