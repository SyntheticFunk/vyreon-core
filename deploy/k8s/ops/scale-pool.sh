#!/usr/bin/env bash
# Sets a worker pool's node count. usage: scale-pool.sh POOL_ID POOL_NAME COUNT
# Needs VYREON_DOKS_CLUSTER. Nodes cost money: the owner's spend ceiling applies (docs/MILESTONES.md).
set -euo pipefail
S=$(cd "$(dirname "$0")" && pwd); C=${VYREON_DOKS_CLUSTER:?set VYREON_DOKS_CLUSTER}
"$S/do.sh" PUT "/v2/kubernetes/clusters/$C/node_pools/$1" "{\"name\":\"$2\",\"count\":$3}" \
  | python3 -c "import json,sys; p=json.load(sys.stdin)['node_pool']; print(p['name'], 'count', p['count'])"
