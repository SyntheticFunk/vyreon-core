#!/usr/bin/env bash
# The worker-image release (deploy/k8s/README.md, "Apply the ScaledJobs": a new image, C-068's order),
# or a core-image deploy, as one command (C-073; MILESTONES' automation debt: the ~8 steps were run by
# hand 9 times on 2026-11-22/23). Stops at the first refusal, says where it stopped and what state it
# left (a paused queue stays paused: the watch alerts it), and prints the evidence it recorded.
#
# usage:
#   release.sh [--dry-run] [--from N] [--old <build>] --evidence <file.json> <engine-image-tag> "<reason>"
#   release.sh [--dry-run] --core <core-image-tag> "<reason>"
#
#   <engine-image-tag>  the local worker image vyreon-worker:<tag> (the engine's deploy/worker/build.sh)
#   --evidence <file>   REQUIRED for a worker release: a JSON object with what the build passed, at least
#                       "tests" (the suites run and their counts) and "checks" (the golden comparison and
#                       the block-start result, C-014b, C-027), each non-empty; merged into the admission
#                       evidence under "validation". There is no default: a build is never admitted on
#                       the script's own metadata alone.
#   --core <tag>        deploy the local core image vyreon-core:<tag> instead (docker build -f
#                       deploy/core/Dockerfile): push, then 20-core.yaml applied with it; nothing else.
#                       A tag the Deployment already runs is refused (a same-tag rollout changes nothing),
#                       and the pod's image digest must be the one pushed
#   --dry-run           every step printed with its command; only reads run (the image's identity, the
#                       cluster's live state, `kubectl diff` of what would be applied, the daily build's
#                       change included); nothing is pushed, registered, paused, applied or written to
#                       VYREON_RELEASE_DIR (its files go to a temporary directory, removed at the end)
#   --from N            resume at step N after a stop, once its cause is fixed: the run file of this build
#                       (VYREON_RELEASE_DIR/run-<commit>.json, written after every step) gives the old
#                       build, the hand-over's id and token and whether this run paused the queue
#   --old <build>       the old build, where neither the live ScaledJobs nor a run file say it
#
# Needs, from the environment (nothing hard-coded, nothing secret printed):
#   KUBECONFIG              the cluster's kubeconfig
#   VYREON_DO_TOKEN_FILE    a DigitalOcean API token file (registry push); read, never printed
#   VYREON_REGISTRY         default registry.digitalocean.com/vyreon
#   VYREON_RELEASE_DIR      where the inventory, evidence and run files are kept (default
#                           ${XDG_STATE_HOME:-$HOME/.local/state}/vyreon-release)
#   VYREON_VERIFY_MINUTES   how long step 11 waits for every kind's first claim (default 20)
# Tools: docker, kubectl, python3. doctl is not needed.
#
# The steps of a worker release (each refuses and stops on any error):
#   1  build identity and slot inventory, from the image itself (`vyreon-worker build-identity`, `inventory`)
#   2  push the image to the registry
#   3  `vyreon-core build register -` (the inventory on stdin)
#   4  `vyreon-core queue pause` (refused before if the queue is already paused: that pause is someone
#      else's, and this run never resumes a pause it did not make)
#   5  `vyreon-core build hand-over --preview` then `--expect <its token>` from the old build (the live
#      ScaledJobs' build, which must equal VYREON_CORE_DAILY_BUILD where that is set); an old build with
#      nothing to hand over is withdrawn, then previewed again (work queued meanwhile is handed over)
#   6  the evidence JSON (with --evidence) kept in VYREON_RELEASE_DIR and `vyreon-core build admit`
#   7  50-keda.yaml applied with the image and build in place of its placeholders; every ScaledJob then
#      counts for the new build, runs the new image, carries no pause annotation and is Ready
#   8  VYREON_CORE_DAILY_BUILD set to the new build in 20-core.yaml (when the daily runs were on the
#      old one), and 20-core.yaml applied with the RUNNING core image in place of VYREON_CORE_IMAGE
#      (read from the live Deployment: the raw file once took core down), its diff printed first
#   9  the unfinished worker Jobs of other images deleted (the withdrawn build's pods compute on with
#      no lease and hold KEDA's slots: hand-over 5), but not a Job whose pod holds a draining lease
#      (C-077: the hand-over let the old build's running day blocks finish the day they compute)
#   10 `vyreon-core queue resume`, only if the pause is still exactly this run's (a cause added while it
#      ran, a bundle refused or a job failed for the last time, leaves it paused and stops here)
#   11 every kind of work KEDA should start for the new build (`core.scaler_count`) gets a lease of the
#      new build within VYREON_VERIFY_MINUTES (a kind whose ScaledJob has draining Jobs, whose slots may
#      all be theirs until they end, is verified at the end of step 12); skipped by `--from 12`
#   12 C-077: the draining blocks (`vyreon-core build drains`) finish their day; each one's Job is deleted
#      once its lease has ended (an engine before C-077 computes on into its next day, unleased); core
#      itself stops a drain at its bound (work::DRAIN_MINUTES, 6 h) and carries its day. Polled every
#      minute; an interrupt leaves the drains to core and `--from 12` resumes the clean-up. Steps 9 and 12
#      delete only Jobs of an image neither this release's nor the live ScaledJobs' (a later release's
#      Jobs are never touched), and nothing at all in a round where a draining pod cannot be looked up;
#      then the kinds the drains held are verified to claim work
#
# Before anything, the operator's kubeconfig is refreshed when it is within 2 days of its end or the
# cluster refuses it (`kubeconfig.sh --if-needed`; a dry run only says whether it would be).
set -euo pipefail

NS=vyreon
REG=${VYREON_REGISTRY:-registry.digitalocean.com/vyreon}
HERE=$(cd "$(dirname "$0")" && pwd)
K8S=$(cd "$HERE/.." && pwd)
VERIFY_MINUTES=${VYREON_VERIFY_MINUTES:-20}
SCALEDJOBS=(tape day-block day-block-daily ordered-pass refcheck republish)

