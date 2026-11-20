#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# machina-cni smoke: the real CNI plugin wires three dual-stack netns "pods"
# against a private machina-bpfd, then checks routing, same-node redirect,
# NetworkPolicy, socket-LB services (Maglev, ClientIP affinity, IPv6) and
# NodePort (XDP fast path + SNAT to a remote backend). The "uplink" is a test
# veth pair whose far end is a netns playing client and remote node, so
# nothing is ever attached to a real NIC and an existing cluster CNI is left
# alone: only test subnets and test service VIPs are used.
#
#   sudo ./scripts/bpf/cni-smoke.sh [dir with machina-bpfd + machina-cni]
set -euo pipefail

BIN=$(cd "${1:-./target/release}" && pwd)
WORK=$(mktemp -d /tmp/mncni-smoke.XXXX)
export MACHINA_BPFD_SOCK=$WORK/bpfd.sock
SOCK=$MACHINA_BPFD_SOCK
SUBNET=10.199.90.0/24
SUBNET6=fd99:90::/64
VIP=10.199.88.10
VIP6=fd99:88::10
UP=mnsmk-up0
UP_PEER=mnsmk-up1
UP_ADDR=10.199.91.1
EXT_ADDR=10.199.91.2
UP_ADDR6=fd99:91::1
EXT_ADDR6=fd99:91::2
REMOTE_BE=10.199.92.5
NS_A=mncni-a
NS_B=mncni-b
NS_C=mncni-c
NS_EXT=mncni-ext
PASS=0
FAIL=0
SKIP=0

cleanup() {
  for ns in "$NS_A" "$NS_B" "$NS_C"; do
    [[ -e /var/run/netns/$ns ]] && cni DEL "$ns" >/dev/null 2>&1 || true
    ip netns del "$ns" 2>/dev/null || true
  done
  ip link del "$UP" 2>/dev/null || true
  ip route del 10.199.92.0/24 2>/dev/null || true
  ip netns del "$NS_EXT" 2>/dev/null || true
  [[ -n "${BPFD_PID:-}" ]] && kill "$BPFD_PID" 2>/dev/null && wait "$BPFD_PID" 2>/dev/null || true
}
trap cleanup EXIT

req() {
  python3 - "$SOCK" "$1" <<'PY'
import socket, sys
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
  python3 -c 'import json,sys; sys.exit(0 if json.loads(sys.argv[1]).get("ok") else 1)' "$out" \
    || echo "WARN  request failed: $out"
}
check() {
  local name=$1
  shift
  if "$@"; then
    echo "PASS  $name"
    PASS=$((PASS + 1))
  else
    echo "FAIL  $name"
    FAIL=$((FAIL + 1))
  fi
}
skip() {
  echo "SKIP  $1"
  SKIP=$((SKIP + 1))
}
status_field() {
  req '{"op":"cni_status"}' | python3 -c "import json,sys; d=json.load(sys.stdin)['data']; print($1)"
}

cni() {
  printf '{"cniVersion":"1.0.0","name":"smoke","type":"machina-cni","subnet":"%s","subnet6":"%s","ipam_dir":"%s"}' \
    "$SUBNET" "$SUBNET6" "$WORK/ipam" |
    CNI_COMMAND=$1 CNI_CONTAINERID="smoke-$2" CNI_NETNS=/var/run/netns/$2 CNI_IFNAME=eth0 \
      CNI_PATH="$BIN" CNI_ARGS="K8S_POD_NAMESPACE=smoke;K8S_POD_NAME=$2" "$BIN/machina-cni"
}
pod_ip() {
  python3 -c "import json,sys; print(next(i['address'].split('/')[0] for i in json.load(sys.stdin)['ips'] if (':' in i['address']) == $1))"
}
serve() { # netns name
  mkdir -p "$WORK/www-$2"
  echo "$2" >"$WORK/www-$2/who"
  ip netns exec "$1" python3 -m http.server 8080 --bind :: --directory "$WORK/www-$2" >/dev/null 2>&1 &
}

