#!/usr/bin/env bash
# Deletes the worker nodes (pools named day*) that hold no unfinished worker pod. Needs KUBECONFIG
# and VYREON_DOKS_CLUSTER (the cluster id). The autoscaler does this by itself (C-066); this is for
# when it does not.
# C-068 (audit opsurface ops.md 3): it fails closed. If the pods cannot be listed (an expired
# kubeconfig, the API unreachable) or DigitalOcean's node list cannot be read, it deletes nothing and
# exits 1. A node with any worker pod still to finish (Pending, Running or Unknown) is never deleted;
# one with only finished worker pods is deleted with DigitalOcean's drain; only a node with no worker
# pod at all skips the drain. A node younger than 15 minutes or not yet running is left (it may be
# coming up for a pending pod). Each deletion's answer is checked: a refused one is reported and the
# script exits 1.
set -euo pipefail
S=$(cd "$(dirname "$0")" && pwd); C=${VYREON_DOKS_CLUSTER:?set VYREON_DOKS_CLUSTER}
if ! PODS=$(kubectl -n vyreon get pods -l app=vyreon-worker -o jsonpath='{range .items[*]}{.spec.nodeName}{" "}{.status.phase}{"\n"}{end}'); then
  echo "delete-idle.sh: the worker pods cannot be listed (is the kubeconfig current?): nothing deleted" >&2
  exit 1
fi
# nodes with a worker pod still to finish, and nodes with any worker pod
UNFINISHED=$(echo "$PODS" | awk '$1 != "" && $2 != "Succeeded" && $2 != "Failed" {print $1}' | sort -u)
ANY=$(echo "$PODS" | awk '$1 != "" {print $1}' | sort -u)
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
            print(p['id'], n['id'], n['name'], (n.get('status') or {}).get('state', 'unknown'), int(age))")
FAILED=0
while read -r P NID NAME STATE AGE; do
  [ -z "${NID:-}" ] && continue
  if echo "$UNFINISHED" | grep -qx "$NAME"; then continue; fi
  if [ "$STATE" != running ] || [ "$AGE" -lt 15 ]; then echo "left $NAME ($STATE, $AGE min old)"; continue; fi
  if echo "$ANY" | grep -qx "$NAME"; then Q="replace=0"; else Q="skip_drain=1&replace=0"; fi
  if "$S/do.sh" DELETE "/v2/kubernetes/clusters/$C/node_pools/$P/nodes/$NID?$Q" >/dev/null; then
    echo "deleted idle $NAME"
  else
    echo "!! $NAME not deleted (above: DigitalOcean's answer)" >&2; FAILED=1
  fi
done <<< "$NODES"
exit $FAILED
