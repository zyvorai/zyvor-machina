#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# VM edge + QEMU sandbox smoke test for machina-bpfd.
#
# A veth pair stands in for a VM tap (named explicitly in the edge state, so
# libvirt discovery and real vnet/tap devices are never involved) and a
# throwaway cgroup stands in for a machine-qemu scope. bpfd runs on a private
# socket/state dir with auto-attach limited to the test veth.
#
#   sudo ./scripts/bpf/vm-edge-smoke.sh [path/to/machina-bpfd]
set -euo pipefail

BPFD=${1:-./target/debug/machina-bpfd}
NS=mnvme-smoke
HOST_IF=mnvme-a0
PEER_IF=mnvme-a0p
HOST_IP=10.199.81.1
VM_IP=10.199.81.2
VM=mnsmk-vm
CG_REL=mnsmk-sandbox
CG=/sys/fs/cgroup/$CG_REL
WORK=$(mktemp -d /tmp/mnvme-smoke.XXXX)
SOCK=$WORK/bpfd.sock
PASS=0
FAIL=0

cleanup() {
  [[ -n "${BPFD_PID:-}" ]] && kill "$BPFD_PID" 2>/dev/null && wait "$BPFD_PID" 2>/dev/null || true
  [[ -n "${HTTP_PID:-}" ]] && kill "$HTTP_PID" 2>/dev/null || true
  [[ -n "${DNS_PID:-}" ]] && kill "$DNS_PID" 2>/dev/null || true
  for p in "${TLS_PID:-}" "${KAFKA_PID:-}" "${VMHTTP_PID:-}"; do [[ -n "$p" ]] && kill "$p" 2>/dev/null || true; done
  pkill -f "http.server 18090" 2>/dev/null || true
  ip netns del "$NS" 2>/dev/null || true
  ip link del "$HOST_IF" 2>/dev/null || true
  [[ -d "$CG" ]] && rmdir "$CG" 2>/dev/null || true
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
    c = s.recv(1 << 20)
    if not c:
        break
    buf += c
print(buf.decode().strip())
PY
}

must() {
  local out
  out=$(cat)
  if ! python3 -c 'import json,sys; sys.exit(0 if json.load(sys.stdin).get("ok") else 1)' <<<"$out"; then
    echo "WARN  request failed: $out"
  fi
}

