#!/usr/bin/env bash
# One DigitalOcean API call. The token is read from VYREON_DO_TOKEN_FILE (default: do-full-token.txt
# at the repository root, git-ignored) and never printed.
# usage: do.sh METHOD /v2/path [json-body]
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
TOKEN_FILE=${VYREON_DO_TOKEN_FILE:-$here/../../../do-full-token.txt}
curl -sS -X "$1" "https://api.digitalocean.com$2" \
  -H "Authorization: Bearer $(tr -d '\n' < "$TOKEN_FILE")" -H "Content-Type: application/json" \
  ${3:+-d "$3"}
