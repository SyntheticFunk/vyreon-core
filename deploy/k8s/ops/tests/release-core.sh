#!/usr/bin/env bash
# release.sh --core against stubbed docker and kubectl (no registry, no cluster): C-080's two defects of
# 2026-11-25. (1) An identical-content rebuild: the push reports a new index digest, the node reports the
# digest it first pulled that content as; the platform manifests are the same, so step 3 verifies. A
# different image under the pod is still refused. (2) `--from N` resumes a core release: the steps before N
# skipped, the run file's pushed digest and replaced image used. Run by `cargo test` (crates/vyreon-core,
# tests/ops_scripts.rs), or alone: bash deploy/k8s/ops/tests/release-core.sh
set -euo pipefail

SRC=$(cd "$(dirname "$0")/.." && pwd)
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/k8s/ops" "$T/bin" "$T/state/manifests" "$T/release"
cp "$SRC/release.sh" "$T/k8s/ops/"
cp "$SRC/../20-core.yaml" "$T/k8s/"
printf '#!/usr/bin/env bash\nexit 0\n' > "$T/k8s/ops/kubeconfig.sh"
chmod +x "$T/k8s/ops/kubeconfig.sh" "$T/k8s/ops/release.sh"
printf 'not-a-real-token\n' > "$T/token"

REG=registry.digitalocean.com/vyreon
TAG=d562aa7
# the live incident's digests (2026-11-25): 9bf80be's index, d562aa7's index of the same image
OLD_INDEX=sha256:7f10da8fa4abbd38bea3d7e848fd31c49d3502d6f6729ece93f2262e844ef0ce
NEW_INDEX=sha256:a9fd0b5d68cb0586eda30acbf075f896c2ef5417f0e0afa6f24ddb3bd7bb9b39
OTHER_INDEX=sha256:1111111111111111111111111111111111111111111111111111111111111111
AMD64=sha256:2222222222222222222222222222222222222222222222222222222222222222
AMD64_OTHER=sha256:3333333333333333333333333333333333333333333333333333333333333333
index() { # an OCI index as buildx pushes it: the platform's manifest and its attestation (which differs per build)
  cat <<JSON
{"schemaVersion":2,"mediaType":"application/vnd.oci.image.index.v1+json","manifests":[
 {"mediaType":"application/vnd.oci.image.manifest.v1+json","digest":"$1","size":1608,"platform":{"architecture":"amd64","os":"linux"}},
 {"mediaType":"application/vnd.oci.image.manifest.v1+json","digest":"$2","size":566,
  "annotations":{"vnd.docker.reference.digest":"$1","vnd.docker.reference.type":"attestation-manifest"},"platform":{"architecture":"unknown","os":"unknown"}}]}
JSON
}
index "$AMD64" sha256:4444444444444444444444444444444444444444444444444444444444444444 > "$T/state/manifests/$OLD_INDEX"
index "$AMD64" sha256:5555555555555555555555555555555555555555555555555555555555555555 > "$T/state/manifests/$NEW_INDEX"
index "$AMD64_OTHER" sha256:6666666666666666666666666666666666666666666666666666666666666666 > "$T/state/manifests/$OTHER_INDEX"

cat > "$T/bin/kubectl" <<'SH'
#!/usr/bin/env bash
S=$STUB_STATE
echo "kubectl $*" >> "$S/calls"
case "$*" in
  *"get deploy vyreon-core"*VYREON_CORE_DAILY_BUILD*)
    grep -A1 'name: VYREON_CORE_DAILY_BUILD' "$STUB_K8S/20-core.yaml" | sed -n 's/.*value: "\(.*\)"/\1/p' ;;
  *"get deploy vyreon-core"*) cat "$S/live_image" ;;
  *"get pod -l app=vyreon-core"*) printf '%s' "$(cat "$S/pod_image_id")" ;;
  *"diff -f -"*) cat > /dev/null ;;
  *"apply -f -"*) sed -n 's/^ *image: \(.*vyreon-core:.*\)$/\1/p' | head -1 > "$S/live_image" ;;
  *"rollout status"*) ;;
  *"vyreon-core queue"*) echo '{"paused": false}' ;;
  *) echo "kubectl stub: unexpected $*" >&2; exit 3 ;;
esac
SH
cat > "$T/bin/docker" <<'SH'
#!/usr/bin/env bash
S=$STUB_STATE A="$*"
echo "docker $A" >> "$S/calls"
case "$*" in
  "image inspect "*"--format"*) echo "$STUB_REG/vyreon-core@$(cat "$S/local_digest") " ;;
  "image inspect "*) [[ ! -f $S/no_local ]] ;;
  "tag "*) ;;
  "--config "*" push "*)
    [[ -f $2/config.json ]] || { echo "no registry credential" >&2; exit 1; }
    echo "The push refers to repository [$STUB_REG/vyreon-core]"
    echo "${A##*:}: digest: $(cat "$S/push_digest") size: 856" ;;
  "buildx imagetools inspect --raw "*)
    [[ -f ${DOCKER_CONFIG:-}/config.json ]] || { echo "no registry credential" >&2; exit 1; }
    cat "$S/manifests/${A##*@}" ;;
  *) echo "docker stub: unexpected $*" >&2; exit 3 ;;
