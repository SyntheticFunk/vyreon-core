#!/usr/bin/env bash
# Deletes the worker nodes (pools named day*) that hold no worker pod still to finish. Needs
# KUBECONFIG and VYREON_DOKS_CLUSTER (the cluster id). The autoscaler does this by itself (C-066);
# this is for when it does not.
# C-068 (audit opsurface ops.md 3; round 2, round2-core.md 4): no path deletes a node with a worker
# pod that is not Succeeded or Failed, and when in doubt it deletes nothing (exit 1):
# - the kubeconfig must be this cluster's: a node is a candidate only if `kubectl get nodes` shows it
#   with DigitalOcean's droplet id (`providerID: digitalocean://<id>`) equal to the node pool's record
#   of it, and if no listed node matches at all, nothing is deleted;
# - a candidate is cordoned first (no pod can be scheduled onto it after), then its pods are listed
#   again; a worker pod not finished (or a listing that fails) uncordons it and leaves it;
# - only a node with no worker pod at all skips DigitalOcean's drain; one with finished worker pods
#   is drained; a node under 15 minutes old or not running is left (it may be coming up for a pod);
# - every refused deletion is reported.
set -euo pipefail
S=$(cd "$(dirname "$0")" && pwd); C=${VYREON_DOKS_CLUSTER:?set VYREON_DOKS_CLUSTER}
if ! KNODES=$(kubectl get nodes -o jsonpath='{range .items[*]}{.metadata.name}{" "}{.spec.providerID}{"\n"}{end}'); then
  echo "delete-idle.sh: the nodes cannot be listed (is the kubeconfig current?): nothing deleted" >&2
  exit 1
fi
if ! LIST=$("$S/do.sh" GET "/v2/kubernetes/clusters/$C/node_pools"); then
  echo "delete-idle.sh: DigitalOcean's node pools cannot be read: nothing deleted" >&2
  exit 1
fi
NODES=$(echo "$LIST" | python3 -c "
import json, sys, datetime
now = datetime.datetime.now(datetime.timezone.utc)
for p in json.load(sys.stdin)['node_pools']:
    if p['name'].startswith('day'):
        for n in p.get('nodes') or []:
            age = (now - datetime.datetime.fromisoformat(n['created_at'].replace('Z', '+00:00'))).total_seconds() / 60
            print(p['id'], n['id'], n['name'], (n.get('status') or {}).get('state', 'unknown'), int(age), n.get('droplet_id') or '-')")
# the kubeconfig's cluster is this one: some day node must match by name and droplet id
MATCHED=0
while read -r P NID NAME STATE AGE DROPLET; do
  [ -z "${NID:-}" ] && continue
  echo "$KNODES" | grep -qx "$NAME digitalocean://$DROPLET" && MATCHED=1
done <<< "$NODES"
if [ -n "$(echo "$NODES" | tr -d '[:space:]')" ] && [ "$MATCHED" = 0 ]; then
  echo "delete-idle.sh: no node of cluster $C is in this kubeconfig's cluster (another cluster's kubeconfig?): nothing deleted" >&2
  exit 1
fi
# the worker pods on a node, "<phase>" a line; fails (exit 1) when they cannot be listed
pods_on() { kubectl -n vyreon get pods -l app=vyreon-worker --field-selector "spec.nodeName=$1" -o jsonpath='{range .items[*]}{.status.phase}{"\n"}{end}'; }
FAILED=0
while read -r P NID NAME STATE AGE DROPLET; do
  [ -z "${NID:-}" ] && continue
  if ! echo "$KNODES" | grep -qx "$NAME digitalocean://$DROPLET"; then echo "left $NAME (not seen in this cluster)"; continue; fi
  if [ "$STATE" != running ] || [ "$AGE" -lt 15 ]; then echo "left $NAME ($STATE, $AGE min old)"; continue; fi
  if ! BEFORE=$(pods_on "$NAME"); then echo "!! $NAME: its pods cannot be listed: left" >&2; FAILED=1; continue; fi
  if echo "$BEFORE" | grep -qvE '^(Succeeded|Failed)?$'; then continue; fi
  if ! kubectl cordon "$NAME" >/dev/null; then echo "!! $NAME: cannot be cordoned: left" >&2; FAILED=1; continue; fi
  # cordoned: nothing is scheduled onto it now; what is there is all there will be
  if ! AFTER=$(pods_on "$NAME") || echo "$AFTER" | grep -qvE '^(Succeeded|Failed)?$'; then
    kubectl uncordon "$NAME" >/dev/null || echo "!! $NAME: left cordoned (uncordon failed)" >&2
    echo "left $NAME (a worker pod came, or its pods cannot be listed)"; continue
  fi
  if [ -n "$(echo "$AFTER" | tr -d '[:space:]')" ]; then Q="replace=0"; else Q="skip_drain=1&replace=0"; fi
  if "$S/do.sh" DELETE "/v2/kubernetes/clusters/$C/node_pools/$P/nodes/$NID?$Q" >/dev/null; then
    echo "deleted idle $NAME"
  else
    kubectl uncordon "$NAME" >/dev/null || true
    echo "!! $NAME not deleted (above: DigitalOcean's answer)" >&2; FAILED=1
  fi
done <<< "$NODES"
exit $FAILED
