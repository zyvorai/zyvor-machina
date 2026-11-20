#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# e2e-platform-install-smoke.sh — post-install health for machina platform stack
#
# Run on the KVM host (root not required for checks; systemctl/curl only):
#   ./scripts/e2e-platform-install-smoke.sh
#
set -euo pipefail

PASS=0
FAIL=0

ok()   { echo "  ✅ $*"; (( PASS++ )) || true; }
fail() { echo "  ❌ $*"; (( FAIL++ )) || true; }
hdr()  { echo ""; echo "=== $* ==="; }

check_service() {
  local svc="$1"
  if systemctl is-active --quiet "$svc" 2>/dev/null; then
    ok "systemctl is-active ${svc}"
  else
    fail "systemctl is-active ${svc} — $(systemctl is-active "$svc" 2>/dev/null || echo unknown)"
  fi
}

hdr "PLATFORM INSTALL: SYSTEMD"
check_service machina-controller
check_service machina-agent

hdr "PLATFORM INSTALL: CONTROLLER HEALTH"
r=""
for _ in $(seq 1 15); do
  r="$(curl -sf http://127.0.0.1:5093/api/v1/health 2>/dev/null || true)"
  if echo "$r" | grep -q '"database":"ok"'; then
    ok "controller health database ok"
    echo "  $r"
    break
  fi
  sleep 2
done
if ! echo "$r" | grep -q '"database":"ok"'; then
  fail "controller not healthy at http://127.0.0.1:5093/api/v1/health"
fi

hdr "PLATFORM INSTALL: AGENT PORT"
agent_port_listening() {
  if command -v ss >/dev/null 2>&1; then
    ss -ltn 2>/dev/null | grep -q ':50051'
  elif command -v netstat >/dev/null 2>&1; then
    netstat -ltn 2>/dev/null | grep -q ':50051'
  else
    return 1
  fi
}
if command -v ss >/dev/null 2>&1 || command -v netstat >/dev/null 2>&1; then
  found=0
  for _ in $(seq 1 15); do
    if agent_port_listening; then
      ok "agent gRPC port 50051 listening"
      found=1
      break
    fi
    sleep 2
  done
  if [[ "$found" -eq 0 ]]; then
    fail "agent gRPC port 50051 not listening"
  fi
else
  echo "  ⚠️  ss/netstat unavailable — skip port check"
fi

echo ""
echo "══════════════════════════════════════════"
echo "  Platform install smoke: ${PASS} passed, ${FAIL} failed"
echo "══════════════════════════════════════════"
if [[ "$FAIL" -eq 0 ]]; then
  echo "✅ Platform install smoke passed"
  exit 0
fi
echo "❌ ${FAIL} install smoke test(s) FAILED"
exit 1