# Every step the header documents is defined (C-077 audit fix 6cb40c2 dropped step 10, the queue resume, unnoticed:
# the 2026-11-25 release left the queue paused until the operator resumed it by hand).
# (release-4 audit, Codex 8) and each does its operation: the worker release's step N (after its label, up to the
# next step's) holds the command that does it
python3 - "$0" <<'PY' || exit 2
import re, sys
text = open(sys.argv[1]).read()
worker = text[text.index("# ---------------------------------------------------------------- worker image"):]
ops = {1: "build-identity", 2: 'push "$LOCAL"', 3: "build register", 4: "queue pause", 5: "hand_over", 6: "build admit",
       7: "50-keda.yaml", 8: "apply_core", 9: "delete_stale", 10: "queue resume --expect", 11: "verify_claims", 12: "delete_stale"}
starts = {int(m.group(1)): m.start() for m in re.finditer(r'^step (\d+) "', worker, re.M)}
for n, op in ops.items():
    if n not in starts:
        sys.exit(f"release.sh: step {n} is not defined in this script")
    later = [p for k, p in starts.items() if p > starts[n]]
    body = worker[starts[n]:min(later) if later else len(worker)]
    if op not in body:
        sys.exit(f"release.sh: step {n} does not do its operation ({op})")
PY

