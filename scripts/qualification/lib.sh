#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0
#
# Shared helpers for the live qualification scripts (source it, do not run it).
#
# Exit codes used by the scripts: 0 pass, 1 fail, 2 missing configuration or tool,
# 77 skipped (the environment does not have what the check needs, for example no vGPU host).
# Evidence (every API answer the script saves) goes to $QUAL_EVIDENCE_DIR, default a fresh
# directory under $TMPDIR; its path is printed at the start and the end. Archive it with the claim.

set -euo pipefail

q_need() {
  local v
  for v in "$@"; do
    [[ -n "${!v:-}" ]] || { echo "set $v" >&2; exit 2; }
  done
}

q_tool() {
  local t
  for t in "$@"; do
    command -v "$t" >/dev/null 2>&1 || { echo "$t is required" >&2; exit 2; }
  done
}

# The operator must name the thing the script is about to do to real infrastructure.
q_confirm() { # VAR EXPECTED
  [[ "${!1:-}" == "$2" ]] || { echo "this script changes real infrastructure: set $1=$2 to proceed" >&2; exit 2; }
}

q_tool curl python3

QUAL_EVIDENCE_DIR="${QUAL_EVIDENCE_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/machina-qual-XXXXXX")}"
mkdir -p "$QUAL_EVIDENCE_DIR"

q_begin() { echo "evidence: $QUAL_EVIDENCE_DIR"; }
q_pass() { echo "PASS: $*"; echo "evidence: $QUAL_EVIDENCE_DIR"; exit 0; }
q_fail() { echo "FAIL: $*" >&2; echo "evidence: $QUAL_EVIDENCE_DIR" >&2; exit 1; }
q_skip() { echo "SKIP: $*"; exit 77; }
q_step() { printf '[%s] %s\n' "$1" "$2"; }

# Authenticated JSON call; fails on an HTTP error status.
api() {
  curl -fsS --max-time "${QUAL_HTTP_TIMEOUT:-60}" \
    -H "Authorization: Bearer ${MACHINA_TOKEN:?}" -H 'Content-Type: application/json' "$@"
}

# HTTP status code of a call, without failing on 4xx / 5xx.
api_code() {
  curl -sS -o /dev/null -w '%{http_code}' --max-time "${QUAL_HTTP_TIMEOUT:-60}" \
    -H "Authorization: Bearer ${MACHINA_TOKEN:?}" -H 'Content-Type: application/json' "$@"
}

# Save stdin to the evidence directory and pass it through.
q_save() { tee "$QUAL_EVIDENCE_DIR/$1"; }

# Read a value from JSON on stdin by a dotted path (list indexes are numbers; `[]` counts a list).
# Prints nothing when absent; booleans print as true / false.
jget() {
  python3 -I -c '
import json, sys
path = sys.argv[1]
try:
    cur = json.load(sys.stdin)
except Exception:
    sys.exit(0)
for part in path.split("."):
    if part == "":
        continue
    if part == "[]":
        cur = len(cur) if isinstance(cur, list) else ""
        break
    if isinstance(cur, list):
        try:
            cur = cur[int(part)]
        except Exception:
            cur = None
    elif isinstance(cur, dict):
        cur = cur.get(part)
    else:
        cur = None
    if cur is None:
        break
if cur is None:
    sys.exit(0)
print(json.dumps(cur) if isinstance(cur, (dict, list)) else ("true" if cur is True else "false" if cur is False else cur))
' "$1"
}

# Build a JSON object from key=value pairs. A value starting with `@` is parsed as JSON
# (numbers, booleans, lists); anything else is a string.
jobj() {
  python3 -I -c '
import json, sys
out = {}
for a in sys.argv[1:]:
    k, _, v = a.partition("=")
    out[k] = json.loads(v[1:]) if v.startswith("@") else v
print(json.dumps(out))
' "$@"
}

# wait_until TIMEOUT_SECS INTERVAL_SECS DESCRIPTION COMMAND...
# Runs COMMAND until it succeeds; fails the script when the time runs out.
wait_until() {
  local timeout="$1" interval="$2" what="$3"
  shift 3
  # QUAL_POLL_SECS shortens every poll (for testing these scripts against a mock, not for real runs).
  interval="${QUAL_POLL_SECS:-$interval}"
  [[ -z "${QUAL_TIMEOUT_CAP:-}" ]] || (( timeout <= QUAL_TIMEOUT_CAP )) || timeout="$QUAL_TIMEOUT_CAP"
  local deadline=$((SECONDS + timeout))
  until "$@"; do
    (( SECONDS < deadline )) || q_fail "timed out after ${timeout}s waiting for: $what"
    sleep "$interval"
  done
}
