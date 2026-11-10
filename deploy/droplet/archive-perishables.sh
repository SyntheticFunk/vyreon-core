#!/usr/bin/env bash
# Nightly archive of the captured inputs Intrinio will not serve again (owner, 2026-11-10: keep them in
# Spaces while the cluster is built): the cohort prints (exq01-events, kept by the provider ~5
# sessions), the ACT01 interval pulls and the company captures (fundamentals are not served as of a
# past date). One tar.zst per category and date goes to a local directory on the volume, then is
# copied to the Spaces bucket. Nothing is ever deleted, locally or in the bucket.
#
# A date is re-archived while its files change (the capture refills the last five sessions); after
# FREEZE_DAYS its archive is final, so a later prune of the home can never shrink it.
#
# Settings: /etc/vyreon.env (VYREON_HOME) and /etc/vyreon-archive.env (see archive.env.example).
# The Spaces keys are read from files and never printed.
set -uo pipefail
set -a; . /etc/vyreon.env; . /etc/vyreon-archive.env; set +a
: "${VYREON_HOME:?}" "${VYREON_ARCHIVE_DIR:?}"
FREEZE_DAYS=${VYREON_ARCHIVE_FREEZE_DAYS:-14}
mkdir -p "$VYREON_ARCHIVE_DIR/logs"
exec >> "$VYREON_ARCHIVE_DIR/logs/$(date -u +%F).log" 2>&1
echo "== $(date -u +%FT%TZ) archive"

alert() {
  echo "ALERT: $1"
  printf 'Archive of captured inputs: %s (log %s/logs)\n' "$1" "$VYREON_ARCHIVE_DIR" \
    | /opt/vyreon/bin/vyreon-notify send --alert - || true
}

if systemctl is-active --quiet vyreon-capture.service; then
  echo "capture is running; skipped (the next timer catches up)"
  exit 0
fi

freeze_before=$(date -u -d "-$FREEZE_DAYS days" +%F)
failed=0 made=0

# archive_one CATEGORY SOURCE_DIR DATE
archive_one() {
  local cat=$1 src=$2 day=$3
  local out="$VYREON_ARCHIVE_DIR/$cat/$day.tar.zst"
  if [[ -f $out && $day < $freeze_before ]]; then return 0; fi
  local manifest
  manifest=$(cd "$src" && find . -type f -printf '%P %s %T@\n' | sort | sha256sum | cut -d' ' -f1)
  if [[ -f $out && -f $out.manifest && $(<"$out.manifest") == "$manifest" ]]; then return 0; fi
  mkdir -p "$VYREON_ARCHIVE_DIR/$cat"
  if tar -C "$(dirname "$src")" --sort=name -cf - "$(basename "$src")" | zstd -q -10 -T2 -o "$out.tmp" -f \
     && zstd -q -t "$out.tmp"; then
    mv -f "$out.tmp" "$out"
    echo "$manifest" > "$out.manifest"
    echo "archived $cat/$day ($(du -h "$out" | cut -f1))"
    made=$((made + 1))
  else
    rm -f "$out.tmp"
    echo "FAILED $cat/$day"
    failed=1
  fi
}

shopt -s nullglob
for d in "$VYREON_HOME"/h5-exq01-events/????-??-??; do archive_one exq01-events "$d" "$(basename "$d")"; done
for d in "$VYREON_HOME"/h5-act01-auto/????-??-??; do archive_one act01-auto "$d" "$(basename "$d")"; done
for d in "$VYREON_HOME"/h5-company-capture-????-??-??; do
  archive_one company-capture "$d" "${d##*h5-company-capture-}"
done
# the captures copied off the laptop before it was purged (2026-11-08): archived once, then frozen
if [[ -n ${VYREON_ARCHIVE_LAPTOP_DIR:-} ]]; then
  for d in "$VYREON_ARCHIVE_LAPTOP_DIR"/h5-exq01-events/????-??-??; do archive_one laptop-exq01-events "$d" "$(basename "$d")"; done
  for d in "$VYREON_ARCHIVE_LAPTOP_DIR"/h5-act01-auto/????-??-??; do archive_one laptop-act01-auto "$d" "$(basename "$d")"; done
fi
echo "local: $made archive(s) written"

if [[ -n ${VYREON_SPACES_BUCKET:-} && -r ${VYREON_SPACES_KEY_ID_FILE:-} && -r ${VYREON_SPACES_SECRET_FILE:-} ]]; then
  export RCLONE_CONFIG_SPACES_TYPE=s3 RCLONE_CONFIG_SPACES_PROVIDER=DigitalOcean \
         RCLONE_CONFIG_SPACES_ENDPOINT="$VYREON_SPACES_ENDPOINT" RCLONE_CONFIG_SPACES_ACL=private \
         RCLONE_CONFIG_SPACES_NO_CHECK_BUCKET=true
  RCLONE_CONFIG_SPACES_ACCESS_KEY_ID=$(tr -d '\n' < "$VYREON_SPACES_KEY_ID_FILE")
  RCLONE_CONFIG_SPACES_SECRET_ACCESS_KEY=$(tr -d '\n' < "$VYREON_SPACES_SECRET_FILE")
  export RCLONE_CONFIG_SPACES_ACCESS_KEY_ID RCLONE_CONFIG_SPACES_SECRET_ACCESS_KEY
  # copy, never sync: a local loss never deletes from the bucket
  if rclone copy "$VYREON_ARCHIVE_DIR" "spaces:$VYREON_SPACES_BUCKET/droplet-capture" \
       --include '*.tar.zst' --stats-one-line --stats-log-level NOTICE; then
    echo "bucket: copied"
  else
    failed=1
    alert "copy to Spaces failed"
  fi
else
  echo "bucket: not configured; local archive only"
fi

if (( failed )); then
  alert "at least one step failed"
  exit 1
fi
echo "== done"
