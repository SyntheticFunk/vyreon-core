#!/usr/bin/env bash
# Refresh the operator's kubeconfig for the DOKS cluster (owner, 2026-11-24: automation debt 2; a DOKS kubeconfig's
# token expires, about weekly). The file holds a credential: it is written (mode 600, atomically) and never printed.
# usage: kubeconfig.sh [out-file]   (default: $KUBECONFIG)
#   VYREON_DO_CLUSTER   cluster name (default vyreon); VYREON_DO_TOKEN_FILE as for do.sh
#   VYREON_KUBECONFIG_SECONDS  requested lifetime (default 604800, seven days; DigitalOcean's own ceiling applies)
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
out=${1:-${KUBECONFIG:-}}
[ -n "$out" ] || { echo "kubeconfig.sh: name the file (argument or KUBECONFIG)" >&2; exit 1; }
name=${VYREON_DO_CLUSTER:-vyreon}
secs=${VYREON_KUBECONFIG_SECONDS:-604800}
id=$("$here/do.sh" GET /v2/kubernetes/clusters | python3 -c '
import json, sys
name = sys.argv[1]
ids = [c["id"] for c in json.load(sys.stdin).get("kubernetes_clusters", []) if c.get("name") == name]
if len(ids) != 1:
    sys.exit(f"kubeconfig.sh: {len(ids)} clusters named {name!r}")
print(ids[0])' "$name")
tmp=$(mktemp "$(dirname "$out")/.kubeconfig.XXXXXX")
trap 'rm -f "$tmp"' EXIT
chmod 600 "$tmp"
"$here/do.sh" GET "/v2/kubernetes/clusters/$id/kubeconfig?expiry_seconds=$secs" > "$tmp"
grep -q '^kind: Config' "$tmp" || { echo "kubeconfig.sh: the answer is not a kubeconfig" >&2; exit 1; }
KUBECONFIG="$tmp" kubectl version --request-timeout=20s >/dev/null 2>&1 || { echo "kubeconfig.sh: the new kubeconfig does not reach the cluster" >&2; exit 1; }
mv -f "$tmp" "$out"
trap - EXIT
echo "kubeconfig.sh: $out refreshed for cluster $name (lifetime requested ${secs}s)"
