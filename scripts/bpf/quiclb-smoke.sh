#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# QUIC CID load balancer smoke test. A client and two backends sit on a
# bridge inside a netns; the host end of one veth (mnq-lb) carries the uplink
# XDP dispatcher and stands in for the uplink. No real interface is touched.
# XDP runs in generic mode (MACHINA_BPF_XDP_SKB): native XDP_TX on a veth
# loses frames unless the peer runs NAPI.
#
#   sudo ./scripts/bpf/quiclb-smoke.sh [path/to/machina-bpfd]
set -euo pipefail

BPFD=${1:-./target/debug/machina-bpfd}
WORK=$(mktemp -d /tmp/mnquic-smoke.XXXX)
SOCK=$WORK/bpfd.sock
VIP=10.199.90.100
PORT=4433
PASS=0
FAIL=0
PIDS=()

cleanup() {
  [[ -n "${BPFD_PID:-}" ]] && kill "$BPFD_PID" 2>/dev/null && wait "$BPFD_PID" 2>/dev/null || true
  for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null || true; done
  for n in mnq-net mnq-c mnq-b1 mnq-b2; do ip netns del "$n" 2>/dev/null || true; done
  ip link del mnq-lb 2>/dev/null || true
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
js() { req '{"op":"quic_lb_status"}' | python3 -c "import json,sys; d=json.load(sys.stdin)['data']; print($1)"; }

# Topology: bridge br0 in mnq-net; host mnq-lb, client and backends attach.
ip netns add mnq-net
ip -n mnq-net link add br0 type bridge
ip -n mnq-net link set br0 up
ip link add mnq-lb type veth peer name mnq-lbp
ip link set mnq-lbp netns mnq-net
ip -n mnq-net link set mnq-lbp master br0 up
ip addr add 10.199.90.254/24 dev mnq-lb
ip link set mnq-lb up
i=0
for n in mnq-c mnq-b1 mnq-b2; do
  i=$((i + 1))
  ip netns add "$n"
  ip link add "$n-0" type veth peer name "$n-p"
  ip link set "$n-p" netns mnq-net
  ip -n mnq-net link set "$n-p" master br0 up
  ip link set "$n-0" netns "$n"
  ip -n "$n" link set lo up
  ip -n "$n" link set "$n-0" up
done
ip -n mnq-c addr add 10.199.90.1/24 dev mnq-c-0
ip -n mnq-b1 addr add 10.199.90.11/24 dev mnq-b1-0
ip -n mnq-b2 addr add 10.199.90.12/24 dev mnq-b2-0
for b in mnq-b1 mnq-b2; do
  ip -n "$b" addr add "$VIP/32" dev lo
  ip netns exec "$b" sysctl -qw net.ipv4.conf.all.arp_ignore=1 net.ipv4.conf.all.arp_announce=2 net.ipv4.conf.all.rp_filter=0
  # UDP listener: one line per datagram ("<tag>").
  ip netns exec "$b" python3 -u -c "
import socket, sys
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.bind(('0.0.0.0', $PORT))
while True:
    d, _ = s.recvfrom(2048)
    print(d.split(b'|')[-1].decode(errors='replace'), flush=True)
" >"$WORK/$b.udp" 2>&1 &
  PIDS+=($!)
  # IPIP sink: logs the tag of every encapsulated datagram.
  ip netns exec "$b" python3 -u -c "
import socket
s = socket.socket(socket.AF_INET, socket.SOCK_RAW, 4)
while True:
    d = s.recv(4096)
    print(d.split(b'|')[-1].decode(errors='replace'), flush=True)
" >"$WORK/$b.ipip" 2>&1 &
  PIDS+=($!)
done
LB_MAC=$(cat /sys/class/net/mnq-lb/address)
ip -n mnq-c neigh replace "$VIP" lladdr "$LB_MAC" dev mnq-c-0
ping -c1 -W1 -I mnq-lb 10.199.90.11 >/dev/null
ping -c1 -W1 -I mnq-lb 10.199.90.12 >/dev/null

# send <sport> <kind> <tag>: kind = short:<sid hex> | initial | unknown
send() {
  ip netns exec mnq-c python3 - "$VIP" "$PORT" "$@" <<'PY'
import os, socket, sys
vip, port, sport, kind, tag = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4], sys.argv[5]
if kind.startswith("short:"):
    sid = int(kind[6:], 16)
    pkt = bytes([0x40, 0x05, sid >> 8, sid & 0xff]) + os.urandom(5)
elif kind == "initial":
    pkt = bytes([0xc0, 0, 0, 0, 1, 8]) + b"\xee" * 8 + bytes([8]) + os.urandom(8)
else:
    pkt = bytes([0x40, 0x05, 0x7e, 0x7e]) + os.urandom(5)
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("0.0.0.0", sport))
s.sendto(pkt + b"|" + tag.encode(), (vip, port))
PY
}
got() { sleep 0.3; grep -qx "$2" "$WORK/$1"; }
not_got() { sleep 0.3; ! grep -qx "$2" "$WORK/$1"; }

