#!/usr/bin/env bash
# Refresh the operator's kubeconfig for the DOKS cluster (owner, 2026-11-24: automation debt 2; a DOKS kubeconfig's
# token expires, about weekly). The file holds a credential: it is written (mode 600, atomically) and never printed.
# usage: kubeconfig.sh [--if-needed | --check] [out-file]   (default: $KUBECONFIG)
#   (no flag)     refresh now
#   --if-needed   refresh only when the current file is missing, within VYREON_KUBECONFIG_MARGIN_SECONDS of its end
#                 (default 172800, two days), or refused by the cluster (401, "Unauthorized", "must be logged in");
#                 release.sh and health.sh call it first, so neither stops on an expired kubeconfig
#   --check       say what --if-needed would do; writes nothing (release.sh --dry-run)
#   VYREON_DO_CLUSTER   cluster name (default vyreon); VYREON_DO_TOKEN_FILE as for do.sh
#   VYREON_KUBECONFIG_SECONDS  requested lifetime (default 604800, seven days; DigitalOcean's own ceiling applies)
# Its end is recorded beside it (<out-file>.expires: the epoch second the requested lifetime ends; a DOKS
# kubeconfig's token states no expiry of its own). A file without that record is taken to end its lifetime after
# its last change. A refusal by the cluster refreshes it whatever the record says (DigitalOcean may cut it short).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
mode=refresh
case "${1:-}" in
  --if-needed) mode=if-needed; shift ;;
  --check) mode=check; shift ;;
esac
out=${1:-${KUBECONFIG:-}}
[ -n "$out" ] || { echo "kubeconfig.sh: name the file (argument or KUBECONFIG)" >&2; exit 1; }
name=${VYREON_DO_CLUSTER:-vyreon}
secs=${VYREON_KUBECONFIG_SECONDS:-604800}
margin=${VYREON_KUBECONFIG_MARGIN_SECONDS:-172800}

# why the file needs a refresh, or nothing when it does not
need() {
  [ -s "$out" ] || { echo "there is no kubeconfig at $out"; return; }
  local now ends said
  now=$(date +%s)
  if [ -s "$out.expires" ]; then ends=$(tr -dc '0-9' < "$out.expires"); else ends=$(( $(stat -c %Y "$out") + secs )); fi
  if [ $((ends - now)) -lt "$margin" ]; then
    echo "it ends $(date -u -d "@$ends" '+%Y-%m-%d %H:%M UTC'), within $((margin / 3600)) h"
    return
  fi
  # the cluster's own answer: a refusal of the credential, never a network failure, is a reason
  if ! said=$(KUBECONFIG="$out" kubectl version --request-timeout=20s 2>&1 >/dev/null); then
    case "$said" in
      *Unauthorized*|*"must be logged in"*|*" 401"*|*orbidden*) echo "the cluster refuses it" ;;
      *) echo "kubeconfig.sh: the cluster cannot be reached with $out (not a refusal of the credential; not refreshed): $(echo "$said" | tail -1 | cut -c1-160)" >&2
         return 2 ;;
    esac
  fi
}

if [ "$mode" != refresh ]; then
  rc=0
  why=$(need) || rc=$?
  if [ -z "$why" ]; then
    [ "$rc" = 0 ] && echo "kubeconfig.sh: $out is current" || echo "kubeconfig.sh: $out kept (the cluster could not be reached to judge it)"
    exit 0
  fi
  if [ "$mode" = check ]; then
    echo "kubeconfig.sh: $out would be refreshed: $why (a dry run writes nothing)"
    exit 0
  fi
  echo "kubeconfig.sh: refreshing $out: $why"
fi

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
asked=$(date +%s)
"$here/do.sh" GET "/v2/kubernetes/clusters/$id/kubeconfig?expiry_seconds=$secs" > "$tmp"
grep -q '^kind: Config' "$tmp" || { echo "kubeconfig.sh: the answer is not a kubeconfig" >&2; exit 1; }
KUBECONFIG="$tmp" kubectl version --request-timeout=20s >/dev/null 2>&1 || { echo "kubeconfig.sh: the new kubeconfig does not reach the cluster" >&2; exit 1; }
mv -f "$tmp" "$out"
trap - EXIT
echo $((asked + secs)) > "$out.expires"
echo "kubeconfig.sh: $out refreshed for cluster $name (lifetime requested ${secs}s, to $(date -u -d "@$((asked + secs))" '+%Y-%m-%d %H:%M UTC'))"
