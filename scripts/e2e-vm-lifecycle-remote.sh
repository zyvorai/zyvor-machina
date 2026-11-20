#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# e2e-vm-lifecycle-remote.sh — platform VM lifecycle + SSH key verification on a remote host.
#
# Usage:
#   ./scripts/e2e-vm-lifecycle-remote.sh USER HOST \
#     --ssh-key ~/.ssh/id_ed25519 \
#     [--deploy] [--template ubuntu-24.04] [--keep-vm]
#
# Requires: curl, ssh, ssh-keygen; VSPASS (or E2E_PLATFORM_PASS) for controller :5093 auth.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"

USER="${1:?usage: $0 USER HOST --ssh-key PATH [--deploy] [--template NAME] [--keep-vm]}"
HOST="${2:?usage: $0 USER HOST --ssh-key PATH [--deploy] [--template NAME] [--keep-vm]}"
shift 2

SSH_KEY=""
DO_DEPLOY=false
TEMPLATE="ubuntu-24.04"
KEEP_VM=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --ssh-key) shift; SSH_KEY="${1:?}"; shift ;;
    --deploy) DO_DEPLOY=true; shift ;;
    --template) shift; TEMPLATE="${1:?}"; shift ;;
    --keep-vm) KEEP_VM=true; shift ;;
    -h|--help)
      sed -n '2,10p' "$0"
      exit 0
      ;;
    *) echo "Unknown flag: $1" >&2; exit 2 ;;
  esac
done

[[ -n "$SSH_KEY" ]] || { echo "error: --ssh-key PATH is required" >&2; exit 2; }
[[ -f "$SSH_KEY" ]] || { echo "error: SSH key not found: $SSH_KEY" >&2; exit 2; }

export E2E_PLATFORM_BASE="${E2E_PLATFORM_BASE:-http://${HOST}:5093}"
export E2E_PLATFORM_USER="${E2E_PLATFORM_USER:-$USER}"
export E2E_PLATFORM_PASS="${E2E_PLATFORM_PASS:-${VSPASS:-}}"

if [[ -z "$E2E_PLATFORM_PASS" ]]; then
  read -rsp "Controller password for ${E2E_PLATFORM_USER}@${E2E_PLATFORM_BASE}: " E2E_PLATFORM_PASS
  echo
  export E2E_PLATFORM_PASS
fi

# shellcheck source=lib/e2e-platform-common.sh
source "${SCRIPT_DIR}/lib/e2e-platform-common.sh"

E2E_PASS=0
E2E_FAIL=0

VM_NAME="e2e-lifecycle-$$"
SNAP_NAME="snap-$$"
SSH_PUB="$(ssh-keygen -y -f "$SSH_KEY" 2>/dev/null | tr -d '\n')"
SSH_USER="ubuntu"
TEMPLATE_REF="${TEMPLATE}@1.0.0"

echo "══════════════════════════════════════════"
echo "  VM lifecycle remote E2E"
echo "  Target: ${USER}@${HOST}"
echo "  API:    ${E2E_PLATFORM_BASE}"
echo "  VM:     ${VM_NAME}"
echo "══════════════════════════════════════════"

if $DO_DEPLOY; then
  e2e_platform_hdr "DEPLOY"
  "${SCRIPT_DIR}/deploy-remote.sh" "${USER}@${HOST}" --platform --ssh-key "$SSH_KEY" --quick || {
    e2e_platform_fail "deploy-remote.sh"
    exit 1
  }
  e2e_platform_ok "deploy complete"
fi

e2e_platform_hdr "HEALTH"
http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/health")"
e2e_platform_assert_http "$http" "200" "controller health"

e2e_platform_hdr "CREATE FROM TEMPLATE (${TEMPLATE_REF})"
r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/from-template" \
  -H "Content-Type: application/json" \
  -d "{\"template_ref\":\"${TEMPLATE_REF}\",\"name\":\"${VM_NAME}\",\"desired_state\":\"running\",\"cloud_init_user\":\"${SSH_USER}\",\"cloud_init_ssh_pubkey\":$(python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))' <<<"$SSH_PUB")}")"
e2e_platform_assert_json_key "$r" "task_id" "from-template create"
e2e_platform_wait_task "vm.apply" 360 || exit 1

r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/vms")"
vm_id="$(echo "$r" | python3 -c "
import json, sys
for v in json.load(sys.stdin):
    if v.get('name') == '${VM_NAME}':
        print(v.get('id',''))
        break
