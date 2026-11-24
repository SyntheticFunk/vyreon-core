#!/usr/bin/env bash
# The worker-image release (deploy/k8s/README.md, "Apply the ScaledJobs": a new image, C-068's order),
# or a core-image deploy, as one command (C-073; MILESTONES' automation debt: the ~8 steps were run by
# hand 9 times on 2026-11-22/23). Stops at the first refusal, says where it stopped and what state it
# left (a paused queue stays paused: the watch alerts it), and prints the evidence it recorded.
#
# usage:
#   release.sh [--dry-run] [--from N] [--old <build>] [--notes <file.json>] <engine-image-tag> "<reason>"
#   release.sh [--dry-run] --core <core-image-tag> "<reason>"
#
#   <engine-image-tag>  the local worker image vyreon-worker:<tag> (the engine's deploy/worker/build.sh)
#   --core <tag>        deploy the local core image vyreon-core:<tag> instead (docker build -f
#                       deploy/core/Dockerfile): push, then 20-core.yaml applied with it; nothing else
#   --dry-run           every step printed with its command; only reads run (the image's identity, the
#                       cluster's live state), nothing is pushed, registered, paused or applied
#   --from N            start at step N (after a stop, once its cause is fixed: the steps before it are
#                       done); the old build then comes from --old when the live ScaledJobs are past it
#   --notes <file>      a JSON object merged into the admission evidence under "notes" (the tests run,
#                       why the release is needed: what the hand-written evidence files said)
#
# Needs, from the environment (nothing hard-coded, nothing secret printed):
#   KUBECONFIG              the cluster's kubeconfig
#   VYREON_DO_TOKEN_FILE    a DigitalOcean API token file (registry push); read, never printed
#   VYREON_REGISTRY         default registry.digitalocean.com/vyreon
#   VYREON_RELEASE_DIR      where the inventory and evidence files are kept (default
#                           ${XDG_STATE_HOME:-$HOME/.local/state}/vyreon-release)
#   VYREON_VERIFY_MINUTES   how long step 11 waits for the new build's first lease (default 20)
# Tools: docker, kubectl, python3. doctl is not needed.
#
# The steps of a worker release (each refuses and stops on any error):
#   1  build identity and slot inventory, from the image itself (`vyreon-worker build-identity`, `inventory`)
#   2  push the image to the registry
#   3  `vyreon-core build register` (the inventory sent on stdin: no file is copied into the pod)
#   4  `vyreon-core queue pause`
#   5  `vyreon-core build hand-over --preview` then `--expect <its token>` from the old build (the
#      live ScaledJobs' build, which must equal VYREON_CORE_DAILY_BUILD where that is set); an old
#      build with nothing to hand over is withdrawn instead
#   6  the evidence JSON written (kept in VYREON_RELEASE_DIR) and `vyreon-core build admit`
#   7  50-keda.yaml applied with the image and build in place of its placeholders
#   8  VYREON_CORE_DAILY_BUILD set to the new build in 20-core.yaml (when the daily runs were on the
#      old one), and 20-core.yaml applied with the RUNNING core image in place of VYREON_CORE_IMAGE
#      (read from the live Deployment: the raw file once took core down), its diff printed first
#   9  the unfinished worker Jobs of other images deleted (the withdrawn build's pods compute on with
#      no lease and hold KEDA's slots: hand-over 5)
#   10 `vyreon-core queue resume`
#   11 the new build's pods claim work: a lease of the new build within VYREON_VERIFY_MINUTES, or
#      nothing of it claimable
set -euo pipefail

NS=vyreon
REG=${VYREON_REGISTRY:-registry.digitalocean.com/vyreon}
HERE=$(cd "$(dirname "$0")" && pwd)
K8S=$(cd "$HERE/.." && pwd)
DIR=${VYREON_RELEASE_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/vyreon-release}
VERIFY_MINUTES=${VYREON_VERIFY_MINUTES:-20}
SCALEDJOBS=(tape day-block day-block-daily ordered-pass refcheck)

