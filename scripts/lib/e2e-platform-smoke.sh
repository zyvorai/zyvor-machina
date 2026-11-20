# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# shellcheck shell=bash
# Read-only platform controller API smoke (direct :5093).

e2e_platform_smoke_get() {
  local path="$1" label="$2"
  local http r attempt
  # Throttle burst smoke traffic so shared hosts stay under per-user rate limits.
  if [[ -n "${E2E_PLATFORM_SMOKE_DELAY_MS:-}" ]]; then
    sleep "$(awk "BEGIN { printf \"%.3f\", ${E2E_PLATFORM_SMOKE_DELAY_MS} / 1000 }")"
  elif [[ -n "${MACHINA_E2E_BYPASS_SECRET:-}" ]]; then
    :
  else
    sleep 0.05
  fi
  http="$(e2e_platform_http_code_retry "${E2E_PLATFORM_BASE}${path}" 3 5)"
  if [[ "$http" != "200" ]]; then
    e2e_platform_fail "${label} — HTTP ${http}"
    return 1
  fi
  r=""
  for attempt in 1 2 3; do
    r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}${path}")"
    if [[ -n "$r" ]]; then
      break
    fi
    sleep 2
  done
  if [[ -z "$r" ]]; then
    e2e_platform_fail "${label} — empty body"
    return 1
  fi
  e2e_platform_ok "${label}"
  return 0
}

e2e_platform_smoke_run() {
  e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/templates/seed" >/dev/null 2>&1 || true

  local endpoints=(
    "/api/v1/health"
    "/api/v1/health/ready"
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
    "/api/v1/templates/marketplace"
    "/api/v1/reports/capacity"
    "/api/v1/projects"
    "/api/v1/notifications"
    "/api/v1/maintenance/schedules"
    "/api/v1/events"
    "/api/v1/audit"
    "/api/v1/cluster"
    "/api/v1/cluster/settings"
    "/api/v1/cluster/leadership"
    "/api/v1/enrollment/tokens"
    "/api/v1/webhooks"
    "/api/v1/api-keys"
    "/api/v1/users"
    "/api/v1/auth/oidc"
    "/api/v1/openapi.json"
    "/api/v1/policy/rules"
    "/api/v1/policy/quotas"
    "/api/v1/support/bundle"
    "/api/v1/upgrade/matrix"
    "/api/v1/backup-targets"
    "/api/v1/content/images"
    "/api/v1/recommendations"
    "/api/v1/applications"
    "/api/v1/topology"
    "/api/v1/blueprints"
    "/api/v1/reports/finops"
    "/api/v1/ai/settings"
    "/api/v1/ai/cost"
    "/api/v1/ai/capacity"
    "/api/v1/ai/security"
    "/api/v1/ai/compliance"
    "/api/v1/backups/timeline"
  )
  local ep host_id r http body

  e2e_platform_hdr "PLATFORM SMOKE: READ-ONLY GETs"
  for ep in "${endpoints[@]}"; do
    e2e_platform_smoke_get "$ep" "GET ${ep}" || true
  done

  e2e_platform_hdr "PLATFORM SMOKE: PROMETHEUS METRICS"
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/metrics/prometheus")"
  body="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/metrics/prometheus")"
  if [[ "$http" == "200" && -n "$body" && "$body" == *machina_platform* ]]; then
    e2e_platform_ok "GET /api/v1/metrics/prometheus"
  else
    e2e_platform_fail "GET /api/v1/metrics/prometheus — HTTP ${http}"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: PLACEMENT REFRESH"
  r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/placement/refresh")"
  echo "$r" | grep -q '\[' && e2e_platform_ok "POST /api/v1/placement/refresh" \
    || e2e_platform_fail "POST /api/v1/placement/refresh — got: $r"

  e2e_platform_hdr "PLATFORM SMOKE: VM METRICS & DISCOVERED FILTER"
  r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/vms")"
  vm_id="$(echo "$r" | python3 -c "
import json, sys
try:
    vms = json.load(sys.stdin)
    print(vms[0]['id'] if vms else '')
except Exception:
    print('')
" 2>/dev/null)"
  if [[ -n "$vm_id" ]]; then
    http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/metrics")"
    if [[ "$http" == "200" || "$http" == "404" ]]; then
      e2e_platform_ok "GET /api/v1/vms/{id}/metrics (HTTP ${http})"
    else
      e2e_platform_fail "GET /api/v1/vms/{id}/metrics — HTTP ${http}"
    fi
    http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/health-check")"
    if [[ "$http" == "200" || "$http" == "400" ]]; then
      e2e_platform_ok "POST /api/v1/vms/{id}/health-check (HTTP ${http})"
    else
      e2e_platform_fail "POST /api/v1/vms/{id}/health-check — HTTP ${http}"
    fi
    http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/guest/health")"
    [[ "$http" == "200" ]] && e2e_platform_ok "GET /api/v1/vms/{id}/guest/health" \
      || e2e_platform_warn "GET guest/health — HTTP ${http}"
    for action in pause resume shutdown; do
      http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/${action}")"
      [[ "$http" == "200" || "$http" == "409" ]] && e2e_platform_ok "POST /api/v1/vms/{id}/${action} (HTTP ${http})" \
        || e2e_platform_warn "POST ${action} — HTTP ${http}"
    done
    http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/domain-xml")"
    [[ "$http" == "200" || "$http" == "404" || "$http" == "502" ]] && e2e_platform_ok "GET /api/v1/vms/{id}/domain-xml (HTTP ${http})" \
      || e2e_platform_warn "GET domain-xml — HTTP ${http}"
    http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/console")"
    [[ "$http" == "200" || "$http" == "404" || "$http" == "502" ]] && e2e_platform_ok "GET /api/v1/vms/{id}/console (HTTP ${http})" \
      || e2e_platform_warn "GET console — HTTP ${http}"
    http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/zeus-firewall/vms/${vm_id}/guest-ports")"
    [[ "$http" == "200" || "$http" == "404" || "$http" == "502" ]] && e2e_platform_ok "GET guest-ports (HTTP ${http})" \
      || e2e_platform_warn "GET guest-ports — HTTP ${http}"
  else
    e2e_platform_warn "no VMs — skip vm metrics"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/vms?managed=false")"
  [[ "$http" == "200" ]] && e2e_platform_ok "GET /api/v1/vms?managed=false" \
    || e2e_platform_fail "GET /api/v1/vms?managed=false — HTTP ${http}"

  e2e_platform_hdr "PLATFORM SMOKE: EVENTS SSE"
  if curl -sfS -N --max-time 3 -K <(printf 'header = "%s"\n' "$(e2e_platform_auth_header)") "${E2E_PLATFORM_BASE}/api/v1/events/stream" | head -c 1 >/dev/null 2>&1; then
    e2e_platform_ok "GET /api/v1/events/stream (SSE reachable)"
  else
    e2e_platform_warn "SSE stream not verified (may need auth or timeout)"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: HOST DETAIL"
  r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/hosts")"
  host_id="$(echo "$r" | python3 -c "
