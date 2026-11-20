#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# AF_XDP fast-path smoke test on a dedicated veth (mnx-in); a Python AF_XDP
# consumer registers its socket with bpfd over SCM_RIGHTS. XDP runs in generic
# mode (MACHINA_BPF_XDP_SKB). No real interface is touched.
#
#   sudo ./scripts/bpf/afxdp-smoke.sh [path/to/machina-bpfd]
set -euo pipefail

BPFD=${1:-./target/debug/machina-bpfd}
WORK=$(mktemp -d /tmp/mnxsk-smoke.XXXX)
SOCK=$WORK/bpfd.sock
HOST_IP=10.199.91.1
PEER_IP=10.199.91.2
PASS=0
FAIL=0
PIDS=()

cleanup() {
  [[ -n "${BPFD_PID:-}" ]] && kill "$BPFD_PID" 2>/dev/null && wait "$BPFD_PID" 2>/dev/null || true
  [[ -n "${CONS_PID:-}" ]] && kill "$CONS_PID" 2>/dev/null || true
  for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null || true; done
  ip netns del mnx 2>/dev/null || true
  ip link del mnx-in 2>/dev/null || true
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
js() { req '{"op":"afxdp_status"}' | python3 -c "import json,sys; d=json.load(sys.stdin)['data']; print($1)"; }

cat >"$WORK/consumer.py" <<'PY'
# Minimal AF_XDP consumer (copy mode, RX only): logs the tag after '|' of
# every frame it receives.
import array, ctypes, json, mmap, socket, struct, sys, time
SOL_XDP, XDP_MMAP_OFFSETS, XDP_RX_RING, XDP_UMEM_REG = 283, 1, 2, 4
XDP_UMEM_FILL_RING, XDP_UMEM_COMPLETION_RING, XDP_COPY = 5, 6, 2
NF, FS = 64, 4096
iface, queue, bpfd, out = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4]
libc = ctypes.CDLL(None, use_errno=True)
s = socket.socket(44, socket.SOCK_RAW, 0)
umem = mmap.mmap(-1, NF * FS, flags=mmap.MAP_PRIVATE | mmap.MAP_ANONYMOUS)
base = ctypes.addressof(ctypes.c_char.from_buffer(umem))
s.setsockopt(SOL_XDP, XDP_UMEM_REG, struct.pack("QQIIII", base, NF * FS, FS, 0, 0, 0))
for opt in (XDP_UMEM_FILL_RING, XDP_UMEM_COMPLETION_RING, XDP_RX_RING):
    s.setsockopt(SOL_XDP, opt, NF)
o = struct.unpack("16Q", s.getsockopt(SOL_XDP, XDP_MMAP_OFFSETS, 128))
rx_p, rx_c, rx_d = o[0], o[1], o[2]
fr_p, fr_d = o[8], o[10]
rx = mmap.mmap(s.fileno(), rx_d + NF * 16, mmap.MAP_SHARED, mmap.PROT_READ | mmap.PROT_WRITE, offset=0)
fr = mmap.mmap(s.fileno(), fr_d + NF * 8, mmap.MAP_SHARED, mmap.PROT_READ | mmap.PROT_WRITE, offset=0x100000000)
for i in range(NF):
    struct.pack_into("Q", fr, fr_d + i * 8, i * FS)
fill = NF
struct.pack_into("I", fr, fr_p, fill)
sa = struct.pack("HHIII", 44, XDP_COPY, socket.if_nametoindex(iface), queue, 0)
if libc.bind(s.fileno(), sa, len(sa)) != 0:
    sys.exit(f"bind: errno {ctypes.get_errno()}")
u = socket.socket(socket.AF_UNIX)
u.connect(bpfd)
msg = json.dumps({"op": "afxdp_register", "iface": iface, "queue": queue}).encode() + b"\n"
u.sendmsg([msg], [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array("i", [s.fileno()]))])
print(u.recv(65536).decode().strip(), flush=True)
u.close()
cons = 0
with open(out, "a", buffering=1) as f:
    while True:
        prod = struct.unpack_from("I", rx, rx_p)[0]
        while cons != prod:
            addr, length, _ = struct.unpack_from("QII", rx, rx_d + (cons % NF) * 16)
            f.write(umem[addr:addr + length].split(b"|")[-1].decode(errors="replace") + "\n")
            cons += 1
            struct.pack_into("I", rx, rx_c, cons)
            struct.pack_into("Q", fr, fr_d + (fill % NF) * 8, addr & ~(FS - 1))
            fill += 1
            struct.pack_into("I", fr, fr_p, fill)
        time.sleep(0.01)
PY

send() {
  ip netns exec mnx python3 -c "
import socket; s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.sendto(b'x|$1', ('$HOST_IP', 5555))"
}
stack_got() { sleep 0.4; grep -qx "$1" "$WORK/stack.log"; }
xsk_got() { sleep 0.4; grep -qx "$1" "$WORK/xsk.log" 2>/dev/null; }
start_consumer() {
  python3 "$WORK/consumer.py" mnx-in 0 "$SOCK" "$WORK/xsk.log" >"$WORK/consumer.out" 2>&1 &
  CONS_PID=$!
  for _ in $(seq 30); do [[ -s "$WORK/consumer.out" ]] && break; sleep 0.1; done
}

