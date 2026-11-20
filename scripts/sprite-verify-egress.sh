#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# End-to-end network_egress verification: boots a real sprite with
# network_egress=true against a given golden image, via the real dashboard
# (Playwright, scripts/demo-videos/e2e-egress-check.mjs — reusing the daemon's
# actual auth/create flow rather than a bypass), then checks host-side
# whether the guest actually completed DHCP over the attached interface.
#
# Codifies a sequence that was otherwise hand-run over SSH during
# development. Exits non-zero if no address appears within the timeout —
# that's the one thing this feature's own automated tests
# (core/src/cloud_hypervisor/sprite.rs) can't cover, since they use a blank
# synthetic disk with no guest OS to actually run DHCP.
#
# Uses `virsh domifaddr` (libvirt's own DHCP-snooping ARP table), not the
# dnsmasq lease file directly — the lease file didn't reliably reflect a
# lease that domifaddr (and the guest's own actual connectivity) confirmed
# was real during development; domifaddr is the same source the dashboard
# itself would use if it ever surfaces a sprite's IP.
#
# Usage: MACH_URL=https://127.0.0.1:15092 scripts/sprite-verify-egress.sh USER HOST GOLDEN_IMAGE [TIMEOUT_SECONDS]
# Requires an SSH tunnel to the daemon already open at MACH_URL (see
# scripts/demo-videos/lib.mjs for MACH_URL/MACH_USER/MACH_PASS defaults).
set -euo pipefail

USER_NAME="${1:?usage: sprite-verify-egress.sh USER HOST GOLDEN_IMAGE [TIMEOUT_SECONDS]}"
HOST="${2:?usage: sprite-verify-egress.sh USER HOST GOLDEN_IMAGE [TIMEOUT_SECONDS]}"
GOLDEN_IMAGE="${3:?usage: sprite-verify-egress.sh USER HOST GOLDEN_IMAGE [TIMEOUT_SECONDS]}"
TIMEOUT_SECONDS="${4:-90}"

SSH_OPTS=(-o StrictHostKeyChecking=no -o ConnectTimeout=30)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "== sprite-verify-egress: booting a sprite (golden_image=${GOLDEN_IMAGE}, network_egress=true) on ${USER_NAME}@${HOST} =="
SPRITE_OUTPUT="$(cd "${SCRIPT_DIR}/demo-videos" && MACH_SPRITE_GOLDEN_IMAGE="${GOLDEN_IMAGE}" node e2e-egress-check.mjs)"
echo "${SPRITE_OUTPUT}"
SPRITE_ID="$(echo "${SPRITE_OUTPUT}" | sed -n 's/^SPRITE_ID=//p')"
if [[ -z "${SPRITE_ID}" ]]; then
  echo "FAIL: could not determine the created sprite's id" >&2
  exit 1
fi
DOMAIN="sprite-${SPRITE_ID}"

echo "-- polling for a DHCP-assigned address on ${DOMAIN} (up to ${TIMEOUT_SECONDS}s) --"
START="$(date +%s)"
DEADLINE=$(( START + TIMEOUT_SECONDS ))
ADDR=""
while [[ "$(date +%s)" -lt "${DEADLINE}" ]]; do
  ADDR="$(ssh "${SSH_OPTS[@]}" "${USER_NAME}@${HOST}" "sudo virsh domifaddr '${DOMAIN}' 2>/dev/null | awk '/ipv4/{print \$4}'" || true)"
  NOW="$(date +%s)"
  if [[ -n "${ADDR}" ]]; then
    echo "address at +$(( NOW - START ))s: ${ADDR}"
    break
  fi
  echo "  +$(( NOW - START ))s: no address yet"
  sleep 5
done

if [[ -n "${ADDR}" ]]; then
  echo "-- cleaning up ${DOMAIN} --"
  (cd "${SCRIPT_DIR}/demo-videos" && node cleanup-sprites.mjs) || true
  echo "== sprite-verify-egress: PASS — ${DOMAIN} got ${ADDR} via DHCP =="
  exit 0
else
  echo "== sprite-verify-egress: FAIL — no DHCP address for ${DOMAIN} within ${TIMEOUT_SECONDS}s ==" >&2
  echo "(the TAP/bridge attachment itself may still be fine — this only proves the guest OS never completed DHCP; check whether ${GOLDEN_IMAGE} is configured to DHCP its interface)" >&2
  echo "${DOMAIN} was left running (not cleaned up) so you can inspect it — e.g. tcpdump -i virbr0, virsh domiflist, or delete it manually via the Sprites page." >&2
  exit 1
fi