import json, sys
try:
    hosts = json.load(sys.stdin)
    print(hosts[0]['id'] if hosts else '')
except Exception:
    print('')
" 2>/dev/null)"
  if [[ -n "$host_id" ]]; then
    e2e_platform_smoke_get "/api/v1/hosts/${host_id}/detail" "GET host detail" || true
  else
    e2e_platform_warn "no hosts — skip host detail"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: ISO APPROVAL WORKFLOW"
  iso_name="e2e-iso-$(date +%s)"
  body="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/content/images" \
    -H 'Content-Type: application/json' \
    -d "{\"name\":\"${iso_name}\",\"path\":\"/tmp/${iso_name}\",\"kind\":\"iso\"}")"
  iso_id="$(echo "$body" | python3 -c "
import json, sys
try:
    print(json.load(sys.stdin).get('id',''))
except Exception:
    print('')
" 2>/dev/null)"
  if [[ -n "$iso_id" ]]; then
    echo "$body" | grep -q '"status":"pending"' && e2e_platform_ok "POST /api/v1/content/images → pending" \
      || e2e_platform_fail "POST /api/v1/content/images — expected pending status"
    http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/content/images/${iso_id}/approve")"
    if [[ "$http" == "200" ]]; then
      e2e_platform_ok "POST /api/v1/content/images/{id}/approve (HTTP ${http})"
    else
      e2e_platform_fail "POST /api/v1/content/images/{id}/approve — HTTP ${http}"
    fi
  else
    e2e_platform_warn "content create failed — skip ISO approval"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: GUEST TOOLS & BLUEPRINTS"
  if [[ -n "$vm_id" ]]; then
    http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/guest-tools/install")"
    if [[ "$http" == "200" || "$http" == "400" ]]; then
      e2e_platform_ok "POST /api/v1/vms/{id}/guest-tools/install (HTTP ${http})"
    else
      e2e_platform_fail "POST /api/v1/vms/{id}/guest-tools/install — HTTP ${http}"
    fi
  fi
  bp_body="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/blueprints" \
    -H 'Content-Type: application/json' \
    -d '{"name":"e2e-bp-'"$(date +%s)"'","actions":["backup"],"vm_ids":[]}')"
  bp_id="$(echo "$bp_body" | python3 -c "import json,sys; print(json.load(sys.stdin).get('id',''))" 2>/dev/null || true)"
  if [[ -n "$bp_id" ]]; then
    e2e_platform_ok "POST /api/v1/blueprints"
  else
    e2e_platform_warn "blueprint create skipped"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: TEMPLATE SEED"
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/templates/seed" -H 'Content-Type: application/json' -d '{}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/templates/seed (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/templates/seed — HTTP ${http}"
  fi
  r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/templates/marketplace")"
  echo "$r" | grep -q 'ubuntu-24.04' && e2e_platform_ok "marketplace has default templates" \
    || e2e_platform_warn "marketplace templates empty"

  e2e_platform_hdr "PLATFORM SMOKE: NETWORK DISCOVER"
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/networks/discover" -H 'Content-Type: application/json' -d '{}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/networks/discover (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/networks/discover — HTTP ${http}"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: STORAGE DISCOVER"
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/storage/pools/discover" -H 'Content-Type: application/json' -d '{}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/storage/pools/discover (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/storage/pools/discover — HTTP ${http}"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: TEMPLATE READINESS"
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' "${E2E_PLATFORM_BASE}/api/v1/templates/ubuntu-24.04/1.0.0/readiness")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/templates/ubuntu-24.04/1.0.0/readiness (HTTP ${http})"
  else
    e2e_platform_fail "GET template readiness — HTTP ${http}"
  fi
  tpl_count="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/templates/marketplace" | python3 -c "import json,sys; print(len(json.load(sys.stdin)))" 2>/dev/null || echo 0)"
  if [[ "$tpl_count" -ge 16 ]]; then
    e2e_platform_ok "marketplace has ${tpl_count} templates (expected 16+)"
  else
    e2e_platform_warn "marketplace has ${tpl_count} templates (expected 16)"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: APPLICATION GROUPS"
  app_id=""
  app_body=""
  for _attempt in 1 2 3; do
    app_body="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/applications" \
      -H 'Content-Type: application/json' \
      -d '{"name":"Finance Application '"$(date +%s)"'","description":"E2E application group","vm_ids":[]}')"
    app_id="$(echo "$app_body" | python3 -c "import json,sys; print(json.load(sys.stdin).get('id',''))" 2>/dev/null || true)"
    if [[ -n "$app_id" ]]; then
      break
    fi
    sleep 3
  done
  if [[ -n "$app_id" ]]; then
    e2e_platform_ok "POST /api/v1/applications (friendly name with spaces)"
  else
    e2e_platform_fail "POST /api/v1/applications — got: $app_body"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: MACHINA AI"
  e2e_platform_smoke_get "/api/v1/ai/settings" "GET /api/v1/ai/settings" || true
  e2e_platform_smoke_get "/api/v1/ai/cost" "GET /api/v1/ai/cost" || true
  e2e_platform_smoke_get "/api/v1/ai/capacity" "GET /api/v1/ai/capacity" || true
  e2e_platform_smoke_get "/api/v1/ai/security" "GET /api/v1/ai/security" || true
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/spotlight" \
    -H 'Content-Type: application/json' -d '{"query":"show offline hosts"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/spotlight (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/ai/spotlight — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/migrations/advisor?provider=vmware&vm=e2e-test")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/migrations/advisor (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/migrations/advisor — HTTP ${http}"
  fi
  if [[ -n "$vm_id" ]]; then
    http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/doctor")"
    if [[ "$http" == "200" ]]; then
      e2e_platform_ok "GET /api/v1/vms/{id}/doctor (HTTP ${http})"
    else
      e2e_platform_fail "GET /api/v1/vms/{id}/doctor — HTTP ${http}"
    fi
  else
    e2e_platform_warn "no VMs — skip doctor endpoint"
  fi
  e2e_platform_hdr "PLATFORM SMOKE: MACHINA AI V3"
  e2e_platform_hdr "PLATFORM SMOKE: MACHINA AI V4"
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/terminal/suggest" \
    -H 'Content-Type: application/json' -d '{"vm_name":"test"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/terminal/suggest (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/ai/terminal/suggest — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/compliance/export")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/compliance/export (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/compliance/export — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/autopilot/propose")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/autopilot/propose (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/autopilot/propose — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/compliance")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/compliance (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/compliance — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/policy/export")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/policy/export (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/policy/export — HTTP ${http}"
  fi
  e2e_platform_hdr "PLATFORM SMOKE: MACHINA AI V5"
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/copilot/stream" \
    -H 'Content-Type: application/json' -d '{"message":"capacity"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/copilot/stream (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/ai/copilot/stream — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/compliance/export.pdf")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/compliance/export.pdf (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/compliance/export.pdf — HTTP ${http}"
  fi
  e2e_platform_hdr "PLATFORM SMOKE: MACHINA AI V6"
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/autopilot/history")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/autopilot/history (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/autopilot/history — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/cost/export.csv")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/cost/export.csv (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/cost/export.csv — HTTP ${http}"
  fi
  e2e_platform_hdr "PLATFORM SMOKE: MACHINA AI V7"
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/spotlight" \
    -H 'Content-Type: application/json' -d '{"query":"Create Windows VM web-01 with 8 vcpu and 32gb"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/spotlight NL create (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/ai/spotlight NL create — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/capacity/export.csv")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/capacity/export.csv (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/capacity/export.csv — HTTP ${http}"
  fi
  e2e_platform_hdr "PLATFORM SMOKE: MACHINA ZEUS OS (AI-88–95)"
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/twin/graph")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/twin/graph (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/twin/graph — HTTP ${http}"
  fi
  local host_id
  host_id="$(e2e_platform_first_host_id)"
  if [[ -z "$host_id" ]]; then
    e2e_platform_fail "POST /api/v1/ai/twin/impact — no hosts registered"
  else
    http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/twin/impact" \
      -H 'Content-Type: application/json' -d "{\"action\":\"shutdown\",\"target_kind\":\"host\",\"target_id\":\"${host_id}\"}")"
    if [[ "$http" == "200" ]]; then
      e2e_platform_ok "POST /api/v1/ai/twin/impact (HTTP ${http})"
    else
      e2e_platform_fail "POST /api/v1/ai/twin/impact — HTTP ${http}"
    fi
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/incidents/analyze?hours=4")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/incidents/analyze (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/incidents/analyze — HTTP ${http}"
  fi
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/intent/environment" \
    -H 'Content-Type: application/json' -d '{"query":"medium staging environment for 20 developers"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/intent/environment (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/ai/intent/environment — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/sre/forecast")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/sre/forecast (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/sre/forecast — HTTP ${http}"
  fi
  e2e_platform_hdr "PLATFORM SMOKE: MACHINA ZEUS OS (AI-96–105)"
  http="$(e2e_platform_http_code_retry "${E2E_PLATFORM_BASE}/api/v1/ai/fleet/heatmap")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/fleet/heatmap (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/fleet/heatmap — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/fleet/rebalance/propose")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/fleet/rebalance/propose (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/fleet/rebalance/propose — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/security/graph")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/security/graph (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/security/graph — HTTP ${http}"
  fi
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/security/attack-path" \
    -H 'Content-Type: application/json' -d '{"source":"admin","target_vm":"db-01"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/security/attack-path (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/ai/security/attack-path — HTTP ${http}"
  fi
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/knowledge/search" \
    -H 'Content-Type: application/json' -d '{"query":"billing"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/knowledge/search (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/ai/knowledge/search — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/services/graph")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/services/graph (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/services/graph — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/memory/incidents?days=7")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/memory/incidents (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/memory/incidents — HTTP ${http}"
  fi
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/mission/stack" \
    -H 'Content-Type: application/json' -d '{"query":"GPU cluster for Llama serving"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/mission/stack (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/ai/mission/stack — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/baremetal/servers")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/baremetal/servers (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/baremetal/servers — HTTP ${http}"
  fi
  e2e_platform_hdr "PLATFORM SMOKE: MACHINA ZEUS OS (AI-106–113)"
  host_id="${host_id:-$(e2e_platform_first_host_id)}"
  if [[ -z "$host_id" ]]; then
    e2e_platform_fail "POST /api/v1/ai/twin/impact migrate — no hosts registered"
  else
    http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/twin/impact" \
      -H 'Content-Type: application/json' -d "{\"action\":\"migrate\",\"target_kind\":\"host\",\"target_id\":\"${host_id}\"}")"
    if [[ "$http" == "200" ]]; then
      e2e_platform_ok "POST /api/v1/ai/twin/impact migrate (HTTP ${http})"
    else
      e2e_platform_fail "POST /api/v1/ai/twin/impact migrate — HTTP ${http}"
    fi
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/cost/attribution")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/cost/attribution (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/cost/attribution — HTTP ${http}"
  fi
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/fleet/rebalance/execute" \
    -H 'Content-Type: application/json' -d '{"dry_run":true}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/fleet/rebalance/execute (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/ai/fleet/rebalance/execute — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code_retry "${E2E_PLATFORM_BASE}/api/v1/ai/compliance/frameworks" 3 5)"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/compliance/frameworks (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/compliance/frameworks — HTTP ${http}"
  fi
  e2e_platform_hdr "PLATFORM SMOKE: MACHINA ZEUS OS (AI-114–121)"
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/mission/stack/execute" \
    -H 'Content-Type: application/json' -d '{"query":"GPU cluster for Llama","dry_run":true}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/mission/stack/execute (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/ai/mission/stack/execute — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/cost/attribution/export.csv")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/cost/attribution/export.csv (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/cost/attribution/export.csv — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/fleet/gpu-placement?workload=inference")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/fleet/gpu-placement (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/fleet/gpu-placement — HTTP ${http}"
  fi
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/knowledge/diagnose" \
    -H 'Content-Type: application/json' -d '{"query":"why is billing slow"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/knowledge/diagnose (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/ai/knowledge/diagnose — HTTP ${http}"
  fi
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/services/impact" \
    -H 'Content-Type: application/json' -d '{"service":"payments"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/services/impact (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/ai/services/impact — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/memory/similar?q=migrate&limit=5")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/memory/similar (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/memory/similar — HTTP ${http}"
  fi
  e2e_platform_hdr "PLATFORM SMOKE: MACHINA ZEUS OS (AI-122–129)"
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/intent/environment/execute" \
    -H 'Content-Type: application/json' -d '{"query":"staging for 10 developers","dry_run":true}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/intent/environment/execute (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/ai/intent/environment/execute — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/sre/remediate")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/sre/remediate (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/sre/remediate — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/compliance/remediate")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/compliance/remediate (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/compliance/remediate — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/zeus/summary")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/zeus/summary (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/zeus/summary — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/fleet/power/optimize")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/fleet/power/optimize (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/fleet/power/optimize — HTTP ${http}"
  fi
  e2e_platform_hdr "PLATFORM SMOKE: MACHINA ZEUS OS PHASE 14 (AI-130–137)"
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/remediate/hub")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/remediate/hub (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/remediate/hub — HTTP ${http}"
  fi
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/knowledge/runbook" \
    -H 'Content-Type: application/json' -d '{"query":"why is billing slow"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/knowledge/runbook (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/ai/knowledge/runbook — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/cost/budget")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/cost/budget (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/cost/budget — HTTP ${http}"
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/ai/mission/stack/status")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/ai/mission/stack/status (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/ai/mission/stack/status — HTTP ${http}"
  fi
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/twin/impact" \
    -H 'Content-Type: application/json' -d '{"action":"drain","target_kind":"storage","target_id":"default"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/twin/impact storage drain (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/ai/twin/impact storage drain — HTTP ${http}"
  fi
  e2e_platform_hdr "PLATFORM SMOKE: GUESTKIT (AI-138–141)"
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/guestkit/status")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/guestkit/status (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/guestkit/status — HTTP ${http}"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: SOC (unified security operations)"
  e2e_platform_smoke_get "/api/v1/soc/overview" "GET /api/v1/soc/overview" || true
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/soc/ingest/run")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/soc/ingest/run (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/soc/ingest/run — HTTP ${http}"
  fi
  ingest_host="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/zeus-security/fleet/sensors" | python3 -c "import json,sys; d=json.load(sys.stdin); m=d.get('matrix') or []; print(m[0]['host_id'] if m else 'test-host')" 2>/dev/null || echo test-host)"
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/zeus-security/hosts/${ingest_host}/fabric-status")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/zeus-security/hosts/{host}/fabric-status (native eBPF, HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/zeus-security/hosts/{host}/fabric-status — HTTP ${http}"
  fi

  e2e_platform_smoke_get "/api/v1/soc/events?limit=5" "GET /api/v1/soc/events" || true
  e2e_platform_smoke_get "/api/v1/soc/alerts?limit=5" "GET /api/v1/soc/alerts" || true
  e2e_platform_smoke_get "/api/v1/soc/rules" "GET /api/v1/soc/rules" || true
  e2e_platform_smoke_get "/api/v1/soc/asm/summary" "GET /api/v1/soc/asm/summary" || true
  e2e_platform_smoke_get "/api/v1/soc/playbooks" "GET /api/v1/soc/playbooks" || true
  e2e_platform_smoke_get "/api/v1/soc/playbook-runs?limit=10" "GET /api/v1/soc/playbook-runs" || true
  e2e_platform_smoke_get "/api/v1/soc/settings" "GET /api/v1/soc/settings" || true

  e2e_platform_hdr "PLATFORM SMOKE: ZEUS FIREWALL (AI-142)"
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/zeus-firewall/status")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/zeus-firewall/status (HTTP ${http})"
  else
    e2e_platform_fail "GET /api/v1/zeus-firewall/status — HTTP ${http}"
  fi
  e2e_platform_smoke_get "/api/v1/zeus-firewall/overview" "GET /api/v1/zeus-firewall/overview" || true
  e2e_platform_smoke_get "/api/v1/zeus-firewall/profiles" "GET /api/v1/zeus-firewall/profiles" || true
  e2e_platform_smoke_get "/api/v1/zeus-firewall/siem/export?hours=24" "GET /api/v1/zeus-firewall/siem/export" || true
  e2e_platform_smoke_get "/api/v1/zeus-firewall/policies/gitops/export" "GET /api/v1/zeus-firewall/policies/gitops/export" || true
  e2e_platform_smoke_get "/api/v1/zeus-firewall/approvals?status=pending" "GET /api/v1/zeus-firewall/approvals" || true

  e2e_platform_hdr "PLATFORM SMOKE: ZEUS FIREWALL PHASES 16-25 (AI-172–371)"
  e2e_platform_smoke_get "/api/v1/zeus-firewall/k8s/status" "GET /api/v1/zeus-firewall/k8s/status" || true
  e2e_platform_smoke_get "/api/v1/zeus-firewall/cloud/overview" "GET /api/v1/zeus-firewall/cloud/overview" || true
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/zeus-firewall/connectivity" \
    -H 'Content-Type: application/json' -d '{"target_id":"local","profile":"ProductionServer"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/zeus-firewall/connectivity (HTTP ${http})"
  else
    e2e_platform_fail "POST /api/v1/zeus-firewall/connectivity — HTTP ${http}"
  fi
  e2e_platform_smoke_get "/api/v1/zeus-firewall/anomalies" "GET /api/v1/zeus-firewall/anomalies" || true

  e2e_platform_hdr "PLATFORM SMOKE: ZEUS SECURITY + NATIVE eBPF FABRIC"
  e2e_platform_smoke_get "/api/v1/zeus-security/status" "GET /api/v1/zeus-security/status" || true
  e2e_platform_smoke_get "/api/v1/zeus-security/fabric/health" "GET /api/v1/zeus-security/fabric/health" || true
  e2e_platform_smoke_get "/api/v1/zeus-security/fleet/threat" "GET /api/v1/zeus-security/fleet/threat" || true
  e2e_platform_smoke_get "/api/v1/zeus-security/fleet/sensors" "GET /api/v1/zeus-security/fleet/sensors" || true
  e2e_platform_smoke_get "/api/v1/zeus-security/fleet/timeline?hours=24" "GET /api/v1/zeus-security/fleet/timeline" || true
  e2e_platform_smoke_get "/api/v1/zeus-security/enforcement/status" "GET /api/v1/zeus-security/enforcement/status" || true
  enf="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/zeus-security/enforcement/policies")"
  if echo "$enf" | grep -q '"policies"'; then
    e2e_platform_ok "GET /api/v1/zeus-security/enforcement/policies (normalized)"
  else
    e2e_platform_fail "GET /api/v1/zeus-security/enforcement/policies — missing policies array"
  fi
  canvas="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/network-canvas")"
  if echo "$canvas" | grep -q '"native_bpf"' && echo "$canvas" | grep -q '"network_pulse"'; then
    e2e_platform_ok "GET /api/v1/network-canvas (native eBPF pulse bundle)"
  else
    e2e_platform_fail "GET /api/v1/network-canvas — missing native_bpf/network_pulse"
  fi
  if echo "$canvas" | grep -q '"service_map"'; then
    e2e_platform_ok "GET /api/v1/network-canvas includes service_map"
  else
    e2e_platform_fail "GET /api/v1/network-canvas — missing service_map"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: ZEUS FIREWALL MAC UX (AI-372–391)"
  e2e_platform_smoke_get "/api/v1/cluster/settings" "GET /api/v1/cluster/settings (Settings hub)" || true
  e2e_platform_smoke_get "/api/v1/ai/zeus/summary" "GET /api/v1/ai/zeus/summary (Control Center strip)" || true
  host_id="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/hosts" | python3 -c "
