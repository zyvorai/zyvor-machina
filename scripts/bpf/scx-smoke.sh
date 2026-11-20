#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# sched_ext smoke test: machina-bpfd supervises machina-scx for a throwaway
# process whose threads are named like QEMU vCPUs ("CPU n/KVM"). No libvirt
# VM is listed, so real VMs are never moved to SCHED_EXT.
#
#   cargo build --release -p machina-scx --features scx
#   sudo ./scripts/bpf/scx-smoke.sh [machina-bpfd] [machina-scx]
set -euo pipefail

BPFD=${1:-./target/release/machina-bpfd}
SCX=${2:-./target/release/machina-scx}
WORK=$(mktemp -d /tmp/mnscx-smoke.XXXX)
SOCK=$WORK/bpfd.sock
PASS=0
FAIL=0

cleanup() {
  [[ -n "${BPFD_PID:-}" ]] && kill "$BPFD_PID" 2>/dev/null && wait "$BPFD_PID" 2>/dev/null || true
  [[ -n "${VM_PID:-}" ]] && kill "$VM_PID" 2>/dev/null || true
  pkill -f "^$SCX" 2>/dev/null || true
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
ok() { python3 -c 'import json,sys; sys.exit(0 if json.load(sys.stdin).get("ok") else 1)'; }
check() {
  local name=$1
  shift
  if "$@"; then echo "PASS  $name"; PASS=$((PASS + 1)); else echo "FAIL  $name"; FAIL=$((FAIL + 1)); fi
}
js() { req '{"op":"scx_status"}' | python3 -c "import json,sys; d=json.load(sys.stdin)['data']; print($1)"; }
policy() { awk '/^policy/ {print $3}' "/proc/$VM_PID/task/$1/sched"; }
vcpu_policies() { for t in $VCPUS; do policy "$t"; done | sort -u | tr '\n' ' '; }
state_is() { for _ in $(seq 30); do [[ $(cat /sys/kernel/sched_ext/state) == "$1" ]] && return 0; sleep 0.2; done; return 1; }

if [[ ! -r /sys/kernel/sched_ext/state ]]; then echo "SKIP: kernel has no sched_ext"; exit 0; fi
if [[ $(cat /sys/kernel/sched_ext/state) != disabled ]]; then echo "SKIP: another sched_ext scheduler is active"; exit 0; fi

# Fake VMM: two busy "vCPU" threads plus an unnamed main thread.
python3 -u -c '
import threading, time
def vcpu(i):
    open(f"/proc/self/task/{threading.get_native_id()}/comm", "w").write(f"CPU {i}/KVM")
    while True:
        t = time.time()
        while time.time() - t < 0.005: pass
        time.sleep(0.001)
for i in range(2): threading.Thread(target=vcpu, args=(i,), daemon=True).start()
while True: time.sleep(1)
' &
VM_PID=$!
sleep 0.5
VCPUS=$(grep -l 'CPU [0-9]*/KVM' /proc/$VM_PID/task/*/comm | awk -F/ '{print $5}' | tr '\n' ' ')
EXTRA="\"extra\":[{\"name\":\"mnsmoke-vm\",\"pid\":$VM_PID}]"

echo '{"telemetry": {"iface_patterns": []}}' >"$WORK/bpfd-state.json"
MACHINA_SCX_BIN=$SCX RUST_LOG=${RUST_LOG:-info} "$BPFD" --socket "$SOCK" --state-dir "$WORK" --socket-group "" >>"$WORK/bpfd.log" 2>&1 &
BPFD_PID=$!
for _ in $(seq 50); do [[ -S "$SOCK" ]] && break; sleep 0.2; done
[[ -S "$SOCK" ]] || { echo "bpfd did not start"; cat "$WORK/bpfd.log"; exit 1; }

check "two fake vCPU threads" test "$(wc -w <<<"$VCPUS")" -eq 2
check "sched_ext supported, helper found" test "$(js "(d['supported'], d['helper'] is not None, d['running'])")" = "(True, True, False)"
check "no lease refused" bash -c "$(declare -f req ok); SOCK=$SOCK; ! req '{\"op\":\"scx_configure\",\"config\":{\"enabled\":true,$EXTRA}}' | ok"
check "unknown VM refused" bash -c "$(declare -f req ok); SOCK=$SOCK; ! req '{\"op\":\"scx_configure\",\"config\":{\"enabled\":true,\"lease_secs\":30,\"vms\":[\"mnsmoke-no-such-vm\"]}}' | ok"
check "scheduler started under a lease" bash -c "$(declare -f req ok); SOCK=$SOCK; req '{\"op\":\"scx_configure\",\"config\":{\"enabled\":true,\"lease_secs\":30,\"latency_target_us\":1,$EXTRA}}' | ok"
check "kernel state enabled" state_is enabled
check "machina_scx is the loaded scheduler" test "$(js "d['ops']")" = machina_scx
check "vCPU threads on SCHED_EXT (7)" test "$(vcpu_policies)" = "7 "
check "main thread left on CFS" test "$(policy "$VM_PID")" = 0
sleep 2
check "per-VM dispatch stats flowing" test "$(js "d['vms'][0]['dispatches'] > 0 and d['vms'][0]['enqueues'] > 0 and d['vms'][0]['vcpus'] == 2")" = True
check "latency violations counted (1us target)" test "$(js "d['vms'][0]['latency_violations'] > 0")" = True
check "lease reported" test "$(js "0 < d['lease_remaining_secs'] <= 30")" = True
check "stopped on request" bash -c "$(declare -f req ok); SOCK=$SOCK; req '{\"op\":\"scx_configure\",\"config\":{\"enabled\":false}}' | ok"
check "kernel state disabled after stop" state_is disabled
check "vCPU threads back on CFS" test "$(vcpu_policies)" = "0 "

req "{\"op\":\"scx_configure\",\"config\":{\"enabled\":true,\"lease_secs\":2,$EXTRA}}" >/dev/null
state_is enabled || true
sleep 4
check "lease expiry stops the scheduler" test "$(js "(d['running'], d['lease_expired'], d['last_exit'])")" = "(False, True, 'lease expired')"
check "kernel state disabled after lease" state_is disabled
check "vCPU threads back on CFS after lease" test "$(vcpu_policies)" = "0 "

req "{\"op\":\"scx_configure\",\"config\":{\"enabled\":true,\"lease_secs\":60,$EXTRA}}" >/dev/null
state_is enabled || true
pkill -9 -f "^$SCX" || true
sleep 2
check "helper death detected" test "$(js "(d['running'], 'helper exited' in (d['last_exit'] or ''))")" = "(False, True)"
check "kernel state disabled after helper death" state_is disabled
check "vCPU threads back on CFS after helper death" test "$(vcpu_policies)" = "0 "

echo "passed=$PASS failed=$FAIL  (log: $WORK/bpfd.log)"
[[ $FAIL -eq 0 ]]