DRY=0 FROM=1 OLD_GIVEN="" EVIDENCE_IN="" CORE_TAG=""
args=()
while (($#)); do
  case "$1" in
    --dry-run) DRY=1 ;;
    --from) FROM=${2:?--from N}; shift ;;
    --old) OLD_GIVEN=${2:?--old <build>}; shift ;;
    --evidence|--notes) EVIDENCE_IN=${2:?--evidence <file.json>}; shift ;;
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
  ((${#args[@]} == 2)) || { echo "usage: release.sh [--dry-run] [--from N] [--old <build>] --evidence <file.json> <engine-image-tag> \"<reason>\"" >&2; exit 2; }
  TAG=${args[0]} REASON=${args[1]}
fi
[[ -n ${REASON// /} ]] || { echo "release.sh: a release needs a reason" >&2; exit 2; }
[[ $FROM =~ ^([1-9]|1[012])$ ]] || { echo "release.sh: --from takes a step from 1 to 12" >&2; exit 2; }
: "${KUBECONFIG:?set KUBECONFIG to the kubeconfig of the cluster}"
export KUBECONFIG
# automation debt 2 (owner, 2026-11-24): a DOKS kubeconfig lives 7 days; a release never stops on an
# expired one. A dry run writes nothing: it says whether the real run would refresh
if ((DRY)); then
  "$(cd "$(dirname "$0")" && pwd)/kubeconfig.sh" --check "$KUBECONFIG" || true
else
  "$(cd "$(dirname "$0")" && pwd)/kubeconfig.sh" --if-needed "$KUBECONFIG" || { echo "release.sh: the kubeconfig could not be refreshed (kubeconfig.sh above says why); nothing was done" >&2; exit 1; }
fi

RESUMED_AT="" STEP=0 DONE=() FAILED="" DOCKER_AUTH="" SCRATCH="" PAUSE_REASON="" OLD="" HAND_OVER="" TOKEN="" BUILD="" RUN="" RUN_OLD="" EVIDENCE=""
if ((DRY)); then
  # a rehearsal writes nothing where real releases keep their records
  SCRATCH=$(mktemp -d)
  DIR=$SCRATCH
else
  DIR=${VYREON_RELEASE_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/vyreon-release}
fi
say() { printf '%s\n' "$*"; }
step() { STEP=$1; shift; say ""; say "== step $STEP: $*"; }
skip() { ((STEP < FROM)); }
cleanup() {
  [[ -n $DOCKER_AUTH ]] && rm -rf "$DOCKER_AUTH"
  [[ -n $SCRATCH ]] && rm -rf "$SCRATCH"
  DOCKER_AUTH="" SCRATCH=""
  return 0
}
fail() {
  FAILED=1
  say "" >&2
  say "release.sh: STOPPED at step $STEP: $*" >&2
  ((${#DONE[@]})) && { say "done before the stop:" >&2; printf '  %s\n' "${DONE[@]}" >&2; }
  if [[ -n $PAUSE_REASON ]] && ((DRY == 0)); then
    say "the queue is left PAUSED by this run (the watch alerts it); fix the cause, then: release.sh --from $STEP --evidence <file> ${TAG:-} \"<reason>\" (run file $RUN)" >&2
  fi
  cleanup
  exit 1
}
# any other stop (a command that failed under `set -e`) is reported the same way; the registry
# credential and a dry run's files are removed whatever happens, an interrupt or a kill included
# (release audit, Gemini 7)
on_exit() {
  local rc=$?
  if ((rc != 0)) && [[ -z $FAILED ]]; then
    trap - EXIT
    fail "stopped (exit $rc; the message is above)"
  fi
  cleanup
}
trap on_exit EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP
core() { kubectl -n "$NS" exec -c core deploy/vyreon-core -- vyreon-core "$@"; }
core_in() { local file=$1; shift; kubectl -n "$NS" exec -i -c core deploy/vyreon-core -- vyreon-core "$@" < "$file"; }
pg_primary() { kubectl -n "$NS" get pod -l cnpg.io/cluster=vyreon-pg,cnpg.io/instanceRole=primary -o jsonpath='{.items[0].metadata.name}'; }
sql() { kubectl -n "$NS" exec "$(pg_primary)" -c postgres -- psql -X -A -t -F ' ' -d vyreon -v ON_ERROR_STOP=1 -c "$1"; }
live_core_image() { kubectl -n "$NS" get deploy vyreon-core -o jsonpath='{.spec.template.spec.containers[?(@.name=="core")].image}'; }
live_daily() { kubectl -n "$NS" get deploy vyreon-core -o jsonpath='{.spec.template.spec.containers[?(@.name=="core")].env[?(@.name=="VYREON_CORE_DAILY_BUILD")].value}'; }
# the build a ScaledJob's trigger counts for (empty when the ScaledJob does not exist)
scaledjob_build() {
  kubectl -n "$NS" get scaledjob "$1" -o jsonpath='{.spec.triggers[0].metadata.query}' 2>/dev/null \
    | grep -oE "'[0-9a-f]+\+[0-9a-f]+'" | tr -d "'" || true
}
scaledjob_image() { kubectl -n "$NS" get scaledjob "$1" -o jsonpath='{.spec.jobTargetRef.template.spec.containers[0].image}' 2>/dev/null || true; }
# the queue: "paused<TAB>reason" or "running"
queue_state() {
  core queue | python3 -c '
import json, sys
d = json.load(sys.stdin)
print("paused\t" + (d.get("pause_reason") or "") if d.get("paused") else "running")'
}
file_daily() {
  python3 - "$1" <<'PY'
import re, sys
text = open(sys.argv[1]).read()
m = re.search(r'- name: VYREON_CORE_DAILY_BUILD\n\s+value: "([^"]*)"', text)
print(m.group(1) if m else "")
PY
}
set_file_daily() {
  python3 - "$1" "$2" <<'PY'
import re, sys
path, build = sys.argv[1], sys.argv[2]
text = open(path).read()
new, n = re.subn(r'(- name: VYREON_CORE_DAILY_BUILD\n\s+value: )"[^"]*"', lambda m: m.group(1) + '"' + build + '"', text)
if n != 1:
    sys.exit("20-core.yaml: VYREON_CORE_DAILY_BUILD is not set once")
open(path, "w").write(new)
PY
}
# `manifest` with `image` in place of its placeholder: its diff against the live objects printed (an
# error of `kubectl diff`, exit 2 and above, stops the release: release audit, Codex 19), then applied
# unless a dry run
apply_core() {
  local manifest=$1 image=$2 rc=0 out
  say "   the live Deployment against $(basename "$manifest") with image $image (kubectl diff):"
  out=$(sed "s#VYREON_CORE_IMAGE#$image#g" "$manifest" | kubectl diff -f - 2>&1) || rc=$?
  [[ -n $out ]] && say "$out" | sed 's/^/     /'
  ((rc <= 1)) || fail "kubectl diff failed (exit $rc): nothing applied"
  if ((DRY)); then
    say "   \$ sed 's#VYREON_CORE_IMAGE#$image#g' deploy/k8s/20-core.yaml | kubectl apply -f -"
  else
    sed "s#VYREON_CORE_IMAGE#$image#g" "$manifest" | kubectl apply -f -
    kubectl -n "$NS" rollout status deployment vyreon-core --timeout=15m
  fi
}
# the registry's credential: a docker config of our own holding the token (DigitalOcean's registry takes
# the API token as both user and password, so `docker login -u` would put it on a command line, which
# any process can read); never printed, removed on any exit
push() {
  local local_ref=$1 remote=$2
  : "${VYREON_DO_TOKEN_FILE:?set VYREON_DO_TOKEN_FILE to a DigitalOcean API token file}"
  say "   \$ docker tag $local_ref $remote"
  say "   \$ docker --config <a config holding the token from \$VYREON_DO_TOKEN_FILE> push $remote"
  ((DRY)) && return 0
  docker tag "$local_ref" "$remote"
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
# the run file: what a later --from needs (release audit, Codex 5 and 7)
save_run() {
  ((DRY)) && return 0
  [[ -n $RUN ]] || return 0
  python3 - "$RUN" "$STEP" "$BUILD" "$OLD" "$HAND_OVER" "$TOKEN" "$PAUSE_REASON" "${REMOTE:-}" "${RESUMED_AT:-}" "${HELD:-}" <<'PY'
import json, sys
path, step, build, old, hand_over, token, pause, image, resumed, held = sys.argv[1:]
json.dump({"at_step": int(step), "build": build, "old": old or None, "hand_over": hand_over or None, "hand_over_token": token or None,
           "pause_reason": pause or None, "image": image, "resumed_at": resumed or None, "held": held.split() or None}, open(path, "w"), indent=1)
PY
}
load_run() {
  [[ -f $RUN ]] || return 0
  eval "$(python3 - "$RUN" <<'PY'
import json, shlex, sys
d = json.load(open(sys.argv[1]))
for k, v in (("RUN_OLD", d.get("old")), ("HAND_OVER", d.get("hand_over")), ("TOKEN", d.get("hand_over_token")), ("PAUSE_REASON", d.get("pause_reason")),
             ("RESUMED_AT", d.get("resumed_at")), ("HELD", " ".join(d.get("held") or []) or None)):
    print(f"{k}={shlex.quote(str(v) if v is not None else '')}")
PY
)"
}
record() { local w=""; ((DRY)) && w="(dry run, not done) "; DONE+=("step $STEP: $w$*"); say "   -> $w$*"; save_run; }

mkdir -p "$DIR"
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
((DRY)) && say "DRY RUN: only reads run; every change is printed, not made; nothing is kept"

# ---------------------------------------------------------------- core image
if [[ -n $CORE_TAG ]]; then
  LOCAL=vyreon-core:$CORE_TAG REMOTE=$REG/vyreon-core:$CORE_TAG DIGEST=""
  step 1 "preflight: the image, the live Deployment, the file's daily build"
  docker image inspect "$LOCAL" >/dev/null 2>&1 || fail "no local image $LOCAL (docker build -f deploy/core/Dockerfile -t $LOCAL .)"
  RUNNING=$(live_core_image) || fail "the live Deployment vyreon-core cannot be read"
  [[ -n $RUNNING ]] || fail "the live Deployment vyreon-core has no core container"
  # release audit (Codex 16): the same tag again changes no pod template; the new bytes would never run
  [[ $RUNNING != "$REMOTE" ]] || fail "the Deployment already runs $REMOTE: tag the new image with its own commit"
  [[ $(file_daily "$K8S/20-core.yaml") == "$(live_daily)" ]] || fail "20-core.yaml's VYREON_CORE_DAILY_BUILD ($(file_daily "$K8S/20-core.yaml")) is not the live one ($(live_daily)): commit what runs first"
  record "running $RUNNING; daily build $(live_daily || true)"
  step 2 "push $REMOTE"
  push "$LOCAL" "$REMOTE"
  ((DRY)) || { DIGEST=$(digest_of "$LOCAL"); [[ -n $DIGEST ]] || fail "the pushed image's registry digest cannot be read"; }
  record "pushed $REMOTE $DIGEST"
  step 3 "apply 20-core.yaml with $REMOTE (migrations run in its init container)"
  apply_core "$K8S/20-core.yaml" "$REMOTE"
  if ((DRY == 0)); then
    [[ $(live_core_image) == "$REMOTE" ]] || fail "the Deployment does not run $REMOTE"
    POD_DIGEST=$(kubectl -n "$NS" get pod -l app=vyreon-core -o jsonpath='{.items[0].status.containerStatuses[?(@.name=="core")].imageID}')
    [[ $POD_DIGEST == *"${DIGEST##*@}" ]] || fail "the core pod runs $POD_DIGEST, not the pushed ${DIGEST##*@}"
  fi
  record "core runs $REMOTE ${DIGEST##*@}"
  step 4 "core answers"
  ((DRY)) || core queue >/dev/null || fail "core does not answer \`vyreon-core queue\`"
  if ((DRY == 0)); then
    EVIDENCE=$DIR/core-$CORE_TAG-$STAMP.json
    python3 - "$EVIDENCE" "$REMOTE" "$RUNNING" "$REASON" "$STAMP" "$DIGEST" <<'PY'
import json, sys
path, image, was, reason, at, digest = sys.argv[1:]
json.dump({"core_image": image, "digest": digest or None, "replaced": was, "reason": reason, "released_at": at,
           "released_by": "deploy/k8s/ops/release.sh --core"}, open(path, "w"), indent=1)
PY
    say ""; say "== released core $REMOTE (was $RUNNING); evidence $EVIDENCE"; cat "$EVIDENCE"; say ""
  else
    say ""; say "== dry run of a core release of $REMOTE (was $RUNNING): nothing was done, nothing kept"
  fi
  exit 0
fi

# ---------------------------------------------------------------- worker image
LOCAL=vyreon-worker:$TAG REMOTE=$REG/vyreon-worker:$TAG

step 0 "preflight: the image, the evidence, core's scaler count"
docker image inspect "$LOCAL" >/dev/null 2>&1 || fail "no local image $LOCAL (the engine's deploy/worker/build.sh)"
# release audit (Codex 2): no admission without what the build passed
[[ -n $EVIDENCE_IN ]] || fail "--evidence <file.json> is required: what the build passed (\"tests\", \"checks\": the golden comparison and the block-start result)"
python3 - "$EVIDENCE_IN" <<'PY' || fail "--evidence $EVIDENCE_IN is not a JSON object with non-empty \"tests\" and \"checks\""
import json, sys
d = json.load(open(sys.argv[1]))
assert isinstance(d, dict) and d.get("tests") and d.get("checks")
PY
# 50-keda.yaml's triggers ask core.scaler_count (migration 0055): a core without it leaves KEDA unable to count
[[ $(sql "SELECT to_regprocedure('core.scaler_count(text,text,boolean)') IS NOT NULL") == t ]] \
  || fail "core's database has no core.scaler_count (migration 0055): deploy the core image first (release.sh --core <tag>)"
# C-074 (release-2 audit, Gemini 10): a build registers its republish plans only with a core that keeps them (0057)
[[ $(sql "SELECT to_regclass('core.build_republish') IS NOT NULL") == t ]] \
  || fail "core's database has no core.build_republish (migration 0057): deploy the core image first (release.sh --core <tag>)"
# C-077: the hand-over drains running blocks, and step 9 and 12 read the drains, only with a core that keeps them (0059)
[[ $(sql "SELECT to_regclass('core.drain') IS NOT NULL") == t ]] \
  || fail "core's database has no core.drain (migration 0059): deploy the core image first (release.sh --core <tag>)"

step 1 "build identity and inventory from $LOCAL; the old build, the queue, the file's daily build"
BUILD=$(docker run --rm --entrypoint /opt/vyreon/bin/vyreon-worker "$LOCAL" build-identity /opt/vyreon/bin /opt/vyreon/bin | tail -1)
[[ $BUILD =~ ^[0-9a-f]+\+[0-9a-f]+$ ]] || fail "build-identity printed '$BUILD', not <commit>+<inventory>"
RUN=$DIR/run-${BUILD%%+*}.json
INVENTORY=$DIR/inventory-${BUILD%%+*}.json
docker run --rm --entrypoint /opt/vyreon/bin/vyreon-worker "$LOCAL" inventory "$BUILD" > "$INVENTORY.part"
python3 - "$INVENTORY.part" "$BUILD" <<'PY' || fail "the inventory is not the build's"
import json, sys
d = json.load(open(sys.argv[1]))
assert d.get("engine_build") == sys.argv[2] and d.get("slots"), "no engine_build or no slots"
# C-073: each profile's layer declared, so core can never take a day-local bundle as a base
p = d.get("profiles") or {}
assert sorted(p) == sorted({s["profile_revision"] for s in d["slots"]}) and set(p.values()) <= {"base", "day-local", "history"}, \
    "the inventory does not declare every profile's layer (profiles: base | day-local | history)"
# C-074: the engine's republish plans (engine D-462), which core keeps at registration
r = d.get("republish") or {}
assert isinstance(r.get("seeds"), dict) and r["seeds"], "the inventory declares no republish plans (an engine before D-462)"
PY
mv "$INVENTORY.part" "$INVENTORY"
if ((FROM > 1)); then
  [[ -f $RUN ]] || [[ -n $OLD_GIVEN ]] || fail "--from $FROM needs this build's run file $RUN (or --old <build>)"
  load_run
elif [[ -f $RUN ]] && ((DRY == 0)); then
  fail "a run file of $BUILD exists ($RUN): a release of it began; resume it with --from <step>, or remove the file once you know it is over"
fi
LIVE_BUILDS=() LIVE_IMAGE=""
for sj in "${SCALEDJOBS[@]}"; do
  b=$(scaledjob_build "$sj"); [[ -n $b ]] && LIVE_BUILDS+=("$b")
  [[ -z $LIVE_IMAGE ]] && LIVE_IMAGE=$(scaledjob_image "$sj")
done
LIVE_OTHER=$( ((${#LIVE_BUILDS[@]})) && printf '%s\n' "${LIVE_BUILDS[@]}" | sort -u | grep -vxF "$BUILD" || true)
DAILY=$(live_daily || true)
say "   live ScaledJobs: build(s) $( ((${#LIVE_BUILDS[@]})) && printf '%s ' $(printf '%s\n' "${LIVE_BUILDS[@]}" | sort -u))image ${LIVE_IMAGE:-none}; VYREON_CORE_DAILY_BUILD ${DAILY:-unset}"
# the old build: given, else the run file's, else the live ScaledJobs' (a partial apply of step 7 leaves
# some on each build: the run file decides then, release audit Codex 5)
OLD=${OLD_GIVEN:-${RUN_OLD:-$LIVE_OTHER}}
(($(grep -c . <<<"$OLD" || true) <= 1)) || fail "the live ScaledJobs count for several builds ($(echo $OLD)) and no run file says which is the old one: --old <build>"
if ((FROM <= 5)); then
  [[ -n $OLD ]] || fail "no old build: the live ScaledJobs count for none (give --old <build> if there is one)"
  [[ $OLD != "$BUILD" ]] || fail "the live ScaledJobs already run $BUILD: nothing to release"
  [[ $LIVE_IMAGE != "$REMOTE" ]] || fail "the ScaledJobs already run the image $REMOTE: tag the new image with its own commit"
  [[ -z $DAILY || $DAILY == "$OLD" ]] || fail "VYREON_CORE_DAILY_BUILD is $DAILY, the ScaledJobs' build $OLD: settle which runs by hand first"
fi
FILE_DAILY=$(file_daily "$K8S/20-core.yaml")
[[ $FILE_DAILY == "$DAILY" || $FILE_DAILY == "$BUILD" ]] \
  || fail "20-core.yaml's VYREON_CORE_DAILY_BUILD ($FILE_DAILY) is neither the live one (${DAILY:-unset}) nor $BUILD: commit what runs first"
# release audit (Codex 1): the queue paused by someone else is theirs; this run neither pauses over it
# nor resumes it
QUEUE=$(queue_state)
if ((FROM <= 4)) && [[ $QUEUE == paused* ]]; then
  fail "the queue is already paused (${QUEUE#paused$'\t'}): a pause this run did not make is never resumed by it; release when it is resumed"
fi
record "build $BUILD, inventory $INVENTORY ($(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1]))["slots"]))' "$INVENTORY") slots); old build ${OLD:-none}"

step 2 "push $REMOTE"
if ! skip; then
  push "$LOCAL" "$REMOTE"
  record "pushed $REMOTE $(digest_of "$LOCAL")"
fi

step 3 "register $BUILD (not admitted: the hand-over refuses an admitted successor)"
if ! skip; then
  say "   \$ vyreon-core build register - < $INVENTORY"
  if ((DRY == 0)); then
    out=$(core_in "$INVENTORY" build register - 2>&1) || fail "core refused the registration: $out"
    say "   $out"
  fi
  record "registered $BUILD"
fi

step 4 "pause the queue"
if ! skip; then
  WANT="release $BUILD: $REASON"
  say "   \$ vyreon-core queue pause \"$WANT\""
  if ((DRY == 0)); then
    out=$(core queue pause "$WANT" 2>&1) || fail "the pause was refused: $out"
    QUEUE=$(queue_state)
    [[ $QUEUE == "paused"$'\t'"$WANT" ]] || fail "the queue's pause is not this run's (${QUEUE#paused$'\t'}): someone else paused it at the same moment; nothing more is done"
    PAUSE_REASON=$WANT
  fi
  record "queue paused by this run"
fi

# a preview, then exactly what it showed; false when the old build has nothing to hand over
hand_over() {
  local preview out
  say "   \$ vyreon-core build hand-over --preview $OLD $BUILD \"$REASON\""
  if preview=$(core build hand-over --preview "$OLD" "$BUILD" "$REASON" 2>&1); then
    say "$preview" | sed 's/^/     /'
    TOKEN=$(grep -oE -- '--expect [0-9a-f]{16}' <<<"$preview" | awk '{print $2}' | head -1)
    [[ -n $TOKEN ]] || fail "the preview printed no token"
    save_run
    say "   \$ vyreon-core build hand-over --expect $TOKEN $OLD $BUILD \"$REASON\""
    out=$(core build hand-over --expect "$TOKEN" "$OLD" "$BUILD" "$REASON" 2>&1) || fail "the hand-over was refused: $out"
    say "$out" | sed 's/^/     /'
    HAND_OVER=$(grep -oE '^hand-over [0-9]+' <<<"$out" | awk '{print $2}' | head -1)
    [[ -n $HAND_OVER ]] || fail "the hand-over printed no id: $out"
    return 0
  elif grep -q "has no unfinished work to hand over" <<<"$preview"; then
    return 1
  else
    fail "the hand-over preview was refused: $preview"
  fi
}

step 5 "hand over from $OLD to $BUILD"
if ! skip && ((DRY)); then
  # the build is not registered in a dry run: the preview would be refused for that alone
  say "   \$ vyreon-core build hand-over --preview $OLD $BUILD \"$REASON\"   (its token, then:)"
  say "   \$ vyreon-core build hand-over --expect <token> $OLD $BUILD \"$REASON\""
  say "   (if $OLD has nothing to hand over: vyreon-core build withdraw $OLD \"released $BUILD: $REASON\", then the preview again)"
  record "hand-over (dry run)"
elif ! skip && [[ -n $HAND_OVER ]]; then
  record "hand-over $HAND_OVER (token $TOKEN) already done by this run (run file)"
elif ! skip; then
  if hand_over; then
    record "hand-over $HAND_OVER token $TOKEN"
  else
    # release audit (Codex 15): withdrawn, then previewed again, so work queued between the two (a
    # daily session at 07:30) is handed over, never stranded under a withdrawn build
    say "   \$ vyreon-core build withdraw $OLD \"released $BUILD: $REASON\""
    out=$(core build withdraw "$OLD" "released $BUILD: $REASON" 2>&1) || fail "the withdrawal was refused: $out"
    if hand_over; then
      record "$OLD withdrawn; work queued meanwhile handed over: hand-over $HAND_OVER token $TOKEN"
    else
      record "$OLD had nothing to hand over: withdrawn"
    fi
  fi
fi

step 6 "evidence and admission"
EVIDENCE=$DIR/evidence-${BUILD%%+*}-$STAMP.json
if ! skip; then
  python3 - "$EVIDENCE" "$BUILD" "$REMOTE" "$(digest_of "$LOCAL")" "$OLD" "$REASON" "$STAMP" "${HAND_OVER:-}" "${TOKEN:-}" "$INVENTORY" "$EVIDENCE_IN" "$DRY" <<'PY'
import json, sys
path, build, image, digest, old, reason, at, hand_over, token, inventory, validation, dry = sys.argv[1:]
e = {"build": build, "image": image, "digest": digest or None, "engine_commit": build.split("+")[0], "replaces": old or None,
     "reason": reason, "hand_over": int(hand_over) if hand_over else None, "hand_over_token": token or None,
     "inventory_slots": len(json.load(open(inventory))["slots"]), "released_at": at,
     "validation": json.load(open(validation)), "admitted_by": "deploy/k8s/ops/release.sh (C-073)"}
if dry == "1":
    e["dry_run"] = "a rehearsal: nothing was admitted"
json.dump(e, open(path, "w"), indent=1)
PY
  say "   \$ vyreon-core build admit $BUILD - < $EVIDENCE"
  if ((DRY == 0)); then
    out=$(core_in "$EVIDENCE" build admit "$BUILD" - 2>&1) || fail "the admission was refused: $out"
  fi
  record "admitted $BUILD with $EVIDENCE"
fi

# every ScaledJob on the new build and image, no pause annotation, Ready (release audit, Codex 6)
verify_scaledjobs() {
  local deadline=$((SECONDS + ${VYREON_SCALEDJOB_WAIT_SECONDS:-120})) bad
  while :; do
    bad=$(kubectl -n "$NS" get scaledjob "${SCALEDJOBS[@]}" -o json | python3 -c '
import json, sys
build, image, names = sys.argv[1], sys.argv[2], sys.argv[3].split()
items = {i["metadata"]["name"]: i for i in json.load(sys.stdin)["items"]}
for n in names:
    i = items.get(n)
    if i is None:
        print(f"{n}: missing"); continue
    q = i["spec"]["triggers"][0]["metadata"]["query"]
    if f"\x27{build}\x27" not in q: print(f"{n}: counts for another build")
    if i["spec"]["jobTargetRef"]["template"]["spec"]["containers"][0]["image"] != image: print(f"{n}: another image")
    paused = [k for k in (i["metadata"].get("annotations") or {}) if k.startswith("autoscaling.keda.sh/paused")]
    if paused: print(f"{n}: paused ({paused[0]})")
    ready = [c for c in (i.get("status") or {}).get("conditions") or [] if c.get("type") == "Ready"]
    if not ready or ready[0].get("status") != "True": print(f"{n}: not Ready")' "$BUILD" "$REMOTE" "${SCALEDJOBS[*]}") || bad="the ScaledJobs cannot be read"
    [[ -z $bad ]] && return 0
    ((SECONDS < deadline)) || fail "the ScaledJobs are not all on $BUILD, unpaused and Ready: $(echo $bad)"
    sleep 10
  done
}

step 7 "apply 50-keda.yaml with $REMOTE and $BUILD"
if ! skip; then
  if ((DRY)); then
    say "   \$ sed -e 's#VYREON_WORKER_IMAGE#$REMOTE#' -e 's#VYREON_ENGINE_BUILD#$BUILD#' deploy/k8s/50-keda.yaml | kubectl apply -f -"
  else
    # a partial apply is recovered by --from 7 (the run file names the old build)
    sed -e "s#VYREON_WORKER_IMAGE#$REMOTE#g" -e "s#VYREON_ENGINE_BUILD#$BUILD#g" "$K8S/50-keda.yaml" | kubectl apply -f -
    verify_scaledjobs
  fi
  record "ScaledJobs on $BUILD and $REMOTE, none paused, all Ready"
fi

step 8 "daily build and core's manifest"
if ! skip; then
  MANIFEST=$K8S/20-core.yaml
  if [[ -n $DAILY && $DAILY != "$BUILD" ]]; then
    say "   20-core.yaml: VYREON_CORE_DAILY_BUILD $DAILY -> $BUILD"
    if ((DRY)); then
      # the diff shows the change; the checked-in file is not touched (release audit, Codex 9)
      cp "$K8S/20-core.yaml" "$DIR/20-core.yaml"
      MANIFEST=$DIR/20-core.yaml
    fi
    set_file_daily "$MANIFEST" "$BUILD"
  fi
  CORE_IMAGE=$(live_core_image)
  [[ -n $CORE_IMAGE && $CORE_IMAGE != VYREON_CORE_IMAGE ]] || fail "the running core image cannot be read"
  apply_core "$MANIFEST" "$CORE_IMAGE"
  ((DRY)) || [[ -z $DAILY || $(live_daily) == "$BUILD" ]] || fail "the live VYREON_CORE_DAILY_BUILD is not $BUILD"
  record "core on $CORE_IMAGE${DAILY:+, daily build $BUILD} (commit deploy/k8s/20-core.yaml)"
fi

# C-077: the live drains (`vyreon-core build drains`, one JSON object a line) and the Jobs of their pods
# (a lease's worker is its pod's name, VYREON_WORKER_NAME); fails when core cannot be asked
DRAINS="" DRAIN_JOBS="" HELD=""
read_drains() { DRAINS=$(core build drains) || fail "core does not answer \`vyreon-core build drains\`"; }
# DRAIN_JOBS: the Jobs whose pods drain. Fails closed (release-3 audit, Codex 17, Gemini 4): a pod that
# cannot be looked up (anything but "not found": it is gone) makes the function fail, and the caller then
# deletes nothing this time
draining_jobs() {
  local w out
  DRAIN_JOBS=""
  for w in $(python3 -c 'import json, sys
for l in sys.stdin:
    if l.strip(): print(json.loads(l)["worker"])' <<<"$DRAINS"); do
    if out=$(kubectl -n "$NS" get pod "$w" -o jsonpath='{.metadata.labels.job-name}' 2>&1); then
      DRAIN_JOBS="$DRAIN_JOBS $out"
    elif ! grep -q NotFound <<<"$out"; then
      say "   cannot look up draining pod $w ($(tail -1 <<<"$out" | cut -c1-120)): nothing is deleted this time"
      return 1
    fi
  done
  return 0
}
# the ScaledJobs (kinds) of the draining Jobs: their slots may all be the drains' (fails closed as above)
held_kinds() {
  local j out
  HELD=""
  for j in $DRAIN_JOBS; do
    if out=$(kubectl -n "$NS" get job "$j" -o jsonpath='{.metadata.labels.scaledjob\.keda\.sh/name}' 2>&1); then
      HELD="$HELD $out"
    elif ! grep -q NotFound <<<"$out"; then
      return 1
    fi
  done
  HELD=$(tr ' ' '\n' <<<"$HELD" | sort -u | tr '\n' ' ')
}
# the drains in a line: "<pod> <day>[ (day done, its check to come)] until HH:MM UTC; ..."
drain_words() {
  python3 -c '
import json, sys
out = []
for line in sys.stdin:
    if line.strip():
        d = json.loads(line)
        done = " (day done, its check to come)" if d["day_done"] else ""
        out.append("%s %s%s until %s UTC" % (d["worker"], d["session"], done, d["until"][11:16]))
print("; ".join(out))'
}
# the unfinished worker Jobs to delete: of an image that is neither this release's nor the one the live
# ScaledJobs run now (release-3 audit, Codex 3: a later release's Jobs are never this run's to delete), and
# not draining (DRAIN_JOBS, read just before); fails when the live ScaledJobs cannot be read
stale_jobs() {
  local live
  live=$(scaledjob_image day-block)
  [[ -n $live ]] || return 1
  kubectl -n "$NS" get jobs -l scaledjob.keda.sh/name -o json | python3 -c '
import json, sys
images, keep = set(sys.argv[1].split()), set(sys.argv[2].split())
for j in json.load(sys.stdin)["items"]:
    done = any(c["type"] in ("Complete", "Failed") and c["status"] == "True" for c in j.get("status", {}).get("conditions") or [])
    if not done and j["spec"]["template"]["spec"]["containers"][0]["image"] not in images and j["metadata"]["name"] not in keep:
        print(j["metadata"]["name"])' "$REMOTE $live" "$DRAIN_JOBS"
}
# deletes the stale Jobs now, when every draining pod could be looked up; else nothing this time
delete_stale() {
  local stale
  read_drains
  draining_jobs || return 1
  stale=$(stale_jobs) || { say "   the live ScaledJobs cannot be read: nothing is deleted this time"; return 1; }
  STALE=$stale
  if [[ -n $STALE ]]; then
    say "   \$ kubectl -n $NS delete job --ignore-not-found $(echo $STALE)"
    # shellcheck disable=SC2086
    ((DRY)) || kubectl -n "$NS" delete job --ignore-not-found $STALE
  fi
}

step 9 "delete the unfinished worker Jobs of other images (a block finishing its day is left to step 12)"
if ! skip; then
  STALE=""
  if delete_stale; then
    if [[ -n $STALE ]]; then record "deleted $(echo $STALE | wc -w) Jobs: $(echo $STALE)${DRAIN_JOBS:+; draining (step 12):$DRAIN_JOBS}"
    else record "no unfinished Job of another image to delete${DRAIN_JOBS:+; draining (step 12):$DRAIN_JOBS}"; fi
  else
    record "nothing deleted yet (a lookup failed): step 12 deletes them"
  fi
fi

# every kind KEDA should start for $BUILD (core.scaler_count) gets a lease of $BUILD granted since $1 (a
# timestamp), within VERIFY_MINUTES; kinds in $2 are not waited for, and when $3 is given only its kinds are
verify_claims() {
  local since=$1 skip_kinds=$2 only=$3 deadline=$((SECONDS + VERIFY_MINUTES * 60)) needed="" missing="" rows q
  while :; do
    # (release-4 audit, Codex 3) a paused queue counts nothing claimable: never read as "nothing to claim"
    q=$(queue_state)
    [[ $q == running ]] || fail "the queue is paused (${q#paused$'\t'}): nothing can be verified while it is; resume it once its cause is understood, then --from 11"
    # per ScaledJob: what KEDA counts for the new build now, and its leases granted since then
    rows=$(sql "SELECT v.k, core.scaler_count(v.kind, '$BUILD', v.daily),
                       (SELECT count(*) FROM core.lease l JOIN core.job j USING (job_id)
                        WHERE j.engine_build = '$BUILD' AND j.kind = v.kind AND (v.daily IS NULL OR (j.priority = 1) = v.daily)
                          AND (l.granted_at >= '$since'::timestamptz OR l.ended_at IS NULL))
                FROM (VALUES ('tape', 'TAPE', NULL::boolean), ('day-block', 'DAY_BLOCK', false), ('day-block-daily', 'DAY_BLOCK', true),
                             ('ordered-pass', 'ORDERED_PASS', NULL), ('refcheck', 'REFCHECK', NULL), ('republish', 'REPUBLISH', NULL)) v(k, kind, daily)") || rows=""
    missing=""
    while read -r k count leases; do
      [[ -z $k ]] && continue
      [[ " $skip_kinds " == *" $k "* ]] && continue
      [[ -n $only && " $only " != *" $k "* ]] && continue
      ((count > 0)) && [[ " $needed " != *" $k "* ]] && needed="$needed $k"
      [[ " $needed " == *" $k "* ]] && ((leases == 0)) && missing="$missing $k"
    done <<<"$rows"
    say "   $(date -u +%H:%M:%S) to start:${needed:- none}; still without a claim:${missing:- none}"
    [[ -n $rows && -z $missing ]] && break
    ((SECONDS < deadline)) || fail "no pod of $BUILD claimed work of:$missing in $VERIFY_MINUTES min (kubectl -n $NS get scaledjob,pods; vyreon-core queue)"
    sleep 30
  done
  record "claimed by the new build:${needed:- nothing to claim}"
}

step 10 "resume the queue (only this run's pause)"
if ! skip; then
  say "   \$ vyreon-core queue resume"
  if ((DRY == 0)); then
    QUEUE=$(queue_state)
    if [[ $QUEUE == running ]]; then
      say "   the queue is already running"
    elif [[ -z $PAUSE_REASON ]]; then
      fail "this run did not pause the queue (no run file says it did): resume it by hand once its pause is understood"
    elif [[ $QUEUE != "paused"$'\t'"$PAUSE_REASON" ]]; then
      # a bundle refused or a job failed for the last time while it ran (`also: ...`), or another pause
      fail "the pause holds more than this run's: ${QUEUE#paused$'\t'} -- left paused for the owner"
    else
      # (release-4 audit, Codex 1) core resumes only a pause that reads exactly this run's, in one statement:
      # a cause added since this read (a refused bundle) is left paused for the owner
      out=$(core queue resume --expect "$PAUSE_REASON" 2>&1) || fail "the resume was refused (left paused for the owner): $out"
    fi
    PAUSE_REASON=""
  fi
  RESUMED_AT=$( ((DRY)) || sql "SELECT now()")
  record "queue resumed"
fi

step 11 "every kind KEDA should start for $BUILD claims work (up to $VERIFY_MINUTES min)"
if ((DRY)); then
  say "   (dry run: polls core.scaler_count and the leases of $BUILD per kind)"
elif ((FROM <= 11)); then
  # (release-3 audit, Gemini 9) only in a run that resumed the queue: `--from 12` resumes the drains alone.
  # (release-4 audit, Codex 4) claims count from the queue's resume (the run file's, on `--from 11`), and a
  # live lease of the new build counts whenever it was granted
  SINCE=${RESUMED_AT:-$(sql "SELECT now()")}
  # C-077: a ScaledJob whose Jobs drain may have every slot held by them (day-block's 8 by phase 1's 8
  # blocks): its kind is verified once they end (step 12)
  read_drains
  draining_jobs && held_kinds || fail "the draining pods or their Jobs cannot be looked up: rerun with --from 11"
  [[ -n ${HELD// /} ]] && say "   verified after the drains (their slots held by draining blocks, step 12): $HELD"
  save_run
  verify_claims "$SINCE" "$HELD" ""
else
  say "   (resumed at step 12: the claims were verified, or are verified after the drains)"
fi

step 12 "the old build's running blocks finish their day (C-077); each one's Job deleted once its lease ends"
if ((DRY)); then
  read_drains
  say "   (dry run: polls \`vyreon-core build drains\` every minute and deletes each ended drain's Job; core stops a drain at its bound)"
  if [[ -n $DRAINS ]]; then say "$DRAINS" | sed 's/^/     draining now: /'; fi
else
  if ((FROM == 12)); then
    # (release-4 audit, Codex 2) the kinds step 11 left to the end: the run file's, and any draining now
    KEPT_HELD=$HELD
    read_drains
    draining_jobs && held_kinds || fail "the draining pods or their Jobs cannot be looked up: rerun with --from 12"
    HELD=$(tr ' ' '\n' <<<"$KEPT_HELD $HELD" | sort -u | tr '\n' ' ')
    # a run file older than these records says none: every kind is verified at the end
    [[ -n ${HELD// /} || -n $RESUMED_AT ]] || HELD="tape day-block day-block-daily ordered-pass refcheck republish"
  fi
  WAITED=$HELD
  while :; do
    STALE=""
    delete_stale || true
    [[ -z $DRAINS ]] && break
    # core stops a drain at its bound (its sweep, every half minute); one standing 15 minutes past it is core's fault
    LATE=$(python3 -c 'import datetime, json, sys
now = datetime.datetime.now(datetime.timezone.utc)
for l in sys.stdin:
    if not l.strip(): continue
    d = json.loads(l)
    if now > datetime.datetime.fromisoformat(d["until"].replace("Z", "+00:00")) + datetime.timedelta(minutes=15):
        print(d["worker"])' <<<"$DRAINS")
    [[ -z $LATE ]] || fail "drains past their bound by 15 minutes, which core should have stopped: $(echo $LATE) (vyreon-core build drains; core's log)"
    say "   $(date -u +%H:%M:%S) draining: $(drain_words <<<"$DRAINS")"
    sleep 60
  done
  record "no block of another build drains; their Jobs deleted"
  # (release-3 audit, Codex 12) the kinds whose slots the drains held claim work of the new build now
  if [[ -n ${WAITED// /} ]]; then
    say "   the kinds the drains held:$WAITED"
    verify_claims "${SINCE:-${RESUMED_AT:-$(sql "SELECT now() - interval '6 hours 30 minutes'")}}" "" "$WAITED"
  fi
fi

say ""
if ((DRY)); then
  say "== dry run of the release of $BUILD ($REMOTE), replacing ${OLD:-nothing}: nothing was done, nothing kept"
else
  say "== released $BUILD ($REMOTE), replacing ${OLD:-nothing}"
  rm -f "$RUN"
fi
printf '  %s\n' "${DONE[@]}"
[[ -f $EVIDENCE ]] && { say "evidence ($EVIDENCE):"; cat "$EVIDENCE"; say ""; }
((DRY)) || say "now commit deploy/k8s/20-core.yaml if it changed, and record the release in docs/MILESTONES.md"