DRY=0 FROM=1 OLD_GIVEN="" NOTES="" CORE_TAG=""
args=()
while (($#)); do
  case "$1" in
    --dry-run) DRY=1 ;;
    --from) FROM=${2:?--from N}; shift ;;
    --old) OLD_GIVEN=${2:?--old <build>}; shift ;;
    --notes) NOTES=${2:?--notes <file.json>}; shift ;;
    --core) CORE_TAG=${2:?--core <core-image-tag>}; shift ;;
    -h|--help) sed -n '2,/^set -euo/p' "$0" | sed '$d'; exit 0 ;;
    --*) echo "release.sh: unknown option $1" >&2; exit 2 ;;
    *) args+=("$1") ;;
  esac
  shift
done
if [[ -n $CORE_TAG ]]; then
  ((${#args[@]} == 1)) || { echo "usage: release.sh [--dry-run] --core <core-image-tag> \"<reason>\"" >&2; exit 2; }
  REASON=${args[0]}
else
  ((${#args[@]} == 2)) || { echo "usage: release.sh [--dry-run] [--from N] [--old <build>] [--notes <file.json>] <engine-image-tag> \"<reason>\"" >&2; exit 2; }
  TAG=${args[0]} REASON=${args[1]}
fi
[[ -n ${REASON// /} ]] || { echo "release.sh: a release needs a reason" >&2; exit 2; }
[[ $FROM =~ ^([1-9]|1[01])$ ]] || { echo "release.sh: --from takes a step from 1 to 11" >&2; exit 2; }
: "${KUBECONFIG:?set KUBECONFIG to the kubeconfig of the cluster}"
export KUBECONFIG

STEP=0 DONE=() FAILED="" DOCKER_AUTH=""
say() { printf '%s\n' "$*"; }
step() { STEP=$1; shift; say ""; say "== step $STEP: $*"; }
skip() { ((STEP < FROM)); }
record() { local w=""; ((DRY)) && w="(dry run, not done) "; DONE+=("step $STEP: $w$*"); say "   -> $w$*"; }
fail() {
  FAILED=1
  say "" >&2
  say "release.sh: STOPPED at step $STEP: $*" >&2
  ((${#DONE[@]})) && { say "done before the stop:" >&2; printf '  %s\n' "${DONE[@]}" >&2; }
  if ((STEP >= 4 && STEP < 10)) && ((DRY == 0)) && [[ -z $CORE_TAG ]]; then
    say "the queue is left PAUSED (the watch alerts it); fix the cause, then: release.sh --from $STEP${OLD:+ --old $OLD} ${TAG:-} \"<reason>\"" >&2
  fi
  exit 1
}
# any other stop (a command that failed under `set -e`) is reported the same way; the registry
# credential is removed whatever happens
on_exit() {
  local rc=$?
  [[ -n $DOCKER_AUTH ]] && rm -rf "$DOCKER_AUTH"
  if ((rc != 0)) && [[ -z $FAILED ]]; then
    trap - EXIT
    fail "a command failed (exit $rc; its message is above)"
  fi
}
trap on_exit EXIT
# a command that changes something: printed, and run unless --dry-run
act() { say "   \$ $*"; ((DRY)) || "$@"; }
core() { kubectl -n "$NS" exec -c core deploy/vyreon-core -- vyreon-core "$@"; }
core_in() { local file=$1; shift; kubectl -n "$NS" exec -i -c core deploy/vyreon-core -- vyreon-core "$@" < "$file"; }
pg_primary() { kubectl -n "$NS" get pod -l cnpg.io/cluster=vyreon-pg,cnpg.io/instanceRole=primary -o jsonpath='{.items[0].metadata.name}'; }
sql() { kubectl -n "$NS" exec "$(pg_primary)" -c postgres -- psql -X -A -t -d vyreon -c "$1"; }
live_core_image() { kubectl -n "$NS" get deploy vyreon-core -o jsonpath='{.spec.template.spec.containers[?(@.name=="core")].image}'; }
live_daily() { kubectl -n "$NS" get deploy vyreon-core -o jsonpath='{.spec.template.spec.containers[?(@.name=="core")].env[?(@.name=="VYREON_CORE_DAILY_BUILD")].value}'; }
# the build a ScaledJob's trigger counts for (empty when the ScaledJob does not exist)
scaledjob_build() {
  kubectl -n "$NS" get scaledjob "$1" -o jsonpath='{.spec.triggers[0].metadata.query}' 2>/dev/null \
    | grep -oE "'[0-9a-f]+\+[0-9a-f]+'" | tr -d "'" || true
}
scaledjob_image() { kubectl -n "$NS" get scaledjob "$1" -o jsonpath='{.spec.jobTargetRef.template.spec.containers[0].image}' 2>/dev/null || true; }
file_daily() {
  python3 - "$K8S/20-core.yaml" <<'PY'
import re, sys
text = open(sys.argv[1]).read()
m = re.search(r'- name: VYREON_CORE_DAILY_BUILD\n\s+value: "([^"]*)"', text)
print(m.group(1) if m else "")
PY
}
set_file_daily() {
  python3 - "$K8S/20-core.yaml" "$1" <<'PY'
import re, sys
path, build = sys.argv[1], sys.argv[2]
text = open(path).read()
new, n = re.subn(r'(- name: VYREON_CORE_DAILY_BUILD\n\s+value: )"[^"]*"', lambda m: m.group(1) + '"' + build + '"', text)
if n != 1:
    sys.exit("20-core.yaml: VYREON_CORE_DAILY_BUILD is not set once")
open(path, "w").write(new)
PY
}
# 20-core.yaml with `image` in place of its placeholder, on stdout
core_manifest() { sed "s#VYREON_CORE_IMAGE#$1#g" "$K8S/20-core.yaml"; }
apply_core() {
  local image=$1
  say "   the live Deployment against 20-core.yaml with image $image (kubectl diff):"
  core_manifest "$image" | kubectl diff -f - | sed 's/^/     /' || true   # exit 1 means "differs"
  if ((DRY)); then
    say "   \$ sed 's#VYREON_CORE_IMAGE#$image#g' deploy/k8s/20-core.yaml | kubectl apply -f -"
  else
    core_manifest "$image" | kubectl apply -f -
    kubectl -n "$NS" rollout status deployment vyreon-core --timeout=15m
  fi
}
# the registry's credential: a docker config of our own holding the token (DigitalOcean's registry takes
# the API token as both user and password, so `docker login -u` would put it on a command line, which
# any process can read); never printed, removed on exit
push() {
  local local_ref=$1 remote=$2
  : "${VYREON_DO_TOKEN_FILE:?set VYREON_DO_TOKEN_FILE to a DigitalOcean API token file}"
  act docker tag "$local_ref" "$remote"
  say "   \$ docker --config <a config holding the token from \$VYREON_DO_TOKEN_FILE> push $remote"
  ((DRY)) && return 0
  DOCKER_AUTH=$(mktemp -d)
  python3 - "$VYREON_DO_TOKEN_FILE" "$DOCKER_AUTH/config.json" "${REG%%/*}" <<'PY'
import base64, json, os, sys
token = open(sys.argv[1]).read().strip()
if not token:
    sys.exit("the token file is empty")
auth = base64.b64encode(f"{token}:{token}".encode()).decode()
fd = os.open(sys.argv[2], os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
with os.fdopen(fd, "w") as f:
    json.dump({"auths": {sys.argv[3]: {"auth": auth}}}, f)
PY
  docker --config "$DOCKER_AUTH" push "$remote" | tail -1
  rm -rf "$DOCKER_AUTH"
  DOCKER_AUTH=""
}
digest_of() { docker image inspect "$1" --format '{{range .RepoDigests}}{{.}} {{end}}' 2>/dev/null | tr ' ' '\n' | grep -F "${REG}/" | head -1 || true; }

mkdir -p "$DIR"
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
((DRY)) && say "DRY RUN: only reads run; every change is printed, not made"

# ---------------------------------------------------------------- core image
if [[ -n $CORE_TAG ]]; then
  LOCAL=vyreon-core:$CORE_TAG REMOTE=$REG/vyreon-core:$CORE_TAG
  step 1 "preflight: the image, the live Deployment, the file's daily build"
  docker image inspect "$LOCAL" >/dev/null 2>&1 || fail "no local image $LOCAL (docker build -f deploy/core/Dockerfile -t $LOCAL .)"
  RUNNING=$(live_core_image) || fail "the live Deployment vyreon-core cannot be read"
  [[ -n $RUNNING ]] || fail "the live Deployment vyreon-core has no core container"
  [[ $(file_daily) == "$(live_daily)" ]] || fail "20-core.yaml's VYREON_CORE_DAILY_BUILD ($(file_daily)) is not the live one ($(live_daily)): commit what runs first"
  record "running $RUNNING; daily build $(live_daily || true)"
  step 2 "push $REMOTE"
  push "$LOCAL" "$REMOTE"
  record "pushed $REMOTE $(digest_of "$LOCAL")"
  step 3 "apply 20-core.yaml with $REMOTE (migrations run in its init container)"
  apply_core "$REMOTE"
  ((DRY)) || [[ $(live_core_image) == "$REMOTE" ]] || fail "the Deployment does not run $REMOTE"
  record "core runs $REMOTE"
  step 4 "core answers"
  ((DRY)) || core queue >/dev/null || fail "core does not answer \`vyreon-core queue\`"
  EVIDENCE=$DIR/core-$CORE_TAG-$STAMP.json
  python3 - "$EVIDENCE" "$REMOTE" "$RUNNING" "$REASON" "$STAMP" "$(digest_of "$LOCAL")" <<'PY'
import json, sys
path, image, was, reason, at, digest = sys.argv[1:]
json.dump({"core_image": image, "digest": digest or None, "replaced": was, "reason": reason, "released_at": at,
           "released_by": "deploy/k8s/ops/release.sh --core"}, open(path, "w"), indent=1)
PY
  say ""; say "== released core $REMOTE (was $RUNNING); evidence $EVIDENCE"; cat "$EVIDENCE"; say ""
  exit 0
fi

# ---------------------------------------------------------------- worker image
LOCAL=vyreon-worker:$TAG REMOTE=$REG/vyreon-worker:$TAG

step 0 "preflight: the image, core's scaler count, the live builds, the file's daily build"
docker image inspect "$LOCAL" >/dev/null 2>&1 || fail "no local image $LOCAL (the engine's deploy/worker/build.sh)"
[[ -z $NOTES ]] || python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); assert isinstance(d, dict) and d' "$NOTES" \
  || fail "--notes $NOTES is not a JSON object"
# 50-keda.yaml's triggers ask core.scaler_count (migration 0055): a core without it leaves KEDA unable to count
[[ $(sql "SELECT to_regprocedure('core.scaler_count(text,text,boolean)') IS NOT NULL") == t ]] \
  || fail "core's database has no core.scaler_count (migration 0055): deploy the core image first (release.sh --core <tag>)"
LIVE_BUILDS=() LIVE_IMAGE=""
for sj in "${SCALEDJOBS[@]}"; do
  b=$(scaledjob_build "$sj"); [[ -n $b ]] && LIVE_BUILDS+=("$b")
  [[ -z $LIVE_IMAGE ]] && LIVE_IMAGE=$(scaledjob_image "$sj")
done
LIVE_BUILD=$(printf '%s\n' "${LIVE_BUILDS[@]}" | sort -u)
(($(printf '%s\n' "${LIVE_BUILDS[@]}" | sort -u | grep -c .) <= 1)) || fail "the live ScaledJobs count for different builds: $(echo $LIVE_BUILD): settle that by hand first"
DAILY=$(live_daily || true)
say "   live ScaledJobs: build ${LIVE_BUILD:-none}, image ${LIVE_IMAGE:-none}; VYREON_CORE_DAILY_BUILD ${DAILY:-unset}"

step 1 "build identity and inventory from $LOCAL"
BUILD=$(docker run --rm --entrypoint /opt/vyreon/bin/vyreon-worker "$LOCAL" build-identity /opt/vyreon/bin /opt/vyreon/bin | tail -1)
[[ $BUILD =~ ^[0-9a-f]+\+[0-9a-f]+$ ]] || fail "build-identity printed '$BUILD', not <commit>+<inventory>"
INVENTORY=$DIR/inventory-${BUILD%%+*}.json
docker run --rm --entrypoint /opt/vyreon/bin/vyreon-worker "$LOCAL" inventory "$BUILD" > "$INVENTORY.part"
python3 - "$INVENTORY.part" "$BUILD" <<'PY' || fail "the inventory is not the build's"
import json, sys
d = json.load(open(sys.argv[1]))
assert d.get("engine_build") == sys.argv[2] and d.get("slots"), "no engine_build or no slots"
PY
mv "$INVENTORY.part" "$INVENTORY"
record "build $BUILD, inventory $INVENTORY ($(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1]))["slots"]))' "$INVENTORY") slots)"

# the old build: the live ScaledJobs', unless they already run the new one (a --from after step 7)
OLD=${OLD_GIVEN:-$LIVE_BUILD}
[[ $OLD == "$BUILD" && -z $OLD_GIVEN ]] && OLD=""
if ((FROM <= 7)); then
  [[ -n $OLD ]] || fail "no old build: the live ScaledJobs count for none (give --old <build> if there is one)"
  [[ $OLD != "$BUILD" ]] || fail "the live ScaledJobs already run $BUILD: nothing to release"
  [[ -z $DAILY || $DAILY == "$OLD" ]] || fail "VYREON_CORE_DAILY_BUILD is $DAILY, the ScaledJobs' build $OLD: settle which runs by hand first"
fi
FILE_DAILY=$(file_daily)
[[ $FILE_DAILY == "$DAILY" || $FILE_DAILY == "$BUILD" ]] \
  || fail "20-core.yaml's VYREON_CORE_DAILY_BUILD ($FILE_DAILY) is neither the live one (${DAILY:-unset}) nor $BUILD: commit what runs first"

step 2 "push $REMOTE"
if ! skip; then
  push "$LOCAL" "$REMOTE"
  record "pushed $REMOTE $(digest_of "$LOCAL")"
fi

step 3 "register $BUILD (not admitted: the hand-over refuses an admitted successor)"
if ! skip; then
  say "   \$ vyreon-core build register /dev/stdin < $INVENTORY"
  if ((DRY == 0)); then
    out=$(core_in "$INVENTORY" build register /dev/stdin 2>&1) || fail "core refused the registration: $out"
    say "   $out"
  fi
  record "registered $BUILD"
fi

step 4 "pause the queue"
if ! skip; then
  say "   \$ vyreon-core queue pause \"release $BUILD: $REASON\""
  ((DRY)) || { out=$(core queue pause "release $BUILD: $REASON" 2>&1) || fail "the pause was refused: $out"; }
  record "queue paused"
fi

step 5 "hand over from $OLD to $BUILD"
HAND_OVER="" TOKEN=""
if ! skip && ((DRY)); then
  # the build is not registered in a dry run: the preview would be refused for that alone
  say "   \$ vyreon-core build hand-over --preview $OLD $BUILD \"$REASON\"   (its token, then:)"
  say "   \$ vyreon-core build hand-over --expect <token> $OLD $BUILD \"$REASON\""
  say "   (if $OLD has nothing to hand over: vyreon-core build withdraw $OLD \"released $BUILD: $REASON\")"
  record "hand-over (dry run)"
elif ! skip; then
  say "   \$ vyreon-core build hand-over --preview $OLD $BUILD \"$REASON\""
  if preview=$(core build hand-over --preview "$OLD" "$BUILD" "$REASON" 2>&1); then
    say "$preview" | sed 's/^/     /'
    TOKEN=$(grep -oE -- '--expect [0-9a-f]{16}' <<<"$preview" | awk '{print $2}' | head -1)
    [[ -n $TOKEN ]] || fail "the preview printed no token"
    say "   \$ vyreon-core build hand-over --expect $TOKEN $OLD $BUILD \"$REASON\""
    out=$(core build hand-over --expect "$TOKEN" "$OLD" "$BUILD" "$REASON" 2>&1) || fail "the hand-over was refused: $out"
    say "$out" | sed 's/^/     /'
    HAND_OVER=$(grep -oE '^hand-over [0-9]+' <<<"$out" | awk '{print $2}' | head -1)
    [[ -n $HAND_OVER ]] || fail "the hand-over printed no id: $out"
    record "hand-over $HAND_OVER token $TOKEN"
  elif grep -q "has no unfinished work to hand over" <<<"$preview"; then
    say "     $preview"
    say "   \$ vyreon-core build withdraw $OLD \"released $BUILD: $REASON\""
    out=$(core build withdraw "$OLD" "released $BUILD: $REASON" 2>&1) || fail "the withdrawal was refused: $out"
    record "$OLD had nothing to hand over: withdrawn"
  else
    fail "the hand-over preview was refused: $preview"
  fi
fi

step 6 "evidence and admission"
EVIDENCE=$DIR/evidence-${BUILD%%+*}-$STAMP.json
if ! skip; then
  python3 - "$EVIDENCE" "$BUILD" "$REMOTE" "$(digest_of "$LOCAL")" "$OLD" "$REASON" "$STAMP" "${HAND_OVER:-}" "${TOKEN:-}" "$INVENTORY" "${NOTES:-}" <<'PY'
import json, sys
path, build, image, digest, old, reason, at, hand_over, token, inventory, notes = sys.argv[1:]
e = {"build": build, "image": image, "digest": digest or None, "engine_commit": build.split("+")[0], "replaces": old or None,
     "reason": reason, "hand_over": int(hand_over) if hand_over else None, "hand_over_token": token or None,
     "inventory_slots": len(json.load(open(inventory))["slots"]), "released_at": at,
     "admitted_by": "deploy/k8s/ops/release.sh (C-073)"}
if notes:
    e["notes"] = json.load(open(notes))
json.dump(e, open(path, "w"), indent=1)
PY
  say "   \$ vyreon-core build admit $BUILD /dev/stdin < $EVIDENCE"
  if ((DRY == 0)); then
    out=$(core_in "$EVIDENCE" build admit "$BUILD" /dev/stdin 2>&1) || fail "the admission was refused: $out"
  fi
  record "admitted $BUILD with $EVIDENCE"
fi

step 7 "apply 50-keda.yaml with $REMOTE and $BUILD"
if ! skip; then
  if ((DRY)); then
    say "   \$ sed -e 's#VYREON_WORKER_IMAGE#$REMOTE#' -e 's#VYREON_ENGINE_BUILD#$BUILD#' deploy/k8s/50-keda.yaml | kubectl apply -f -"
  else
    sed -e "s#VYREON_WORKER_IMAGE#$REMOTE#g" -e "s#VYREON_ENGINE_BUILD#$BUILD#g" "$K8S/50-keda.yaml" | kubectl apply -f -
    for sj in "${SCALEDJOBS[@]}"; do
      [[ $(scaledjob_build "$sj") == "$BUILD" ]] || fail "ScaledJob $sj does not count for $BUILD after the apply"
    done
  fi
  record "ScaledJobs on $BUILD"
fi

step 8 "daily build and core's manifest"
if ! skip; then
  if [[ -n $DAILY ]]; then
    say "   20-core.yaml: VYREON_CORE_DAILY_BUILD $DAILY -> $BUILD"
    ((DRY)) || set_file_daily "$BUILD"
  fi
  CORE_IMAGE=$(live_core_image)
  [[ -n $CORE_IMAGE && $CORE_IMAGE != VYREON_CORE_IMAGE ]] || fail "the running core image cannot be read"
  apply_core "$CORE_IMAGE"
  ((DRY)) || [[ -z $DAILY || $(live_daily) == "$BUILD" ]] || fail "the live VYREON_CORE_DAILY_BUILD is not $BUILD"
  record "core on $CORE_IMAGE${DAILY:+, daily build $BUILD} (commit deploy/k8s/20-core.yaml)"
fi

step 9 "delete the unfinished worker Jobs of other images"
if ! skip; then
  STALE=$(kubectl -n "$NS" get jobs -l scaledjob.keda.sh/name -o json | python3 -c '
import json, sys
image = sys.argv[1]
for j in json.load(sys.stdin)["items"]:
    done = any(c["type"] in ("Complete", "Failed") and c["status"] == "True" for c in j.get("status", {}).get("conditions") or [])
    if not done and j["spec"]["template"]["spec"]["containers"][0]["image"] != image:
        print(j["metadata"]["name"])' "$REMOTE")
  if [[ -n $STALE ]]; then
    # shellcheck disable=SC2086
    act kubectl -n "$NS" delete job $STALE
    record "deleted $(echo $STALE | wc -w) Jobs: $(echo $STALE)"
  else
    record "no unfinished Job of another image"
  fi
fi

step 10 "resume the queue"
if ! skip; then
  say "   \$ vyreon-core queue resume"
  ((DRY)) || { out=$(core queue resume 2>&1) || fail "the resume was refused: $out"; }
  record "queue resumed"
fi

step 11 "the new build's pods claim work (up to $VERIFY_MINUTES min)"
if ((DRY)); then
  say "   (dry run: polls \`vyreon-core queue\` for a LEASED job of $BUILD)"
else
  deadline=$((SECONDS + VERIFY_MINUTES * 60)) verdict=""
  while ((SECONDS < deadline)); do
    verdict=$(core queue | python3 -c '
import json, sys
d, build = json.load(sys.stdin), sys.argv[1]
mine = [j for j in d["jobs"] if j.get("build") == build]
leased = sum(j["jobs"] for j in mine if j["state"] == "LEASED")
queued = sum(j["jobs"] for j in mine if j["state"] == "QUEUED")
if d.get("paused"): print("paused:", d.get("pause_reason"))
elif leased: print(f"ok: {leased} jobs of the new build leased, {queued} queued")
elif not queued: print("ok: nothing of the new build is queued, nothing to claim")
else: print(f"waiting: {queued} queued, none leased yet")' "$BUILD") || verdict="waiting: core did not answer"
    say "   $(date -u +%H:%M:%S) $verdict"
    [[ $verdict == ok:* ]] && break
    [[ $verdict == paused:* ]] && fail "the queue is paused again: ${verdict#paused: }"
    sleep 30
  done
  [[ $verdict == ok:* ]] || fail "no pod of $BUILD claimed work in $VERIFY_MINUTES min (kubectl -n $NS get pods -l app=vyreon-worker; vyreon-core queue)"
  record "${verdict#ok: }"
fi

say ""
say "== released $BUILD ($REMOTE), replacing ${OLD:-nothing}"
printf '  %s\n' "${DONE[@]}"
[[ -f $EVIDENCE ]] && { say "evidence ($EVIDENCE):"; cat "$EVIDENCE"; say ""; }
((DRY)) || say "now commit deploy/k8s/20-core.yaml if it changed, and record the release in docs/MILESTONES.md"
