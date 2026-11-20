#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

set -euo pipefail
# Dedicated libvirt TEST host only. Uses a unique network; never touches an
# existing one. Choose a CIDR not present on the host or its upstream network.
: "${MACHINA_CLOUD_SMOKE_CIDR:?Set an unused /24 RFC1918 test CIDR}"
command -v virsh >/dev/null
command -v python3 >/dev/null
command -v cargo >/dev/null
id=$(cat /proc/sys/kernel/random/uuid)
name="mc-$id"
xml=$(mktemp)
cleanup() {
  virsh net-destroy "$name" >/dev/null 2>&1 || true
  virsh net-undefine "$name" >/dev/null 2>&1 || true
  rm -f "$xml"
}
trap cleanup EXIT
# Render the exact production XML using a spec example (no agent credentials).
cargo run -q -p machina-spec --example cloud_subnet_xml -- "$id" "$MACHINA_CLOUD_SMOKE_CIDR" > "$xml"
virsh net-define "$xml"
virsh net-start "$name"
virsh net-autostart "$name"
test "$(virsh net-uuid "$name")" = "$id"
virsh net-dumpxml "$name" > "$xml"
python3 - "$xml" <<'PY'
import sys, xml.etree.ElementTree as ET
root=ET.parse(sys.argv[1]).getroot()
assert root.find('forward') is None, 'expected isolated network'
assert root.find('ip/dhcp/range') is not None, 'missing DHCP range'
print('Isolated cloud subnet XML, UUID, activation and DHCP range verified.')
PY
# Cleanup trap is deliberately limited to this newly generated UUID/name.
