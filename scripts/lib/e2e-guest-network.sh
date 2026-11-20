# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# shellcheck shell=bash
# Guest network bootstrap via QGA guest-exec — distro / network-manager aware.
#
# Ubuntu cloud images use netplan → systemd-networkd (not dhclient). RHEL-family
# images typically use NetworkManager. Pick the stack at runtime instead of
# hard-coding enp1s0 + dhclient.

# Inline script executed inside the guest (stdout captured when guest-exec succeeds).
e2e_guest_network_bootstrap_script() {
  cat <<'BOOTSTRAP'
set -euo pipefail

primary_iface() {
  ip -o link show up 2>/dev/null | awk -F': ' '
    $2 !~ /^(lo|docker|veth|br-|virbr)/ { gsub(/@.*/, "", $2); print $2; exit }'
  ip -o link show 2>/dev/null | awk -F': ' '
    $2 !~ /^(lo|docker|veth|br-|virbr)/ { gsub(/@.*/, "", $2); print $2; exit }'
}

IFACE="$(primary_iface)"
[[ -n "$IFACE" ]] || { echo "bootstrap: no data NIC found"; exit 1; }
ip link set "$IFACE" up 2>/dev/null || true

# shellcheck disable=SC1091
. /etc/os-release 2>/dev/null || true
OS_ID="${ID:-unknown}"
OS_LIKE="${ID_LIKE:-}"

detect_network_manager() {
  if command -v networkctl >/dev/null 2>&1 \
    && systemctl is-active --quiet systemd-networkd 2>/dev/null; then
    echo systemd-networkd
  elif command -v nmcli >/dev/null 2>&1 \
    && systemctl is-active --quiet NetworkManager 2>/dev/null; then
    echo NetworkManager
  elif [[ -d /etc/netplan ]] || [[ -f /run/netplan/generate.done ]]; then
    echo netplan
  elif [[ -f /etc/network/interfaces ]]; then
    echo ifupdown
  else
    echo unknown
  fi
}

NM="$(detect_network_manager)"

bootstrap_systemd_networkd() {
  local iface="$1"
  if ! command -v networkctl >/dev/null 2>&1; then
    return 1
  fi
  networkctl renew "$iface" 2>/dev/null \
    || networkctl reconfigure "$iface" 2>/dev/null \
    || networkctl up "$iface" 2>/dev/null \
    || true
  if [[ -d /etc/netplan ]] && command -v netplan >/dev/null 2>&1; then
    netplan apply 2>/dev/null || true
  fi
}

bootstrap_network_manager() {
  local iface="$1"
  nmcli general status >/dev/null 2>&1 || nmcli networking on 2>/dev/null || true
  nmcli device set "$iface" managed yes 2>/dev/null || true
  nmcli device connect "$iface" 2>/dev/null \
    || nmcli device reapply "$iface" 2>/dev/null \
    || true
}

bootstrap_ifupdown() {
  local iface="$1"
  ifup "$iface" 2>/dev/null || ifup --force "$iface" 2>/dev/null || true
}

bootstrap_legacy_dhcp() {
  local iface="$1"
  if command -v dhclient >/dev/null 2>&1; then
    dhclient -1 -4 "$iface" 2>/dev/null || dhclient -1 "$iface" 2>/dev/null || true
  elif command -v dhcpcd >/dev/null 2>&1; then
    dhcpcd -4 -1 "$iface" 2>/dev/null || true
  fi
}

case "$OS_ID" in
  ubuntu|debian|pop)
    bootstrap_systemd_networkd "$IFACE" || bootstrap_network_manager "$IFACE"
    ;;
  fedora|rhel|centos|rocky|almalinux|ol)
    if [[ "$NM" == NetworkManager ]]; then
      bootstrap_network_manager "$IFACE"
    else
      bootstrap_systemd_networkd "$IFACE" || bootstrap_network_manager "$IFACE"
    fi
    ;;
  *)
    case "$NM" in
      systemd-networkd|netplan) bootstrap_systemd_networkd "$IFACE" ;;
      NetworkManager)         bootstrap_network_manager "$IFACE" ;;
      ifupdown)               bootstrap_ifupdown "$IFACE" ;;
      *)                      bootstrap_legacy_dhcp "$IFACE" ;;
    esac
    ;;
esac

IP="$(ip -4 -o addr show dev "$IFACE" 2>/dev/null | awk '{print $4}' | head -1)"
echo "bootstrap: os=${OS_ID} like=${OS_LIKE} manager=${NM} iface=${IFACE} ip=${IP:-none}"
BOOTSTRAP
}

# Build QGA guest-exec JSON for a bash -lc script.
e2e_guest_qemu_exec_payload() {
  local script="$1"
  python3 -c '
import json, sys
script = sys.stdin.read()
print(json.dumps({
    "execute": "guest-exec",
    "arguments": {
        "path": "/usr/bin/bash",
        "arg": ["-lc", script],
        "capture-output": True,
    },
}))
' <<<"$script"
}

# Run guest-exec on the hypervisor via SSH + virsh qemu-agent-command.
e2e_guest_qemu_exec_remote() {
  local ssh_cmd="$1" vm="$2" uri="$3" guest_script="$4"
  local payload remote_script quoted_script
  payload="$(e2e_guest_qemu_exec_payload "$guest_script")"
  # Build the remote virsh invocation with each dynamic value shell-escaped for
  # a single parse pass, then %q-escape the whole thing once more so it survives
  # as one literal argument to `bash -lc` on the remote end. Do NOT additionally
  # wrap this in manual single quotes: printf '%q' emits backslash escapes, and a
  # literal "'" in an escaped value (e.g. a guest_script containing an apostrophe)
  # would prematurely terminate a hand-written single-quoted wrapper and let the
  # remainder be re-parsed as shell syntax on the hypervisor.
  remote_script="virsh -c $(printf '%q' "$uri") qemu-agent-command $(printf '%q' "$vm") $(printf '%q' "$payload")"
  quoted_script="$(printf '%q' "$remote_script")"
  # shellcheck disable=SC2086
  $ssh_cmd "bash -lc $quoted_script" \
    2>/dev/null || true
}

# Renew DHCP / bring up the primary NIC using the guest OS network stack.
e2e_guest_network_bootstrap_remote() {
  local ssh_cmd="$1" vm="$2" uri="${3:-qemu:///system}" wait_secs="${4:-8}"
  local script
  script="$(e2e_guest_network_bootstrap_script)"
  e2e_guest_qemu_exec_remote "$ssh_cmd" "$vm" "$uri" "$script"
  sleep "$wait_secs"
}