" 2>/dev/null)"
[[ -n "$vm_id" ]] || { e2e_platform_fail "VM not listed"; exit 1; }
e2e_platform_ok "VM id ${vm_id}"

e2e_platform_wait_vm_state "$vm_id" "running" 360 || e2e_platform_warn "VM not running yet"

e2e_platform_hdr "LIFECYCLE: pause → resume → snapshot"
r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/pause")"
e2e_platform_assert_json_key "$r" "task_id" "pause"
e2e_platform_wait_task "vm.power" 120 || true
e2e_platform_wait_vm_state "$vm_id" "paused" 120 || e2e_platform_warn "not paused"

r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/resume")"
e2e_platform_assert_json_key "$r" "task_id" "resume"
e2e_platform_wait_task "vm.power" 120 || true
e2e_platform_wait_vm_state "$vm_id" "running" 180 || e2e_platform_warn "not running after resume"

r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/snapshots" \
  -H "Content-Type: application/json" -d "{\"name\":\"${SNAP_NAME}\"}")"
e2e_platform_assert_json_key "$r" "task_id" "snapshot create"
e2e_platform_wait_task "vm.snapshot" 180 || true

e2e_platform_hdr "GUEST SSH"
guest_ip=""
for _ in $(seq 1 40); do
  r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/guest/health")"
  guest_ip="$(echo "$r" | python3 -c "
import json, sys
try:
    d = json.load(sys.stdin)
    print((d.get('guest_ip') or '').strip())
except Exception:
    print('')
" 2>/dev/null)"
  [[ -n "$guest_ip" ]] && break
  sleep 15
done
if [[ -z "$guest_ip" ]]; then
  e2e_platform_warn "guest_ip not reported — skipping ssh verify"
else
  e2e_platform_ok "guest_ip ${guest_ip}"
  if ssh -i "$SSH_KEY" -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new \
    -o ConnectTimeout=20 "${SSH_USER}@${guest_ip}" true 2>/dev/null; then
    e2e_platform_ok "ssh ${SSH_USER}@${guest_ip}"
  else
    e2e_platform_fail "ssh ${SSH_USER}@${guest_ip}"
  fi
fi

e2e_platform_hdr "CONSOLE + GUEST PORTS"
http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/console")"
[[ "$http" == "200" ]] && e2e_platform_ok "GET console (HTTP ${http})" || e2e_platform_warn "console HTTP ${http}"
http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/zeus-firewall/vms/${vm_id}/guest-ports")"
if [[ "$http" == "200" ]]; then
  r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/zeus-firewall/vms/${vm_id}/guest-ports")"
  echo "  $r" | head -c 200
  echo "$r" | grep -q '"ports"' && e2e_platform_ok "guest-ports report"
else
  e2e_platform_warn "guest-ports HTTP ${http}"
fi

e2e_platform_hdr "EXPORT"
spec_out="$(mktemp --suffix=.json "/tmp/machina-vm-export-${VM_NAME}.XXXXXX")"
e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/spec" >"$spec_out"
[[ -s "$spec_out" ]] && e2e_platform_ok "spec → ${spec_out}" || e2e_platform_fail "spec export"

xml_out="$(mktemp --suffix=.xml "/tmp/machina-vm-export-${VM_NAME}.XXXXXX")"
http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/domain-xml")"
if [[ "$http" == "200" ]]; then
  e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/domain-xml" | python3 -c "
import json, sys
d = json.load(sys.stdin)
open('${xml_out}', 'w').write(d.get('xml',''))
" 2>/dev/null
  [[ -s "$xml_out" ]] && e2e_platform_ok "domain-xml → ${xml_out}" || e2e_platform_warn "domain-xml empty"
else
  e2e_platform_warn "domain-xml HTTP ${http}"
fi

e2e_platform_hdr "SHUTDOWN → FORCE STOP"
r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/shutdown")"
e2e_platform_assert_json_key "$r" "task_id" "shutdown"
e2e_platform_wait_task "vm.power" 180 || true
sleep 5
r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/stop")"
e2e_platform_assert_json_key "$r" "task_id" "force stop"
e2e_platform_wait_task "vm.power" 120 || true

if ! $KEEP_VM; then
  e2e_platform_hdr "TEARDOWN"
  r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/delete" \
    -H "Content-Type: application/json" -d '{"confirmed":true}')"
  e2e_platform_assert_json_key "$r" "task_id" "vm delete" || true
  e2e_platform_wait_task "vm.delete" 240 || e2e_platform_warn "delete task"
else
  e2e_platform_ok "keeping VM ${VM_NAME} (${vm_id})"
fi

e2e_platform_summary
exit $?
