#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# VM runtime intelligence smoke test: boots a throwaway KVM microVM (host
# kernel, no root fs; it panics and idles) in a private cgroup, tracks it as an
# extra target and checks every report section. bpfd runs on a private socket
# with interface auto-attach disabled, so real VMs and taps are never touched.
#
#   sudo ./scripts/bpf/vmintel-smoke.sh [path/to/machina-bpfd]
set -euo pipefail

BPFD=${1:-./target/debug/machina-bpfd}
WORK=$(mktemp -d /tmp/mnvmi-smoke.XXXX)
SOCK=$WORK/bpfd.sock
CG_REL=mnsmoke-vmi
CG=/sys/fs/cgroup/$CG_REL
KERNEL=${KERNEL:-$(ls -1 /boot/vmlinuz-* 2>/dev/null | sort -V | tail -1)}
PASS=0
FAIL=0

cleanup() {
  [[ -n "${QEMU_PID:-}" ]] && kill "$QEMU_PID" 2>/dev/null && wait "$QEMU_PID" 2>/dev/null || true
  [[ -n "${BPFD_PID:-}" ]] && kill "$BPFD_PID" 2>/dev/null && wait "$BPFD_PID" 2>/dev/null || true
  [[ -d "$CG" ]] && rmdir "$CG" 2>/dev/null || true
  [[ -n "${DISK:-}" ]] && rm -f "$DISK"
}
trap cleanup EXIT

req() {
  python3 - "$SOCK" "$1" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX)
s.connect(sys.argv[1])
s.sendall(sys.argv[2].encode() + b"\n")
buf = b""
while not buf.endswith(b"\n"):
    c = s.recv(65536)
    if not c:
        break
    buf += c
print(buf.decode().strip())
PY
}
must() {
  local out
  out=$(cat)
  python3 -c 'import json,sys; sys.exit(0 if json.loads(sys.argv[1]).get("ok") else 1)' "$out" || echo "WARN  request failed: $out"
}
check() {
  local name=$1
  shift
  if "$@"; then echo "PASS  $name"; PASS=$((PASS + 1)); else echo "FAIL  $name"; FAIL=$((FAIL + 1)); fi
}
js() { req "$1" | python3 -c "import json,sys; d=json.load(sys.stdin)['data']; print($2)"; }

[[ -e /dev/kvm ]] || { echo "SKIP  no /dev/kvm"; exit 0; }
command -v qemu-system-x86_64 >/dev/null || { echo "SKIP  no qemu-system-x86_64"; exit 0; }
[[ -r "$KERNEL" ]] || { echo "SKIP  no readable kernel image"; exit 0; }

echo '{"telemetry": {"iface_patterns": []}}' >"$WORK/bpfd-state.json"
RUST_LOG=${RUST_LOG:-info} "$BPFD" --socket "$SOCK" --state-dir "$WORK" --socket-group "" >"$WORK/bpfd.log" 2>&1 &
BPFD_PID=$!
for _ in $(seq 50); do [[ -S "$SOCK" ]] && break; sleep 0.2; done
[[ -S "$SOCK" ]] || { echo "bpfd did not start"; cat "$WORK/bpfd.log"; exit 1; }

check "off by default" test "$(js '{"op":"vm_intel_status"}' "(d['config']['enabled'], len(d['hooks']))")" = "(False, 0)"
check "unknown feature rejected" bash -c "$(declare -f req); SOCK=$SOCK; ! req '{\"op\":\"vm_intel_configure\",\"config\":{\"enabled\":true,\"features\":[\"nope\"]}}' | grep -q '\"ok\":true'"

mkdir -p "$CG"
# Not on tmpfs: O_DIRECT reads must reach the block layer.
DISK=$(mktemp /var/tmp/mnvmi-disk.XXXX)
dd if=/dev/zero of="$DISK" bs=1M count=16 status=none
req "{\"op\":\"vm_intel_configure\",\"config\":{\"enabled\":true,\"extra\":[{\"name\":\"mnsmoke-vm\",\"cgroup\":\"$CG_REL\"}]}}" | must
bash -c "echo \$\$ > $CG/cgroup.procs && exec qemu-system-x86_64 -accel kvm -m 256 -smp 2 -display none -serial none -monitor none \
  -kernel '$KERNEL' -append 'panic=0 quiet' -drive file=$DISK,if=virtio,format=raw,cache=none,aio=native" \
  >"$WORK/qemu.log" 2>&1 &
QEMU_PID=$!
sleep 2
req '{"op":"vm_refresh"}' | must
sleep 6
# Direct I/O from inside the VM's cgroup (the guest may not touch its disk).
bash -c "echo \$\$ > $CG/cgroup.procs && exec dd if=$DISK of=/dev/null iflag=direct bs=4k count=256 status=none" 2>/dev/null || true
sleep 2

st() { js '{"op":"vm_intel_status"}' "$1"; }
rep() { js '{"op":"vm_intel_vm","name":"mnsmoke-vm"}' "$1"; }
check "hooks attached (flight/mem/topology)" test "$(st "all(any(h==p or h.startswith(p+'@') for h in d['hooks']) for p in ('mn_vmi_kvm_exit','mn_vmi_switch','mn_vmi_kvm_entry','mn_vmi_fault','mn_vmi_irq_entry'))")" = True
check "microVM tracked with 2 vCPUs" test "$(st "[(v['processes'], v['vcpus']) for v in d['vms'] if v['name']=='mnsmoke-vm']")" = "[(1, 2)]"
check "KVM exit reasons recorded" test "$(rep "sum(e['count'] for e in d['exits'])")" -gt 0
check "exit reasons named from tracefs" test "$(rep "any(not e['name'].startswith('reason ') for e in d['exits'])")" = True
check "vCPU run-queue latency histogram" test "$(rep "d['runq']['count']")" -gt 0
check "vCPU residency per pCPU" test "$(rep "sum(r['ns'] for r in d['residency'])")" -gt 0
check "page-fault latency histogram" test "$(rep "d['fault']['count']")" -gt 0
check "boot to first KVM entry (0-10 s)" test "$(rep "0 < (d['boot_to_first_entry_ms'] or -1) < 10000")" = True
check "per-CPU IRQ time (topology)" test "$(st "sum(c['irq_ns']+c['softirq_ns'] for c in d['cpus'])")" -gt 0
check "block I/O latency histogram" test "$(rep "d['block']['count']")" -gt 0
check "unknown VM is an error" bash -c "$(declare -f req); SOCK=$SOCK; ! req '{\"op\":\"vm_intel_vm\",\"name\":\"nope\"}' | grep -q '\"ok\":true'"

req '{"op":"vm_intel_configure","config":{"features":["flight"]}}' | must
check "off: hooks detached, tracking emptied" test "$(st "(len(d['hooks']), len(d['vms']))")" = "(0, 0)"

echo
echo "passed=$PASS failed=$FAIL  (log: $WORK/bpfd.log)"
grep -E "WARN|ERROR" "$WORK/bpfd.log" | head -20 || true
[[ $FAIL -eq 0 ]]
