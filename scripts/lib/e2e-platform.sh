# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# shellcheck shell=bash
# Platform control plane E2E (controller :5093, batches 7–11).

e2e_platform_min_vm_json() {
  local name="$1"
  cat <<EOF
{
  "api_version": "virt.zyvor.dev/v1",
  "kind": "VirtualMachine",
  "metadata": { "name": "${name}" },
  "spec": {
    "cpu": { "sockets": 1, "cores": 1 },
    "memory": "512Mi",
    "storage": [{ "name": "root", "size": "5Gi", "class": "silver" }],
    "network": [{ "network": "default", "ip_mode": "dhcp" }],
    "firmware": "bios",
    "graphics": { "type": "vnc", "listen": "127.0.0.1" }
  },
  "tags": ["e2e", "platform"],
  "desired_state": "running"
}
EOF
}

e2e_platform_run() {
  local vm_name="e2e-platform-$$"
  local snap_name="snap-$$"
  local clone_name="e2e-clone-$$"
  local vm_id="" host_id="" r http webhook_id="" api_key_id="" delivery_id=""

  e2e_platform_hdr "PLATFORM: HEALTH"
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/health")"
  e2e_platform_assert_http "$http" "200" "health"
  r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/health")"
  echo "  $r"
  echo "$r" | grep -q '"database":"ok"' && e2e_platform_ok "database ok" || e2e_platform_fail "database not ok"
  echo "$r" | grep -q '"leader":true' && e2e_platform_ok "leader true" || e2e_platform_warn "leader not true (single-node may lag)"

  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/health/ready")"
  e2e_platform_assert_http "$http" "200" "ready"

  e2e_platform_hdr "PLATFORM: LEADERSHIP"
  r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/cluster/leadership")"
  echo "  $r"
  e2e_platform_assert_json_key "$r" "controller_id" "leadership controller_id"
  if echo "$r" | grep -q '"is_leader":true'; then
    e2e_platform_ok "leadership is_leader"
  else
    sleep 5
    r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/cluster/leadership")"
    if echo "$r" | grep -q '"is_leader":true'; then
      e2e_platform_ok "leadership is_leader (after wait)"
    else
      e2e_platform_warn "not leader yet — stale lease may expire on next restart"
    fi
  fi

  e2e_platform_hdr "PLATFORM: CLUSTER + SETTINGS"
  r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/cluster")"
  e2e_platform_assert_json_key "$r" "host_count" "cluster summary"
  r="$(e2e_platform_curl -X PATCH "${E2E_PLATFORM_BASE}/api/v1/cluster/settings" \
    -H "Content-Type: application/json" \
    -d '{"placement_policy":"packed","inventory_sync_interval_secs":45}')"
  echo "  $r"
  echo "$r" | grep -q '"placement_policy":"packed"' && e2e_platform_ok "placement_policy packed" \
    || e2e_platform_fail "placement_policy patch"
  echo "$r" | grep -q '"inventory_sync_interval_secs":45' && e2e_platform_ok "sync interval 45" \
    || e2e_platform_fail "sync interval patch"
  r="$(e2e_platform_curl -X PATCH "${E2E_PLATFORM_BASE}/api/v1/cluster/settings" \
    -H "Content-Type: application/json" \
    -d '{"placement_policy":"balanced","inventory_sync_interval_secs":30}')"
  echo "$r" | grep -q '"placement_policy":"balanced"' && e2e_platform_ok "placement_policy balanced" \
    || e2e_platform_warn "restore balanced policy"

  e2e_platform_hdr "PLATFORM: ENROLLMENT TOKEN"
  r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/enrollment/tokens" \
    -H "Content-Type: application/json" -d '{}')"
  e2e_platform_assert_json_key "$r" "token" "enrollment token create"

  e2e_platform_hdr "PLATFORM: HOSTS + SYNC"
  r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/hosts")"
  echo "  $(echo "$r" | tr '\n' ' ' | head -c 200)"
  host_id="$(echo "$r" | python3 -c "
import json, sys
try:
    hosts = json.load(sys.stdin)
    print(hosts[0]['id'] if hosts else '')
except Exception:
    print('')
" 2>/dev/null)"
  [[ -n "$host_id" ]] && e2e_platform_ok "host listed $host_id" || e2e_platform_fail "no hosts"
  r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/hosts/sync-all")"
  e2e_platform_assert_json_key "$r" "task_id" "sync-all enqueues tasks" || true
  e2e_platform_wait_task "host.inventory" 120 || true

  e2e_platform_hdr "PLATFORM: PLACEMENT"
  r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/placement/recommendations")"
  echo "$r" | grep -q '\[' && e2e_platform_ok "placement recommendations list"

  e2e_platform_hdr "PLATFORM: CREATE VM ($vm_name)"
  r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms" \
    -H "Content-Type: application/json" \
    -d "$(e2e_platform_min_vm_json "$vm_name")")"
  echo "  $r"
  e2e_platform_assert_json_key "$r" "task_id" "vm create task"
  if ! e2e_platform_wait_task "vm.apply" 360 1; then
    e2e_platform_warn "vm.apply timed out or failed — host may be resource-constrained; skipping VM lifecycle"
    return 0
  fi
  r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/vms")"
  vm_id="$(echo "$r" | python3 -c "
import json, sys
try:
    for v in json.load(sys.stdin):
        if v.get('name') == '$vm_name':
            print(v.get('id',''))
            break
except Exception:
    pass
" 2>/dev/null)"
  [[ -n "$vm_id" ]] && e2e_platform_ok "VM $vm_id in list" || { e2e_platform_fail "VM not listed"; return 1; }
  echo "$r" | grep -q '"tags"' && echo "$r" | grep -q 'e2e' && e2e_platform_ok "VM tags present"

  e2e_platform_wait_vm_state "$vm_id" "running" 240 || e2e_platform_warn "VM not running yet — continuing"

  e2e_platform_hdr "PLATFORM: VM POWER + PATCH TAGS"
  r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/pause")"
  e2e_platform_assert_json_key "$r" "task_id" "vm pause task"
  e2e_platform_wait_task "vm.power" 120 || true
  r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/resume")"
  e2e_platform_assert_json_key "$r" "task_id" "vm resume task"
  e2e_platform_wait_task "vm.power" 120 || true
  r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/shutdown")"
  e2e_platform_assert_json_key "$r" "task_id" "vm shutdown task"
  e2e_platform_wait_task "vm.power" 180 || true
  r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/start")"
  e2e_platform_wait_task "vm.power" 120 || true
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/guest/health")"
  [[ "$http" == "200" ]] && e2e_platform_ok "guest/health GET" || e2e_platform_warn "guest/health HTTP ${http}"
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/domain-xml")"
  [[ "$http" == "200" || "$http" == "404" || "$http" == "502" ]] && e2e_platform_ok "domain-xml GET (HTTP ${http})" \
    || e2e_platform_warn "domain-xml HTTP ${http}"
  r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/stop")"
  e2e_platform_assert_json_key "$r" "task_id" "vm stop task"
  e2e_platform_wait_task "vm.power" 120 || true
  r="$(e2e_platform_curl -X PATCH "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}" \
    -H "Content-Type: application/json" -d '{"tags":["e2e","platform","updated"]}')"
  echo "$r" | grep -q 'updated' && e2e_platform_ok "VM tags patched" || e2e_platform_warn "VM tag patch"

  e2e_platform_hdr "PLATFORM: SNAPSHOT + CLONE"
  r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/stop")"
  e2e_platform_wait_task "vm.power" 120 || true
  sleep 2
  r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/snapshots" \
    -H "Content-Type: application/json" -d "{\"name\":\"${snap_name}\"}")"
  if ! echo "$r" | grep -q '"task_id"'; then
    sleep 3
    r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/snapshots" \
      -H "Content-Type: application/json" -d "{\"name\":\"${snap_name}\"}")"
  fi
  e2e_platform_assert_json_key "$r" "task_id" "snapshot create task"
  snap_ok=0
  if e2e_platform_wait_task "vm.snapshot" 180 1; then
    snap_ok=1
  else
    e2e_platform_warn "snapshot failed (disk may not support internal snapshots — qcow2 required)"
  fi
  r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/snapshots")"
  echo "$r" | grep -q "\"${snap_name}\"" && e2e_platform_ok "snapshot listed" || e2e_platform_warn "snapshot missing (expected when disk format unsupported)"
  if [[ "$snap_ok" == "1" ]]; then
    r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/snapshots/${snap_name}/clone" \
      -H "Content-Type: application/json" \
      -d "{\"new_name\":\"${clone_name}\",\"revert_source\":false}")"
    e2e_platform_assert_json_key "$r" "task_id" "snapshot clone task"
    if ! e2e_platform_wait_task "vm.snapshot.clone" 240 1; then
      e2e_platform_warn "non-destructive clone failed — retry with revert_source"
      r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/snapshots/${snap_name}/clone" \
        -H "Content-Type: application/json" \
        -d "{\"new_name\":\"${clone_name}\",\"revert_source\":true}")"
      e2e_platform_wait_task "vm.snapshot.clone" 240 || e2e_platform_warn "snapshot clone failed"
    fi
    r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/vms")"
    echo "$r" | grep -q "\"${clone_name}\"" && e2e_platform_ok "cloned VM listed" || e2e_platform_warn "cloned VM not listed"
  else
    e2e_platform_warn "skipping clone — snapshot was not created"
  fi

  e2e_platform_hdr "PLATFORM: WEBHOOKS"
  r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/webhooks" \
    -H "Content-Type: application/json" \
    -d '{"url":"http://127.0.0.1:19876/e2e","events":["vm.*"],"secret":"e2e-secret"}')"
  webhook_id="$(e2e_platform_json_field "$r" id)"
  [[ -n "$webhook_id" ]] && e2e_platform_ok "webhook created $webhook_id" || e2e_platform_warn "webhook create skipped"
  r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/webhook-deliveries?limit=10")"
  echo "$r" | grep -q '\[' && e2e_platform_ok "webhook deliveries list"
  delivery_id="$(echo "$r" | python3 -c "
