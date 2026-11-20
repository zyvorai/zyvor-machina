#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Optional KVM lifecycle smoke: define → destroy a tiny transient domain when /dev/kvm exists.
set -euo pipefail

if [[ ! -e /dev/kvm ]]; then
  echo "No /dev/kvm — skipping KVM lifecycle smoke"
  exit 0
fi

if ! command -v virsh >/dev/null 2>&1; then
  echo "virsh missing — skipping KVM lifecycle smoke"
  exit 0
fi

URI="${LIBVIRT_DEFAULT_URI:-qemu:///system}"
NAME="machina-ci-smoke-$$"
XML="$(mktemp)"
trap 'rm -f "$XML"; virsh -c "$URI" destroy "$NAME" 2>/dev/null || true; virsh -c "$URI" undefine "$NAME" --remove-all-storage 2>/dev/null || true' EXIT

cat >"$XML" <<EOF
<domain type='kvm'>
  <name>${NAME}</name>
  <memory unit='KiB'>65536</memory>
  <vcpu placement='static'>1</vcpu>
  <os><type arch='x86_64' machine='pc'>hvm</type></os>
  <devices><emulator>/usr/bin/qemu-system-x86_64</emulator></devices>
</domain>
EOF

virsh -c "$URI" define "$XML"
virsh -c "$URI" list --all | grep -q "$NAME"
virsh -c "$URI" undefine "$NAME"
echo "KVM lifecycle smoke OK ($NAME)"
