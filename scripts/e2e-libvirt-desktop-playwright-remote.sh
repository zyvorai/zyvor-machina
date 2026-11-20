#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Run Playwright live VNC spec for libvirt ubuntu-desktop from this laptop.
#
# Usage:
#   VSPASS=max ./scripts/e2e-libvirt-desktop-playwright-remote.sh sus 212.8.252.194 [VM_ID]
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"

USER="${1:?usage: $0 USER HOST [VM_ID]}"
HOST="${2:?usage: $0 USER HOST [VM_ID]}"
VM_ID="${3:-}"

PASS="${VSPASS:-${SSHPASS:-}}"
if [[ -z "$PASS" ]]; then
  read -rsp "Password for ${USER}@${HOST}: " PASS
  echo
fi

if [[ -z "$VM_ID" && -f /tmp/machina-ubuntu-desktop-e2e.env ]]; then
  # Parse the one key we need instead of `source`-ing this file: it lives at a
  # predictable, world-writable /tmp path, and sourcing it would run arbitrary
  # shell code if another local user planted/raced it there.
  _cached_line="$(grep -m1 -E '^E2E_LIBVIRT_VM_ID=' /tmp/machina-ubuntu-desktop-e2e.env 2>/dev/null || true)"
  _cached_vm_id="${_cached_line#E2E_LIBVIRT_VM_ID=}"
  VM_ID="${_cached_vm_id:-$VM_ID}"
  unset _cached_line _cached_vm_id
fi

if [[ -z "$VM_ID" ]]; then
  # `curl -u user:pass` puts the password in argv, readable via `ps` by any
  # local user for the life of the process (same concern the VSPASS comment
  # above already calls out for the SSH password) — use a netrc file instead
  # so the credential only ever touches a 0600 temp file, never argv.
  _netrc="$(mktemp)"
  chmod 600 "$_netrc"
  trap 'rm -f "$_netrc"' EXIT
  printf 'machine %s login %s password %s\n' "$HOST" "$USER" "$PASS" > "$_netrc"
  VM_ID="$(curl -sk --netrc-file "$_netrc" "http://${HOST}:5093/api/v1/vms" | python3 -c "
import json, sys
for v in json.load(sys.stdin):
    if v.get('name') == 'ubuntu-desktop':
        print(v.get('id', ''))
        break
" 2>/dev/null)"
  rm -f "$_netrc"
  trap - EXIT
fi

[[ -n "$VM_ID" ]] || { echo "❌ VM id required (ubuntu-desktop not found)" >&2; exit 1; }

cd "${REPO}/web"
if ! npm run playwright -- install chromium >/dev/null 2>&1; then
  npm run test:e2e:install
fi

export PLAYWRIGHT_LIVE_URL="https://${HOST}:5092"
export PLAYWRIGHT_LIVE_USER="${PLAYWRIGHT_LIVE_USER:-${USER}}"
export PLAYWRIGHT_LIVE_PASS="${PLAYWRIGHT_LIVE_PASS:-${PASS}}"
export PLAYWRIGHT_LIBVIRT_VM_ID="${VM_ID}"
export PLAYWRIGHT_LIBVIRT_VM_NAME="${PLAYWRIGHT_LIBVIRT_VM_NAME:-ubuntu-desktop}"

export PLAYWRIGHT_PLATFORM_USER="${PLAYWRIGHT_PLATFORM_USER:-${E2E_PLATFORM_USER:-admin}}"
export PLAYWRIGHT_PLATFORM_PASS="${PLAYWRIGHT_PLATFORM_PASS:-${E2E_PLATFORM_PASS:-}}"

npm run test:e2e -- --workers=1 --timeout=420000 \
  e2e/platform-live-libvirt-desktop.spec.ts
