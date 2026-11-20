#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

set -euo pipefail
: "${MACHINA_URL:?set MACHINA_URL}"
: "${MACHINA_TOKEN:?set MACHINA_TOKEN}"
: "${FAILED_HOST_ID:?set FAILED_HOST_ID}"
: "${HA_VM_ID:?set HA_VM_ID}"

api() { curl -fsS -H "Authorization: Bearer $MACHINA_TOKEN" -H 'Content-Type: application/json' "$@"; }

echo "[1/5] baseline VM and host"
api "$MACHINA_URL/api/v1/hosts/$FAILED_HOST_ID" | tee /tmp/machina-ha-host-before.json
api "$MACHINA_URL/api/v1/vms/$HA_VM_ID" | tee /tmp/machina-ha-vm-before.json

echo "[2/5] operator action required: hard power-off host $FAILED_HOST_ID now"
[[ "${MACHINA_NONINTERACTIVE:-0}" == 1 ]] || read -r -p 'Press Enter after the host is physically off... '

echo "[3/5] wait for fenced/offline state"
deadline=$((SECONDS + ${HA_TIMEOUT_SECS:-600}))
while (( SECONDS < deadline )); do
  j="$(api "$MACHINA_URL/api/v1/hosts/$FAILED_HOST_ID" || true)"
  if grep -Eq '"(state|status)"[[:space:]]*:[[:space:]]*"(offline|fenced)"' <<<"$j"; then break; fi
  sleep 5
done
(( SECONDS < deadline )) || { echo 'host never became offline/fenced' >&2; exit 1; }

echo "[4/5] wait for VM recovery on another host"
while (( SECONDS < deadline )); do
  j="$(api "$MACHINA_URL/api/v1/vms/$HA_VM_ID" || true)"
  if grep -Eq '"state"[[:space:]]*:[[:space:]]*"running"' <<<"$j" && ! grep -Fq "$FAILED_HOST_ID" <<<"$j"; then
    echo "$j" | tee /tmp/machina-ha-vm-after.json
    break
  fi
  sleep 5
done
(( SECONDS < deadline )) || { echo 'VM did not recover on another host' >&2; exit 1; }

echo "[5/5] PASS: fenced host did not recover before isolation; VM is running elsewhere"