import json, sys
try:
    hosts = json.load(sys.stdin)
    print(hosts[0]['id'] if hosts else '')
except Exception:
    print('')
" 2>/dev/null)"
  if [[ -n "$host_id" ]]; then
    http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/zeus-firewall/targets/${host_id}/plan" \
      -H 'Content-Type: application/json' -d '{"profile":"ProductionServer","dry_run":true}')"
    if [[ "$http" == "200" ]]; then
      e2e_platform_ok "POST /api/v1/zeus-firewall/targets/{id}/plan dry-run (HTTP ${http})"
    else
      e2e_platform_fail "POST /api/v1/zeus-firewall/targets/{id}/plan dry-run — HTTP ${http}"
    fi
  else
    e2e_platform_ok "POST firewall plan dry-run — skipped (no hosts)"
  fi
  vm_id="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/vms" | python3 -c "
import json, sys
try:
    vms = json.load(sys.stdin)
    print(vms[0]['id'] if vms else '')
except Exception:
    print('')
" 2>/dev/null)"
  if [[ -n "$vm_id" ]]; then
    http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/zeus-firewall/vms/${vm_id}/guest-ports")"
    if [[ "$http" == "200" || "$http" == "404" || "$http" == "502" || "$http" == "503" ]]; then
      e2e_platform_ok "GET /api/v1/zeus-firewall/vms/{id}/guest-ports (HTTP ${http}, soft)"
    else
      e2e_platform_fail "GET /api/v1/zeus-firewall/vms/{id}/guest-ports — HTTP ${http}"
    fi
  else
    e2e_platform_ok "GET guest-ports — skipped (no VMs)"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: PLATFORM MAC UX WAVE 4 (UX-49–56)"
  for q in "firewall settings" "open ports" "block incoming"; do
    http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/spotlight" \
      -H 'Content-Type: application/json' -d "{\"query\":\"${q}\"}")"
    if [[ "$http" == "200" ]]; then
      e2e_platform_ok "POST /api/v1/ai/spotlight \"${q}\" (HTTP ${http})"
    else
      e2e_platform_fail "POST /api/v1/ai/spotlight \"${q}\" — HTTP ${http}"
    fi
  done

  e2e_platform_hdr "PLATFORM SMOKE: ZEUS FIREWALL PHASE 23 (AI-312–331)"
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/baremetal/servers" \
    -H 'Content-Type: application/json' -d '{"hostname":"e2e-metal-01","bmc_address":"10.0.0.99","bmc_vlan":"vlan-bmc","firewall_profile":"BareMetalBmc"}')"
  metal_id=""
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/baremetal/servers (HTTP ${http})"
    metal_id="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/baremetal/servers" | python3 -c "