cat >"$WORK/bpfd-state.json" <<EOF
{"telemetry": {"exec": false, "connect": false, "flows": false, "dns": false,
               "iface_patterns": ["mncni-none"]}}
EOF
RUST_LOG=${RUST_LOG:-info} "$BIN/machina-bpfd" --socket "$SOCK" --state-dir "$WORK" --socket-group "" \
  >"$WORK/bpfd.log" 2>&1 &
BPFD_PID=$!
for _ in $(seq 50); do [[ -S "$SOCK" ]] && break; sleep 0.2; done
[[ -S "$SOCK" ]] || { echo "bpfd failed to start:"; cat "$WORK/bpfd.log"; exit 1; }

for ns in "$NS_A" "$NS_B" "$NS_C"; do ip netns add "$ns"; done
RES_A=$(cni ADD "$NS_A")
RES_B=$(cni ADD "$NS_B")
RES_C=$(cni ADD "$NS_C")
A=$(pod_ip False <<<"$RES_A")
B=$(pod_ip False <<<"$RES_B")
C=$(pod_ip False <<<"$RES_C")
A6=$(pod_ip True <<<"$RES_A")
B6=$(pod_ip True <<<"$RES_B")
C6=$(pod_ip True <<<"$RES_C")
echo "      pod a=$A/$A6 b=$B/$B6 c=$C/$C6"
check "plugin ADD assigns addresses" test -n "$A" -a -n "$B" -a "$A" != "$B"
check "plugin ADD assigns IPv6 addresses" test -n "$A6" -a -n "$B6" -a "$A6" != "$B6"
check "pod has default route via link-local gateway" bash -c "ip netns exec $NS_A ip route | grep -q 'default via 169.254.1.1'"
check "pod has IPv6 default route via fe80::1" bash -c "ip netns exec $NS_A ip -6 route | grep -q 'default via fe80::1'"

NODE_ADDR=$(ip -4 route get "$A" | sed -n 's/.* src \([0-9.]*\).*/\1/p')
req "{\"op\":\"cni_configure\",\"node_addr\":\"$NODE_ADDR\",\"uplink\":null}" | must
check "bpfd lists one endpoint per pod address" bash -c "[[ \$($(declare -f req status_field); SOCK=$SOCK; status_field 'len(d[\"endpoints\"])') == 6 ]]"

serve "$NS_B" b
serve "$NS_C" c
sleep 0.8

ping_ab() { ip netns exec "$NS_A" ping -c1 -W1 "$B" >/dev/null 2>&1; }
http_ab() { ip netns exec "$NS_A" curl -s -m2 -o /dev/null "http://$B:8080/who"; }
ping_hb() { ping -c1 -W1 "$B" >/dev/null 2>&1; }
ping6_ab() { ip netns exec "$NS_A" ping -6 -c1 -W1 "$B6" >/dev/null 2>&1; }
http6_ab() { ip netns exec "$NS_A" curl -s -m2 -o /dev/null "http://[$B6]:8080/who"; }
ping6_hb() { ping -6 -c1 -W1 "$B6" >/dev/null 2>&1; }
check "host -> pod ping" ping_hb
check "pod -> pod ping (bpf redirect)" ping_ab
check "pod -> pod http" http_ab
check "host -> pod ping6" ping6_hb
check "pod -> pod ping6 (bpf redirect)" ping6_ab
check "pod -> pod http over IPv6" http6_ab

# bpfd reads one request per line.
ident() { # ip id ingress egress
  printf '{"ip":"%s","identity":%s,"ingress_isolated":%s,"egress_isolated":%s}' "$1" "$2" "$3" "$4"
}
cni_state() {
  local ids
  ids="$(ident "$A" 1001 false "${A_EGRESS:-false}"),$(ident "$A6" 1001 false "${A_EGRESS:-false}")"
  ids+=",$(ident "$B" 1002 "${B_INGRESS:-false}" false),$(ident "$B6" 1002 "${B_INGRESS:-false}" false)"
  ids+=",$(ident "$C" 1003 false false),$(ident "$C6" 1003 false false)"
  req "{\"op\":\"cni_sync\",\"state\":{\"version\":2,\"identities\":[$ids],\"policy\":[${POLICY:-}],\"cidrs\":[],\"services\":[${SERVICES:-}]}}" | must
}
check "state with a foreign ABI version is rejected" bash -c "$(declare -f req); SOCK=$SOCK; req '{\"op\":\"cni_sync\",\"state\":{\"version\":1,\"identities\":[],\"policy\":[],\"cidrs\":[],\"services\":[]}}' | grep -q '\"ok\":false'"

