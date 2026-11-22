#!/usr/bin/env bash
# One DigitalOcean API call. The token is read from VYREON_DO_TOKEN_FILE (default: do-full-token.txt
# at the repository root, git-ignored) and never printed: it goes to curl as a header file, not on
# its command line.
# usage: do.sh METHOD /v2/path [json-body]
# C-068 (audit opsurface ops.md 3, 11): only a 2xx answer is a success. Any other answer (401 for an
# expired token, 429, 5xx, a refused deletion) prints the status and the start of DigitalOcean's
# message to stderr and exits 1, so no caller reads an error's JSON as a result.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
TOKEN_FILE=${VYREON_DO_TOKEN_FILE:-$here/../../../do-full-token.txt}
body=$(mktemp); trap 'rm -f "$body"' EXIT
if ! code=$(curl -sS -o "$body" -w '%{http_code}' -X "$1" "https://api.digitalocean.com$2" \
    -H @<(printf 'Authorization: Bearer %s\n' "$(tr -d '\n' < "$TOKEN_FILE")") -H "Content-Type: application/json" \
    ${3:+-d "$3"}); then
  echo "do.sh: $1 $2: no answer from DigitalOcean" >&2
  exit 1
fi
case "$code" in
  2??) cat "$body" ;;
  *) echo "do.sh: $1 $2: HTTP $code: $(head -c 300 "$body" | tr -d '\n')" >&2; exit 1 ;;
esac