import json, sys
try:
    rows = json.load(sys.stdin)
    for r in rows:
        if r.get('hostname') == 'e2e-metal-01':
            print(r['id'])
            break
except Exception:
    print('')
" 2>/dev/null)"
  else
    e2e_platform_fail "POST /api/v1/baremetal/servers — HTTP ${http}"
  fi
  e2e_platform_smoke_get "/api/v1/zeus-firewall/baremetal/overview" "GET /api/v1/zeus-firewall/baremetal/overview" || true
  ov="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/zeus-firewall/overview")"
  if echo "$ov" | python3 -c "import json,sys; d=json.load(sys.stdin); sys.exit(0 if any(t.get('kind')=='bare_metal' for t in d.get('targets',[])) else 1)" 2>/dev/null; then
    e2e_platform_ok "GET /api/v1/zeus-firewall/overview includes bare_metal target"
  elif [[ -n "$metal_id" ]]; then
    e2e_platform_fail "GET /api/v1/zeus-firewall/overview — no bare_metal target"
  else
    e2e_platform_ok "GET /api/v1/zeus-firewall/overview bare_metal — skipped (no register)"
  fi
  if [[ -n "$metal_id" ]]; then
    http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/zeus-firewall/baremetal/${metal_id}/scan")"
    if [[ "$http" == "200" ]]; then
      e2e_platform_ok "POST /api/v1/zeus-firewall/baremetal/{id}/scan (HTTP ${http})"
    else
      e2e_platform_fail "POST baremetal scan — HTTP ${http}"
    fi
    http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/zeus-firewall/targets/${metal_id}/plan" \
      -H 'Content-Type: application/json' -d '{"profile":"BareMetalPxe","dry_run":true}')"
    if [[ "$http" == "200" ]]; then
      e2e_platform_ok "POST metal plan dry-run (HTTP ${http})"
    else
      e2e_platform_fail "POST metal plan dry-run — HTTP ${http}"
    fi
  fi
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/zeus-firewall/compliance/metal/export.pdf")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET compliance/metal/export.pdf (HTTP ${http})"
  else
    e2e_platform_warn "GET compliance/metal/export.pdf — soft (HTTP ${http})"
  fi
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/spotlight" \
    -H 'Content-Type: application/json' -d '{"query":"bare metal firewall"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/spotlight bare metal firewall (HTTP ${http})"
  else
    e2e_platform_fail "POST spotlight bare metal — HTTP ${http}"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: ZEUS FIREWALL PHASE 22 (AI-292–311)"
  e2e_platform_smoke_get "/api/v1/zeus-firewall/finops/exposure" "GET /api/v1/zeus-firewall/finops/exposure" || true
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/zeus-firewall/finops/exposure/export.csv")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/zeus-firewall/finops/exposure/export.csv (HTTP ${http})"
  else
    e2e_platform_fail "GET exposure export.csv — HTTP ${http}"
  fi
  e2e_platform_smoke_get "/api/v1/ai/remediate/hub" "GET /api/v1/ai/remediate/hub (FinOps waste)" || true
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/spotlight" \
    -H 'Content-Type: application/json' -d '{"query":"exposure cost finops"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/spotlight exposure cost (HTTP ${http})"
  else
    e2e_platform_fail "POST spotlight exposure cost — HTTP ${http}"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: ZEUS FIREWALL PHASE 24 (AI-332–351)"
  e2e_platform_smoke_get "/api/v1/zeus-firewall/multisite/overview" "GET /api/v1/zeus-firewall/multisite/overview" || true
  e2e_platform_smoke_get "/api/v1/zeus-firewall/multisite/export" "GET /api/v1/zeus-firewall/multisite/export" || true
  e2e_platform_smoke_get "/api/v1/zeus-firewall/multisite/drift" "GET /api/v1/zeus-firewall/multisite/drift" || true
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/zeus-firewall/multisite/sync" \
    -H 'Content-Type: application/json' -d '{"source_site":"primary-local","target_site":"dr-replica","apply_profiles":false}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/zeus-firewall/multisite/sync (HTTP ${http})"
  else
    e2e_platform_fail "POST multisite sync — HTTP ${http}"
  fi
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/spotlight" \
    -H 'Content-Type: application/json' -d '{"query":"dr firewall multisite"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/spotlight dr firewall (HTTP ${http})"
  else
    e2e_platform_fail "POST spotlight dr firewall — HTTP ${http}"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: ZEUS FIREWALL PHASE 25 (AI-352–371)"
  e2e_platform_smoke_get "/api/v1/zeus-firewall/operator/plan" "GET /api/v1/zeus-firewall/operator/plan" || true
  e2e_platform_smoke_get "/api/v1/zeus-firewall/operator/thresholds" "GET /api/v1/zeus-firewall/operator/thresholds" || true
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/zeus-firewall/operator/execute" \
    -H 'Content-Type: application/json' -d '{"host_id":"00000000-0000-0000-0000-000000000001","dry_run":true}')"
  if [[ "$http" == "200" || "$http" == "400" ]]; then
    e2e_platform_ok "POST /api/v1/zeus-firewall/operator/execute dry-run (HTTP ${http})"
  else
    e2e_platform_fail "POST operator execute dry-run — HTTP ${http}"
  fi
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/spotlight" \
    -H 'Content-Type: application/json' -d '{"query":"secure all hosts ai operator"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/spotlight ai operator (HTTP ${http})"
  else
    e2e_platform_fail "POST spotlight ai operator — HTTP ${http}"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: ZEUS/PLATFORM PHASE 26 (AI-392–411)"
  e2e_platform_smoke_get "/api/v1/network/segments/overview" "GET /api/v1/network/segments/overview" || true
  e2e_platform_smoke_get "/api/v1/network/ipam/pools" "GET /api/v1/network/ipam/pools" || true
  e2e_platform_smoke_get "/api/v1/network/segments/gitops/export" "GET /api/v1/network/segments/gitops/export" || true
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/spotlight" \
    -H 'Content-Type: application/json' -d '{"query":"micro-segment overlay IPAM"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/spotlight overlay IPAM (HTTP ${http})"
  else
    e2e_platform_fail "POST spotlight overlay IPAM — HTTP ${http}"
  fi
  host_id="$(e2e_platform_curl -s "${E2E_PLATFORM_BASE}/api/v1/hosts" | python3 -c 'import sys,json; h=json.load(sys.stdin); print(h[0]["id"] if h else "")' 2>/dev/null || true)"
  if [[ -n "$host_id" ]]; then
    e2e_platform_smoke_get "/api/v1/hosts/${host_id}/lldp" "GET /api/v1/hosts/{id}/lldp" || true
  fi

  e2e_platform_hdr "PLATFORM SMOKE: V1 STUB HARDENING (operator apply, multisite sync, LLDP cache)"
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/zeus-firewall/operator/execute-batch" \
    -H 'Content-Type: application/json' -d '{"dry_run":true,"auto_only":true}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/zeus-firewall/operator/execute-batch dry-run (HTTP ${http})"
  else
    e2e_platform_fail "POST operator execute-batch — HTTP ${http}"
  fi
  e2e_platform_smoke_get "/api/v1/topology" "GET /api/v1/topology (LLDP cache edges)" || true

  e2e_platform_hdr "PLATFORM SMOKE: OPERATIONS (AI-452–471)"
  e2e_platform_smoke_get "/api/v1/operations/overview" "GET /api/v1/operations/overview" || true
  e2e_platform_smoke_get "/api/v1/operations/runbooks" "GET /api/v1/operations/runbooks" || true
  e2e_platform_smoke_get "/api/v1/operations/showback" "GET /api/v1/operations/showback" || true
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/spotlight" \
    -H 'Content-Type: application/json' -d '{"query":"operations runbook showback"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/spotlight operations (HTTP ${http})"
  else
    e2e_platform_fail "POST spotlight operations — HTTP ${http}"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: DEVELOPER ECOSYSTEM (AI-472–491)"
  e2e_platform_smoke_get "/api/v1/developer/overview" "GET /api/v1/developer/overview" || true
  e2e_platform_smoke_get "/api/v1/developer/terraform/schema" "GET /api/v1/developer/terraform/schema" || true
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/spotlight" \
    -H 'Content-Type: application/json' -d '{"query":"terraform sdk developer overview"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/spotlight developer (HTTP ${http})"
  else
    e2e_platform_fail "POST spotlight developer — HTTP ${http}"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: OBSERVABILITY (AI-492–511)"
  e2e_platform_smoke_get "/api/v1/observability/overview" "GET /api/v1/observability/overview" || true
  e2e_platform_smoke_get "/api/v1/observability/traces" "GET /api/v1/observability/traces" || true
  body="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/metrics/prometheus")"
  if [[ "$body" == *machina_slo_current_pct* || "$body" == *machina_api_trace_p95_ms* ]]; then
    e2e_platform_ok "GET /api/v1/metrics/prometheus SLO gauges"
  else
    e2e_platform_fail "Prometheus missing SLO gauges"
  fi
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/spotlight" \
    -H 'Content-Type: application/json' -d '{"query":"slo observability traces dashboard"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/spotlight observability (HTTP ${http})"
  else
    e2e_platform_fail "POST spotlight observability — HTTP ${http}"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: MARKETPLACE PLUGINS + PHASE 26 POLISH"
  e2e_platform_smoke_get "/api/v1/marketplace/plugins" "GET /api/v1/marketplace/plugins" || true
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/spotlight" \
    -H 'Content-Type: application/json' -d '{"query":"plugin marketplace install"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/spotlight plugin marketplace (HTTP ${http})"
  else
    e2e_platform_fail "POST spotlight plugin marketplace — HTTP ${http}"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: STORAGE TIERS (AI-412–431)"
  e2e_platform_smoke_get "/api/v1/storage/tiers/overview" "GET /api/v1/storage/tiers/overview" || true
  e2e_platform_smoke_get "/api/v1/storage/backup-sla" "GET /api/v1/storage/backup-sla" || true
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/spotlight" \
    -H 'Content-Type: application/json' -d '{"query":"storage tier backup sla"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/spotlight storage tiers (HTTP ${http})"
  else
    e2e_platform_fail "POST spotlight storage tiers — HTTP ${http}"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: ENTERPRISE SECURITY (AI-432–451)"
  e2e_platform_smoke_get "/api/v1/enterprise/security/overview" "GET /api/v1/enterprise/security/overview" || true
  e2e_platform_smoke_get "/api/v1/enterprise/vault/providers" "GET /api/v1/enterprise/vault/providers" || true
  e2e_platform_smoke_get "/api/v1/enterprise/mfa/policies" "GET /api/v1/enterprise/mfa/policies" || true
  e2e_platform_smoke_get "/api/v1/enterprise/air-gap/bundles" "GET /api/v1/enterprise/air-gap/bundles" || true
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/spotlight" \
    -H 'Content-Type: application/json' -d '{"query":"vault mfa air gap bundle"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/spotlight enterprise security (HTTP ${http})"
  else
    e2e_platform_fail "POST spotlight enterprise security — HTTP ${http}"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: ENTERPRISE HARDENING (AI-512–531)"
  e2e_platform_smoke_get "/api/v1/enterprise/mfa/compliance" "GET /api/v1/enterprise/mfa/compliance" || true
  e2e_platform_smoke_get "/api/v1/enterprise/fips/matrix" "GET /api/v1/enterprise/fips/matrix" || true
  e2e_platform_smoke_get "/api/v1/enterprise/tenants/overview" "GET /api/v1/enterprise/tenants/overview" || true
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/enterprise/vault/sync-all" \
    -H 'Content-Type: application/json' -d '{}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/enterprise/vault/sync-all (HTTP ${http})"
  else
    e2e_platform_fail "POST vault sync-all — HTTP ${http}"
  fi
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/spotlight" \
    -H 'Content-Type: application/json' -d '{"query":"fips tenant isolation vault sync"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/spotlight enterprise hardening (HTTP ${http})"
  else
    e2e_platform_fail "POST spotlight enterprise hardening — HTTP ${http}"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: HOST OS (AI-532–541)"
  host_id="$(e2e_platform_curl -s "${E2E_PLATFORM_BASE}/api/v1/hosts" | python3 -c 'import sys,json; h=json.load(sys.stdin); print(h[0]["id"] if h else "")' 2>/dev/null || true)"
  if [[ -n "$host_id" ]]; then
    e2e_platform_smoke_get "/api/v1/hosts/${host_id}/linux/observability" "GET /api/v1/hosts/{id}/linux/observability" || true
    e2e_platform_smoke_get "/api/v1/hosts/${host_id}/linux/network-diag" "GET /api/v1/hosts/{id}/linux/network-diag" || true
    e2e_platform_smoke_get "/api/v1/hosts/${host_id}/linux/audit" "GET /api/v1/hosts/{id}/linux/audit" || true
    e2e_platform_smoke_get "/api/v1/hosts/${host_id}/linux/filesystems" "GET /api/v1/hosts/{id}/linux/filesystems" || true
    e2e_platform_smoke_get "/api/v1/hosts/${host_id}/linux/processes?order=cpu&limit=10" "GET /api/v1/hosts/{id}/linux/processes" || true
    http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/hosts/${host_id}/linux/package-upgrade" \
      -H 'Content-Type: application/json' -d '{"dry_run":true}')"
    if [[ "$http" == "200" ]]; then
      e2e_platform_ok "POST /api/v1/hosts/{id}/linux/package-upgrade preview (HTTP ${http})"
    else
      e2e_platform_fail "POST package-upgrade preview — HTTP ${http}"
    fi
  fi
  vm_id="$(e2e_platform_curl -s "${E2E_PLATFORM_BASE}/api/v1/vms" | python3 -c 'import sys,json; v=json.load(sys.stdin); print(v[0]["id"] if v else "")' 2>/dev/null || true)"
  if [[ -n "$vm_id" ]]; then
    e2e_platform_smoke_get "/api/v1/vms/${vm_id}/guest/health" "GET /api/v1/vms/{id}/guest/health" || true
    e2e_platform_smoke_get "/api/v1/vms/${vm_id}/guest/services" "GET /api/v1/vms/{id}/guest/services" || true
  fi

  e2e_platform_hdr "PLATFORM SMOKE: HOST OS AI (AI-542–551)"
  if [[ -n "$host_id" ]]; then
    http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/hosts/${host_id}/diagnose" \
      -H 'Content-Type: application/json' -d '{"query":"host disk pressure"}')"
    if [[ "$http" == "200" ]]; then
      e2e_platform_ok "POST /api/v1/hosts/{id}/diagnose (HTTP ${http})"
    else
      e2e_platform_fail "POST host diagnose — HTTP ${http}"
    fi
  fi
  if [[ -n "$vm_id" ]]; then
    http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/diagnose" \
      -H 'Content-Type: application/json' -d '{"query":"guest ports exposed"}')"
    if [[ "$http" == "200" ]]; then
      e2e_platform_ok "POST /api/v1/vms/{id}/diagnose (HTTP ${http})"
    else
      e2e_platform_fail "POST vm diagnose — HTTP ${http}"
    fi
  fi
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/spotlight" \
    -H 'Content-Type: application/json' -d '{"query":"host disk pressure systemd network down"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/spotlight host OS intents (HTTP ${http})"
  else
    e2e_platform_fail "POST spotlight host OS intents — HTTP ${http}"
  fi

  e2e_platform_hdr "PLATFORM SMOKE: FLEET DESKTOP (Phases 35–36)"
  e2e_platform_smoke_get "/api/v1/fleet/desktop" "GET /api/v1/fleet/desktop" || true
  e2e_platform_smoke_get "/api/v1/fleet/linux-health" "GET /api/v1/fleet/linux-health" || true
  e2e_platform_smoke_get "/api/v1/fleet/activity" "GET /api/v1/fleet/activity" || true
  e2e_platform_smoke_get "/api/v1/fleet/backups" "GET /api/v1/fleet/backups" || true
  e2e_platform_smoke_get "/api/v1/fleet/finder" "GET /api/v1/fleet/finder" || true
  e2e_platform_smoke_get "/api/v1/fleet/network" "GET /api/v1/fleet/network" || true
  e2e_platform_smoke_get "/api/v1/fleet/storage" "GET /api/v1/fleet/storage" || true
  e2e_platform_smoke_get "/api/v1/fleet/console" "GET /api/v1/fleet/console" || true
  e2e_platform_smoke_get "/api/v1/fleet/updates" "GET /api/v1/fleet/updates" || true
  e2e_platform_smoke_get "/api/v1/fleet/keychain" "GET /api/v1/fleet/keychain" || true
  e2e_platform_smoke_get "/api/v1/fleet/users" "GET /api/v1/fleet/users" || true
  e2e_platform_smoke_get "/api/v1/fleet/shortcuts" "GET /api/v1/fleet/shortcuts" || true
  e2e_platform_smoke_get "/api/v1/fleet/spaces" "GET /api/v1/fleet/spaces" || true
  e2e_platform_smoke_get "/api/v1/fleet/general" "GET /api/v1/fleet/general" || true
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/spotlight" \
    -H 'Content-Type: application/json' -d '{"query":"general settings wallpaper dock"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/spotlight general settings (HTTP ${http})"
  else
    e2e_platform_fail "POST spotlight general settings — HTTP ${http}"
  fi
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' "${E2E_PLATFORM_BASE}/api/v1/vms?folder=running")"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "GET /api/v1/vms?folder=running (HTTP ${http})"
  else
    e2e_platform_fail "GET vms folder filter — HTTP ${http}"
  fi
  http="$(e2e_platform_curl -o /dev/null -w '%{http_code}' -X POST "${E2E_PLATFORM_BASE}/api/v1/ai/fleet/diagnose" \
    -H 'Content-Type: application/json' -d '{"query":"fleet linux pressure"}')"
  if [[ "$http" == "200" ]]; then
    e2e_platform_ok "POST /api/v1/ai/fleet/diagnose (HTTP ${http})"
  else
    e2e_platform_fail "POST fleet diagnose — HTTP ${http}"
  fi
}
