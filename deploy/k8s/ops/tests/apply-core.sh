#!/usr/bin/env bash
# apply-core.sh against a stubbed kubectl (no cluster): the 2026-11-26 outage (20-core.yaml applied raw, the
# placeholder VYREON_CORE_IMAGE taken as core's image). The running image goes in place of the placeholder;
# a placeholder left, or a running image that cannot be read, refuses before anything is applied; a dry run
# applies nothing. Run by `cargo test` (crates/vyreon-core, tests/ops_scripts.rs), or alone:
# bash deploy/k8s/ops/tests/apply-core.sh
set -euo pipefail

SRC=$(cd "$(dirname "$0")/.." && pwd)
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/k8s/ops" "$T/bin" "$T/state"
cp "$SRC/apply-core.sh" "$T/k8s/ops/"
cp "$SRC/../20-core.yaml" "$T/k8s/"
chmod +x "$T/k8s/ops/apply-core.sh"
REG=registry.digitalocean.com/vyreon

cat > "$T/bin/kubectl" <<'SH'
#!/usr/bin/env bash
S=$STUB_STATE
echo "kubectl $*" >> "$S/calls"
case "$*" in
  *"get deploy vyreon-core"*) [[ -f $S/unreadable ]] && exit 1; cat "$S/live_image" ;;
  *"diff -f -"*) cat > "$S/diffed"; exit 1 ;;
  *"apply -f -"*) cat > "$S/applied"; sed -n 's/^ *image: \(.*\)$/\1/p' "$S/applied" | head -1 > "$S/live_image" ;;
  *"rollout status"*) ;;
  *) echo "kubectl stub: unexpected $*" >&2; exit 3 ;;
esac
SH
chmod +x "$T/bin/kubectl"
export PATH="$T/bin:$PATH" STUB_STATE=$T/state KUBECONFIG=$T/kubeconfig
run() { "$T/k8s/ops/apply-core.sh" "$@" > "$T/out" 2>&1; }
ok() { echo "ok - $*"; }
no() { echo "FAILED - $*"; sed 's/^/    /' "$T/out"; exit 1; }
start() { rm -f "$T/state/calls" "$T/state/applied" "$T/state/diffed" "$T/state/unreadable"; echo "$REG/vyreon-core:9bf80be" > "$T/state/live_image"; }

# 1. applied with the running image in both places (core and its migrate step), and the rollout waited for
start
run || no "the manifest applies"
[[ $(grep -c "image: $REG/vyreon-core:9bf80be" "$T/state/applied") == 2 ]] || no "both images are the running one"
! grep -q VYREON_CORE_IMAGE "$T/state/applied" || no "no placeholder is applied"
grep -q "rollout status deployment vyreon-core" "$T/state/calls" || no "the rollout is waited for"
grep -q "applied; core runs $REG/vyreon-core:9bf80be" "$T/out" || no "says what runs"
ok "the running image replaces the placeholder"

# 2. a dry run: the diff only
start
run --dry-run || no "a dry run succeeds"
[[ -f $T/state/diffed && ! -f $T/state/applied ]] || no "a dry run diffs and applies nothing"
ok "a dry run applies nothing"

# 3. the live Deployment already broken (the placeholder as its image), or unreadable: refused
start
echo "VYREON_CORE_IMAGE" > "$T/state/live_image"
! run || no "a placeholder as the running image is refused"
grep -q "REFUSED, nothing applied: the running core image is not a registry image" "$T/out" || no "says why"
[[ ! -f $T/state/applied ]] || no "nothing applied"
start
touch "$T/state/unreadable"
! run || no "an unreadable Deployment is refused"
[[ ! -f $T/state/applied ]] || no "nothing applied"
ok "a running image that cannot be read is refused"

# 4. another placeholder left in the manifest: refused before the diff
start
sed 's#image: VYREON_CORE_IMAGE#image: VYREON_WORKER_IMAGE#' "$T/k8s/20-core.yaml" > "$T/other.yaml"
! run "$T/other.yaml" || no "another image placeholder is refused"
grep -q "an image that is not the running core image remains" "$T/out" || no "says why"
[[ ! -f $T/state/applied && ! -f $T/state/diffed ]] || no "nothing diffed or applied"
ok "a placeholder left is refused"
echo "apply-core: all passed"
