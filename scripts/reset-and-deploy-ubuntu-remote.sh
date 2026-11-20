#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Wipe platform libvirt VMs on a remote host, then deploy ubuntu-desktop via Machina platform API.
# KubeVirt inventory is left untouched.
#
# Usage:
#   ./scripts/reset-and-deploy-ubuntu-remote.sh sus 212.8.252.194
#   SSH_KEY=~/.ssh/id_ed25519 ./scripts/reset-and-deploy-ubuntu-remote.sh sus 212.8.252.194
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/e2e-platform-common.sh
source "${SCRIPT_DIR}/lib/e2e-platform-common.sh"

USER="${1:?usage: $0 USER HOST}"
HOST="${2:?usage: $0 USER HOST}"
# USER/HOST are spliced verbatim into command strings the remote shell re-parses
# (e.g. line ~95 below) — reject shell metacharacters up front.
[[ "$USER" =~ ^[A-Za-z0-9_.-]+$ ]] || { echo "invalid username: '$USER'" >&2; exit 1; }
[[ "$HOST" =~ ^[A-Za-z0-9_.:-]+$ ]] || { echo "invalid host: '$HOST'" >&2; exit 1; }
E2E_PLATFORM_BASE="${E2E_PLATFORM_BASE:-http://${HOST}:5093}"
SSH="ssh -o BatchMode=yes -o StrictHostKeyChecking=no ${USER}@${HOST}"

SSH_KEY="${SSH_KEY:-${E2E_SSH_KEY:-}}"
CLOUD_INIT_USER="${CLOUD_INIT_USER:-ubuntu}"
VM_NAME="${VM_NAME:-ubuntu-desktop}"
DISK_SIZE="${DISK_SIZE:-30Gi}"
DESKTOP_TEMPLATE="${DESKTOP_TEMPLATE:-ubuntu-24.04-desktop@1.0.0}"
BUILD_DESKTOP_GOLDEN="${BUILD_DESKTOP_GOLDEN:-1}"

info() { echo "== $*"; }

platform_curl() {
  e2e_platform_curl "$@"
}

build_create_payload() {
  local pubkey_json='None'
  if [[ -n "$SSH_KEY" && -f "$SSH_KEY" ]]; then
    pubkey_json="$(python3 -c 'import json,sys; print(json.dumps(sys.stdin.read().strip()))' < <(ssh-keygen -y -f "$SSH_KEY" 2>/dev/null))"
  fi
  python3 - <<PY
import json
pubkey = ${pubkey_json}
payload = {
    "api_version": "virt.zyvor.dev/v1",
    "kind": "VirtualMachine",
    "metadata": {"name": "${VM_NAME}", "project": "default"},
    "spec": {
        "cpu": {"sockets": 1, "cores": 2},
        "memory": "4Gi",
        "template_ref": "${DESKTOP_TEMPLATE}",
        "storage": [{"name": "root", "size": "${DISK_SIZE}", "class": "silver"}],
        "network": [{"network": "default", "ip_mode": "dhcp"}],
        "firmware": "bios",
        "graphics": {"type": "vnc", "listen": "127.0.0.1"},
    },
    "tags": [],
    "desired_state": "running",
}
if pubkey:
    payload["spec"]["cloud_init"] = {
        "user": "${CLOUD_INIT_USER}",
        "ssh_pubkey": pubkey,
    }
print(json.dumps(payload))
PY
}

info "Platform API → ${E2E_PLATFORM_BASE} (libvirt-only reset)"

vms_json="$(platform_curl "${E2E_PLATFORM_BASE}/api/v1/vms")"

while read -r vid name managed src; do
  [[ -z "$vid" ]] && continue
  if [[ "$src" == "kubevirt" ]]; then
    info "Skipping KubeVirt VM ${name} (${vid})"
    continue
  fi
  info "Deleting platform VM ${name} (${vid}, managed=${managed}, source=${src:-libvirt})"
  platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vid}/delete" >/dev/null || true
  for _ in $(seq 1 60); do
    st="$(platform_curl "${E2E_PLATFORM_BASE}/api/v1/tasks?operation=vm.delete&limit=1" | python3 -c "
import json, sys
t = json.load(sys.stdin)
print(t[0]['status'] if t else 'done')
" 2>/dev/null || echo done)"
    [[ "$st" == "completed" || "$st" == "failed" || "$st" == "done" ]] && break
    sleep 2
  done
