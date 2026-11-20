#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# VMM guard (BPF-LSM) smoke test on a throwaway cgroup standing in for a QEMU
# scope; real VMs are never guarded (no machine-qemu scope is listed in `vms`).
#
# Without `bpf` in /sys/kernel/security/lsm it checks the lsm_inactive
# refusals and that the hooks pass the verifier (MACHINA_BPF_GUARD_ASSUME_LSM);
# with it active it also checks audit and lease-gated enforcement.
#
#   sudo ./scripts/bpf/guard-smoke.sh [path/to/machina-bpfd]
set -euo pipefail

BPFD=${1:-./target/debug/machina-bpfd}
WORK=$(mktemp -d /tmp/mnguard-smoke.XXXX)
SOCK=$WORK/bpfd.sock
CG_REL=mnsmoke-guard
CG=/sys/fs/cgroup/$CG_REL
PASS=0
FAIL=0

stop_bpfd() { [[ -n "${BPFD_PID:-}" ]] && kill "$BPFD_PID" 2>/dev/null && wait "$BPFD_PID" 2>/dev/null || true; BPFD_PID=; }
cleanup() { stop_bpfd; [[ -d "$CG" ]] && rmdir "$CG" 2>/dev/null || true; }
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
js() { req "$1" | python3 -c "import json,sys; d=json.load(sys.stdin)['data']; print($2)"; }
start_bpfd() {
  echo '{"telemetry": {"iface_patterns": []}}' >"$WORK/bpfd-state.json"
  env "$@" RUST_LOG=${RUST_LOG:-info} "$BPFD" --socket "$SOCK" --state-dir "$WORK" --socket-group "" >>"$WORK/bpfd.log" 2>&1 &
  BPFD_PID=$!
  rm -f "$SOCK"
  for _ in $(seq 50); do [[ -S "$SOCK" ]] && break; sleep 0.2; done
  [[ -S "$SOCK" ]] || { echo "bpfd did not start"; cat "$WORK/bpfd.log"; exit 1; }
}
in_cg() { bash -c "echo \$\$ > $CG/cgroup.procs && exec \"\$@\"" _ "$@"; }

mkdir -p "$CG"
ACTIVE=False
grep -qw bpf /sys/kernel/security/lsm && ACTIVE=True
GUARD="{\"enabled\":true,\"extra\":[{\"name\":\"mnsmoke-qemu\",\"cgroup\":\"$CG_REL\"}],\"vms\":[\"mnsmoke-none\"]"

start_bpfd
check "lsm_active reported ($ACTIVE)" test "$(js '{"op":"guard_status"}' "d['lsm_active']")" = "$ACTIVE"
check "enforce without a lease refused" bash -c "$(declare -f req ok); SOCK=$SOCK; ! req '{\"op\":\"guard_configure\",\"config\":$GUARD,\"mode\":\"enforce\"}}' | ok"
check "lease over 3600s refused" bash -c "$(declare -f req ok); SOCK=$SOCK; ! req '{\"op\":\"guard_configure\",\"config\":$GUARD,\"mode\":\"enforce\",\"lease_secs\":7200}}' | ok"
check "bad mode refused" bash -c "$(declare -f req ok); SOCK=$SOCK; ! req '{\"op\":\"guard_configure\",\"config\":$GUARD,\"mode\":\"block\"}}' | ok"
if [[ $ACTIVE == False ]]; then
  check "lsm_inactive: refuses to enable" bash -c "$(declare -f req); SOCK=$SOCK; req '{\"op\":\"guard_configure\",\"config\":$GUARD}}' | grep -q lsm_inactive"
  stop_bpfd
  start_bpfd MACHINA_BPF_GUARD_ASSUME_LSM=1
  req "{\"op\":\"guard_configure\",\"config\":$GUARD}}" | ok || echo "WARN  audit configure failed: $(req "{\"op\":\"guard_configure\",\"config\":$GUARD}}")"
  check "hooks pass the verifier and attach" test "$(js '{"op":"guard_status"}' "len(d['hooks'])")" -eq 3
  check "inactive note shown" test "$(js '{"op":"guard_status"}' "any('lsm_inactive' in n for n in d['notes'])")" = True
  check "lsm_inactive: refuses to arm enforcement" bash -c "$(declare -f req); SOCK=$SOCK; req '{\"op\":\"guard_configure\",\"config\":$GUARD,\"mode\":\"enforce\",\"lease_secs\":30}}' | grep -q lsm_inactive"
else
  req "{\"op\":\"guard_configure\",\"config\":$GUARD}}" | ok
  check "hooks attached" test "$(js '{"op":"guard_status"}' "len(d['hooks'])")" -eq 3
fi
check "test cgroup guarded, real VMs untouched" test "$(js '{"op":"guard_status"}' "[g['name'] for g in d['guarded']]")" = "['mnsmoke-qemu']"
check "QEMU device defaults allowlisted (/dev/kvm)" test "$(js '{"op":"guard_status"}' "any(r.startswith('c 10:232') for r in d['allowed_devices'])")" = True

if [[ $ACTIVE == True ]]; then
  in_cg /bin/true || true
  in_cg python3 -c 'import mmap; mmap.mmap(-1, 4096, prot=mmap.PROT_READ|mmap.PROT_WRITE|mmap.PROT_EXEC)' || true
  sleep 0.5
  ev() { js '{"op":"guard_events","limit":200}' "sum(1 for r in d if r['hook']=='$1' and r['vm']=='mnsmoke-qemu' and not r['denied'])"; }
  check "audit: exec of non-allowlisted binary recorded" test "$(ev exec)" -ge 1
  check "audit: exec still ran" in_cg /bin/true
  req "{\"op\":\"guard_configure\",\"config\":$GUARD,\"mode\":\"enforce\",\"lease_secs\":5}}" | ok
  check "enforce: exec denied under lease" bash -c "$(declare -f in_cg); CG=$CG; ! in_cg /bin/true 2>/dev/null"
  sleep 7
  check "lease expired: back to audit, exec allowed" in_cg /bin/true
  check "lease expiry reported" test "$(js '{"op":"guard_status"}' "(d['config']['mode'], d['lease_expired'])")" = "('audit', True)"
else
  echo "SKIP  audit/enforce behaviour (needs lsm=...,bpf on the kernel command line)"
fi

req '{"op":"guard_configure","config":{}}' | ok
check "off: hooks detached" test "$(js '{"op":"guard_status"}' "(len(d['hooks']), len(d['guarded']))")" = "(0, 0)"
check "never persisted as enforce" bash -c "! grep -q '\"enforce\"' $WORK/bpfd-state.json"

echo
echo "passed=$PASS failed=$FAIL  (log: $WORK/bpfd.log)"
[[ $FAIL -eq 0 ]]