B_INGRESS=true cni_state
check "default-deny ingress: pod -> pod http blocked" bash -c "$(declare -f http_ab); NS_A=$NS_A B=$B; ! http_ab"
check "default-deny ingress: pod -> pod ping blocked" bash -c "$(declare -f ping_ab); NS_A=$NS_A B=$B; ! ping_ab"
check "default-deny ingress: IPv6 http blocked" bash -c "$(declare -f http6_ab); NS_A=$NS_A B6=$B6; ! http6_ab"
check "default-deny ingress: host (kubelet) still allowed" ping_hb

ALLOW_B='{"subject":1002,"peer":1001,"egress":false,"proto":6,"port":8080}'
B_INGRESS=true POLICY=$ALLOW_B cni_state
check "ingress allow tcp/8080: http passes" http_ab
check "ingress allow tcp/8080: IPv6 http passes" http6_ab
check "ingress allow tcp/8080: ping still blocked" bash -c "$(declare -f ping_ab); NS_A=$NS_A B=$B; ! ping_ab"
check "ingress allow tcp/8080: ping6 still blocked" bash -c "$(declare -f ping6_ab); NS_A=$NS_A B6=$B6; ! ping6_ab"

A_EGRESS=true B_INGRESS=true POLICY=$ALLOW_B cni_state
check "default-deny egress on a: http blocked" bash -c "$(declare -f http_ab); NS_A=$NS_A B=$B; ! http_ab"
ALLOW_A='{"subject":1001,"peer":1002,"egress":true,"proto":6,"port":8080}'
A_EGRESS=true B_INGRESS=true POLICY="$ALLOW_B,$ALLOW_A" cni_state
check "egress + ingress allow: http passes" http_ab

be() { printf '{"addr":"%s","port":8080}' "$1"; }
SVC="{\"addr\":\"$VIP\",\"port\":80,\"proto\":6,\"backends\":[$(be "$B")],\"name\":\"smoke/web\"}"
B_INGRESS=true POLICY=$ALLOW_B SERVICES=$SVC cni_state
check "service VIP from host (socket LB)" curl -s -m2 -o /dev/null "http://$VIP/who"
check "service VIP from pod (socket LB + policy)" ip netns exec "$NS_A" curl -s -m2 -o /dev/null "http://$VIP/who"
SVC6="{\"addr\":\"$VIP6\",\"port\":80,\"proto\":6,\"backends\":[$(be "$B6")],\"name\":\"smoke/web6\"}"
B_INGRESS=true POLICY=$ALLOW_B SERVICES="$SVC,$SVC6" cni_state
check "IPv6 service VIP from host (connect6)" curl -s -m2 -o /dev/null "http://[$VIP6]/who"
check "IPv6 service VIP from pod" ip netns exec "$NS_A" curl -s -m2 -o /dev/null "http://[$VIP6]/who"
B_INGRESS=true POLICY=$ALLOW_B cni_state
check "service removed: VIP unreachable" bash -c "! curl -s -m1 -o /dev/null http://$VIP/"