done < <(echo "$vms_json" | python3 -c "
import json, sys
for v in json.load(sys.stdin):
    src = v.get('inventory_source') or 'libvirt'
    print(v.get('id', ''), v.get('name', ''), v.get('managed', True), src)
")

info "Purging leftover libvirt domains on host"
$SSH "printf '%s\n' '${VSPASS:-max}' | sudo -S bash -c 'for n in \$(virsh list --all --name 2>/dev/null); do
  [ -z \"\$n\" ] && continue
  virsh destroy \"\$n\" 2>/dev/null || true
  virsh undefine \"\$n\" --remove-all-storage 2>/dev/null || virsh undefine \"\$n\" 2>/dev/null || true
done'"

info "Removing stale VM disk images (linked clone must match template backing)"
$SSH "printf '%s\n' '${VSPASS:-max}' | sudo -S bash -c 'rm -f /var/lib/libvirt/images/${VM_NAME}.qcow2 /var/lib/libvirt/images/${VM_NAME}-cloud-init.iso'"

info "Prune stale missing VM records"
platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/prune-missing" >/dev/null || true

info "Sync host inventory"
platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/hosts/sync-all" >/dev/null || true
sleep 5

if [[ "${BUILD_DESKTOP_GOLDEN}" == "1" ]]; then
  info "Ensuring Ubuntu desktop golden image on host (${DESKTOP_TEMPLATE})"
  FORCE="${FORCE_DESKTOP_GOLDEN:-0}" "${SCRIPT_DIR}/build-ubuntu-desktop-golden-remote.sh" "$USER" "$HOST"
fi

info "Creating ${VM_NAME} (2 vCPU, 4 GiB, ${DISK_SIZE}, template ${DESKTOP_TEMPLATE}, VNC, running)"
create_body="$(build_create_payload)"
create_resp="$(platform_curl -X POST -H "Content-Type: application/json" \
  "${E2E_PLATFORM_BASE}/api/v1/vms" \
  -d "$create_body")"
echo "$create_resp"
task_id="$(echo "$create_resp" | python3 -c "import json,sys; print(json.load(sys.stdin).get('task_id',''))")"
if [[ -z "$task_id" ]]; then
  err="$(echo "$create_resp" | python3 -c "import json,sys; print(json.load(sys.stdin).get('error',''))" 2>/dev/null || true)"
  echo "❌ VM create failed: ${err:-$create_resp}"
  exit 1
fi

info "Waiting for vm.apply task ${task_id}"
for _ in $(seq 1 90); do
  task="$(platform_curl "${E2E_PLATFORM_BASE}/api/v1/tasks/${task_id}")"
  st="$(echo "$task" | python3 -c "import json,sys; print(json.load(sys.stdin).get('status',''))")"
  msg="$(echo "$task" | python3 -c "import json,sys; print(json.load(sys.stdin).get('message',''))")"
  echo "  task: ${st} — ${msg}"
  [[ "$st" == "completed" ]] && break
  [[ "$st" == "failed" ]] && { echo "❌ vm.apply failed: $task"; exit 1; }
  sleep 3
done

vm_row="$(platform_curl "${E2E_PLATFORM_BASE}/api/v1/vms" | python3 -c "
import json, sys
for v in json.load(sys.stdin):
    if v.get('name') == '${VM_NAME}':
        print(json.dumps(v))
        break
")"

[[ -n "$vm_row" ]] || { echo "❌ ${VM_NAME} not found in platform inventory"; exit 1; }

echo "$vm_row" | python3 -c "
import json, sys
v = json.load(sys.stdin)
err = v.get('last_error') or ''
phase = v.get('lifecycle_phase') or ''
obs = v.get('observed_state') or ''
des = v.get('desired_state') or ''
src = v.get('inventory_source') or 'libvirt'
print(f'  id={v.get(\"id\")} name={v.get(\"name\")} source={src} desired={des} observed={obs} phase={phase}')
if src == 'kubevirt':
    print('❌ expected libvirt inventory_source')
    sys.exit(1)
if err:
    print(f'❌ last_error: {err}')
    sys.exit(1)
if obs != 'running' or phase == 'error':
    print(f'❌ VM not healthy: observed={obs} phase={phase}')
    sys.exit(1)
print('✅ ${VM_NAME} deployed with zero errors')
print(v.get('id', ''))
"

# Fixed, predictable /tmp paths shared with e2e-libvirt-desktop-playwright-remote.sh
# (which later `source`s the .env file) — kept as-is for that consumer, but refuse
# to write through a pre-planted symlink (a local attacker could otherwise point
# either file at an arbitrary path and have it overwritten, or worse, have attacker
# shell code sourced by the downstream script).
VM_ID_FILE=/tmp/machina-ubuntu-desktop-vm-id.txt
ENV_FILE=/tmp/machina-ubuntu-desktop-e2e.env
[ -L "$VM_ID_FILE" ] && { echo "❌ refusing to write through symlink: $VM_ID_FILE" >&2; exit 1; }
[ -L "$ENV_FILE" ] && { echo "❌ refusing to write through symlink: $ENV_FILE" >&2; exit 1; }

echo "$vm_row" | python3 -c "
import json, sys
v = json.load(sys.stdin)
print(v.get('id', ''))
" | tee "$VM_ID_FILE"

vm_id="$(tail -1 "$VM_ID_FILE")"
echo "VM_ID=${vm_id}" > "$ENV_FILE"
echo "HOST=${HOST}" >> "$ENV_FILE"

info "Done — https://${HOST}:5092/platform/vms/${vm_id}"
