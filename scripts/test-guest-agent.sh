#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Test guest agent (GuestKit implements QGA on org.qemu.guest_agent.0) for a libvirt VM.
# Usage: ./scripts/test-guest-agent.sh [VM_NAME] [LIBVIRT_URI]
set -euo pipefail

VM="${1:-ubuntu-desktop}"
URI="${2:-qemu:///system}"

if [[ "$VM" == "-h" || "$VM" == "--help" ]]; then
  echo "Usage: $0 [VM_NAME] [LIBVIRT_URI]"
  exit 0
fi

# Auto-pick first running VM if default not found
if ! virsh -c "$URI" dominfo "$VM" &>/dev/null; then
  VM="$(virsh -c "$URI" list --name --state-running 2>/dev/null | head -1 || true)"
fi
if [[ -z "$VM" ]]; then
  echo "SKIP: no VM found (pass VM_NAME or start a guest)"
  exit 0
fi

echo "== Guest agent test (GuestKit / QGA): ${VM} (${URI}) =="

xml="$(virsh -c "$URI" dumpxml "$VM" 2>/dev/null || true)"
if [[ -z "$xml" ]]; then
  echo "FAIL: VM not found"
  exit 1
fi

if echo "$xml" | grep -q 'org.qemu.guest_agent.0'; then
  echo "OK  : Virtio channel org.qemu.guest_agent.0 in domain XML"
else
  echo "FAIL: Virtio channel missing (Machina Install guest tools)"
  exit 1
fi

if echo "$xml" | grep -q "org.qemu.guest_agent.0" && ! echo "$xml" | grep -q "state='disconnected'"; then
  echo "OK  : Channel connected"
else
  echo "WARN: Channel disconnected — start guestkit-agent inside the guest"
fi

echo "--- guest-ping (virsh qemu-agent-command) ---"
if virsh -c "$URI" qemu-agent-command "$VM" '{"execute":"guest-ping"}' 2>/dev/null | grep -q '"return"'; then
  echo "OK  : guest-ping"
  echo "--- guest-info ---"
  virsh -c "$URI" qemu-agent-command "$VM" '{"execute":"guest-info"}' 2>/dev/null | head -8 || true
else
  echo "FAIL: guest-ping — install guestkit-agent in VM:"
  echo "      sudo systemctl enable --now guestkit-agent"
  exit 1
fi

echo "--- domifaddr (lease / arp / agent) ---"
virsh -c "$URI" domifaddr "$VM" --source lease 2>/dev/null | head -5 || true
virsh -c "$URI" domifaddr "$VM" --source agent 2>/dev/null | head -5 || true

echo "DONE"
