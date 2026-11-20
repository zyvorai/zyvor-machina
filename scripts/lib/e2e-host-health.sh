# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# shellcheck shell=bash
# Host checklist / SMART observability checks (nbd exclusion).

e2e_host_health_run() {
  local r http

  e2e_hdr "HOST HEALTH: CHECKLIST (no nbd SMART)"
  r="$(${E2E_CURL} -b "$E2E_COOKIE" "${E2E_BASE}/api/v1/health/problems" --max-time 30)"
  http="$(${E2E_CURL} -b "$E2E_COOKIE" -o /dev/null -w '%{http_code}' "${E2E_BASE}/api/v1/health/problems" --max-time 30)"
  e2e_assert_http "$http" "200" "health/problems"
  if echo "$r" | python3 -c "
import json, sys
items = json.load(sys.stdin).get('items') or []
for it in items:
    iid = str(it.get('id') or '')
    title = str(it.get('title') or '')
    if iid.startswith('smart_nbd') or 'SMART health failed on nbd' in title:
        sys.exit(1)
sys.exit(0)
" 2>/dev/null; then
    e2e_ok "no nbd SMART items in host checklist"
  else
    e2e_fail "host checklist contains nbd SMART failures"
  fi

  e2e_hdr "HOST HEALTH: LINUX OBSERVABILITY (smart excludes nbd)"
  r="$(${E2E_CURL} -b "$E2E_COOKIE" "${E2E_BASE}/api/v1/host/linux-observability" --max-time 30)"
  http="$(${E2E_CURL} -b "$E2E_COOKIE" -o /dev/null -w '%{http_code}' "${E2E_BASE}/api/v1/host/linux-observability" --max-time 30)"
  e2e_assert_http "$http" "200" "linux-observability"
  if echo "$r" | python3 -c "
import json, sys
smart = json.load(sys.stdin).get('smart') or []
for row in smart:
    dev = str(row.get('device') or '')
    if dev.startswith('nbd'):
        sys.exit(1)
sys.exit(0)
" 2>/dev/null; then
    e2e_ok "smart probe excludes nbd devices"
  else
    e2e_fail "linux-observability smart contains nbd device"
  fi
}