# jpath OP EXPR: evaluate a Python expression over the response data `d`.
jpath() {
  req "{\"op\":\"$1\"}" | python3 -c "import json,sys; r=json.load(sys.stdin); d=r.get('data', r); print($2)"
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

edge() { req "{\"op\":\"vm_edge_sync\",\"state\":$1}" | must; }
enforce() { req '{"op":"set_mode","mode":"enforce","lease_secs":60}' | must; }
observe() { req '{"op":"set_mode","mode":"observe"}' | must; }
tap_stat() { jpath vm_edge_status "sum(t['stats']['$1'] for t in d['taps'])"; }
gt() { [[ "$1" -gt "$2" ]]; }
not() { ! "$@"; }
auth_has() { req '{"op":"vm_auth_table"}' | grep -q "$1"; }

ping_ok() { ip netns exec "$NS" ping -c1 -W1 "$HOST_IP" >/dev/null 2>&1; }
ping_blocked() { ! ping_ok; }
http_ok() { ip netns exec "$NS" curl -s -m3 -o /dev/null "http://$HOST_IP:18080/"; }

ip netns add "$NS"
ip link add "$HOST_IF" type veth peer name "$PEER_IF"
ip link set "$PEER_IF" netns "$NS"
ip addr add "$HOST_IP/30" dev "$HOST_IF"
ip link set "$HOST_IF" up
ip netns exec "$NS" ip addr add "$VM_IP/30" dev "$PEER_IF"
ip netns exec "$NS" ip link set "$PEER_IF" up
ip netns exec "$NS" ip link set lo up

head -c 393216 /dev/urandom >"$WORK/blob"
(cd "$WORK" && exec python3 -m http.server 18080 --bind "$HOST_IP" >/dev/null 2>&1) &
HTTP_PID=$!

cat >"$WORK/bpfd-state.json" <<EOF
{"telemetry": {"flows": true, "iface_patterns": ["$HOST_IF"]}}
EOF

RUST_LOG=${RUST_LOG:-info} "$BPFD" --socket "$SOCK" --state-dir "$WORK" --socket-group "" \
  >"$WORK/bpfd.log" 2>&1 &
BPFD_PID=$!
for _ in $(seq 50); do [[ -S "$SOCK" ]] && break; sleep 0.2; done
if [[ ! -S "$SOCK" ]]; then
  echo "bpfd failed to start:"
  cat "$WORK/bpfd.log"
  exit 1
fi
sleep 0.5

# ---- VM edge ----------------------------------------------------------------

VMSPEC="{\"name\":\"$VM\",\"addresses\":[\"$VM_IP\"],\"taps\":[\"$HOST_IF\"]"
edge "{\"vms\":[$VMSPEC}]}"
check "edge: tap programmed" [ "$(jpath vm_edge_status "len(d['taps'])")" = 1 ]
check "edge: edge programs chained on tap" bash -c "tc filter show dev $HOST_IF ingress 2>/dev/null | grep -q mn_vm_edge_in || bpftool net show dev $HOST_IF 2>/dev/null | grep -q mn_vm_edge_in"
check "edge: baseline ping" ping_ok
check "edge: baseline http" http_ok
check "edge: counters move" gt "$(tap_stat out_pkts)" 0

edge "{\"vms\":[$VMSPEC,\"isolate_egress\":true}]}"
check "observe: isolation does not block" ping_ok
check "observe: miss counted" gt "$(tap_stat observed)" 0

enforce
check "enforce: isolated VM cannot ping" ping_blocked
http_blocked() { ! http_ok; }
check "enforce: isolated VM cannot reach http" http_blocked
check "enforce: denials counted" gt "$(tap_stat denied)" 0

edge "{\"vms\":[$VMSPEC,\"isolate_egress\":true}],\"policy\":[{\"group\":\"vm:$VM\",\"peer\":\"world\",\"egress\":true,\"proto\":6,\"port\":18080}]}"
check "enforce: allow rule opens tcp/18080" http_ok
check "enforce: ping still blocked" ping_blocked
check "edge: status reports enforcing" [ "$(jpath vm_edge_status "d['enforcing']")" = True ]

observe
check "observe again: ping restored" ping_ok

edge "{\"vms\":[$VMSPEC,\"ingress_mbps\":1}]}"
SPEED=$(ip netns exec "$NS" curl -s -m25 -o /dev/null -w '%{speed_download}' "http://$HOST_IP:18080/blob" || echo 0)
SPEED=${SPEED%.*}
echo "      download with 1 Mbit/s cap: ${SPEED} B/s"
check "rate: ingress cap holds (< 250 kB/s)" bash -c "[ ${SPEED:-0} -gt 0 ] && [ ${SPEED:-0} -lt 250000 ]"
check "rate: drops counted" gt "$(tap_stat rate_dropped)" 0

edge '{"vms":[]}'
check "edge: empty state unprograms tap" [ "$(jpath vm_edge_status "len(d['taps'])")" = 0 ]
check "edge: no edge program left" bash -c "! bpftool net show dev $HOST_IF 2>/dev/null | grep -q mn_vm_edge"
check "edge: unrestricted after unprogram" ping_ok

# ---- VM network policy (compiled identity rules) ------------------------------
# Same shape the netpol compiler emits: per-VM identity, peers by address or
# prefix, identity rules with ranges / deny / ICMP type+1, flow log on.

VMID=5000
CIDR_ID=2147483700
NPVM="{\"name\":\"$VM\",\"addresses\":[\"$VM_IP\"],\"taps\":[\"$HOST_IF\"],\"identity\":$VMID,\"isolate_egress\":true"
HOSTPEER="{\"cidr\":\"$HOST_IP\",\"identity\":1,\"name\":\"host\"}"
R_RANGE="{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":true,\"proto\":6,\"port\":18070,\"port_end\":18090,\"source\":\"smoke spec.egress[0]\"}"
R_ICMP="{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":true,\"proto\":1,\"port\":9,\"source\":\"smoke spec.egress[1]\"}"
R_DENY="{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":true,\"proto\":6,\"port\":18080,\"deny\":true,\"source\":\"smoke spec.egressDeny[0]\"}"
R_CIDR="{\"subject_identity\":$VMID,\"peer_identity\":$CIDR_ID,\"egress\":true,\"proto\":6,\"port\":18080,\"source\":\"smoke spec.egress[2]\"}"
np() { edge "{\"vms\":[$1}],\"peers\":[$2],\"policy\":[$3],\"flow_log\":true,\"owner\":\"smoke\"}"; }
flow_has() {
  sleep 0.5
  req '{"op":"vm_flows","limit":1000}' | python3 -c "import json,sys; d=json.load(sys.stdin).get('data') or []; sys.exit(0 if any($1 for f in d) else 1)"
}
host_ping_vm() { ping -c1 -W1 "$VM_IP" >/dev/null 2>&1; }

np "$NPVM" "$HOSTPEER" "$R_RANGE"
check "netpol: identity + peers reported" [ "$(jpath vm_edge_status "(d['owner'], d['peers'], d['flow_log'])")" = "('smoke', 1, True)" ]
check "netpol observe: default-deny miss still forwards" ping_ok
check "netpol observe: AUDIT flow for the miss" flow_has "f['verdict']=='AUDIT' and f['proto']=='icmp' and f['drop_reason']=='default-deny'"

enforce
check "netpol enforce: port range allows tcp/18080" http_ok
check "netpol enforce: FORWARDED flow attributed to rule" flow_has "f['verdict']=='FORWARDED' and f['dst_port']==18080 and f.get('policy')=='smoke spec.egress[0]'"
check "netpol enforce: ICMP not allowed yet" ping_blocked
check "netpol enforce: DROPPED flow for ICMP" flow_has "f['verdict']=='DROPPED' and f['proto']=='icmp' and f.get('icmp_type')==8"

np "$NPVM" "$HOSTPEER" "$R_RANGE,$R_ICMP"
check "netpol enforce: ICMP echo-request rule allows ping" ping_ok
edge "{\"vms\":[$NPVM}],\"policy\":[$R_RANGE,$R_ICMP],\"flow_log\":true,\"owner\":\"smoke\"}"
check "netpol enforce: without a host peer the host address is not host" ping_blocked
edge "{\"vms\":[$NPVM}],\"policy\":[$R_RANGE,$R_ICMP],\"flow_log\":true,\"owner\":\"smoke\",\"node_is_host\":true}"
check "netpol enforce: node_is_host makes node addresses host" ping_ok

np "$NPVM" "$HOSTPEER" "$R_RANGE,$R_ICMP,$R_DENY"
check "netpol enforce: deny beats the allow range" bash -c "! ip netns exec $NS curl -s -m3 -o /dev/null http://$HOST_IP:18080/"
check "netpol enforce: policy-deny flow" flow_has "f['verdict']=='DROPPED' and f.get('drop_reason')=='policy-deny' and f.get('policy')=='smoke spec.egressDeny[0]'"

np "$NPVM" "{\"cidr\":\"10.199.81.0/30\",\"identity\":$CIDR_ID,\"name\":\"10.199.81.0/30\"}" "$R_CIDR"
check "netpol enforce: CIDR peer via LPM allows tcp/18080" http_ok
check "netpol enforce: host identity rules no longer match (ping blocked)" ping_blocked

NPVM_OPEN="{\"name\":\"$VM\",\"addresses\":[\"$VM_IP\"],\"taps\":[\"$HOST_IF\"],\"identity\":$VMID"
np "$NPVM_OPEN" "$HOSTPEER" "{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":false,\"proto\":1,\"port\":0,\"deny\":true,\"source\":\"smoke spec.ingressDeny[0]\"}"
check "netpol enforce: ingressDeny applies without isolation" bash -c "! ping -c1 -W1 $VM_IP >/dev/null 2>&1"
check "netpol enforce: VM egress unaffected by ingress deny" http_ok

# toFQDNs: a stub resolver on the host answers svc.smoke.test → HOST_IP. No
# host peer, so HOST_IP starts as `world` and is learned as its own identity.
python3 - "$HOST_IP" >"$WORK/dns.log" 2>&1 <<'PY' &
import socket, struct, sys
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.bind((sys.argv[1], 53))
ip = socket.inet_aton(sys.argv[1])
while True:
    q, a = s.recvfrom(512)
    i, name = 12, []
    while q[i]:
        name.append(q[i + 1:i + 1 + q[i]].decode()); i += q[i] + 1
    end = i + 5
    ok = ".".join(name).lower() == "svc.smoke.test"
    hdr = q[:2] + struct.pack(">HHHHH", 0x8180 if ok else 0x8183, 1, int(ok), 0, 0)
    ans = (b"\xc0\x0c" + struct.pack(">HHIH", 1, 1, 5, 4) + ip) if ok else b""
    s.sendto(hdr + q[12:end] + ans, a)
PY
DNS_PID=$!
resolve() {
  ip netns exec "$NS" python3 - "$HOST_IP" "$1" <<'PY'
import socket, struct, sys
q = struct.pack(">HHHHHH", 0x4d4e, 0x0100, 1, 0, 0, 0)
q += b"".join(bytes([len(p)]) + p.encode() for p in sys.argv[2].split(".")) + b"\x00" + struct.pack(">HH", 1, 1)
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.settimeout(2)
s.sendto(q, (sys.argv[1], 53)); r = s.recv(512)
sys.exit(0 if struct.unpack(">H", r[6:8])[0] > 0 else 1)
PY
}
fqdn_has() { req '{"op":"vm_fqdn_cache"}' | python3 -c "import json,sys; d=json.load(sys.stdin).get('data') or []; sys.exit(0 if any($1 for e in d) else 1)"; }
R_DNS="{\"subject_identity\":$VMID,\"peer_identity\":2,\"egress\":true,\"proto\":17,\"port\":53,\"source\":\"smoke spec.egress[3]\"}"
F_SVC="{\"pattern\":\"svc.smoke.test\",\"subject_identity\":$VMID,\"proto\":6,\"port\":18080,\"source\":\"smoke spec.egress[4]\"}"
npf() { edge "{\"vms\":[$NPVM}],\"policy\":[$1],\"fqdn\":[$2],\"flow_log\":true,\"owner\":\"smoke\"}"; }
sleep 0.3
npf "$R_DNS" "$F_SVC"
check "fqdn: status counts the rule, tap snoops DNS" [ "$(jpath vm_edge_status "(d['fqdn_rules'], 'fqdn' in d['taps'][0]['flags'])")" = "(1, True)" ]
check "fqdn enforce: blocked before the lookup" bash -c "! ip netns exec $NS curl -s -m2 -o /dev/null http://$HOST_IP:18080/"
check "fqdn enforce: DNS to world allowed" resolve svc.smoke.test
sleep 1.5
check "fqdn: binding learned from the reply" fqdn_has "e['name']=='svc.smoke.test' and e['address']=='$HOST_IP' and e['identity']>=2**31 and e['vm']=='$VM'"
check "fqdn enforce: allowed after the lookup" http_ok
check "fqdn enforce: flow attributed to the toFQDNs rule" flow_has "f['verdict']=='FORWARDED' and f['dst_port']==18080 and f.get('policy')=='smoke spec.egress[4]'"
resolve other.smoke.test || true
sleep 1
check "fqdn: unmatched / NXDOMAIN names learn nothing" [ "$(req '{"op":"vm_fqdn_cache"}' | python3 -c "import json,sys; print(len(json.load(sys.stdin).get('data') or []))")" = 1 ]
R_WDENY="{\"subject_identity\":$VMID,\"peer_identity\":2,\"egress\":true,\"proto\":6,\"port\":18080,\"deny\":true,\"source\":\"smoke spec.egressDeny[1]\"}"
npf "$R_DNS,$R_WDENY" "$F_SVC"
check "fqdn enforce: world deny still wins for a learned address" bash -c "! ip netns exec $NS curl -s -m2 -o /dev/null http://$HOST_IP:18080/"
npf "$R_DNS" ""
check "fqdn: removing the rule drops the binding" bash -c "! ip netns exec $NS curl -s -m2 -o /dev/null http://$HOST_IP:18080/"
kill "$DNS_PID" 2>/dev/null || true

# ---- L7 (native, bpfd-parsed) ----------------------------------------------
# Requests on L7 ports are held in the kernel until bpfd decides; denied
# HTTP gets a 403 from bpfd, other protocols a RST, DNS a REFUSED.

echo allowed >"$WORK/allowed.txt"
echo secret >"$WORK/secret.txt"
HOSTPEER_W="{\"cidr\":\"$HOST_IP\",\"identity\":1,\"name\":\"host\"}"
R_L7="{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":true,\"proto\":6,\"port\":18080,\"l7\":true,\"source\":\"smoke spec.egress[5]\"}"
L7_HTTP="{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":true,\"proto\":6,\"port\":18080,\"rules\":{\"http\":[{\"method\":\"GET\",\"path\":\"/allowed.*\"}]},\"source\":\"smoke spec.egress[5]\"}"
npl() { edge "{\"vms\":[$NPVM}],\"peers\":[$HOSTPEER_W],\"policy\":[$1],\"l7\":[$2],\"flow_log\":true,\"owner\":\"smoke\"}"; }
code() { ip netns exec "$NS" curl -s -m6 -o /dev/null -w '%{http_code}' "$@" || true; }
enforce
npl "$R_L7" "$L7_HTTP"
check "l7: status counts the rule, tap flagged" [ "$(jpath vm_edge_status "(d['l7_rules'], 'l7_auth' in d['taps'][0]['flags'])")" = "(1, True)" ]
check "l7 enforce: GET /allowed.txt passes" [ "$(code "http://$HOST_IP:18080/allowed.txt")" = 200 ]
check "l7 enforce: GET /secret.txt gets 403 from bpfd" [ "$(code "http://$HOST_IP:18080/secret.txt")" = 403 ]
check "l7 enforce: POST is denied" [ "$(code -X POST -d x "http://$HOST_IP:18080/allowed.txt")" = 403 ]
check "l7 enforce: keep-alive second request checked too" bash -c "ip netns exec $NS curl -s -m8 -o /dev/null -o /dev/null -w '%{http_code} ' http://$HOST_IP:18080/allowed.txt http://$HOST_IP:18080/secret.txt | grep -q '^200 403'"
check "l7: DROPPED flow with the request" flow_has "f['verdict']=='DROPPED' and f.get('l7_type')=='http' and 'GET' in (f.get('l7') or '') and '/secret.txt' in f['l7'] and f.get('drop_reason')=='l7-deny'"
check "l7: FORWARDED flow for the allowed request" flow_has "f['verdict']=='FORWARDED' and f.get('l7_type')=='http' and '/allowed.txt' in (f.get('l7') or '')"
check "l7: reinject veth up" bash -c "ip link show mnl7inj0 | grep -q UP && { tc filter show dev mnl7inj1 ingress; bpftool net show dev mnl7inj1; } 2>/dev/null | grep -q mn_vm_l7_inject"
check "l7 enforce: no retransmit wait (< 150 ms)" python3 -c "import sys; sys.exit(0 if float(sys.argv[1]) < 0.15 else 1)" "$(ip netns exec "$NS" curl -s -m6 -o /dev/null -w '%{time_total}' "http://$HOST_IP:18080/allowed.txt" || echo 9)"
split_get() {
  ip netns exec "$NS" python3 - "$HOST_IP" "$1" <<'PY'
import socket, sys, time
c = socket.create_connection((sys.argv[1], 18080), timeout=5)
c.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
c.sendall(b"GET " + sys.argv[2].encode() + b" HTTP/1.1\r\nHost: x\r\nX-Pad: " + b"a" * 3000)
time.sleep(0.2)
c.sendall(b"a" * 3000 + b"\r\nConnection: close\r\n\r\n")
try:
    print(c.recv(64).split(b"\r\n")[0].split()[1].decode())
except (OSError, IndexError):
    print("reset")
PY
}
check "l7 enforce: request head split across segments allowed" [ "$(split_get /allowed.txt)" = 200 ]
check "l7 enforce: split head of a denied request gets 403" [ "$(split_get /secret.txt)" = 403 ]

# Bodies: a server that counts POST bytes (Content-Length or chunked).
python3 - "$HOST_IP" >/dev/null 2>&1 <<'PY' &
import http.server, sys
class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def do_POST(self):
        n = 0
        if self.headers.get("Transfer-Encoding", "").lower() == "chunked":
            while True:
                size = int(self.rfile.readline().split(b";")[0], 16)
                if size == 0:
                    while self.rfile.readline() not in (b"\r\n", b""):
                        pass
                    break
                n += len(self.rfile.read(size)); self.rfile.readline()
        else:
            n = len(self.rfile.read(int(self.headers.get("Content-Length", 0))))
        b = f"got {n}".encode()
        self.send_response(200); self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)
    do_GET = do_POST
    def log_message(self, *a):
        pass
http.server.ThreadingHTTPServer((sys.argv[1], 18081), H).serve_forever()
PY
POST_PID=$!
R_L7B="{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":true,\"proto\":6,\"port\":18081,\"l7\":true,\"source\":\"smoke spec.egress[9]\"}"
L7_POST="{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":true,\"proto\":6,\"port\":18081,\"rules\":{\"http\":[{\"method\":\"POST\",\"path\":\"/upload.*\"}]},\"source\":\"smoke spec.egress[9]\"}"
sleep 0.5
npl "$R_L7,$R_L7B" "$L7_HTTP,$L7_POST"
check "l7 enforce: 384 KiB POST body passes" [ "$(ip netns exec "$NS" curl -s -m10 --data-binary @"$WORK/blob" "http://$HOST_IP:18081/upload")" = "got 393216" ]
chunked() {
  ip netns exec "$NS" python3 - "$HOST_IP" <<'PY'
import socket, sys, time
c = socket.create_connection((sys.argv[1], 18081), timeout=5)
c.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
c.sendall(b"POST /upload HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n")
time.sleep(0.1); c.sendall(b"5\r\nhello\r\n")
time.sleep(0.1); c.sendall(b"6\r\n world\r\n0\r\n\r\n")
r = b""
while not r.endswith(b"got 11"):
    x = c.recv(4096)
    if not x:
        break
    r += x
ok = r.startswith(b"HTTP/1.1 200") and r.endswith(b"got 11")
c.sendall(b"GET /upload HTTP/1.1\r\nHost: x\r\n\r\n")
try:
    r2 = c.recv(4096).split(b"\r\n")[0].split()[1].decode()
except (OSError, IndexError):
    r2 = "reset"
print(int(ok), r2)
PY
}
check "l7 enforce: chunked POST passes, next request on the connection denied" [ "$(chunked)" = "1 403" ]
kill "$POST_PID" 2>/dev/null || true

# HTTP/2 (prior knowledge) and gRPC-style paths: a stub h2c server answers
# every request with :status 200.
python3 - "$HOST_IP" >/dev/null 2>&1 <<'PY' &
import socket, sys, threading
def serve(c):
    c.sendall(b"\x00\x00\x00\x04\x00\x00\x00\x00\x00")
    buf, pre = b"", False
    while True:
        x = c.recv(65536)
        if not x:
            return
        buf += x
        if not pre:
            if len(buf) < 24:
                continue
            buf, pre = buf[24:], True
        while len(buf) >= 9:
            n = int.from_bytes(buf[:3], "big"); t, f = buf[3], buf[4]
            sid = int.from_bytes(buf[5:9], "big") & 0x7fffffff
            if len(buf) < 9 + n:
                break
            buf = buf[9 + n:]
            if t == 4 and not f & 1:
                c.sendall(b"\x00\x00\x00\x04\x01\x00\x00\x00\x00")
            if t in (0, 1) and f & 1:
                s = sid.to_bytes(4, "big")
                c.sendall(b"\x00\x00\x01\x01\x04" + s + b"\x88" + b"\x00\x00\x02\x00\x01" + s + b"ok")
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind((sys.argv[1], 18083)); s.listen(8)
while True:
    c, _ = s.accept(); threading.Thread(target=serve, args=(c,), daemon=True).start()
PY
H2_PID=$!
R_H2="{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":true,\"proto\":6,\"port\":18083,\"l7\":true,\"source\":\"smoke spec.egress[13]\"}"
L7_H2="{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":true,\"proto\":6,\"port\":18083,\"rules\":{\"http\":[{\"method\":\"GET\",\"path\":\"/allowed.*\"},{\"method\":\"POST\",\"path\":\"/smoke.Echo/.*\"}]},\"source\":\"smoke spec.egress[13]\"}"
h2code() { ip netns exec "$NS" curl -s -m6 -o /dev/null -w '%{http_code}' --http2-prior-knowledge "$@" || true; }
sleep 0.5
npl "$R_H2" "$L7_H2"
check "l7 h2: GET /allowed.txt over HTTP/2 passes" [ "$(h2code "http://$HOST_IP:18083/allowed.txt")" = 200 ]
check "l7 h2: GET /secret.txt over HTTP/2 reset" [ "$(h2code "http://$HOST_IP:18083/secret.txt")" = 000 ]
check "l7 h2: gRPC-style POST /smoke.Echo/Say passes" [ "$(h2code -H 'content-type: application/grpc' -d x "http://$HOST_IP:18083/smoke.Echo/Say")" = 200 ]
check "l7 h2: gRPC-style POST /smoke.Admin/Drop reset" [ "$(h2code -H 'content-type: application/grpc' -d x "http://$HOST_IP:18083/smoke.Admin/Drop")" = 000 ]
check "l7 h2: flow names the HTTP/2 request" flow_has "f.get('l7_type')=='http' and '/smoke.Admin/Drop' in (f.get('l7') or '') and f['verdict']=='DROPPED'"
kill "$H2_PID" 2>/dev/null || true

# IPv6: the same hold / parse / reinject path.
HOST_IP6=fd00:5e:81::1
VM_IP6=fd00:5e:81::2
ip addr add "$HOST_IP6/64" dev "$HOST_IF" nodad
ip netns exec "$NS" ip addr add "$VM_IP6/64" dev "$PEER_IF" nodad
(cd "$WORK" && exec python3 -m http.server 18082 --bind "$HOST_IP6" >/dev/null 2>&1) &
HTTP6_PID=$!
NPVM6="{\"name\":\"$VM\",\"addresses\":[\"$VM_IP\",\"$VM_IP6\"],\"taps\":[\"$HOST_IF\"],\"identity\":$VMID,\"isolate_egress\":true"
HOSTPEER6="{\"cidr\":\"$HOST_IP6\",\"identity\":1,\"name\":\"host6\"}"
R_L76="{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":true,\"proto\":6,\"port\":18082,\"l7\":true,\"source\":\"smoke spec.egress[12]\"}"
L7_HTTP6="{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":true,\"proto\":6,\"port\":18082,\"rules\":{\"http\":[{\"method\":\"GET\",\"path\":\"/allowed.*\"}]},\"source\":\"smoke spec.egress[12]\"}"
sleep 0.5
edge "{\"vms\":[$NPVM6}],\"peers\":[$HOSTPEER_W,$HOSTPEER6],\"policy\":[$R_L76],\"l7\":[$L7_HTTP6],\"flow_log\":true,\"owner\":\"smoke\"}"
check "l7 v6: GET /allowed.txt passes" [ "$(code -g "http://[$HOST_IP6]:18082/allowed.txt")" = 200 ]
check "l7 v6: GET /secret.txt gets 403 from bpfd" [ "$(code -g "http://[$HOST_IP6]:18082/secret.txt")" = 403 ]
check "l7 v6: flow over IPv6" flow_has "f.get('l7_type')=='http' and f['dst']=='$HOST_IP6' and f['verdict']=='DROPPED'"
kill "$HTTP6_PID" 2>/dev/null || true
npl "$R_L7" "$L7_HTTP"
observe
check "l7 observe: denied request still served" [ "$(code "http://$HOST_IP:18080/secret.txt")" = 200 ]
check "l7 observe: AUDIT flow" flow_has "f['verdict']=='AUDIT' and f.get('l7_type')=='http'"

# TLS serverNames: SNI of the ClientHello.
openssl req -x509 -newkey rsa:2048 -nodes -subj /CN=smoke -keyout "$WORK/k.pem" -out "$WORK/c.pem" -days 1 >/dev/null 2>&1
openssl s_server -quiet -accept "$HOST_IP:18443" -www -cert "$WORK/c.pem" -key "$WORK/k.pem" >/dev/null 2>&1 &
TLS_PID=$!
R_TLS="{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":true,\"proto\":6,\"port\":18443,\"l7\":true,\"source\":\"smoke spec.egress[6]\"}"
L7_TLS="{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":true,\"proto\":6,\"port\":18443,\"rules\":{\"server_names\":[\"ok.smoke.test\"]},\"source\":\"smoke spec.egress[6]\"}"
tls() { ip netns exec "$NS" curl -sk -m6 -o /dev/null --resolve "$1:18443:$HOST_IP" "https://$1:18443/"; }
sleep 0.5
enforce
npl "$R_TLS" "$L7_TLS"
check "l7 tls: allowed SNI connects" tls ok.smoke.test
check "l7 tls: other SNI reset" bash -c "! ip netns exec $NS curl -sk -m6 -o /dev/null --resolve bad.smoke.test:18443:$HOST_IP https://bad.smoke.test:18443/"
check "l7 tls: flow names the SNI" flow_has "f.get('l7_type')=='tls' and 'bad.smoke.test' in (f.get('l7') or '') and f['verdict']=='DROPPED'"
kill "$TLS_PID" 2>/dev/null || true

# Kafka: a stub broker answers any request; produce to `orders` only.
python3 - "$HOST_IP" >/dev/null 2>&1 <<'PY' &
import socket, struct, sys, threading
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind((sys.argv[1], 19092)); s.listen(8)
def serve(c):
    try:
        while True:
            h = c.recv(4)
            if len(h) < 4: return
            n = struct.unpack(">i", h)[0]; b = b""
            while len(b) < n:
                x = c.recv(n - len(b))
                if not x: return
                b += x
            c.sendall(struct.pack(">ii", 4, struct.unpack(">i", b[4:8])[0]))
    except OSError:
        pass
while True:
    c, _ = s.accept(); threading.Thread(target=serve, args=(c,), daemon=True).start()
PY
KAFKA_PID=$!
produce() {
  ip netns exec "$NS" python3 - "$HOST_IP" "$1" <<'PY'
import socket, struct, sys
def s16(x): return struct.pack(">h", len(x)) + x
topic = sys.argv[2].encode()
body = struct.pack(">hi", 1, 1000) + struct.pack(">i", 1) + s16(topic) + struct.pack(">i", 1) + struct.pack(">i", 0) + struct.pack(">i", 0)
req = struct.pack(">hhi", 0, 0, 7) + s16(b"smoke") + body
try:
    c = socket.create_connection((sys.argv[1], 19092), timeout=6)
    c.sendall(struct.pack(">i", len(req)) + req)
    r = c.recv(8)
except OSError:
    sys.exit(1)
sys.exit(0 if len(r) == 8 and struct.unpack(">ii", r)[1] == 7 else 1)
PY
}
R_KAFKA="{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":true,\"proto\":6,\"port\":19092,\"l7\":true,\"source\":\"smoke spec.egress[7]\"}"
L7_KAFKA="{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":true,\"proto\":6,\"port\":19092,\"rules\":{\"kafka\":[{\"role\":\"produce\",\"topic\":\"orders\"}]},\"source\":\"smoke spec.egress[7]\"}"
sleep 0.5
enforce
npl "$R_KAFKA" "$L7_KAFKA"
check "l7 kafka: produce to orders answered" produce orders
check "l7 kafka: produce to payments reset" not produce payments
check "l7 kafka: flow names the topic" flow_has "f.get('l7_type')=='kafka' and 'payments' in (f.get('l7') or '') and f['verdict']=='DROPPED'"
kill "$KAFKA_PID" 2>/dev/null || true

# DNS rules: bpfd forwards allowed queries and answers REFUSED otherwise.
python3 - "$HOST_IP" >"$WORK/dns2.log" 2>&1 <<'PY' &
import socket, struct, sys
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.bind((sys.argv[1], 53))
ip = socket.inet_aton(sys.argv[1])
while True:
    q, a = s.recvfrom(512)
    i = 12
    while q[i]: i += q[i] + 1
    end = i + 5
    hdr = q[:2] + struct.pack(">HHHHH", 0x8180, 1, 1, 0, 0)
    s.sendto(hdr + q[12:end] + b"\xc0\x0c" + struct.pack(">HHIH", 1, 1, 5, 4) + ip, a)
PY
DNS_PID=$!
rcode() {
  ip netns exec "$NS" python3 - "$HOST_IP" "$1" <<'PY'
import socket, struct, sys
q = struct.pack(">HHHHHH", 0x4d4f, 0x0100, 1, 0, 0, 0)
q += b"".join(bytes([len(p)]) + p.encode() for p in sys.argv[2].split(".")) + b"\x00" + struct.pack(">HH", 1, 1)
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.settimeout(3)
s.sendto(q, (sys.argv[1], 53))
try:
    r = s.recv(512); print(r[3] & 15, struct.unpack(">H", r[6:8])[0])
except socket.timeout:
    print("timeout")
PY
}
R_DNSL7="{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":true,\"proto\":17,\"port\":53,\"l7\":true,\"source\":\"smoke spec.egress[8]\"}"
L7_DNS="{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":true,\"proto\":17,\"port\":53,\"rules\":{\"dns\":[\"*.smoke.test\"]},\"source\":\"smoke spec.egress[8]\"}"
sleep 0.3
enforce
npl "$R_DNSL7" "$L7_DNS"
check "l7 dns: allowed name forwarded by bpfd and answered" [ "$(rcode svc.smoke.test)" = "0 1" ]
check "l7 dns: other name REFUSED" [ "$(rcode example.org)" = "5 0" ]
check "l7 dns: flow names the query" flow_has "f.get('l7_type')=='dns' and 'example.org' in (f.get('l7') or '') and f['verdict']=='DROPPED'"
kill "$DNS_PID" 2>/dev/null || true

# DNS over TCP: rules.dns on tcp/53, and toFQDNs learns from the answer.
python3 - "$HOST_IP" >/dev/null 2>&1 <<'PY' &
import socket, struct, sys, threading
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind((sys.argv[1], 53)); s.listen(8)
ip = socket.inet_aton(sys.argv[1])
def serve(c):
    try:
        n = struct.unpack(">H", c.recv(2))[0]; q = b""
        while len(q) < n:
            x = c.recv(n - len(q))
            if not x: return
            q += x
        i = 12
        while q[i]: i += q[i] + 1
        r = q[:2] + struct.pack(">HHHHH", 0x8180, 1, 1, 0, 0) + q[12:i + 5] + b"\xc0\x0c" + struct.pack(">HHIH", 1, 1, 5, 4) + ip
        c.sendall(struct.pack(">H", len(r)) + r)
    except OSError:
        pass
    finally:
        c.close()
while True:
    c, _ = s.accept(); threading.Thread(target=serve, args=(c,), daemon=True).start()
PY
DNST_PID=$!
tcp_rcode() {
  ip netns exec "$NS" python3 - "$HOST_IP" "$1" <<'PY'
import socket, struct, sys
q = struct.pack(">HHHHHH", 0x4d50, 0x0100, 1, 0, 0, 0)
q += b"".join(bytes([len(p)]) + p.encode() for p in sys.argv[2].split(".")) + b"\x00" + struct.pack(">HH", 1, 1)
try:
    c = socket.create_connection((sys.argv[1], 53), timeout=3)
    c.sendall(struct.pack(">H", len(q)) + q)
    r = c.recv(512)[2:]
    print(r[3] & 15, struct.unpack(">H", r[6:8])[0])
except (OSError, IndexError):
    print("reset")
PY
}
R_DNST="{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":true,\"proto\":6,\"port\":53,\"l7\":true,\"source\":\"smoke spec.egress[10]\"}"
L7_DNST="{\"subject_identity\":$VMID,\"peer_identity\":1,\"egress\":true,\"proto\":6,\"port\":53,\"rules\":{\"dns\":[\"*.smoke.test\"]},\"source\":\"smoke spec.egress[10]\"}"
F_SVCT="{\"pattern\":\"svc.smoke.test\",\"subject_identity\":$VMID,\"proto\":6,\"port\":18080,\"source\":\"smoke spec.egress[11]\"}"
sleep 0.3
edge "{\"vms\":[$NPVM}],\"peers\":[$HOSTPEER_W],\"policy\":[$R_DNST],\"l7\":[$L7_DNST],\"fqdn\":[$F_SVCT],\"flow_log\":true,\"owner\":\"smoke\"}"
check "l7 dns/tcp: allowed name answered" [ "$(tcp_rcode svc.smoke.test)" = "0 1" ]
check "l7 dns/tcp: other name reset" [ "$(tcp_rcode example.org)" = reset ]
check "l7 dns/tcp: flow names the query" flow_has "f.get('l7_type')=='dns' and f['proto']=='tcp' and 'example.org' in (f.get('l7') or '') and f['verdict']=='DROPPED'"
sleep 1.5
check "fqdn: learned from a DNS-over-TCP answer" fqdn_has "e['name']=='svc.smoke.test' and e['address']=='$HOST_IP'"
kill "$DNST_PID" 2>/dev/null || true

# ---- authentication + source guard -------------------------------------------
# HOST_IP is a fleet VM peer (identity 5001) for `required`; the host
# connects to a server inside the netns.

SPOOF_IP=10.199.83.3
(ip netns exec "$NS" python3 -m http.server 18090 --bind "$VM_IP" >/dev/null 2>&1) &
VMHTTP_PID=$!
AVM="{\"name\":\"$VM\",\"addresses\":[\"$VM_IP\"],\"taps\":[\"$HOST_IF\"],\"identity\":$VMID,\"isolate_ingress\":true"
FLEETPEER="{\"cidr\":\"$HOST_IP\",\"identity\":5001,\"name\":\"fleet-peer\"},{\"cidr\":\"$SPOOF_IP\",\"identity\":5002,\"name\":\"other-vm\"}"
R_AUTH="{\"subject_identity\":$VMID,\"peer_identity\":5001,\"egress\":false,\"proto\":6,\"port\":18090,\"auth\":1,\"source\":\"smoke spec.ingress[0]\"}"
R_AFAIL="{\"subject_identity\":$VMID,\"peer_identity\":5001,\"egress\":false,\"proto\":6,\"port\":18090,\"auth\":2,\"source\":\"smoke spec.ingress[0]\"}"
npa() { edge "{\"vms\":[$AVM}],\"peers\":[$FLEETPEER],\"policy\":[$1],\"flow_log\":true,\"owner\":\"smoke\"}"; }
to_vm() { curl -s -m6 -o /dev/null "http://$VM_IP:18090/"; }
sleep 0.5
enforce
npa "$R_AFAIL"
check "auth: source guard on the tap" [ "$(jpath vm_edge_status "'source_guard' in d['taps'][0]['flags']")" = True ]
check "auth enforce: test-always-fail blocks" bash -c "! curl -s -m3 -o /dev/null http://$VM_IP:18090/"
check "auth: flow says auth-test-always-fail" flow_has "f.get('drop_reason')=='auth-test-always-fail' and f['verdict']=='DROPPED'"
npa "$R_AUTH"
check "auth enforce: required blocks a remote VM without a host certificate" not to_vm
check "auth: first SYN dropped as auth-required" flow_has "f.get('drop_reason')=='auth-required'"
check "auth: table says why" auth_has 'this host has no certificate'
check "auth: nothing authenticated" [ "$(jpath vm_edge_status "d['auth_entries']")" = 0 ]
# A peer address that resolves to a VM on this host's taps authenticates
# locally, without mTLS.
LOCALPEER="{\"cidr\":\"$HOST_IP\",\"identity\":$VMID,\"name\":\"$VM\"},{\"cidr\":\"$SPOOF_IP\",\"identity\":5002,\"name\":\"other-vm\"}"
R_ALOCAL="{\"subject_identity\":$VMID,\"peer_identity\":$VMID,\"egress\":false,\"proto\":6,\"port\":18090,\"auth\":1,\"source\":\"smoke spec.ingress[0]\"}"
edge "{\"vms\":[$AVM}],\"peers\":[$LOCALPEER],\"policy\":[$R_ALOCAL],\"flow_log\":true,\"owner\":\"smoke\"}"
check "auth enforce: required authenticates a local VM peer" to_vm
check "auth: table shows the pair" auth_has 'authenticated (local VM'
check "auth: status counts it" [ "$(jpath vm_edge_status "d['auth_entries']")" = 1 ]
npa "$R_AUTH"
ip netns exec "$NS" ip addr add "$SPOOF_IP/32" dev "$PEER_IF"
ip route add "$SPOOF_IP/32" dev "$HOST_IF"
from_spoof() {
  ip netns exec "$NS" python3 -c "import socket,sys; s=socket.socket(); s.settimeout(3); s.bind(('$SPOOF_IP',0)); s.connect(('$HOST_IP',18080))" 2>/dev/null
}
check "guard: own address still works" http_ok
check "guard enforce: spoofed source (another identity) dropped" not from_spoof
check "guard: flow says spoofed-source" flow_has "f.get('drop_reason')=='spoofed-source' and f['src']=='$SPOOF_IP'"
observe
check "guard observe: spoofed source only audited" from_spoof
ip netns exec "$NS" ip addr del "$SPOOF_IP/32" dev "$PEER_IF"
kill "$VMHTTP_PID" 2>/dev/null || true

observe
check "netpol observe: ingressDeny only audited" host_ping_vm
learned_has() { jpath vm_edge_status "'$VM_IP' in d.get('learned', {}).get('$VM', [])" | grep -q True; }
check "learned: VM source address noted from tap traffic" learned_has

edge '{"vms":[]}'
check "netpol: cleared" [ "$(jpath vm_edge_status "len(d['taps'])")" = 0 ]

# ---- per-project egress IP (SNAT) ---------------------------------------------
# A second netns stands in for the outside world; the "VM" netns is routed
# through the host and the egress IP is a /32 on the host side of that link.
# Skipped when the host already has a machina_egress table (a real bpfd owns it).

if nft list table ip machina_egress >/dev/null 2>&1; then
  echo "SKIP  egress: ip machina_egress already exists on this host"
else
  OUT_NS=mnvme-out
  OUT_IF=mnvme-b0
  OUT_HOST_IP=10.199.82.1
  OUT_IP=10.199.82.2
  EGRESS_IP=10.199.84.1
  FWD_WAS=$(cat /proc/sys/net/ipv4/ip_forward)
  egress_cleanup() {
    nft delete table ip machina_egress 2>/dev/null || true
    for d in "-i $HOST_IF -o $OUT_IF" "-i $OUT_IF -o $HOST_IF"; do
      # shellcheck disable=SC2086
      iptables -D FORWARD $d -j ACCEPT 2>/dev/null || true
    done
    [[ -n "${OUTSRV_PID:-}" ]] && kill "$OUTSRV_PID" 2>/dev/null || true
    ip netns del "$OUT_NS" 2>/dev/null || true
    ip link del "$OUT_IF" 2>/dev/null || true
    echo "$FWD_WAS" >/proc/sys/net/ipv4/ip_forward
  }
  trap 'egress_cleanup; cleanup' EXIT
  ip netns add "$OUT_NS"
  ip link add "$OUT_IF" type veth peer name "${OUT_IF}p"
  ip link set "${OUT_IF}p" netns "$OUT_NS"
  ip addr add "$OUT_HOST_IP/30" dev "$OUT_IF"
  ip addr add "$EGRESS_IP/32" dev "$OUT_IF"
  ip link set "$OUT_IF" up
  ip netns exec "$OUT_NS" ip addr add "$OUT_IP/30" dev "${OUT_IF}p"
  ip netns exec "$OUT_NS" ip link set "${OUT_IF}p" up
  ip netns exec "$OUT_NS" ip route add 10.199.81.0/30 via "$OUT_HOST_IP"
  ip netns exec "$OUT_NS" ip route add "$EGRESS_IP/32" via "$OUT_HOST_IP"
  ip netns exec "$NS" ip route add 10.199.82.0/30 via "$HOST_IP"
  echo 1 >/proc/sys/net/ipv4/ip_forward
  if command -v iptables >/dev/null; then
    iptables -I FORWARD -i "$HOST_IF" -o "$OUT_IF" -j ACCEPT
    iptables -I FORWARD -i "$OUT_IF" -o "$HOST_IF" -j ACCEPT
  fi
  (ip netns exec "$OUT_NS" python3 -c "
import socket
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(('$OUT_IP', 18095)); s.listen(8)
while True:
    c, a = s.accept(); c.sendall(a[0].encode()); c.close()
" >/dev/null 2>&1) &
  OUTSRV_PID=$!
  sleep 0.5
  seen_as() {
    ip netns exec "$NS" python3 -c "import socket; s=socket.create_connection(('$OUT_IP',18095),3); print(s.recv(64).decode())" 2>/dev/null
  }
  snat() { req "{\"op\":\"vm_egress_snat_set\",\"config\":{\"rules\":$1,\"exclude\":[],\"manage_addresses\":false}}"; }
  check "egress: routed out with the VM address" [ "$(seen_as)" = "$VM_IP" ]
  snat "[{\"project\":\"smoke\",\"egress_ip\":\"$EGRESS_IP\",\"sources\":[\"$VM_IP\"]}]" | must
  check "egress: status active with one rule" [ "$(jpath vm_egress_snat_status "(d['active'], len(d['rules']), d.get('error'))")" = "(True, 1, None)" ]
  check "egress: nft table installed" bash -c "nft list table ip machina_egress | grep -q 'snat to $EGRESS_IP'"
  check "egress: outside sees the project egress IP" [ "$(seen_as)" = "$EGRESS_IP" ]
  bad_snat_ok() { snat "[{\"project\":\"x\",\"egress_ip\":\"nope\",\"sources\":[\"$VM_IP\"]}]" | grep -q '"ok":true'; }
  check "egress: invalid address refused" not bad_snat_ok
  check "egress: refused config left the rule in place" [ "$(seen_as)" = "$EGRESS_IP" ]
  snat "[{\"project\":\"smoke\",\"egress_ip\":\"10.199.85.9\",\"sources\":[\"$VM_IP\"]}]" | must
  check "egress: non-local egress IP skipped" [ "$(jpath vm_egress_snat_status "(d['active'], len(d['skipped']))")" = "(False, 1)" ]
  no_table() { ! nft list table ip machina_egress >/dev/null 2>&1; }
  check "egress: skipped rule leaves no table" no_table
  check "egress: VM address again without a rule" [ "$(seen_as)" = "$VM_IP" ]
  snat "[{\"project\":\"smoke\",\"egress_ip\":\"$EGRESS_IP\",\"sources\":[\"$VM_IP\"]}]" | must
  check "egress: re-applied" [ "$(seen_as)" = "$EGRESS_IP" ]
  snat '[]' | must
  check "egress: cleared removes the table" no_table
  check "egress: cleared restores the VM address" [ "$(seen_as)" = "$VM_IP" ]

  # Managed addresses: bpfd adds a missing egress IP to the (overridden)
  # uplink, announces it, and removes only what it added.
  MANAGED_IP=10.199.86.9
  MANAGED_IP6=fd99:86::9
  OUT_MAC=$(cat "/sys/class/net/$OUT_IF/address")
  ip netns exec "$OUT_NS" ip route add 10.199.86.0/24 dev "${OUT_IF}p"
  ip netns exec "$OUT_NS" sysctl -qw "net.ipv4.conf.${OUT_IF}p.arp_accept=1"
  NA_KNOB=/proc/sys/net/ipv6/conf/${OUT_IF}p/accept_untracked_na
  ip netns exec "$OUT_NS" sh -c "[ -e $NA_KNOB ] && echo 1 > $NA_KNOB && sysctl -qw net.ipv6.conf.${OUT_IF}p.forwarding=1" || true
  managed() { req "{\"op\":\"vm_egress_snat_set\",\"config\":{\"rules\":$1,\"exclude\":[],\"interface\":\"$OUT_IF\"}}"; }
  managed "[{\"project\":\"smoke\",\"egress_ip\":\"$MANAGED_IP\",\"sources\":[\"$VM_IP\"]},{\"project\":\"smoke6\",\"egress_ip\":\"$MANAGED_IP6\",\"sources\":[\"fd99:81::2\"]}]" | must
  on_out() { ip -o addr show dev "$OUT_IF" | grep -q " $1 "; }
  check "egress managed: IPv4 /32 added to the interface" on_out "$MANAGED_IP/32"
  check "egress managed: IPv6 /128 added to the interface" on_out "$MANAGED_IP6/128"
  check "egress managed: status lists both, nothing skipped" [ "$(jpath vm_egress_snat_status "(sorted(d['managed']), len(d['skipped']))")" = "(['$MANAGED_IP/32@$OUT_IF', '$MANAGED_IP6/128@$OUT_IF'], 0)" ]
  garp_seen() { ip netns exec "$OUT_NS" ip neigh show "$MANAGED_IP" dev "${OUT_IF}p" | grep -qi "$OUT_MAC"; }
  check "egress managed: gratuitous ARP taught the outside the address" garp_seen
  if ip netns exec "$OUT_NS" test -e "$NA_KNOB"; then
    na_seen() { ip netns exec "$OUT_NS" ip -6 neigh show "$MANAGED_IP6" dev "${OUT_IF}p" | grep -qi "$OUT_MAC"; }
    check "egress managed: unsolicited NA taught the outside the address" na_seen
  fi
  check "egress managed: outside sees the added address" [ "$(seen_as)" = "$MANAGED_IP" ]
  managed '[]' | must
  not_out() { ! on_out "$1"; }
  check "egress managed: cleared removes the IPv4 address" not_out "$MANAGED_IP/32"
  check "egress managed: cleared removes the IPv6 address" not_out "$MANAGED_IP6/128"
  check "egress managed: status empty" [ "$(jpath vm_egress_snat_status "d.get('managed', [])")" = "[]" ]
  check "egress managed: pre-existing address kept" on_out "$EGRESS_IP/32"
  egress_cleanup
  trap cleanup EXIT
fi

# ---- WireGuard overlay ----------------------------------------------------------
# A netns with its own WireGuard interface stands in for a peer hypervisor that
# owns 100.96.2.0/24; the "VM" gets fleet address 100.96.1.2 on this host.
# Skipped when wg is missing or machina-wg is already in use (a real bpfd).

if ! command -v wg >/dev/null || ip link show machina-wg >/dev/null 2>&1 ||
  nft list table ip machina_overlay >/dev/null 2>&1; then
  echo "SKIP  overlay: wg missing or machina-wg already in use"
else
  WG_NS=mnvme-wg
  WG_IF=mnvme-c0
  WG_HOST_IP=10.199.87.1
  WG_PEER_IP=10.199.87.2
  FLEET_VM=100.96.1.2
  REMOTE_VM=100.96.2.5
  SPOOF_IP=100.96.1.50
  OV_PORT=51879
  # Distro AppArmor profiles only let wg read keys under /etc/wireguard.
  WG_DIR_NEW=
  [[ -d /etc/wireguard ]] || { install -d -m700 /etc/wireguard && WG_DIR_NEW=1; }
  PEER_KEY=/etc/wireguard/mnvme-smoke-peer.key
  FWD_WAS=$(cat /proc/sys/net/ipv4/ip_forward)
  FWD_RULES=("-i $HOST_IF -o machina-wg -j ACCEPT"
    "-i machina-wg -o $HOST_IF -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT")
  overlay_cleanup() {
    req '{"op":"vm_overlay_set","config":{"enabled":false}}' >/dev/null 2>&1 || true
    for p in "${WGSRV_PID:-}" "${VMSRV_PID:-}"; do [[ -n "$p" ]] && kill "$p" 2>/dev/null || true; done
    for r in "${FWD_RULES[@]}"; do
      # shellcheck disable=SC2086
      iptables -D FORWARD $r 2>/dev/null || true
    done
    ip netns exec "$NS" ip route del 100.96.0.0/12 2>/dev/null || true
    ip netns del "$WG_NS" 2>/dev/null || true
    ip link del "$WG_IF" 2>/dev/null || true
    rm -f "$PEER_KEY"
    [[ -n "$WG_DIR_NEW" ]] && rmdir /etc/wireguard 2>/dev/null || true
    echo "$FWD_WAS" >/proc/sys/net/ipv4/ip_forward
  }
  trap 'overlay_cleanup; cleanup' EXIT
  ip netns add "$WG_NS"
  ip link add "$WG_IF" type veth peer name "${WG_IF}p"
  ip link set "${WG_IF}p" netns "$WG_NS"
  ip addr add "$WG_HOST_IP/30" dev "$WG_IF"
  ip link set "$WG_IF" up
  ip netns exec "$WG_NS" ip link set lo up
  ip netns exec "$WG_NS" ip addr add "$WG_PEER_IP/30" dev "${WG_IF}p"
  ip netns exec "$WG_NS" ip addr add "$REMOTE_VM/32" dev lo
  ip netns exec "$WG_NS" ip addr add "$SPOOF_IP/32" dev lo
  ip netns exec "$WG_NS" ip link set "${WG_IF}p" up
  ip netns exec "$NS" ip route add 100.96.0.0/12 via "$HOST_IP"
  echo 1 >/proc/sys/net/ipv4/ip_forward
  (umask 077 && wg genkey >"$PEER_KEY")
  PEER_PUB=$(cat "$PEER_KEY" | wg pubkey)
  ov_set() {
    req "{\"op\":\"vm_overlay_set\",\"config\":{\"enabled\":true,\"listen_port\":$OV_PORT,\"prefixes\":[\"100.96.1.0/24\"],\"fleet_prefixes\":[\"100.96.0.0/12\"],\"mappings\":[{\"local\":\"$VM_IP\",\"fleet\":\"$FLEET_VM\",\"vm\":\"$VM\"}],\"peers\":[{\"host\":\"peer\",\"public_key\":\"$PEER_PUB\",\"endpoint\":\"$WG_PEER_IP:51872\",\"prefixes\":[\"100.96.2.0/24\"]}]}}"
  }
  ov_set | must
  HOST_PUB=$(jpath vm_overlay_status "d.get('public_key') or ''")
  ip netns exec "$WG_NS" ip link add wg-peer type wireguard
  ip netns exec "$WG_NS" wg set wg-peer listen-port 51872 private-key "$PEER_KEY" \
    peer "$HOST_PUB" endpoint "$WG_HOST_IP:$OV_PORT" allowed-ips 100.96.1.0/24,10.199.81.0/30
  ip netns exec "$WG_NS" ip link set wg-peer mtu 1420 up
  ip netns exec "$WG_NS" ip route add 100.96.1.0/24 dev wg-peer src "$REMOTE_VM"
  ip netns exec "$WG_NS" ip route add 10.199.81.0/30 dev wg-peer src "$REMOTE_VM"
  for r in "${FWD_RULES[@]}"; do
    # shellcheck disable=SC2086
    iptables -I FORWARD $r
  done
  echo_srv() {
    python3 -c "
import socket
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(('$1', $2)); s.listen(8)
while True:
    c, a = s.accept(); c.sendall(a[0].encode()); c.close()
"
  }
  (ip netns exec "$WG_NS" bash -c "$(declare -f echo_srv); echo_srv $REMOTE_VM 18096" >/dev/null 2>&1) &
  WGSRV_PID=$!
  (ip netns exec "$NS" bash -c "$(declare -f echo_srv); echo_srv $VM_IP 18097" >/dev/null 2>&1) &
  VMSRV_PID=$!
  # bpfd's first handshake went out before this peer existed; WireGuard
  # retries after 5 s, so wait for the tunnel instead of the first dial.
  for _ in $(seq 15); do ip netns exec "$WG_NS" ping -c1 -W1 100.96.1.1 >/dev/null 2>&1 && break; done
  # dial NETNS SRC DST PORT: print the address the server saw.
  dial() {
    ip netns exec "$1" python3 -c "
import socket
s = socket.socket(); s.settimeout(3); s.bind(('$2', 0)); s.connect(('$3', $4)); print(s.recv(64).decode())" 2>/dev/null
  }
  has_key() { [[ ${#HOST_PUB} -eq 44 ]] && ip link show machina-wg 2>/dev/null | grep -q ",UP"; }
  check "overlay: machina-wg up with a public key" has_key
  check "overlay: applied without error" [ "$(jpath vm_overlay_status "d.get('error')")" = None ]
  check "overlay: VM reaches the remote VM as its fleet address" [ "$(dial "$NS" "$VM_IP" "$REMOTE_VM" 18096)" = "$FLEET_VM" ]
  check "overlay: remote VM reaches the VM by fleet address" [ "$(dial "$WG_NS" "$REMOTE_VM" "$FLEET_VM" 18097)" = "$REMOTE_VM" ]
  handshake() { [ "$(jpath vm_overlay_status "[p['latest_handshake'] > 0 and p['rx_bytes'] > 0 for p in d['peers']]")" = "[True]" ]; }
  check "overlay: peer handshake and encrypted transfer" handshake
  spoof_ok() { [ -n "$(dial "$WG_NS" "$SPOOF_IP" "$FLEET_VM" 18097)" ]; }
  check "overlay: source outside the peer's prefixes dropped" not spoof_ok
  direct_ok() { [ -n "$(dial "$WG_NS" "$REMOTE_VM" "$VM_IP" 18097)" ]; }
  check "overlay: unmapped destination from a peer dropped" not direct_ok
  unreach() { ip route show type unreachable | grep -q '^unreachable 100.96.0.0/12'; }
  check "overlay: fleet range unreachable outside peer routes" unreach
  req '{"op":"vm_overlay_set","config":{"enabled":false}}' | must
  no_wg() { ! ip link show machina-wg >/dev/null 2>&1 && ! nft list table ip machina_overlay >/dev/null 2>&1; }
  check "overlay: disable removes the interface and tables" no_wg
  check "overlay: disable removes the unreachable routes" not unreach
  no_fwd() { ! iptables -S FORWARD | grep -q machina-overlay; }
  check "overlay: disable removes the forward rule" no_fwd
  overlay_cleanup
  trap cleanup EXIT
fi

# ---- wake on traffic ---------------------------------------------------------
# Three "sleeping VMs" with nobody behind their addresses: one reached from
# the host (output hook), one routed from the smoke VM (forward hook), one
# ARPed for by a peer on a private bridge (bridge prerouting).

if nft list table inet machina_wake >/dev/null 2>&1; then
  echo "SKIP  wake: machina_wake already in use"
else
  WB_NS=mnvme-wb
  WB_BR=mnvme-br
  WB_IF=mnvme-d0
  WAKE_FWD_WAS=$(cat /proc/sys/net/ipv4/ip_forward)
  wake_cleanup() {
    req '{"op":"vm_wake_set","config":{"entries":[]}}' >/dev/null 2>&1 || true
    [[ -n "${SUB_PID:-}" ]] && kill "$SUB_PID" 2>/dev/null || true
    ip netns exec "$NS" ip route del 10.199.89.0/24 2>/dev/null || true
    ip link del mnvme-dz 2>/dev/null || true
    ip netns del "$WB_NS" 2>/dev/null || true
    ip link del "$WB_IF" 2>/dev/null || true
    ip link del "$WB_BR" 2>/dev/null || true
    echo "$WAKE_FWD_WAS" >/proc/sys/net/ipv4/ip_forward
  }
  trap 'wake_cleanup; cleanup' EXIT
  ip link add "$WB_BR" type bridge
  ip link set "$WB_BR" up
  ip netns add "$WB_NS"
  ip link add "$WB_IF" type veth peer name "${WB_IF}p"
  ip link set "$WB_IF" master "$WB_BR" up
  ip link set "${WB_IF}p" netns "$WB_NS"
  ip netns exec "$WB_NS" ip addr add 10.199.90.2/24 dev "${WB_IF}p"
  ip netns exec "$WB_NS" ip link set "${WB_IF}p" up
  # Nothing answers for the sleeping addresses: a dummy device stands in for
  # the bridge of a VM whose tap is gone.
  ip link add mnvme-dz type dummy
  ip link set mnvme-dz up
  ip route add 10.199.89.0/24 dev mnvme-dz
  ip netns exec "$NS" ip route add 10.199.89.0/24 via "$HOST_IP"
  echo 1 >/proc/sys/net/ipv4/ip_forward
  python3 - "$SOCK" "$WORK/wake.events" <<'PY' &
import socket, sys
s = socket.socket(socket.AF_UNIX)
s.connect(sys.argv[1])
s.sendall(b'{"op":"subscribe","topics":["vm_wake"]}\n')
f = s.makefile()
f.readline()
with open(sys.argv[2], "a") as out:
    for line in f:
        out.write(line)
        out.flush()
PY
  SUB_PID=$!
  sleep 0.5
  req '{"op":"vm_wake_set","config":{"entries":[{"vm":"mnsmk-sleep-host","addresses":["10.199.89.2"]},{"vm":"mnsmk-sleep-fwd","addresses":["10.199.89.3","fd99::3"]},{"vm":"mnsmk-sleep-arp","addresses":["10.199.90.5"]}]}}' | must
  check "wake: set applied without error" [ "$(jpath vm_wake_status "d.get('error')")" = None ]
  tables() { nft list table inet machina_wake >/dev/null 2>&1 && nft list table bridge machina_wake >/dev/null 2>&1; }
  check "wake: inet and bridge tables installed" tables
  woke() {
    for _ in $(seq 12); do
      grep -q "\"vm\":\"$1\"" "$WORK/wake.events" 2>/dev/null && return 0
      sleep 0.25
    done
    return 1
  }
  check "wake: no event before traffic" not woke mnsmk-sleep-host
  ping -c1 -W1 10.199.89.2 >/dev/null 2>&1 || true
  check "wake: host-originated packet publishes vm_wake" woke mnsmk-sleep-host
  ip netns exec "$NS" ping -c1 -W1 10.199.89.3 >/dev/null 2>&1 || true
  check "wake: routed packet from another VM publishes vm_wake" woke mnsmk-sleep-fwd
  ip netns exec "$WB_NS" ping -c1 -W1 10.199.90.5 >/dev/null 2>&1 || true
  check "wake: ARP from a bridge peer publishes vm_wake" woke mnsmk-sleep-arp
  addr_ok() { grep -q '"vm":"mnsmk-sleep-fwd","address":"10.199.89.3"' "$WORK/wake.events"; }
  check "wake: event names the address that was hit" addr_ok
  hist() { [ "$(jpath vm_wake_status "sorted({w['vm'] for w in d['wakes']})")" = "['mnsmk-sleep-arp', 'mnsmk-sleep-fwd', 'mnsmk-sleep-host']" ]; }
  check "wake: status keeps the wake history" hist
  bad=$(req '{"op":"vm_wake_set","config":{"entries":[{"vm":"a","addresses":["10.199.89.9"]},{"vm":"b","addresses":["10.199.89.9"]}]}}')
  check "wake: an address claimed by two VMs is rejected" grep -q "claimed by both" <<<"$bad"
  req '{"op":"vm_wake_set","config":{"entries":[]}}' | must
  check "wake: empty set removes the tables" not tables
  wake_cleanup
  trap cleanup EXIT
fi

# ---- QEMU sandbox -----------------------------------------------------------

mkdir -p "$CG"
# /dev/kmsg (c 1:11) is outside the default QEMU device list.
# in_cg CMD...: run CMD inside the fake QEMU scope.
in_cg() { bash -c "echo \$\$ > $CG/cgroup.procs && exec \"\$@\"" _ "$@"; }
dev_ok() { in_cg python3 -c "open('/dev/kmsg','rb').close()" 2>/dev/null; }
cg_http() { in_cg curl -s -m2 -o /dev/null "http://$HOST_IP:$1/"; }
cg_dropped() {
  local rc=0
  in_cg curl -s -m2 -o /dev/null "http://$HOST_IP:$1/" || rc=$?
  [[ $rc -eq 28 ]]
}

req "{\"op\":\"vm_sandbox_configure\",\"config\":{\"mode\":\"enforce\",\"egress_ports\":[\"18080\"]}}" | must
req "{\"op\":\"vm_sandbox_attach\",\"vm\":\"$VM\",\"cgroup\":\"$CG_REL\"}" | must
check "sandbox: attached to fake scope" [ "$(jpath vm_sandbox_status "d['attached'].get('$VM','')")" = "$CG_REL" ]
check "sandbox: device + egress programs on cgroup" bash -c "bpftool cgroup show $CG | grep -q mn_qemu_device && bpftool cgroup show $CG | grep -q mn_qemu_egress"
check "sandbox: allowed device works (/dev/null)" in_cg python3 -c "open('/dev/null','rb').close()"
check "sandbox no lease: unlisted device still opens" dev_ok
check "sandbox no lease: allowed port reachable" cg_http 18080
cg_http 18081 || true
hits() { jpath vm_sandbox_status "[h['target'] for h in d['$1']]" | grep -q "$2"; }
check "sandbox: device hit recorded" hits device_hits '1:11'
check "sandbox: egress hit recorded" hits egress_hits '18081'

enforce
check "sandbox enforce: status enforcing" [ "$(jpath vm_sandbox_status "d['enforcing']")" = True ]
dev_denied() { ! dev_ok; }
check "sandbox enforce: unlisted device denied" dev_denied
check "sandbox enforce: allowed device works" in_cg python3 -c "open('/dev/null','rb').close()"
check "sandbox enforce: allowed port reachable" cg_http 18080
check "sandbox enforce: other port dropped (timeout, not refused)" cg_dropped 18081
check "sandbox enforce: host outside scope unaffected" python3 -c "open('/dev/kmsg','rb').close()"

observe
check "sandbox observe: device opens again" dev_ok

req "{\"op\":\"vm_sandbox_detach\",\"vm\":\"$VM\"}" | must
check "sandbox: detached" bash -c "! bpftool cgroup show $CG 2>/dev/null | grep -q mn_qemu"
rmdir "$CG" 2>/dev/null || true

# ---- chaos faults --------------------------------------------------------------
# A bridged veth stands in for a VM tap (explicit `tap`, so real taps are
# never looked up). netem acts on traffic towards the "VM"; the partition is
# the bridge table. Runs last because it restarts bpfd.

CH_NS=mnvme-cn
CH_BR=mnvme-cb
CH_IF=mnvme-c0
ch_cleanup() {
  req '{"op":"vm_chaos_stop","prefix":"smoke"}' >/dev/null 2>&1 || true
  ip netns del "$CH_NS" 2>/dev/null || true
  ip link del "$CH_IF" 2>/dev/null || true
  ip link del "$CH_BR" 2>/dev/null || true
}
trap 'ch_cleanup; cleanup' EXIT
ip link add "$CH_BR" type bridge
ip addr add 10.199.91.1/24 dev "$CH_BR"
ip link set "$CH_BR" up
ip netns add "$CH_NS"
ip link add "$CH_IF" type veth peer name "${CH_IF}p"
ip link set "$CH_IF" master "$CH_BR" up
ip link set "${CH_IF}p" netns "$CH_NS"
ip netns exec "$CH_NS" ip addr add 10.199.91.2/24 dev "${CH_IF}p"
ip netns exec "$CH_NS" ip link set "${CH_IF}p" up
for _ in $(seq 20); do ping -c1 -W1 10.199.91.2 >/dev/null 2>&1 && break; sleep 0.25; done

rtt() { ping -c3 -i0.3 -W2 10.199.91.2 2>/dev/null | awk -F/ '/^rtt|^round-trip/ {printf "%d", $5}'; }
root_kind() { tc -j qdisc show dev "$CH_IF" root | python3 -c "import json,sys; q=json.load(sys.stdin); print(q[0]['kind'] if q else '')"; }
fault() { req "{\"op\":\"vm_chaos_start\",\"fault\":{\"id\":\"$1\",\"vm\":\"mnsmk-chaos\",\"tap\":\"$CH_IF\",$2}}"; }
ch_ok() { ip netns exec "$CH_NS" ping -c1 -W1 10.199.91.1 >/dev/null 2>&1; }
active() { jpath vm_chaos_status "sorted(f['id'] for f in d['faults'])"; }

R=$(rtt)
check "chaos: baseline rtt under 50 ms" bash -c "[ ${R:-999} -lt 50 ]"
tc qdisc replace dev "$CH_IF" root fq
fault smoke-lat '"delay_ms":300,"jitter_ms":0,"secs":60' | must
check "chaos: netem root installed" [ "$(root_kind)" = netem ]
R=$(rtt); echo "      rtt with 300 ms delay: ${R:-?} ms"
check "chaos: latency applied" bash -c "[ ${R:-0} -ge 280 ]"
dup=$(fault smoke-lat2 '"loss_pct":10,"secs":60')
check "chaos: second netem fault on the same tap refused" grep -qi "already" <<<"$dup"
big=$(fault smoke-big '"delay_ms":20000,"secs":60')
check "chaos: delay over the cap rejected" grep -q '"ok":false' <<<"$big"
long=$(fault smoke-long '"delay_ms":10,"secs":7200')
check "chaos: lease over an hour rejected" grep -q '"ok":false' <<<"$long"
req '{"op":"vm_chaos_stop","id":"smoke-lat"}' | must
check "chaos: stop puts the prior fq back" [ "$(root_kind)" = fq ]
tc qdisc del dev "$CH_IF" root 2>/dev/null || true

fault smoke-short '"loss_pct":100,"secs":3' | must
check "chaos: 100% loss drops traffic to the VM" not ping -c1 -W1 10.199.91.2
sleep 5
check "chaos: lease end lifts the fault" [ "$(active)" = "[]" ]
check "chaos: lease end removes netem" [ "$(root_kind)" != netem ]
check "chaos: traffic back after lease" ping -c1 -W1 10.199.91.2

if nft list table bridge machina_chaos >/dev/null 2>&1; then
  echo "SKIP  chaos partition: machina_chaos already in use"
else
  check "chaos: bridge peer reachable before partition" ch_ok
  fault smoke-part '"partition":["10.199.91.1/32"],"secs":60' | must
  check "chaos: partition table installed" nft list table bridge machina_chaos
  check "chaos: partition drops VM to peer" not ch_ok
  req '{"op":"vm_chaos_stop","prefix":"smoke-part"}' | must
  check "chaos: stop removes the partition table" not nft list table bridge machina_chaos
  check "chaos: peer reachable again" ch_ok
fi

fault smoke-persist '"delay_ms":250,"secs":120' | must
kill "$BPFD_PID"; wait "$BPFD_PID" 2>/dev/null || true
rm -f "$SOCK"
RUST_LOG=${RUST_LOG:-info} "$BPFD" --socket "$SOCK" --state-dir "$WORK" --socket-group "" >>"$WORK/bpfd.log" 2>&1 &
BPFD_PID=$!
for _ in $(seq 50); do [[ -S "$SOCK" ]] && break; sleep 0.2; done
sleep 1.5
check "chaos: fault survives a bpfd restart" [ "$(active)" = "['smoke-persist']" ]
check "chaos: netem still on after restart" [ "$(root_kind)" = netem ]
req '{"op":"vm_chaos_stop","prefix":"smoke"}' | must
check "chaos: stop by prefix clears everything" [ "$(active)" = "[]" ]
check "chaos: no netem left" [ "$(root_kind)" != netem ]
ch_cleanup
trap cleanup EXIT

if grep -qiE "verifier|panicked" "$WORK/bpfd.log"; then
  echo "---- bpfd log (verifier/panic) ----"
  grep -iE -A20 "verifier|panicked" "$WORK/bpfd.log" | head -60
fi
echo "passed=$PASS failed=$FAIL"
[[ $FAIL -eq 0 ]]
