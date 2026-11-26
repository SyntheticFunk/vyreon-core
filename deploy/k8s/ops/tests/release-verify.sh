#!/usr/bin/env bash
# release.sh step 11's check (`verify_claims`), taken from the script and run against canned answers of core's
# database (no cluster). The false stop of 2026-11-25: the re-run watch cancelled the queued REFCHECK jobs while
# step 11 waited for a pod to claim them, and the release stopped with "no pod claimed refcheck". A kind whose
# work stops being claimable before a claim is now said and not verified; one still claimable with no claim
# still stops the release. Run by `cargo test` (crates/vyreon-core, tests/ops_scripts.rs), or alone:
# bash deploy/k8s/ops/tests/release-verify.sh
set -euo pipefail

SRC=$(cd "$(dirname "$0")/.." && pwd)
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
sed -n '/^verify_claims() {/,/^}/p' "$SRC/release.sh" > "$T/verify.sh"
grep -q '^verify_claims() {' "$T/verify.sh" || { echo "FAILED - release.sh has no verify_claims"; exit 1; }

# each poll's rows: kind, KEDA's count, claimable, leases since the resume
cat > "$T/harness.sh" <<'SH'
set -euo pipefail
BUILD=93a04676+abc NS=vyreon
say() { printf '%s\n' "$*"; }
record() { echo "RECORD $*"; }
fail() { echo "FAIL $*"; exit 1; }
queue_state() { echo running; }
sleep() { :; }
# the rows are read in a subshell (`rows=$(sql ...)`): the poll's number is kept in a file; past the last
# poll the last answer stands, and a run that never ends its wait is stopped at 50 polls
sql() {
  local n=$(($(cat "$POLLS/n" 2>/dev/null || echo 0) + 1)) f
  echo "$n" > "$POLLS/n"
  ((n <= 50)) || { echo "the wait did not end" >&2; exit 9; }
  f=$POLLS/$n; [[ -f $f ]] || f=$(ls "$POLLS"/[0-9]* | sort -V | tail -1); cat "$f"
}
source "$VERIFY"
verify_claims "2026-11-25 20:00:00+00" "" ""
SH
ok() { echo "ok - $*"; }
no() { echo "FAILED - $*"; sed 's/^/    /' "$T/out"; exit 1; }
polls() { rm -rf "$T/polls"; mkdir -p "$T/polls"; local i=0; for p in "$@"; do i=$((i + 1)); printf '%b' "$p" > "$T/polls/$i"; done; }
run() { POLLS=$T/polls VERIFY=$T/verify.sh VERIFY_MINUTES=$1 bash "$T/harness.sh" > "$T/out" 2>&1; }

# 1. 2026-11-25: re-runs to start, cancelled by the watch before a pod claimed them; the blocks claimed
polls "day-block 1 5 1\nrefcheck 1 2 0\ntape 0 0 0\n" "day-block 0 4 1\nrefcheck 0 0 0\ntape 0 0 0\n"
run 20 || no "re-runs cancelled meanwhile do not stop the release"
grep -q "RECORD claimed by the new build: day-block; nothing claimable left before a claim (not verified): refcheck" "$T/out" || no "says which kind was not verified"
ok "a kind cancelled before its claim is said, not waited for"

# 2. still claimable and no pod claimed it within the minutes: stopped as before
polls "day-block 1 5 1\nrefcheck 1 2 0\n"
! run 0 || no "a kind still claimable with no claim stops the release"
grep -q "FAIL no pod of 93a04676+abc claimed work of: refcheck" "$T/out" || no "says which kind"
ok "a kind still claimable with no claim stops the release"

# 3. held by KEDA's count for a while (the budget pressed, the ramp) but still claimable: waited for, then claimed
polls "ordered-pass 1 1 0\nday-block 1 3 0\n" "ordered-pass 0 1 0\nday-block 0 3 1\n" "ordered-pass 0 0 1\nday-block 0 3 1\n"
run 20 || no "claimed after a wait"
grep -q "RECORD claimed by the new build: ordered-pass day-block$" "$T/out" || no "both claimed"
[[ $(grep -c "still without a claim: ordered-pass" "$T/out") == 2 ]] || no "the pass was waited for while it was claimable"
ok "a kind still claimable is waited for until it is claimed"
echo "release-verify: all passed"