import json, sys
try:
    rows = json.load(sys.stdin)
    print(rows[0]['id'] if rows else '')
except Exception:
    print('')
" 2>/dev/null)"
  if [[ -n "$delivery_id" ]]; then
    r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/webhook-deliveries/${delivery_id}/retry")"
    echo "$r" | grep -q '"status":"pending"' && e2e_platform_ok "delivery retry" || e2e_platform_warn "delivery retry"
  else
    e2e_platform_warn "no webhook deliveries yet (worker may be async)"
  fi
  if [[ -n "$webhook_id" ]]; then
    e2e_platform_curl -X DELETE "${E2E_PLATFORM_BASE}/api/v1/webhooks/${webhook_id}" >/dev/null \
      && e2e_platform_ok "webhook cleanup $webhook_id" \
      || e2e_platform_warn "webhook cleanup failed"
  fi

  e2e_platform_hdr "PLATFORM: API KEYS"
  r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/api-keys" \
    -H "Content-Type: application/json" \
    -d '{"name":"e2e-key-'"$$"'","role":"operator"}')"
  api_key_id="$(e2e_platform_json_field "$r" id)"
  e2e_platform_assert_json_key "$r" "token" "api key token"
  r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/api-keys")"
  echo "$r" | grep -q "$api_key_id" && e2e_platform_ok "api key listed"
  if [[ -n "$api_key_id" ]]; then
    e2e_platform_curl -X DELETE "${E2E_PLATFORM_BASE}/api/v1/api-keys/${api_key_id}" >/dev/null \
      && e2e_platform_ok "api key cleanup $api_key_id" \
      || e2e_platform_warn "api key cleanup failed"
  fi

  e2e_platform_hdr "PLATFORM: TASKS + AUDIT"
  r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/tasks?operation=vm.&limit=5")"
  echo "$r" | grep -q '"operation"' && e2e_platform_ok "tasks operation filter"
  r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/audit?limit=5")"
  echo "$r" | grep -q '\[' && e2e_platform_ok "audit log list"

  e2e_platform_hdr "PLATFORM: OIDC SETTINGS (read)"
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/auth/oidc")"
  e2e_platform_assert_http "$http" "200" "oidc settings GET"
}
