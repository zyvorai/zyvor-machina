#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Libvirt desktop E2E: reset ubuntu-desktop, GuestKit agent, lifecycle, port-forward SSH, VNC prep.
#
# Usage:
#   VSPASS=max ./scripts/e2e-libvirt-desktop-full-remote.sh sus 212.8.252.194 --ssh-key ~/.ssh/id_ed25519
#   VSPASS=max ./scripts/e2e-libvirt-desktop-full-remote.sh sus 212.8.252.194 --skip-playwright
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"

USER="${1:?usage: $0 USER HOST [--ssh-key PATH] [--skip-playwright] [--skip-reset]}"
HOST="${2:?usage: $0 USER HOST [--ssh-key PATH] [--skip-playwright] [--skip-reset]}"
shift 2

SSH_KEY="${E2E_SSH_KEY:-$HOME/.ssh/id_ed25519}"
SKIP_PLAYWRIGHT=false
SKIP_RESET=false
SSH_HOST_PORT="${E2E_SSH_HOST_PORT:-30222}"
VM_NAME="${VM_NAME:-ubuntu-desktop}"
SNAP_NAME="e2e-snap-$$"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --ssh-key) shift; SSH_KEY="${1:?}"; shift ;;
    --skip-playwright) SKIP_PLAYWRIGHT=true; shift ;;
    --skip-reset) SKIP_RESET=true; shift ;;
    --ssh-port) shift; SSH_HOST_PORT="${1:?}"; shift ;;
    -h|--help)
      sed -n '2,12p' "$0"
      exit 0
      ;;
    *) echo "Unknown flag: $1" >&2; exit 2 ;;
  esac
done

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
# shellcheck source=lib/e2e-guest-network.sh
source "${SCRIPT_DIR}/lib/e2e-guest-network.sh"

E2E_PASS=0
E2E_FAIL=0
SSH="ssh -o BatchMode=yes -o StrictHostKeyChecking=no ${USER}@${HOST}"

LIBVIRT_URI="${LIBVIRT_URI:-qemu:///system}"

