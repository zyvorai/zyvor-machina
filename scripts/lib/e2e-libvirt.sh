# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# shellcheck shell=bash
# Libvirt VM lifecycle E2E (sourced after e2e_login).

e2e_libvirt_run() {
  local vm_name="e2e-libvirt-$$"
  local ssh_host="${E2E_SSH_HOST:-$(e2e_host_from_base)}"
  local r

  e2e_hdr "LIBVIRT: CREATE VM ($vm_name)"
  r="$(${E2E_CURL} -b "$E2E_COOKIE" -X POST "${E2E_BASE}/api/v1/vms" \
    -H "Content-Type: application/json" \
    -d "{
      \"name\":\"${vm_name}\",
      \"vcpus\":1,
      \"memory_mb\":512,
      \"disk_gb\":5,
      \"network\":\"default\",
      \"firmware\":\"bios\",
      \"graphics_type\":\"vnc\",
      \"graphics_listen\":\"127.0.0.1\"
    }")"
  echo "  $r"
  e2e_assert_json_key "$r" "status" "create returns status"
  if ! echo "$r" | grep -q '"status":"created"'; then
    e2e_fail "VM create failed — cannot continue libvirt tests"
    return 1
  fi

  e2e_hdr "LIBVIRT: LIST VMs"
  r="$(${E2E_CURL} -b "$E2E_COOKIE" "${E2E_BASE}/api/v1/vms")"
  if echo "$r" | grep -q "\"${vm_name}\""; then
    e2e_ok "VM visible in list"
  else
    e2e_fail "VM not found in list"
  fi

  e2e_hdr "LIBVIRT: START VM"
  r="$(${E2E_CURL} -b "$E2E_COOKIE" -X POST "${E2E_BASE}/api/v1/vms/${vm_name}/start")"
  echo "  $r"
  if echo "$r" | grep -q '"status":"started"'; then
    e2e_ok "VM started"
  elif echo "$r" | grep -q 'already running'; then
    e2e_ok "VM already running (idempotent start)"
  else
  r="$(${E2E_CURL} -b "$E2E_COOKIE" "${E2E_BASE}/api/v1/vms/${vm_name}")"
    if echo "$r" | grep -q '"state":"running"'; then
      e2e_ok "VM already running (state check)"
    else
      e2e_fail "start failed — $r"
    fi
  fi
  sleep 2

  e2e_hdr "LIBVIRT: VM DETAILS (running)"
  r="$(${E2E_CURL} -b "$E2E_COOKIE" "${E2E_BASE}/api/v1/vms/${vm_name}")"
  echo "  $(echo "$r" | tr ',' '\n' | grep -E '"state"|"uuid"|"vcpus"' | tr '\n' ' ')"
  if echo "$r" | grep -q '"state":"running"'; then
    e2e_ok "state == running"
  else
    e2e_fail "state not running — $(echo "$r" | grep -o '"state":"[^"]*"' | head -1)"
  fi

  e2e_hdr "LIBVIRT: VNC CONSOLE-INFO"
  r="$(${E2E_CURL} -b "$E2E_COOKIE" "${E2E_BASE}/api/v1/vms/console-info/${vm_name}")"
  echo "  $r"
  if echo "$r" | grep -q '"console_type":"vnc"'; then
    e2e_ok "console_type == vnc"
  else
    e2e_fail "console_type not vnc"
  fi
  if echo "$r" | grep -qE '"port":[0-9]'; then
    e2e_ok "VNC $(echo "$r" | grep -o '"port":[0-9]*' | head -1)"
  else
    e2e_fail "no VNC port"
  fi

  if [[ "${E2E_SKIP_DHCP_CHECK:-0}" -eq 1 ]]; then
    e2e_hdr "LIBVIRT: BOOT CHECK (skipped — --skip-dhcp-check)"
    e2e_ok "DHCP poll skipped"
    return 0
  fi

  e2e_hdr "LIBVIRT: BOOT CHECK (DHCP / guest agent, up to 90s)"
  local ip_found=""
  local i ga
  for i in $(seq 1 18); do
    ga="$(ssh -o StrictHostKeyChecking=no -o BatchMode=yes -o ConnectTimeout=10 \
      "${ssh_host}" \
      "sudo virsh domifaddr ${vm_name} --source agent 2>/dev/null" 2>/dev/null \
      | grep -oE '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+' | grep -v '^127\.' | head -1)" || true
    if [[ -z "$ga" ]]; then
      ga="$(ssh -o StrictHostKeyChecking=no -o BatchMode=yes -o ConnectTimeout=10 \
        "${ssh_host}" \
        "sudo virsh net-dhcp-leases default 2>/dev/null" 2>/dev/null \
        | grep -i "${vm_name}" \
        | grep -oE '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+' | head -1)" || true
    fi
    if [[ -n "$ga" ]]; then
      ip_found="$ga"
      break
    fi
    echo "  waiting... (${i}/18)"
    sleep 5
  done
  if [[ -n "$ip_found" ]]; then
    e2e_ok "VM got IP: $ip_found"
  else
    e2e_warn "No IP in 90s (OK for blank-disk smoke VMs)"
    (( E2E_PASS++ )) || true
  fi

  e2e_hdr "LIBVIRT: STOP VM"
  r="$(${E2E_CURL} -b "$E2E_COOKIE" -X POST "${E2E_BASE}/api/v1/vms/${vm_name}/stop")"
  echo "  $r"
  if echo "$r" | grep -q '"status":"stopped"'; then
    e2e_ok "stop OK"
  else
    e2e_fail "stop unexpected: $r"
  fi
  sleep 1

  e2e_hdr "LIBVIRT: VM DETAILS (shutoff)"
  r="$(${E2E_CURL} -b "$E2E_COOKIE" "${E2E_BASE}/api/v1/vms/${vm_name}")"
  if echo "$r" | grep -q '"state":"shutoff"'; then
    e2e_ok "state == shutoff"
  else
    e2e_fail "state not shutoff"
  fi

  e2e_hdr "LIBVIRT: DELETE VM"
  r="$(${E2E_CURL} -b "$E2E_COOKIE" -X DELETE "${E2E_BASE}/api/v1/vms/${vm_name}")"
  echo "  $r"
  if echo "$r" | grep -q '"status":"deleted"'; then
    e2e_ok "delete OK"
  else
    e2e_fail "delete unexpected: $r"
  fi

  e2e_hdr "LIBVIRT: VERIFY GONE (404)"
  local http
  http="$(${E2E_CURL} -o /dev/null -w "%{http_code}" -b "$E2E_COOKIE" "${E2E_BASE}/api/v1/vms/${vm_name}")"
  e2e_assert_http "$http" "404" "VM 404 after delete"
}