who_set() { # url count -> sorted distinct backend names
  for _ in $(seq "$2"); do curl -s -m2 "$1" || true; done | sort -u | tr '\n' ' '
}
MAGLEV="{\"addr\":\"$VIP\",\"port\":80,\"proto\":6,\"backends\":[$(be "$B"),$(be "$C")],\"name\":\"smoke/lb\"}"
SERVICES=$MAGLEV cni_state
check "multi-backend service gets a Maglev table" bash -c "[[ \$($(declare -f req status_field); SOCK=$SOCK; status_field 'd.get(\"maglev_services\",0)') -ge 1 ]]"
SEEN=$(who_set "http://$VIP/who" 30)
echo "      maglev spread over 30 connections: $SEEN"
check "Maglev spreads connections over both backends" test "$SEEN" = "b c "
AFF="{\"addr\":\"$VIP\",\"port\":80,\"proto\":6,\"backends\":[$(be "$B"),$(be "$C")],\"name\":\"smoke/lb\",\"affinity_secs\":60}"
SERVICES=$AFF cni_state
SEEN=$(who_set "http://$VIP/who" 20)
echo "      affinity over 20 connections: $SEEN"
check "ClientIP affinity pins one client to one backend" test "$(wc -w <<<"$SEEN")" -eq 1

# ---- NodePort over a test uplink -----------------------------------------
ip netns add "$NS_EXT"
ip link add "$UP" type veth peer name "$UP_PEER"
ip link set "$UP_PEER" netns "$NS_EXT"
ip addr add "$UP_ADDR/24" dev "$UP"
ip -6 addr add "$UP_ADDR6/64" dev "$UP" nodad
ip link set "$UP" up
ip netns exec "$NS_EXT" sh -c "ip addr add $EXT_ADDR/24 dev $UP_PEER && ip -6 addr add $EXT_ADDR6/64 dev $UP_PEER nodad \
  && ip link set $UP_PEER up && ip link set lo up && ip addr add $REMOTE_BE/32 dev lo \
  && ip route add default via $UP_ADDR && ip -6 route add default via $UP_ADDR6"
ip route replace 10.199.92.0/24 via "$EXT_ADDR" dev "$UP"
serve "$NS_EXT" remote
sleep 0.8

req "{\"op\":\"cni_configure\",\"node_addr\":\"$UP_ADDR\",\"node_addr6\":\"$UP_ADDR6\",\"uplink\":\"$UP\",\"xdp\":true}" | must
check "XDP dispatcher attached to the uplink" bash -c "ip -d link show $UP | grep -q xdp"
check "cni_status reports xdp + node_addr6" bash -c "[[ \$($(declare -f req status_field); SOCK=$SOCK; status_field 'str(d[\"xdp\"])+\",\"+str(d[\"node_addr6\"])') == True,$UP_ADDR6 ]]"

NP="{\"addr\":\"0.0.0.0\",\"port\":30999,\"proto\":6,\"backends\":[$(be "$B")],\"name\":\"smoke/np\"}"
NP6="{\"addr\":\"::\",\"port\":30999,\"proto\":6,\"backends\":[$(be "$B6")],\"name\":\"smoke/np6\"}"
NPR="{\"addr\":\"0.0.0.0\",\"port\":30998,\"proto\":6,\"backends\":[{\"addr\":\"$REMOTE_BE\",\"port\":8080,\"remote\":true,\"node\":\"$EXT_ADDR\"}],\"name\":\"smoke/np-remote\"}"
SERVICES="$NP,$NP6,$NPR" cni_state