echo '{"telemetry": {"iface_patterns": []}}' >"$WORK/bpfd-state.json"
MACHINA_BPF_XDP_SKB=1 RUST_LOG=${RUST_LOG:-info} "$BPFD" --socket "$SOCK" --state-dir "$WORK" --socket-group "" >>"$WORK/bpfd.log" 2>&1 &
BPFD_PID=$!
for _ in $(seq 50); do [[ -S "$SOCK" ]] && break; sleep 0.2; done
[[ -S "$SOCK" ]] || { echo "bpfd did not start"; cat "$WORK/bpfd.log"; exit 1; }

check "no LB: VIP unreachable" bash -c "$(declare -f send got not_got); VIP=$VIP PORT=$PORT WORK=$WORK; send 40000 short:0101 pre; not_got mnq-b1.udp pre && not_got mnq-b2.udp pre"
check "bad cid_len refused" bash -c "$(declare -f req ok); SOCK=$SOCK; ! req '{\"op\":\"quic_lb_configure\",\"config\":{\"iface\":\"mnq-lb\",\"vip\":\"$VIP\",\"port\":$PORT,\"cid_len\":2,\"backends\":[{\"addr\":\"10.199.90.11\"}]}}' | ok"
check "IPIP to IPv6 refused" bash -c "$(declare -f req ok); SOCK=$SOCK; ! req '{\"op\":\"quic_lb_configure\",\"config\":{\"iface\":\"mnq-lb\",\"vip\":\"fd00::1\",\"port\":$PORT,\"mode\":\"ipip\",\"backends\":[{\"addr\":\"fd00::2\",\"mac\":\"02:00:00:00:00:01\"}]}}' | ok"
SVC="\"iface\":\"mnq-lb\",\"vip\":\"$VIP\",\"port\":$PORT,\"backends\":[{\"addr\":\"10.199.90.11\",\"server_id\":257},{\"addr\":\"10.199.90.12\",\"server_id\":514}]"
check "DSR service configured (MACs from ARP)" bash -c "$(declare -f req ok); SOCK=$SOCK; req '{\"op\":\"quic_lb_configure\",\"config\":{$SVC}}' | ok"
check "dispatcher attached" test "$(js "d['attached']")" = True
check "XDP on mnq-lb" bash -c "ip link show mnq-lb 2>/dev/null | grep -q xdp"
check "short header sid 0x0101 → b1" bash -c "$(declare -f send got); VIP=$VIP PORT=$PORT WORK=$WORK; send 40001 short:0101 s1 && got mnq-b1.udp s1"
check "short header sid 0x0202 → b2" bash -c "$(declare -f send got); VIP=$VIP PORT=$PORT WORK=$WORK; send 40001 short:0202 s2 && got mnq-b2.udp s2"
for k in 1 2 3; do send 40100 initial "i$k"; done
sleep 0.3
B1=$(grep -c '^i[123]$' "$WORK/mnq-b1.udp" || true)
B2=$(grep -c '^i[123]$' "$WORK/mnq-b2.udp" || true)
check "Initials of one flow stick to one backend (Maglev)" bash -c "[[ ($B1 == 3 && $B2 == 0) || ($B1 == 0 && $B2 == 3) ]]"
send 40200 unknown u1
sleep 0.3
check "unknown server id still delivered" bash -c "grep -qx u1 $WORK/mnq-b1.udp || grep -qx u1 $WORK/mnq-b2.udp"
check "counters: cid/initial/unknown/tx" test "$(js "[d['services'][0][k] for k in ('routed_cid','initial','unknown_sid')] == [2,3,1] and d['services'][0]['tx'] >= 6")" = True
check "other traffic passes the dispatcher" bash -c "ip netns exec mnq-c ping -c1 -W1 10.199.90.254 >/dev/null"
check "IPIP service reconfigured" bash -c "$(declare -f req ok); SOCK=$SOCK; req '{\"op\":\"quic_lb_configure\",\"config\":{$SVC,\"mode\":\"ipip\"}}' | ok"
check "IPIP: sid 0x0202 encapsulated to b2" bash -c "$(declare -f send got); VIP=$VIP PORT=$PORT WORK=$WORK; send 40002 short:0202 e2 && got mnq-b2.ipip e2"
check "IPIP: b1 got nothing encapsulated" bash -c "! grep -qx e2 $WORK/mnq-b1.ipip"
check "service removed" bash -c "$(declare -f req ok); SOCK=$SOCK; req '{\"op\":\"quic_lb_configure\",\"config\":{\"vip\":\"$VIP\",\"port\":$PORT,\"enabled\":false}}' | ok"
check "dispatcher detached" bash -c "! ip link show mnq-lb 2>/dev/null | grep -q xdp"
check "no LB again: VIP unreachable" bash -c "$(declare -f send not_got); VIP=$VIP PORT=$PORT WORK=$WORK; send 40003 short:0101 post; not_got mnq-b1.udp post"

echo "passed=$PASS failed=$FAIL  (log: $WORK/bpfd.log)"
[[ $FAIL -eq 0 ]]
