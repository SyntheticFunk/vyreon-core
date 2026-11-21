#!/usr/bin/env bash
# Deletes every worker node (pools named day*) with no running, pending or starting worker pod.
# Needs KUBECONFIG and VYREON_DOKS_CLUSTER (the cluster id) set. Run after every worker run ends.
set -euo pipefail
S=$(cd "$(dirname "$0")" && pwd); C=${VYREON_DOKS_CLUSTER:?set VYREON_DOKS_CLUSTER}
BUSY=$(kubectl -n vyreon get pods -l app=vyreon-worker -o wide --no-headers | grep -E "Running|Pending|ContainerCreating" | awk '{print $7}' | sort -u || true)
POOLS=$("$S/do.sh" GET "/v2/kubernetes/clusters/$C/node_pools" | python3 -c "
import json,sys
for p in json.load(sys.stdin)['node_pools']:
    if p['name'].startswith('day'):
        for n in p.get('nodes') or []: print(p['id'], n['id'], n['name'])")
echo "$POOLS" | while read -r P NID NAME; do
  [ -z "${NID:-}" ] && continue
  echo "$BUSY" | grep -q "^$NAME$" && continue
  "$S/do.sh" DELETE "/v2/kubernetes/clusters/$C/node_pools/$P/nodes/$NID?skip_drain=1&replace=0" >/dev/null && echo "deleted idle $NAME"
done