ip netns add mnx
ip link add mnx-in type veth peer name mnx-p
ip link set mnx-p netns mnx
ip addr add "$HOST_IP/30" dev mnx-in
ip link set mnx-in up
ip -n mnx addr add "$PEER_IP/30" dev mnx-p
ip -n mnx link set mnx-p up
ip -n mnx link set lo up
# The gate also steals ARP; pin the neighbour.
ip -n mnx neigh replace "$HOST_IP" lladdr "$(cat /sys/class/net/mnx-in/address)" dev mnx-p
python3 -u -c "
import socket; s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.bind(('$HOST_IP', 5555))
while True: print(s.recv(2048).split(b'|')[-1].decode(), flush=True)" >"$WORK/stack.log" 2>&1 &
PIDS+=($!)

echo '{"telemetry": {"iface_patterns": []}}' >"$WORK/bpfd-state.json"
MACHINA_BPF_XDP_SKB=1 RUST_LOG=${RUST_LOG:-info} "$BPFD" --socket "$SOCK" --state-dir "$WORK" --socket-group "" >>"$WORK/bpfd.log" 2>&1 &
BPFD_PID=$!
for _ in $(seq 50); do [[ -S "$SOCK" ]] && break; sleep 0.2; done
[[ -S "$SOCK" ]] || { echo "bpfd did not start"; cat "$WORK/bpfd.log"; exit 1; }

DEF=$(ip route show default | awk '{print $5; exit}')
if [[ -n "$DEF" ]]; then
  check "default-route interface refused ($DEF)" bash -c "$(declare -f req ok); SOCK=$SOCK; ! req '{\"op\":\"afxdp_configure\",\"config\":{\"iface\":\"$DEF\",\"enabled\":true}}' | ok"
fi
check "AF_XDP attached to mnx-in" bash -c "$(declare -f req ok); SOCK=$SOCK; req '{\"op\":\"afxdp_configure\",\"config\":{\"iface\":\"mnx-in\",\"enabled\":true}}' | ok"
check "no socket: frames reach the stack" bash -c "$(declare -f send stack_got); HOST_IP=$HOST_IP WORK=$WORK; send k1; stack_got k1"
check "register without an fd refused" bash -c "$(declare -f req); SOCK=$SOCK; req '{\"op\":\"afxdp_register\",\"iface\":\"mnx-in\",\"queue\":0}' | grep -q 'SCM_RIGHTS'"
check "non-XSK fd refused" python3 - "$SOCK" <<'PY'
import array, json, socket, sys
u = socket.socket(socket.AF_UNIX); u.connect(sys.argv[1])
d = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
u.sendmsg([b'{"op":"afxdp_register","iface":"mnx-in","queue":0}\n'], [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array("i", [d.fileno()]))])
r = json.loads(u.recv(65536))
sys.exit(0 if not r["ok"] and "not an AF_XDP" in r["error"] else 1)
PY
start_consumer
check "consumer registered over SCM_RIGHTS" bash -c "$(declare -f ok); ok <$WORK/consumer.out"
check "queue 0 gate open" test "$(js "[q['queue'] for q in d['queues'] if q['enabled']]")" = "[0]"
check "gate open: frame goes to AF_XDP" bash -c "$(declare -f send xsk_got); HOST_IP=$HOST_IP WORK=$WORK; send x1; xsk_got x1"
check "gate open: stack does not see it" bash -c "! grep -qx x1 $WORK/stack.log"
check "redirect counted" test "$(js "d['queues'][0]['redirected'] > 0")" = True
kill "$CONS_PID"; wait "$CONS_PID" 2>/dev/null || true; CONS_PID=
sleep 0.3
check "consumer gone: frames fall back to the stack" bash -c "$(declare -f send stack_got); HOST_IP=$HOST_IP WORK=$WORK; send x2; stack_got x2"
check "no_socket counted" test "$(js "d['queues'][0]['no_socket'] > 0")" = True
start_consumer
check "unregistered" bash -c "$(declare -f req ok); SOCK=$SOCK; req '{\"op\":\"afxdp_unregister\",\"iface\":\"mnx-in\",\"queue\":0}' | ok"
check "gate closed: stack gets frames again" bash -c "$(declare -f send stack_got); HOST_IP=$HOST_IP WORK=$WORK; send x3; stack_got x3"
check "gate closed: consumer gets nothing" bash -c "! grep -qx x3 $WORK/xsk.log"
check "disabled" bash -c "$(declare -f req ok); SOCK=$SOCK; req '{\"op\":\"afxdp_configure\",\"config\":{\"iface\":\"mnx-in\",\"enabled\":false}}' | ok"
check "XDP detached" bash -c "! ip link show mnx-in | grep -q xdp"

echo "passed=$PASS failed=$FAIL  (log: $WORK/bpfd.log)"
[[ $FAIL -eq 0 ]]