resolve_guest_ip_remote() {
  $SSH "bash -lc '
    VM=\"${VM_NAME}\"
    URI=\"${LIBVIRT_URI}\"
    ip=\$(virsh -c \"\$URI\" domifaddr \"\$VM\" --source agent 2>/dev/null | awk \"/ipv4/ && \\\$4 !~ /^127\\\\./ {print \\\$4; exit}\" | cut -d/ -f1)
    if [[ -z \"\$ip\" ]]; then
      mac=\$(virsh -c \"\$URI\" dumpxml \"\$VM\" 2>/dev/null | awk -F\"[\\\"']\" \"/mac address/ {print \\\$4; exit}\")
      if [[ -n \"\$mac\" ]]; then
        ip=\$(virsh -c \"\$URI\" net-dhcp-leases default 2>/dev/null | awk -v m=\"\$mac\" \"\\\$3==m {print \\\$5; exit}\" | cut -d/ -f1)
      fi
    fi
    if [[ -z \"\$ip\" ]]; then
      ip=\$(virsh -c \"\$URI\" net-dhcp-leases default 2>/dev/null | awk \"\\\$6==\\\"${VM_NAME}\\\" {print \\\$5; exit}\" | cut -d/ -f1)
    fi
    echo \"\$ip\"
  '" 2>/dev/null | tr -d '\r'
}

bootstrap_guest_network_remote() {
  e2e_guest_network_bootstrap_remote "$SSH" "${VM_NAME}" "${LIBVIRT_URI}" 8
}

inject_ssh_key_remote() {
  [[ -f "$SSH_KEY" ]] || return 1
  local pubkey payload remote_script quoted_script
  pubkey="$(ssh-keygen -y -f "$SSH_KEY" 2>/dev/null | tr -d '\n')"
  [[ -n "$pubkey" ]] || return 1
  payload="$(python3 -c 'import json,sys; print(json.dumps({"execute":"guest-ssh-add-authorized-keys","arguments":{"username":"ubuntu","keys":[sys.argv[1]]}}))' "$pubkey")"
  # payload embeds the pubkey's comment field verbatim (attacker/user-controlled if
  # SSH_KEY points at an arbitrary keyfile) — do NOT %q-escape it and then splice
  # that into a hand-written single-quoted wrapper: a literal "'" surviving the %q
  # escaping (rendered as \') closes the wrapper early and lets the remainder be
  # re-parsed as shell syntax on the remote host (see e2e-guest-network.sh's
  # e2e_guest_qemu_exec_remote for the identical bug and full explanation). Escape
  # each dynamic value once, then %q-escape the whole composed command once more
  # so it survives as a single literal argument to `bash -lc`.
  remote_script="virsh -c $(printf '%q' "$LIBVIRT_URI") qemu-agent-command $(printf '%q' "$VM_NAME") $(printf '%q' "$payload") | grep -q '\"return\"'"
  quoted_script="$(printf '%q' "$remote_script")"
  $SSH "bash -lc $quoted_script" 2>/dev/null
}

echo "══════════════════════════════════════════"
echo "  Libvirt desktop E2E"
echo "  Target: ${USER}@${HOST}"
echo "  API:    ${E2E_PLATFORM_BASE}"
echo "  VM:     ${VM_NAME}"
echo "══════════════════════════════════════════"

if ! $SKIP_RESET; then
  e2e_platform_hdr "RESET + DEPLOY ubuntu-desktop"
  SSH_KEY="$SSH_KEY" CLOUD_INIT_USER=ubuntu \
    "${SCRIPT_DIR}/reset-and-deploy-ubuntu-remote.sh" "$USER" "$HOST" || exit 1
fi

vm_id="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/vms" | python3 -c "
import json, sys
for v in json.load(sys.stdin):
    if v.get('name') == '${VM_NAME}':
        print(v.get('id', ''))
        break
" 2>/dev/null)"
[[ -n "$vm_id" ]] || { e2e_platform_fail "${VM_NAME} not in inventory"; e2e_platform_summary; exit 1; }
e2e_platform_ok "VM id ${vm_id}"

e2e_platform_hdr "GUESTKIT AGENT (test-guest-agent.sh on host)"
agent_ok=false
for attempt in $(seq 1 24); do
  if $SSH "bash -lc '
    VM=\"${VM_NAME}\"
    URI=\"${LIBVIRT_URI}\"
    for s in /usr/local/share/machina/scripts/test-guest-agent.sh \"\$HOME/machina/scripts/test-guest-agent.sh\"; do
      if [[ -x \"\$s\" ]]; then \"\$s\" \"\$VM\" \"\$URI\"; exit \$?; fi
    done
    exit 1
  '" 2>/dev/null; then
    agent_ok=true
    break
  fi
  echo "  guest-ping attempt ${attempt}/24 — waiting for GuestKit agent..."
  sleep 15
done
if $agent_ok; then
  e2e_platform_ok "GuestKit guest-ping via test-guest-agent.sh"
else
  e2e_platform_fail "GuestKit agent not responding (guest-ping)"
fi

e2e_platform_hdr "GUEST HEALTH + GUESTKIT STATUS API"
guest_health=""
for _ in $(seq 1 30); do
  guest_health="$(E2E_PLATFORM_TIMEOUT=45 e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/guest/health" 2>/dev/null)" || guest_health='{}'
  guest_ip="$(echo "$guest_health" | python3 -c "
import json, sys
try:
    d = json.load(sys.stdin)
    print((d.get('guest_ip') or '').strip())
except Exception:
    print('')
" 2>/dev/null)"
  agent_ping="$(echo "$guest_health" | python3 -c "
import json, sys
try:
    d = json.load(sys.stdin)
    print('true' if d.get('agent_ping') else 'false')
except Exception:
    print('false')
" 2>/dev/null)"
  [[ -n "$guest_ip" && "$agent_ping" == "true" ]] && break
  sleep 10
done
if [[ -n "${guest_ip:-}" ]]; then
  e2e_platform_ok "guest_ip ${guest_ip}"
else
  bootstrap_guest_network_remote
  guest_ip="$(resolve_guest_ip_remote)"
  if [[ -n "${guest_ip:-}" ]]; then
    e2e_platform_ok "guest_ip ${guest_ip} (after GuestKit network bootstrap)"
  else
    e2e_platform_warn "guest_ip not reported yet"
  fi
fi
if [[ "${agent_ping:-}" == "true" ]]; then
  e2e_platform_ok "guest/health agent_ping"
else
  e2e_platform_fail "guest/health agent_ping false"
fi

gk_status="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/guestkit/status" 2>/dev/null)" || gk_status='{}'
echo "  guestkit/status: $(echo "$gk_status" | head -c 200)"
echo "$gk_status" | grep -qE '"ok"|"enabled"|"version"|"worker"' && e2e_platform_ok "guestkit/status" || e2e_platform_warn "guestkit/status unexpected"

e2e_platform_hdr "LIFECYCLE: pause → resume → snapshot"
r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/pause")"
e2e_platform_assert_json_key "$r" "task_id" "pause"
e2e_platform_wait_task "vm.power" 120 || true
e2e_platform_wait_vm_state "$vm_id" "paused" 120 || e2e_platform_warn "not paused"

r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/resume")"
e2e_platform_assert_json_key "$r" "task_id" "resume"
e2e_platform_wait_task "vm.power" 120 || true
e2e_platform_wait_vm_state "$vm_id" "running" 180 || e2e_platform_warn "not running after resume"
for attempt in $(seq 1 12); do
  if $SSH "virsh -c \"${LIBVIRT_URI}\" qemu-agent-command \"${VM_NAME}\" '{\"execute\":\"guest-ping\"}' 2>/dev/null | grep -q '\"return\"'"; then
    break
  fi
  echo "  guest-ping after resume attempt ${attempt}/12..."
  sleep 10
done

r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/snapshots" \
  -H "Content-Type: application/json" -d "{\"name\":\"${SNAP_NAME}\"}")"
e2e_platform_assert_json_key "$r" "task_id" "snapshot create"
e2e_platform_wait_task "vm.snapshot" 180 || true

snaps="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/snapshots")"
echo "$snaps" | grep -q "${SNAP_NAME}" && e2e_platform_ok "snapshot ${SNAP_NAME} listed" || e2e_platform_fail "snapshot missing"

e2e_platform_hdr "CONSOLE + WS TOKEN"
http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/console")"
[[ "$http" == "200" ]] && e2e_platform_ok "GET console (HTTP ${http})" || e2e_platform_fail "console HTTP ${http}"
r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/ws-token")"
e2e_platform_assert_json_key "$r" "token" "ws-token"

plan_json="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/consolehub/plan")"
echo "$plan_json" | grep -q '"serial"' && e2e_platform_ok "consolehub plan includes serial" || e2e_platform_fail "consolehub plan missing serial"
echo "$plan_json" | grep -q '"novnc"' && e2e_platform_ok "consolehub plan includes novnc" || e2e_platform_fail "consolehub plan missing novnc"
rec="$(echo "$plan_json" | python3 -c "import json,sys; print(json.load(sys.stdin).get('recommended',''))" 2>/dev/null || true)"
[[ "$rec" == "novnc" ]] && e2e_platform_ok "consolehub recommends novnc (desktop golden)" || e2e_platform_warn "consolehub recommended=${rec:-unknown} (expected novnc for desktop golden)"
echo "$plan_json" | grep -q 'platform/serial' && e2e_platform_ok "consolehub native serial_ws_path" || e2e_platform_warn "consolehub serial_ws_path missing"

e2e_platform_hdr "PORT FORWARD + SSH FROM LAPTOP"
if [[ -n "${guest_ip:-}" ]]; then
  if [[ -f "$SSH_KEY" ]]; then
    if inject_ssh_key_remote; then
      e2e_platform_ok "GuestKit guest-ssh-add-authorized-keys"
    else
      e2e_platform_warn "GuestKit SSH key inject failed (cloud-init key may still work)"
    fi
  fi
  r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/port-forwards" \
    -H "Content-Type: application/json" \
    -d "{\"protocol\":\"tcp\",\"host_port\":${SSH_HOST_PORT},\"vm_ip\":\"${guest_ip}\",\"vm_port\":22,\"description\":\"e2e-ssh\"}")"
  echo "$r" | grep -q '"ok"' && e2e_platform_ok "port-forward host:${SSH_HOST_PORT} → ${guest_ip}:22" || e2e_platform_warn "port-forward: $r"
  sleep 2
  if [[ -f "$SSH_KEY" ]]; then
    if ssh -i "$SSH_KEY" -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new \
      -o ConnectTimeout=20 -p "$SSH_HOST_PORT" "ubuntu@${HOST}" 'echo ok' 2>/dev/null | grep -q '^ok$'; then
      e2e_platform_ok "SSH from laptop ubuntu@${HOST}:${SSH_HOST_PORT}"
    else
      e2e_platform_fail "SSH from laptop ubuntu@${HOST}:${SSH_HOST_PORT}"
    fi
  else
    e2e_platform_warn "SSH key missing at ${SSH_KEY} — skip laptop SSH"
  fi
else
  e2e_platform_warn "no guest_ip — skip port-forward SSH"
fi

e2e_platform_hdr "DAEMON GUEST SCREENSHOT (libvirt API)"
shot_out="${REPO}/web/test-results/libvirt-desktop-vnc/guest-screenshot-api.png"
mkdir -p "$(dirname "$shot_out")"
daemon_user="${PLAYWRIGHT_LIVE_USER:-${E2E_PLATFORM_USER:-sus}}"
daemon_pass="${PLAYWRIGHT_LIVE_PASS:-${VSPASS:-max}}"
cookie_jar="$(mktemp)"
curl -sk -c "$cookie_jar" -X POST "https://${HOST}:5092/api/v1/auth/login" \
  -H "Content-Type: application/json" \
  -d "{\"username\":\"${daemon_user}\",\"password\":\"${daemon_pass}\"}" >/dev/null 2>&1 || true
http="$(curl -sk -b "$cookie_jar" -o "$shot_out" -w '%{http_code}' \
  "https://${HOST}:5092/api/v1/vms/${VM_NAME}/guest/screenshot?screen=0" 2>/dev/null || echo 000)"
rm -f "$cookie_jar"
if [[ "$http" == "200" && -s "$shot_out" ]]; then
  e2e_platform_ok "guest/screenshot saved → ${shot_out}"
else
  e2e_platform_warn "guest/screenshot HTTP ${http}"
fi

export E2E_LIBVIRT_VM_ID="$vm_id"
echo "E2E_LIBVIRT_VM_ID=${vm_id}" > /tmp/machina-ubuntu-desktop-e2e.env
echo "HOST=${HOST}" >> /tmp/machina-ubuntu-desktop-e2e.env

if ! $SKIP_PLAYWRIGHT; then
  e2e_platform_hdr "PLAYWRIGHT VNC (laptop)"
  if "${SCRIPT_DIR}/e2e-libvirt-desktop-playwright-remote.sh" "$USER" "$HOST" "$vm_id"; then
    e2e_platform_ok "Playwright VNC desktop screenshots"
  else
    e2e_platform_fail "Playwright VNC desktop screenshots"
  fi
fi

e2e_platform_summary
exit $?