esac
SH
chmod +x "$T/bin/kubectl" "$T/bin/docker"

export PATH="$T/bin:$PATH" STUB_STATE=$T/state STUB_K8S=$T/k8s STUB_REG=$REG KUBECONFIG=$T/kubeconfig \
  VYREON_DO_TOKEN_FILE=$T/token VYREON_RELEASE_DIR=$T/release VYREON_REGISTRY=$REG
RUN=$T/release/run-core-$TAG.json
release() { "$T/k8s/ops/release.sh" "$@" > "$T/out" 2>&1; }
ok() { echo "ok - $*"; }
no() { echo "FAILED - $*"; sed 's/^/    /' "$T/out"; exit 1; }
start() { # the cluster before the release: 9bf80be running
  rm -f "$T/state/calls" "$RUN" "$T"/release/core-*.json
  echo "$REG/vyreon-core:9bf80be" > "$T/state/live_image"
  echo "$NEW_INDEX" > "$T/state/push_digest"
  echo "$NEW_INDEX" > "$T/state/local_digest"
}

# 1. an identical rebuild: the pod reports the old index, the same platform manifest: released
start
echo "$REG/vyreon-core@$OLD_INDEX" > "$T/state/pod_image_id"
release --core "$TAG" "a deploy-only change" || no "an identical rebuild verifies"
grep -q "is the pushed ${NEW_INDEX}'s content (an identical rebuild)" "$T/out" || no "says it is the same content"
[[ ! -f $RUN ]] || no "the run file is removed once released"
python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); assert d["digest"].endswith(sys.argv[2]) and d["replaced"].endswith(":9bf80be"), d' \
  "$(ls "$T"/release/core-$TAG-*.json)" "$NEW_INDEX" || no "the evidence names the pushed digest and the image replaced"
! grep -q 'not-a-real-token' "$T/out" || no "the token is never printed"
ok "an identical-content rebuild verifies by its platform manifests"

# 2. the digest the push reported, run by the pod: verified without asking the registry
start
echo "$REG/vyreon-core@$NEW_INDEX" > "$T/state/pod_image_id"
echo "sha256:9999999999999999999999999999999999999999999999999999999999999999" > "$T/state/local_digest"
release --core "$TAG" "a new core" || no "the pushed digest verifies"
! grep -q imagetools "$T/state/calls" || no "no registry read when the digests are equal"
grep -q "core runs $REG/vyreon-core:$TAG ${NEW_INDEX}" "$T/out" || no "the push's digest, not the local image's, is the one verified"
ok "the digest docker push reported is the one compared"

# 3. another image under the pod: stopped at step 3, the run file kept
start
echo "$REG/vyreon-core@$OTHER_INDEX" > "$T/state/pod_image_id"
! release --core "$TAG" "a new core" || no "another image is refused"
grep -q "STOPPED at step 3: the core pod runs $OTHER_INDEX, not the pushed $NEW_INDEX nor an image of its content" "$T/out" || no "says why"
python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); assert d["at_step"] == 2 and d["digest"].endswith(sys.argv[2]) and d["replaced"].endswith(":9bf80be"), d' "$RUN" "$NEW_INDEX" \
  || no "the run file keeps the pushed digest and the image replaced"
ok "an image of other content is refused"

# 4. a release that began is not started again; --from 4 resumes it (the Deployment already runs the tag)
release --core "$TAG" "again" && no "a second start is refused"
grep -q "a run file of core $TAG exists" "$T/out" || no "says to resume"
echo "$REG/vyreon-core@$NEW_INDEX" > "$T/state/pod_image_id"
rm -f "$T/state/calls"
release --from 4 --core "$TAG" "resumed after the cause was understood" || no "--from 4 resumes"
! grep -qE 'push|apply|diff' "$T/state/calls" || no "--from 4 pushes and applies nothing"
python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); assert d["digest"].endswith(sys.argv[2]) and d["replaced"].endswith(":9bf80be"), d' \
  "$(ls "$T"/release/core-$TAG-*.json)" "$NEW_INDEX" || no "the evidence carries the run file's digest and replaced image"
[[ ! -f $RUN ]] || no "the run file is removed once released"
ok "--from 4 resumes a core release from its run file"

# 5. --from 3 with the run file: the apply and its verification again, no push
start
echo "$REG/vyreon-core@$OTHER_INDEX" > "$T/state/pod_image_id"
! release --core "$TAG" "a new core" || no "stopped at step 3"
echo "$REG/vyreon-core@$OLD_INDEX" > "$T/state/pod_image_id"
rm -f "$T/state/calls"
release --from 3 --core "$TAG" "resumed" || no "--from 3 resumes"
! grep -q ' push ' "$T/state/calls" && grep -q 'apply -f -' "$T/state/calls" || no "--from 3 applies, never pushes"
ok "--from 3 verifies again with the run file's digest"

# 6. --from past the core release's steps
release --from 5 --core "$TAG" "x" && no "--from 5 is refused for --core"
grep -q "from 1 to 4 for --core" "$T/out" || no "says the range"
ok "--from is bounded to the core release's steps"
echo "release-core: all passed"
