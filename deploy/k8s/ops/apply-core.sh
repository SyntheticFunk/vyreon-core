#!/usr/bin/env bash
# Applies core's manifest (deploy/k8s/20-core.yaml: core's settings) with the core image the Deployment
# RUNS now in place of the file's placeholder VYREON_CORE_IMAGE, so a settings change never changes the
# image. On 2026-11-26 the file was applied raw: the Deployment took the placeholder as its image and core
# was down about 20 minutes (InvalidImageName), and a daily block lost its lease (C-088). A new core image
# is deployed by `release.sh --core`, not by this.
#
# usage: apply-core.sh [--dry-run] [<manifest>]
#   <manifest>  default deploy/k8s/20-core.yaml beside this script
#   --dry-run   the diff against the live objects only; nothing applied
#
# Refuses, applying nothing, when the running image cannot be read or is itself a placeholder, or when the
# manifest with it in place still holds a placeholder (VYREON_CORE_IMAGE, or any `image:` that is not the
# running image), or when the manifest's VYREON_CORE_DAILY_BUILD is not the live one (it would stop the
# nightly). Prints the diff (`kubectl diff`), applies, and waits for the rollout (15 min), then says
# which image runs. Needs KUBECONFIG; tools: kubectl, sed, grep.
set -euo pipefail
NS=vyreon
DRY=0
while (($#)); do
  case "$1" in
    --dry-run) DRY=1 ;;
    -h|--help) sed -n '2,/^set -euo/p' "$0" | sed '$d'; exit 0 ;;
    --*) echo "apply-core.sh: unknown option $1" >&2; exit 2 ;;
    *) MANIFEST=$1 ;;
  esac
  shift
done
MANIFEST=${MANIFEST:-$(cd "$(dirname "$0")/.." && pwd)/20-core.yaml}
: "${KUBECONFIG:?set KUBECONFIG to the kubeconfig of the cluster}"
refuse() { echo "apply-core.sh: REFUSED, nothing applied: $*" >&2; exit 1; }

[[ -r $MANIFEST ]] || refuse "$MANIFEST cannot be read"
IMAGE=$(kubectl -n "$NS" get deploy vyreon-core -o jsonpath='{.spec.template.spec.containers[?(@.name=="core")].image}') \
  || refuse "the live Deployment vyreon-core cannot be read"
[[ -n $IMAGE && $IMAGE == */vyreon-core:* && $IMAGE != *VYREON_* ]] \
  || refuse "the running core image is not a registry image of vyreon-core (read: '${IMAGE}'); deploy one with release.sh --core"
RENDERED=$(sed "s#VYREON_CORE_IMAGE#$IMAGE#g" "$MANIFEST")
! grep -q 'VYREON_CORE_IMAGE' <<<"$RENDERED" || refuse "the placeholder VYREON_CORE_IMAGE remains"
OTHER=$(grep -nE '^[[:space:]-]*image:' <<<"$RENDERED" | grep -vF "image: $IMAGE" || true)
[[ -z $OTHER ]] || refuse "an image that is not the running core image remains (a placeholder?): $OTHER"
# C-094 (audit 2026-11-26): a checkout whose daily build lags the live one would revert it, and core then
# queues no nightly session (as release.sh's preflight refuses). The build changes only through release.sh.
FILE_DAILY=$(grep -A1 -- '- name: VYREON_CORE_DAILY_BUILD' <<<"$RENDERED" | sed -n 's/^ *value: "\(.*\)"$/\1/p' | head -1)
LIVE_DAILY=$(kubectl -n "$NS" get deploy vyreon-core -o jsonpath='{.spec.template.spec.containers[?(@.name=="core")].env[?(@.name=="VYREON_CORE_DAILY_BUILD")].value}') \
  || refuse "the live Deployment's VYREON_CORE_DAILY_BUILD cannot be read"
[[ $FILE_DAILY == "$LIVE_DAILY" ]] \
  || refuse "the manifest's VYREON_CORE_DAILY_BUILD ('$FILE_DAILY') is not the live one ('$LIVE_DAILY'): the nightly's build changes only through release.sh; apply from a checkout that holds what runs"

echo "core runs $IMAGE; $(basename "$MANIFEST") against the live objects with it (kubectl diff):"
rc=0
out=$(kubectl diff -f - <<<"$RENDERED" 2>&1) || rc=$?
((rc <= 1)) || refuse "kubectl diff failed (exit $rc): $out"
if [[ -n $out ]]; then sed 's/^/  /' <<<"$out"; else echo "  (no change)"; fi
if ((DRY)); then
  echo "dry run: nothing applied"
  exit 0
fi
kubectl apply -f - <<<"$RENDERED"
kubectl -n "$NS" rollout status deployment vyreon-core --timeout=15m
NOW=$(kubectl -n "$NS" get deploy vyreon-core -o jsonpath='{.spec.template.spec.containers[?(@.name=="core")].image}')
[[ $NOW == "$IMAGE" ]] || { echo "apply-core.sh: the Deployment now names $NOW, not $IMAGE" >&2; exit 1; }
echo "applied; core runs $NOW"