# A raw SYN from the "outside" netns: cgroup socket-LB hooks see every
# connect() on the host whatever its netns, so only a raw socket exercises
# the uplink XDP/tc path. Passes when the SYN-ACK comes back from the
# frontend, i.e. DNAT/SNAT forward and reverse NAT both worked.
syn_probe() { # src dst port
  ip netns exec "$NS_EXT" python3 - "$@" <<'PY'
import ipaddress, random, socket, struct, sys, time
src, dst, port = sys.argv[1], sys.argv[2], int(sys.argv[3])
v6 = ":" in dst
fam = socket.AF_INET6 if v6 else socket.AF_INET
s = socket.socket(fam, socket.SOCK_RAW, socket.IPPROTO_TCP)
s.settimeout(0.5)
sport = random.randint(40000, 60000)
hdr = struct.pack("!HHIIBBHHH", sport, port, random.getrandbits(32), 0, 5 << 4, 0x02, 64240, 0, 0)
sa, da = ipaddress.ip_address(src).packed, ipaddress.ip_address(dst).packed
pseudo = sa + da + (struct.pack("!I", len(hdr)) + b"\0\0\0\x06" if v6 else struct.pack("!BBH", 0, 6, len(hdr)))
data = pseudo + hdr
c = sum(struct.unpack("!%dH" % (len(data) // 2), data))
while c >> 16:
    c = (c & 0xFFFF) + (c >> 16)
hdr = hdr[:16] + struct.pack("!H", ~c & 0xFFFF) + hdr[18:]
s.sendto(hdr, (dst, 0))
end = time.time() + 3
while time.time() < end:
    try:
        pkt, addr = s.recvfrom(65535)
    except socket.timeout:
        continue
    tcp = pkt if v6 else pkt[(pkt[0] & 0xF) * 4:]
    sp, dp = struct.unpack("!HH", tcp[:4])
    if addr[0] == dst and sp == port and dp == sport and tcp[13] & 0x12 == 0x12:
        sys.exit(0)
sys.exit(1)
PY
}
xdp_runs() { # tail-called programs are counted on the dispatcher
  bpftool prog show name mn_xdp_uplink -j 2>/dev/null | python3 -c 'import json,sys
try: d=json.load(sys.stdin)
except Exception: d=[]
d=d if isinstance(d,list) else [d]
print(sum(p.get("run_cnt",0) for p in d))'
}
STATS_WAS=$(cat /proc/sys/kernel/bpf_stats_enabled 2>/dev/null || echo 0)
echo 1 >/proc/sys/kernel/bpf_stats_enabled 2>/dev/null || true
XDP_BEFORE=$(xdp_runs)
check "NodePort -> local backend (raw SYN, DNAT + reverse NAT)" syn_probe "$EXT_ADDR" "$UP_ADDR" 30999
XDP_AFTER=$(xdp_runs)
echo "$STATS_WAS" >/proc/sys/kernel/bpf_stats_enabled 2>/dev/null || true
if command -v bpftool >/dev/null; then
  check "NodePort SYN went through the XDP fast path (run_cnt $XDP_BEFORE -> $XDP_AFTER)" test "$XDP_AFTER" -gt "$XDP_BEFORE"
else
  skip "XDP run count (no bpftool)"
fi
if [[ $(cat /proc/sys/net/ipv6/conf/all/forwarding) == 1 ]]; then
  check "IPv6 NodePort -> local backend (raw SYN)" syn_probe "$EXT_ADDR6" "$UP_ADDR6" 30999
else
  skip "IPv6 NodePort (host IPv6 forwarding is off; the smoke does not enable it)"
fi
check "NodePort -> remote backend (SNAT via node address, raw SYN)" syn_probe "$EXT_ADDR" "$UP_ADDR" 30998
check "unknown NodePort gets no SYN-ACK" bash -c "$(declare -f syn_probe); NS_EXT=$NS_EXT; ! syn_probe $EXT_ADDR $UP_ADDR 30997"
check "NodePort from a host-local client (socket LB)" test "$(ip netns exec "$NS_EXT" curl -s -m3 "http://$UP_ADDR:30999/who")" = b

HOST_A=$(python3 -c 'import json,sys; print(json.load(sys.stdin)["interfaces"][0]["name"])' <<<"$RES_A")
cni DEL "$NS_A" >/dev/null
cni DEL "$NS_A" >/dev/null
check "DEL removes host veth (idempotent)" bash -c "! ip link show $HOST_A >/dev/null 2>&1"
check "DEL unregisters both endpoints" bash -c "[[ \$($(declare -f req status_field); SOCK=$SOCK; status_field 'len(d[\"endpoints\"])') == 4 ]]"
check "DEL releases addresses" bash -c "! grep -rqs smoke-$NS_A $WORK/ipam"

echo
echo "passed=$PASS failed=$FAIL skipped=$SKIP  (log: $WORK/bpfd.log)"
grep -E "WARN|ERROR" "$WORK/bpfd.log" | head -20 || true
if [[ $FAIL -ne 0 ]]; then
  echo "--- notes"; req '{"op":"status"}' | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["notes"])'
fi
[[ $FAIL -eq 0 ]]
