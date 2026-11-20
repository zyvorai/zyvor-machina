#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Chaos smoke tests for platform controller (batch 16)
# Usage: ./scripts/e2e-chaos-platform.sh USER HOST
set -euo pipefail

USER=${1:?usage: e2e-chaos-platform.sh USER HOST}
HOST=${2:?usage: e2e-chaos-platform.sh USER HOST}
BASE="http://${HOST}:5093"
AUTH_USER="${MACHINA_E2E_USER:-admin}"
AUTH_PASS="${MACHINA_E2E_PASS:-admin}"
# Basic-auth credentials go through a curl config read via process
# substitution rather than `-u user:pass` on the command line — argv is
# readable by any local user via `ps`/proc for the life of the process; a
# config fed over a pipe never touches argv or disk.
acurl() {
  curl -K <(printf 'user = "%s:%s"\n' "$AUTH_USER" "$AUTH_PASS") "$@"
}

echo "== Chaos: controller health after rapid task listing =="
for _ in $(seq 1 20); do
  curl -sfS "${BASE}/api/v1/health" >/dev/null
  acurl -sfS "${BASE}/api/v1/tasks" >/dev/null || true
done
echo "OK: health stable under burst reads"

echo "== Chaos: migrate precheck structure =="
VM_ID=$(acurl -sfS "${BASE}/api/v1/vms" | jq -r '.[0].id // empty')
HOST_ID=$(acurl -sfS "${BASE}/api/v1/hosts" | jq -r '.[0].id // empty')
if [[ -n "$VM_ID" && -n "$HOST_ID" ]]; then
  BODY=$(acurl -sfS -H 'Content-Type: application/json' \
    -d "{\"dest_host_id\":\"$HOST_ID\",\"live\":true}" \
    "${BASE}/api/v1/vms/${VM_ID}/migrate/precheck")
  echo "$BODY" | jq -e '.checks | type == "array"' >/dev/null
  echo "OK: precheck returns checks array"
else
  echo "SKIP: no VM/host for precheck"
fi

echo "== Chaos: support bundle + policy endpoints =="
acurl -sfS "${BASE}/api/v1/support/bundle" | jq -e '.controller_id' >/dev/null
acurl -sfS "${BASE}/api/v1/policy/rules" >/dev/null
acurl -sfS "${BASE}/api/v1/upgrade/matrix" >/dev/null
echo "OK: batch 15/16 APIs"

echo "All chaos smoke checks passed."
