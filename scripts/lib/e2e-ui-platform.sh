# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# shellcheck shell=bash
# Daemon-session platform proxy E2E (UI browser path via /api/v1/platform/controller).

E2E_UI_PROXY_PREFIX="/api/v1/platform/controller"

e2e_ui_platform_direct_base() {
  if [[ -n "${E2E_PLATFORM_DIRECT:-}" ]]; then
    echo "${E2E_PLATFORM_DIRECT%/}"
    return
  fi
  local host
  host="$(e2e_host_from_base)"
  echo "http://${host}:5093"
}

e2e_ui_platform_proxy_url() {
  local path="$1"
  [[ "$path" == /* ]] || path="/${path}"
  echo "${E2E_BASE}${E2E_UI_PROXY_PREFIX}${path}"
}

e2e_ui_platform_auth_curl() {
  ${E2E_CURL} -b "$E2E_COOKIE" "$@"
}

e2e_ui_platform_assert_proxy_get() {
  local path="$1" label="$2"
  local url http body tmp attempt
  url="$(e2e_ui_platform_proxy_url "$path")"
  for attempt in 1 2 3; do
    tmp="$(mktemp -t machina_e2e_proxy.XXXXXX)"
    http="$(e2e_ui_platform_auth_curl -o "$tmp" -w '%{http_code}' "$url" --max-time 30)"
    body="$(cat "$tmp" 2>/dev/null || true)"
    rm -f "$tmp"
    if [[ "$http" == "200" && -n "$body" ]]; then
      e2e_ok "${label}"
      return 0
    fi
    if [[ "$attempt" -lt 3 ]]; then
      sleep 1
    fi
  done
  if [[ "$http" != "200" ]]; then
    e2e_fail "${label} — proxy GET HTTP ${http}"
  else
    e2e_fail "${label} — empty body"
  fi
  return 1
}

e2e_ui_platform_run() {
  local r http direct_base
  direct_base="$(e2e_ui_platform_direct_base)"

  e2e_hdr "UI PLATFORM: PLATFORM-INFO"
  r="$(e2e_ui_platform_auth_curl "${E2E_BASE}/api/v1/system/platform-info")"
  http="$(e2e_ui_platform_auth_curl -o /dev/null -w '%{http_code}' "${E2E_BASE}/api/v1/system/platform-info")"
  e2e_assert_http "$http" "200" "platform-info"
  if echo "$r" | python3 -c "
import json, sys
o = json.load(sys.stdin).get('control_plane') or {}
sys.exit(0 if o.get('proxy_url') == '/api/v1/platform/controller' else 1)
" 2>/dev/null; then
    e2e_ok "control_plane.proxy_url"
  else
    e2e_fail "control_plane.proxy_url missing or wrong — got: $(echo "$r" | tr '\n' ' ' | head -c 200)"
  fi
  if echo "$r" | grep -q '"direct_url"'; then
    e2e_ok "control_plane.direct_url present"
  else
    e2e_fail "control_plane.direct_url missing"
  fi

  e2e_hdr "UI PLATFORM: PROXY HEALTH"
  r="$(e2e_ui_platform_auth_curl "$(e2e_ui_platform_proxy_url /api/v1/health)")"
  http="$(e2e_ui_platform_auth_curl -o /dev/null -w '%{http_code}' "$(e2e_ui_platform_proxy_url /api/v1/health)")"
  e2e_assert_http "$http" "200" "proxy health"
  echo "$r" | grep -q '"database":"ok"' && e2e_ok "proxy database ok" || e2e_fail "proxy database not ok"

  e2e_hdr "UI PLATFORM: PROXY READ SMOKE"
  local endpoints=(
    "/api/v1/hosts"
    "/api/v1/vms"
    "/api/v1/tasks"
    "/api/v1/placement/recommendations"
    "/api/v1/ha/status"
    "/api/v1/migrations"
    "/api/v1/fence/events"
    "/api/v1/storage/pools"
    "/api/v1/networks"
    "/api/v1/templates"
    "/api/v1/reports/capacity"
    "/api/v1/projects"
    "/api/v1/notifications"
    "/api/v1/maintenance/schedules"
    "/api/v1/events"
    "/api/v1/audit"
    "/api/v1/cluster"
    "/api/v1/cluster/settings"
    "/api/v1/enrollment/tokens"
    "/api/v1/webhooks"
    "/api/v1/api-keys"
    "/api/v1/users"
  )
  local ep
  for ep in "${endpoints[@]}"; do
    e2e_ui_platform_assert_proxy_get "$ep" "proxy GET ${ep}" || true
  done

  e2e_hdr "UI PLATFORM: PROXY PARITY (hosts + vms)"
  local tmp_p tmp_d
  tmp_p="$(mktemp -t machina_e2e_proxy.XXXXXX)"
  tmp_d="$(mktemp -t machina_e2e_direct.XXXXXX)"
  e2e_ui_platform_auth_curl "$(e2e_ui_platform_proxy_url /api/v1/hosts)" >"$tmp_p"
  e2e_platform_curl "${direct_base}/api/v1/hosts" >"$tmp_d"
  if python3 - "$tmp_p" "$tmp_d" <<'PY'
import json, sys
p = json.load(open(sys.argv[1]))
d = json.load(open(sys.argv[2]))
if not isinstance(p, list) or not isinstance(d, list):
    sys.exit(1)
if len(p) != len(d):
    sys.exit(2)
if p and d and p[0].get('id') != d[0].get('id'):
    sys.exit(3)
PY
  then
    e2e_ok "proxy hosts parity with direct controller"
  else
    e2e_fail "proxy hosts parity mismatch"
  fi

  e2e_ui_platform_auth_curl "$(e2e_ui_platform_proxy_url /api/v1/vms)" >"$tmp_p"
  e2e_platform_curl "${direct_base}/api/v1/vms" >"$tmp_d"
  if python3 - "$tmp_p" "$tmp_d" <<'PY'
import json, sys
p = json.load(open(sys.argv[1]))
d = json.load(open(sys.argv[2]))
if not isinstance(p, list) or not isinstance(d, list):
    sys.exit(1)
if len(p) != len(d):
    sys.exit(2)
PY
  then
    e2e_ok "proxy vms parity with direct controller"
  else
    e2e_fail "proxy vms parity mismatch"
  fi
  rm -f "$tmp_p" "$tmp_d"

  e2e_hdr "UI PLATFORM: UNAUTHENTICATED PROXY"
  http="$(${E2E_CURL} -o /dev/null -w '%{http_code}' "$(e2e_ui_platform_proxy_url /api/v1/health)" --max-time 15)"
  if [[ "$http" == "401" ]]; then
    e2e_ok "unauthenticated proxy returns 401"
  else
    e2e_fail "unauthenticated proxy — expected 401, got ${http}"
  fi
}
