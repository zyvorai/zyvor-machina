#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Netns smoke test for machina-bpfd: a veth pair stands in for a VM tap.
# Runs bpfd on a private socket/state dir with auto-attach limited to the test
# veth, so real VM taps on the host are never touched.
#
#   sudo ./scripts/bpf/netns-smoke.sh [path/to/machina-bpfd]
set -euo pipefail

BPFD=${1:-./target/debug/machina-bpfd}
NS=mnbpf-smoke
HOST_IF=mnt-smoke0
PEER_IF=mnt-smoke0p
HOST_IP=10.199.77.1
PEER_IP=10.199.77.2
WORK=$(mktemp -d /tmp/mnbpf-smoke.XXXX)
SOCK=$WORK/bpfd.sock
# mn_sockops is scoped to this cgroup instead of the host root.
TCP_CG=/sys/fs/cgroup/mnbpf-smoke-tcp
PASS=0
FAIL=0

cleanup() {
  [[ -n "${BPFD_PID:-}" ]] && kill "$BPFD_PID" 2>/dev/null && wait "$BPFD_PID" 2>/dev/null || true
  [[ -n "${HTTP_PID:-}" ]] && kill "$HTTP_PID" 2>/dev/null || true
  ip netns del "$NS" 2>/dev/null || true
  ip link del "$HOST_IF" 2>/dev/null || true
  ip link del mnsmoke-rtnl0 2>/dev/null || true
  ip netns del mnd-a 2>/dev/null || true
  ip netns del mnd-b 2>/dev/null || true
  [[ -d "$TCP_CG" ]] && rmdir "$TCP_CG" 2>/dev/null || true
  rmdir /sys/fs/cgroup/mnsmoke-attr.service 2>/dev/null || true
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

jq_ok() { python3 -c 'import json,sys; d=json.load(sys.stdin); sys.exit(0 if d.get("ok") else 1)'; }
must() {
  local out
  out=$(cat)
  if ! jq_ok <<<"$out"; then
    echo "WARN  request failed: $out"
  fi
}
policy() { req "{\"op\":\"apply_policy\",\"policy\":{\"id\":\"$1\",\"kind\":\"$2\",\"match\":\"$3\"}}" | must; }
unpolicy() { req "{\"op\":\"remove_policy\",\"id\":\"$1\"}" | must; }
enforce() { req '{"op":"set_mode","mode":"enforce","lease_secs":60}' | must; }
observe() { req '{"op":"set_mode","mode":"observe"}' | must; }

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

ping_ok() { ip netns exec "$NS" ping -c1 -W1 "$HOST_IP" >/dev/null 2>&1; }
ping_blocked() { ! ping_ok; }
http_ok() { ip netns exec "$NS" curl -s -m2 -o /dev/null "http://$HOST_IP:18080/"; }

ip netns add "$NS"
ip link add "$HOST_IF" type veth peer name "$PEER_IF"
ip link set "$PEER_IF" netns "$NS"
ip addr add "$HOST_IP/30" dev "$HOST_IF"
ip link set "$HOST_IF" up
ip netns exec "$NS" ip addr add "$PEER_IP/30" dev "$PEER_IF"
ip netns exec "$NS" ip link set "$PEER_IF" up
ip netns exec "$NS" ip link set lo up

python3 -m http.server 18080 --bind "$HOST_IP" >/dev/null 2>&1 &
HTTP_PID=$!

cat >"$WORK/bpfd-state.json" <<EOF
{"telemetry": {"exec": true, "connect": true, "flows": true, "dns": true,
               "file_watch": ["/etc/shadow"], "iface_patterns": ["$HOST_IF"]}}
EOF

mkdir -p "$TCP_CG"
MACHINA_BPF_SOCKOPS_CGROUP=$TCP_CG MACHINA_BPF_TLSFP_CGROUP=$TCP_CG MACHINA_BPF_L7S_CGROUP=$TCP_CG RUST_LOG=${RUST_LOG:-info} "$BPFD" --socket "$SOCK" --state-dir "$WORK" --socket-group "" \
  >"$WORK/bpfd.log" 2>&1 &
BPFD_PID=$!
for _ in $(seq 50); do [[ -S "$SOCK" ]] && break; sleep 0.2; done
if [[ ! -S "$SOCK" ]]; then
  echo "bpfd failed to start:"
  cat "$WORK/bpfd.log"
  exit 1
fi

check "status ok" bash -c "$(declare -f req jq_ok); SOCK=$SOCK; req '{\"op\":\"status\"}' | jq_ok"
check "test veth auto-attached" bash -c "$(declare -f req); SOCK=$SOCK; req '{\"op\":\"list_interfaces\"}' | grep -q $HOST_IF"
check "baseline ping" ping_ok
check "baseline http" http_ok
sleep 1
check "flows recorded" bash -c "$(declare -f req); SOCK=$SOCK; req '{\"op\":\"flows\"}' | grep -q $PEER_IP"

req "{\"op\":\"apply_policy\",\"policy\":{\"id\":\"smoke-deny\",\"name\":\"smoke\",\"kind\":\"deny_ip\",\"match\":\"$HOST_IP/32\",\"enabled\":true}}" | must
check "observe: deny_ip does not block" ping_ok
sleep 0.5
check "observe: deny event recorded" bash -c "$(declare -f req); SOCK=$SOCK; req '{\"op\":\"events\",\"kind\":\"deny\"}' | grep -q observed"

req '{"op":"set_mode","mode":"enforce","lease_secs":60}' | must
check "enforce: deny_ip blocks ping" ping_blocked
req '{"op":"set_mode","mode":"observe"}' | must
check "observe again: ping restored" ping_ok
req '{"op":"remove_policy","id":"smoke-deny"}' | must

req "{\"op\":\"apply_policy\",\"policy\":{\"id\":\"smoke-allow\",\"name\":\"smoke\",\"kind\":\"tc_allow\",\"match\":\"$HOST_IP:18080/tcp\",\"enabled\":true}}" | must
req '{"op":"set_mode","mode":"enforce","lease_secs":60}' | must
check "enforce allowlist: allowed http passes" http_ok
check "enforce allowlist: icmp blocked" ping_blocked
req '{"op":"set_mode","mode":"observe"}' | must
req '{"op":"remove_policy","id":"smoke-allow"}' | must
check "allowlist removed: ping restored" ping_ok

req '{"op":"capture_start","iface":"'"$HOST_IF"'","duration_secs":3}' | must
ip netns exec "$NS" ping -c3 -i0.2 "$HOST_IP" >/dev/null 2>&1 || true
sleep 4
check "capture has packets" bash -c "$(declare -f req); SOCK=$SOCK; req '{\"op\":\"capture_list\"}' | python3 -c 'import json,sys; d=json.load(sys.stdin)[\"data\"]; sys.exit(0 if d and d[0][\"packets\"]>0 else 1)'"

cat /etc/hostname >/dev/null
check "exec events recorded" bash -c "$(declare -f req); SOCK=$SOCK; req '{\"op\":\"proc_events\",\"kind\":\"exec\"}' | grep -q '\"exec\"'"
check "net health readable" bash -c "$(declare -f req jq_ok); SOCK=$SOCK; req '{\"op\":\"net_health\"}' | jq_ok"


# Process / file / port enforcement (SIGKILL / drop only while the lease is active).
cp /bin/true "$WORK/mn-forbidden"
echo secret >"$WORK/secret"
policy smoke-exec deny_process "$WORK/mn-forbidden"
policy smoke-file deny_file "$WORK/secret"
policy smoke-port deny_port "18080/tcp"
check "observe: denied exec still runs" "$WORK/mn-forbidden"
check "observe: denied file still readable" bash -c "cat '$WORK/secret' >/dev/null"
check "observe: denied port still reachable" http_ok
enforce
check "enforce: denied exec killed" bash -c "! '$WORK/mn-forbidden'"
check "enforce: denied file read killed" bash -c "! cat '$WORK/secret' >/dev/null 2>&1"
check "enforce: denied port blocked" bash -c "$(declare -f http_ok); NS=$NS HOST_IP=$HOST_IP; ! http_ok"
check "enforce: other exec unaffected" /bin/true
observe
check "observe again: exec restored" "$WORK/mn-forbidden"
sleep 0.5
check "exec deny event attributed" bash -c "$(declare -f req); SOCK=$SOCK; req '{\"op\":\"proc_events\",\"kind\":\"exec\",\"limit\":5000}' | grep -q smoke-exec"
unpolicy smoke-exec
unpolicy smoke-file
unpolicy smoke-port

# QoS: cap traffic towards the workload at 8 Mbit/s and time a 3 MB download.
head -c 3000000 /dev/urandom >"$WORK/blob"
(cd "$WORK" && python3 -m http.server 18081 --bind "$HOST_IP" >/dev/null 2>&1) &
BLOB_PID=$!
sleep 0.5
req '{"op":"set_qos","iface":"'"$HOST_IF"'","ingress_bps":8000000}' | must
start=$(date +%s.%N)
ip netns exec "$NS" curl -s -m20 -o /dev/null "http://$HOST_IP:18081/blob" || true
secs=$(echo "$(date +%s.%N) - $start" | bc)
echo "      3 MB at 8 Mbit/s took ${secs}s"
check "qos paces download (>= 2s)" bash -c "(( \$(echo '$secs >= 2' | bc) ))"
req '{"op":"set_qos","iface":"'"$HOST_IF"'","ingress_bps":0}' | must
kill $BLOB_PID 2>/dev/null || true

# Connection rate limiting: 2 new connections/s, burst 2. The opening SYN of
# each excess connection is dropped while enforcing (observed otherwise).
http_burst() {
  local ok=0
  for _ in $(seq 10); do
    ip netns exec "$NS" curl -s -m0.5 -o /dev/null "http://$HOST_IP:18080/" && ok=$((ok + 1))
  done
  echo "$ok"
}
policy smoke-rate rate_limit "2/s burst 2"
n=$(http_burst)
echo "      observe: $n/10 connections"
check "observe: rate_limit does not block" test "$n" -eq 10
check "observe: rate_limit counted" bash -c "$(declare -f req); SOCK=$SOCK; req '{\"op\":\"status\"}' | python3 -c 'import json,sys; sys.exit(0 if json.load(sys.stdin)[\"data\"][\"counters\"][\"rate_limited\"]>0 else 1)'"
enforce
n=$(http_burst)
echo "      enforce: $n/10 connections"
check "enforce: rate_limit drops excess connections" bash -c "(( $n >= 1 && $n <= 6 ))"
observe
unpolicy smoke-rate
sleep 1
n=$(http_burst)
check "rate_limit removed: http restored" test "$n" -eq 10

# L7 visibility: HTTP request line + Host, TLS ClientHello SNI/ALPN.
python3 - "$HOST_IP" <<'PY' >/dev/null 2>&1 &
import socket, sys, time
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind((sys.argv[1], 18443))
s.listen(8)
end = time.time() + 30
while time.time() < end:
    c, _ = s.accept()
    c.recv(4096)
    c.close()
PY
TLS_PID=$!
sleep 0.5
ip netns exec "$NS" curl -s -m2 -o /dev/null -A mn-smoke/1 "http://$HOST_IP:18080/l7-probe" || true
ip netns exec "$NS" python3 - "$HOST_IP" <<'PY' >/dev/null 2>&1 || true
import socket, ssl, sys
ctx = ssl.create_default_context()
ctx.set_alpn_protocols(["h2", "http/1.1"])
ctx.check_hostname = False
ctx.verify_mode = ssl.CERT_NONE
s = socket.create_connection((sys.argv[1], 18443), timeout=2)
try:
    ctx.wrap_socket(s, server_hostname="smoke.machina.test")
except Exception:
    pass
PY
sleep 1
kill $TLS_PID 2>/dev/null || true
l7() { req '{"op":"l7","limit":200}'; }
check "l7: http request recorded" bash -c "$(declare -f req l7); SOCK=$SOCK; l7 | python3 -c 'import json,sys; d=json.load(sys.stdin)[\"data\"]; sys.exit(0 if any(r.get(\"path\")==\"/l7-probe\" and r.get(\"method\")==\"GET\" and r.get(\"user_agent\")==\"mn-smoke/1\" for r in d) else 1)'"
check "l7: tls sni + alpn recorded" bash -c "$(declare -f req l7); SOCK=$SOCK; l7 | python3 -c 'import json,sys; d=json.load(sys.stdin)[\"data\"]; sys.exit(0 if any(r.get(\"host\")==\"smoke.machina.test\" and \"h2\" in (r.get(\"alpn\") or []) for r in d) else 1)'"

# Per-workload accounting: the test veth has no VM, so it is keyed by iface.
acct() { req "{\"op\":\"accounting\",\"vm\":\"iface:$HOST_IF\"}"; }
check "accounting: bytes counted both ways" bash -c "$(declare -f req acct); SOCK=$SOCK HOST_IF=$HOST_IF; acct | python3 -c 'import json,sys; d=json.load(sys.stdin)[\"data\"]; sys.exit(0 if d and d[0][\"tx_bytes\"]>0 and d[0][\"rx_bytes\"]>0 and d[0][\"rx_pkts\"]>0 else 1)'"
check "accounting: enforcement drops counted" bash -c "$(declare -f req acct); SOCK=$SOCK HOST_IF=$HOST_IF; acct | python3 -c 'import json,sys; sys.exit(0 if json.load(sys.stdin)[\"data\"][0][\"drops\"]>0 else 1)'"
req '{"op":"reset_accounting"}' | must
check "accounting: reset zeroes totals" bash -c "$(declare -f req acct); SOCK=$SOCK HOST_IF=$HOST_IF; acct | python3 -c 'import json,sys; d=json.load(sys.stdin)[\"data\"]; sys.exit(0 if d[0][\"tx_bytes\"] < 2000 else 1)'"

# deny_dns: answers for *.blocked.test feed their A records into the deny set.
DNS_TARGET=10.199.77.5
ip addr add "$DNS_TARGET/32" dev "$HOST_IF"
ip netns exec "$NS" ip route add "$DNS_TARGET/32" dev "$PEER_IF"
python3 - "$HOST_IP" "$DNS_TARGET" <<'PY' >/dev/null 2>&1 &
import socket, sys, time
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.bind((sys.argv[1], 53))
ip = socket.inet_aton(sys.argv[2])
end = time.time() + 30
while time.time() < end:
    q, a = s.recvfrom(512)
    end_q = 12
    while q[end_q] != 0:
        end_q += q[end_q] + 1
    question = q[12:end_q + 5]
    ans = b"\xc0\x0c\x00\x01\x00\x01\x00\x00\x00\x3c\x00\x04" + ip
    s.sendto(q[:2] + b"\x81\x80\x00\x01\x00\x01\x00\x00\x00\x00" + question + ans, a)
PY
DNS_PID=$!
sleep 0.5
dns_query() {
  ip netns exec "$NS" python3 - "$HOST_IP" "$1" <<'PY'
import socket, sys
q = b"\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00"
q += b"".join(bytes([len(p)]) + p.encode() for p in sys.argv[2].split(".")) + b"\x00\x00\x01\x00\x01"
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.settimeout(2)
s.sendto(q, (sys.argv[1], 53))
s.recv(512)
PY
}
ping_target() { ip netns exec "$NS" ping -c1 -W1 "$DNS_TARGET" >/dev/null 2>&1; }
check "dns: baseline target reachable" ping_target
policy smoke-dns deny_dns "*.blocked.test"
enforce
dns_query www.blocked.test || true
sleep 1
check "dns: query recorded" bash -c "$(declare -f req); SOCK=$SOCK; req '{\"op\":\"dns\",\"limit\":200}' | grep -q www.blocked.test"
check "enforce: deny_dns blocks the resolved address" bash -c "$(declare -f ping_target); NS=$NS DNS_TARGET=$DNS_TARGET; ! ping_target"
check "enforce: deny_dns leaves other hosts alone" ping_ok
observe
unpolicy smoke-dns
check "deny_dns removed: target restored" ping_target
kill $DNS_PID 2>/dev/null || true

# XDP shield on the test veth (host side = traffic from the netns): per-source
# ICMP bucket of 5 pps, then deny/allow CIDRs. Drops only under the lease.
shield() { req "{\"op\":\"shield_configure\",\"config\":{\"iface\":\"$HOST_IF\",\"protected\":[\"$HOST_IP\"],\"icmp_pps\":5,\"burst_secs\":1,$1}}" | must; }
shjs() { req '{"op":"shield_status"}' | python3 -c "import json,sys; d=json.load(sys.stdin)['data']; print($1)"; }
flood() { ip netns exec "$NS" ping -c40 -i0.01 -W1 -q "$HOST_IP" 2>/dev/null | sed -n 's/.* \([0-9]*\) received.*/\1/p'; }
xdp_on() { ip -d link show dev "$HOST_IF" | grep -q 'prog/xdp'; }
shield '"mode":"audit"'
check "shield: dispatcher attached to test veth" xdp_on
n=$(flood)
echo "      shield audit: $n/40 replies"
check "shield audit: flood not dropped" test "${n:-0}" -eq 40
check "shield audit: over-rate audited" test "$(shjs "d['stats']['audited']")" -gt 0
check "shield: top source is the netns" bash -c "$(declare -f req shjs); SOCK=$SOCK; shjs \"[s['addr']+'/'+s['class'] for s in d['sources']]\" | grep -q '$PEER_IP/icmp'"
shield '"mode":"enforce"'
check "shield enforce without lease: not enforcing" test "$(shjs "d['enforcing']")" = False
enforce
check "shield enforce: status enforcing" test "$(shjs "d['enforcing']")" = True
sleep 1
n=$(flood)
echo "      shield enforce: $n/40 replies"
check "shield enforce: flood rate-limited" bash -c "(( ${n:-40} >= 1 && ${n:-40} <= 20 ))"
check "shield enforce: drops counted" test "$(shjs "d['stats']['dropped']")" -gt 0
shield "\"mode\":\"enforce\",\"deny\":[\"$PEER_IP/32\"]"
sleep 1
check "shield enforce: deny CIDR blocks source" ping_blocked
shield "\"mode\":\"enforce\",\"allow\":[\"$PEER_IP/32\"]"
n=$(flood)
check "shield enforce: allow CIDR bypasses limits" test "${n:-0}" -eq 40
observe
shield '"mode":"off"'
check "shield off: dispatcher detached" bash -c "$(declare -f xdp_on); HOST_IF=$HOST_IF; ! xdp_on"
check "shield off: ping restored" ping_ok

# TCP connect latency / pressure via mn_sockops (test cgroup only) and the
# ICMP error histogram on the test veth.
in_tcp_cg() { bash -c "echo \$\$ > $TCP_CG/cgroup.procs && exec \"\$@\"" _ "$@"; }
health() { req '{"op":"net_health"}' | python3 -c "import json,sys; d=json.load(sys.stdin)['data']; print($1)"; }
check "sockops: attached to test cgroup" test "$(health "d.get('sockops')")" = "$TCP_CG"
for _ in 1 2 3; do in_tcp_cg curl -s -m2 -o /dev/null "http://$HOST_IP:18080/" || true; done
in_tcp_cg curl -s -m2 -o /dev/null "http://$HOST_IP:18099/" || true
check "sockops: connect latency recorded" test "$(health "sum(c['count'] for c in d['connect'] if c['addr']=='$HOST_IP' and c['port']==18080)")" -ge 3
check "sockops: refused connect counted as failure" test "$(health "sum(c['failures'] for c in d['connect'] if c['port']==18099)")" -ge 1
check "sockops: peer pressure snapshot" test "$(health "sum(1 for p in d['pressure'] if p['addr']=='$HOST_IP' and p['mss']>0)")" -ge 1
curl -s -m2 -o /dev/null "http://$HOST_IP:18080/" || true
check "sockops: other cgroups not observed" test "$(health "sum(c['count'] for c in d['connect'] if c['addr']=='$HOST_IP' and c['port']==18080)")" -le 3
ip netns exec "$NS" python3 -c "import socket; s=socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.sendto(b'x', ('$HOST_IP', 33999))"
sleep 0.3
check "icmp: port unreachable counted on test veth" bash -c "$(declare -f req); SOCK=$SOCK; req '{\"op\":\"icmp_errors\"}' | python3 -c 'import json,sys; d=json.load(sys.stdin)[\"data\"]; sys.exit(0 if any(e[\"iface\"]==\"$HOST_IF\" and e[\"kind\"]==\"unreachable\" and e[\"code\"]==3 and e[\"direction\"]==\"to_workload\" for e in d) else 1)'"

# TLS: JA3/JA4 from mn_tlsfp (test cgroup) and the tap L7 path, and HTTP
# metadata from libssl uprobes limited to `curl`.
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj /CN=smoke.test \
  -keyout "$WORK/tls.key" -out "$WORK/tls.crt" >/dev/null 2>&1
python3 - "$HOST_IP" "$WORK" <<'PY' >/dev/null 2>&1 &
import http.server, ssl, sys, threading, time
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200); self.end_headers(); self.wfile.write(b"secret-body")
    def log_message(self, *a): pass
srv = http.server.HTTPServer((sys.argv[1], 18444), H)
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.load_cert_chain(sys.argv[2] + "/tls.crt", sys.argv[2] + "/tls.key")
srv.socket = ctx.wrap_socket(srv.socket, server_side=True)
threading.Timer(40, srv.shutdown).start()
srv.serve_forever()
PY
HTTPS_PID=$!
sleep 1
req '{"op":"tls_configure","config":{"fingerprints":true,"ssl_uprobes":true,"ssl_comms":["curl"]}}' | must
tls() { req "{\"op\":\"$1\",\"limit\":500}" | python3 -c "import json,sys; d=json.load(sys.stdin)['data']; print($2)"; }
check "tls: fingerprint sampler on test cgroup" test "$(tls tls_status "d.get('fingerprint_cgroup')")" = "$TCP_CG"
check "tls: libssl uprobes attached" test "$(tls tls_status "len(d['ssl_libraries'])")" -ge 1
in_tcp_cg curl -sk -m3 -o /dev/null --resolve "smoke.test:18444:$HOST_IP" "https://smoke.test:18444/ssl-probe" || true
ip netns exec "$NS" python3 - "$HOST_IP" <<'PY' >/dev/null 2>&1 || true
import socket, ssl, sys
ctx = ssl.create_default_context(); ctx.check_hostname = False; ctx.verify_mode = ssl.CERT_NONE
ctx.set_alpn_protocols(["http/1.1"])
with ctx.wrap_socket(socket.create_connection((sys.argv[1], 18444), timeout=2), server_hostname="tap.smoke.test") as s:
    s.sendall(b"GET /from-python HTTP/1.1\r\nHost: tap\r\n\r\n"); s.recv(100)
PY
sleep 1
check "tls: host ClientHello fingerprinted (JA4 t13d…)" test "$(tls tls_fingerprints "sum(1 for f in d if f['source']=='host' and f['sni']=='smoke.test' and f['ja4'].startswith('t13d') and len(f['ja3_hash'])==32)")" -ge 1
check "tls: tap ClientHello fingerprinted" test "$(tls tls_fingerprints "sum(1 for f in d if f['source']=='tap' and f['sni']=='tap.smoke.test')")" -ge 1
if curl -V 2>/dev/null | grep -q OpenSSL; then
  check "ssl: curl request metadata (method/host/path)" test "$(tls ssl_events "sum(1 for r in d if r['comm']=='curl' and r.get('method')=='GET' and r.get('path')=='/ssl-probe' and (r.get('host') or '').startswith('smoke.test'))")" -ge 1
  check "ssl: response status captured" test "$(tls ssl_events "sum(1 for r in d if r['comm']=='curl' and r.get('status')==200)")" -ge 1
  check "ssl: no plaintext bodies leave bpfd" bash -c "$(declare -f req); SOCK=$SOCK; ! req '{\"op\":\"ssl_events\",\"limit\":500}' | grep -q secret-body"
  check "ssl: processes outside the allowlist ignored" test "$(tls ssl_events "sum(1 for r in d if r['comm']!='curl')")" -eq 0
else
  echo "SKIP  ssl uprobes (curl is not linked against OpenSSL)"
fi
req '{"op":"tls_configure","config":{}}' | must
check "tls off: sampler and uprobes detached" test "$(tls tls_status "(d.get('fingerprint_cgroup'), len(d['ssl_libraries']))")" = "(None, 0)"
kill $HTTPS_PID 2>/dev/null || true

# Node isolation on the test veth only (never a real uplink): XDP ingress
# from the netns, mn_nodeiso on egress towards it. Own short lease.
iso() { req "{\"op\":\"node_iso_configure\",\"config\":{\"iface\":\"$HOST_IF\",$1}}"; }
isojs() { req '{"op":"node_iso_status"}' | python3 -c "import json,sys; d=json.load(sys.stdin)['data']; print($1)"; }
host_ping() { ping -c1 -W1 "$PEER_IP" >/dev/null 2>&1; }
check "nodeiso: lease is mandatory" bash -c "$(declare -f req iso); SOCK=$SOCK HOST_IF=$HOST_IF; ! iso '\"enabled\":true' | grep -q '\"ok\":true'"
check "nodeiso: lease capped at 900s" bash -c "$(declare -f req iso); SOCK=$SOCK HOST_IF=$HOST_IF; ! iso '\"enabled\":true,\"lease_secs\":3600' | grep -q '\"ok\":true'"
check "nodeiso: refuses to drop SSH without an exempt CIDR" bash -c "$(declare -f req iso); SOCK=$SOCK HOST_IF=$HOST_IF; ! iso '\"enabled\":true,\"lease_secs\":30,\"allow_tcp\":[6443]' | grep -q '\"ok\":true'"
iso '"enabled":true,"lease_secs":30,"dry_run":true,"allow_icmp":false' | must
check "nodeiso dry-run: ping still passes" ping_ok
check "nodeiso dry-run: would-drop counted" test "$(isojs "d['stats']['would_drop']")" -gt 0
check "nodeiso dry-run: not isolating" test "$(isojs "d['isolating']")" = False
iso '"enabled":true,"lease_secs":30,"allow_icmp":false,"allow_tcp":[22,18080]' | must
check "nodeiso: isolating under lease" test "$(isojs "d['isolating'] and 0 < d['lease_remaining_secs'] <= 30")" = True
check "nodeiso: inbound ping dropped (XDP)" ping_blocked
check "nodeiso: outbound ping dropped (tc egress)" bash -c "$(declare -f host_ping); PEER_IP=$PEER_IP; ! host_ping"
check "nodeiso: allowlisted tcp port still served" http_ok
check "nodeiso: drops counted both ways" test "$(isojs "d['stats']['dropped_in'] > 0 and d['stats']['dropped_out'] > 0")" = True
check "nodeiso: policy lease untouched" bash -c "$(declare -f req); SOCK=$SOCK; req '{\"op\":\"status\"}' | python3 -c 'import json,sys; m=json.load(sys.stdin)[\"data\"][\"mode\"]; sys.exit(0 if m[\"mode\"]==\"observe\" and \"node_isolation\" in m[\"covers\"] else 1)'"
iso "\"enabled\":true,\"lease_secs\":30,\"allow_icmp\":false,\"exempt\":[\"$PEER_IP/32\"]" | must
check "nodeiso: exempt CIDR passes" ping_ok
iso '"enabled":true,"lease_secs":10,"allow_icmp":false' | must
check "nodeiso: short lease armed" ping_blocked
sleep 12
check "nodeiso: lease lapsed, traffic restored" ping_ok
check "nodeiso: auto-reverted and detached" test "$(isojs "(d['lease_expired'], d['attached'], d['isolating'])")" = "(True, None, False)"
check "nodeiso: dispatcher gone from test veth" bash -c "$(declare -f xdp_on); HOST_IF=$HOST_IF; ! xdp_on"
iso '"enabled":true,"lease_secs":30,"allow_icmp":false' | must
iso '"enabled":false' | must
check "nodeiso off: ping restored" ping_ok

# Network change audit: requests are credited to the process that sent them.
rtnl_has() {
  req '{"op":"rtnl_events","limit":2000}' | python3 -c "import json,sys; d=json.load(sys.stdin)['data']; sys.exit(0 if any($1 for r in d) else 1)"
}
check "rtnl: kprobe attached" bash -c "$(declare -f req); SOCK=$SOCK; req '{\"op\":\"rtnl_status\"}' | grep -q '\"attached\":true'"
ip link add mnsmoke-rtnl0 type dummy
ip route add 10.199.250.0/24 dev "$HOST_IF"
ip route del 10.199.250.0/24 dev "$HOST_IF"
ip link del mnsmoke-rtnl0
sleep 0.5
check "rtnl: link create by ip recorded" rtnl_has "r['kind']=='link' and r['action']=='new' and r['create'] and r['comm']=='ip' and r.get('iface')=='mnsmoke-rtnl0'"
check "rtnl: link delete recorded" rtnl_has "r['kind']=='link' and r['action']=='del' and r['comm']=='ip'"
check "rtnl: route add with destination" rtnl_has "r['kind']=='route' and r['action']=='new' and r.get('dst')=='10.199.250.0/24' and r.get('iface')=='$HOST_IF'"
check "rtnl: route delete recorded" rtnl_has "r['kind']=='route' and r['action']=='del' and r.get('dst')=='10.199.250.0/24'"
ip netns exec "$NS" ip link add mnsmoke-ns0 type dummy
ip netns exec "$NS" ip link del mnsmoke-ns0
sleep 0.5
check "rtnl: host requests carry the host netns" rtnl_has "r['comm']=='ip' and r.get('host_netns') is True"
check "rtnl: other netns filtered in the kernel" bash -c "$(declare -f req rtnl_has); SOCK=$SOCK; ! rtnl_has \"r.get('iface')=='mnsmoke-ns0'\""

# Sampled plaintext L7 on the test cgroup: a fake Redis and PostgreSQL
# server; only operation names may come back out of bpfd.
python3 - <<'PY' >/dev/null 2>&1 &
import socket, threading, time
def serve(port, reply):
    s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("127.0.0.1", port)); s.listen(8)
    def conn(c):
        while c.recv(4096): c.sendall(reply)
    while True:
        c, _ = s.accept(); threading.Thread(target=conn, args=(c,), daemon=True).start()
for p, r in ((16379, b"+OK\r\n"), (15432, b"C\x00\x00\x00\x0dSELECT 1\x00")):
    threading.Thread(target=serve, args=(p, r), daemon=True).start()
time.sleep(30)
PY
L7S_PID=$!
sleep 0.5
l7sjs() { req '{"op":"l7_sample_status"}' | python3 -c "import json,sys; d=json.load(sys.stdin)['data']; print($1)"; }
req '{"op":"l7_sample_configure","config":{"enabled":true,"flow_gap_ms":0,"ports":[{"port":16379,"protocol":"redis"},{"port":15432,"protocol":"postgres"}]}}' | must
check "l7s: sampler on test cgroup" test "$(l7sjs "d.get('attached')")" = "$TCP_CG"
in_tcp_cg python3 - <<'PY' || true
import socket, struct, time
r = socket.create_connection(("127.0.0.1", 16379), timeout=2)
r.sendall(b"*3\r\n$3\r\nSET\r\n$9\r\nsecretkey\r\n$11\r\nsecretvalue\r\n"); r.recv(64)
time.sleep(0.05)
r.sendall(b"*2\r\n$3\r\nGET\r\n$9\r\nsecretkey\r\n"); r.recv(64)
q = b"select * from users where pw='secretpw'\x00"
p = socket.create_connection(("127.0.0.1", 15432), timeout=2)
p.sendall(b"Q" + struct.pack(">I", len(q) + 4) + q); p.recv(64)
PY
sleep 1
l7s_has() {
  req '{"op":"l7","limit":500}' | python3 -c "import json,sys; d=json.load(sys.stdin)['data']; sys.exit(0 if any($1 for r in d) else 1)"
}
check "l7s: redis SET sampled" l7s_has "r['protocol']=='redis' and r.get('method')=='SET' and r['server_port']==16379 and r['direction']=='outbound'"
check "l7s: redis GET sampled" l7s_has "r['protocol']=='redis' and r.get('method')=='GET'"
check "l7s: redis reply sampled" l7s_has "r['protocol']=='redis' and r.get('method')=='reply REPLY'"
check "l7s: postgres query verb" l7s_has "r['protocol']=='postgres' and r.get('method')=='SELECT' and r['server_port']==15432"
check "l7s: no keys, values or literals leave bpfd" bash -c "$(declare -f req); SOCK=$SOCK; ! req '{\"op\":\"l7\",\"limit\":500}' | grep -q secret"
check "l7s: top operations counted" test "$(l7sjs "sum(o['count'] for o in d['top'] if o['op'] in ('SET','GET','SELECT'))")" -ge 3
req '{"op":"l7_sample_configure","config":{"enabled":true,"flow_gap_ms":10000,"ports":[{"port":16379,"protocol":"redis"}]}}' | must
in_tcp_cg python3 -c '
import socket
r = socket.create_connection(("127.0.0.1", 16379), timeout=2)
for _ in range(5): r.sendall(b"PING\r\n"); r.recv(64)' || true
sleep 0.5
check "l7s: per-flow gap rate-limits" test "$(l7sjs "d['rate_limited']")" -gt 0
req '{"op":"l7_sample_configure","config":{"enabled":false}}' | must
check "l7s off: detached" test "$(l7sjs "d.get('attached')")" = None
kill $L7S_PID 2>/dev/null || true

# Bridge-less direct redirect: netns A (outer side) and netns B (the "VM"
# behind a tap stand-in) share no bridge; only bpfd's redirect joins them.
ip netns add mnd-a
ip netns add mnd-b
ip link add mnd-out type veth peer name mnd-outp
ip link add mnd-tap type veth peer name mnd-tapp
ip link set mnd-outp netns mnd-a
ip link set mnd-tapp netns mnd-b
ip link set mnd-out up
ip link set mnd-tap up
ip -n mnd-a link set lo up
ip -n mnd-b link set lo up
ip -n mnd-b link set mnd-tapp address 52:54:00:88:00:02
ip -n mnd-a addr add 10.199.88.1/24 dev mnd-outp
ip -n mnd-b addr add 10.199.88.2/24 dev mnd-tapp
ip -n mnd-a link set mnd-outp up
ip -n mnd-b link set mnd-tapp up
A_MAC=$(ip netns exec mnd-a cat /sys/class/net/mnd-outp/address)
ip -n mnd-a neigh replace 10.199.88.2 lladdr 52:54:00:88:00:02 dev mnd-outp
ip -n mnd-b neigh replace 10.199.88.1 lladdr "$A_MAC" dev mnd-tapp
d_ping() { ip netns exec mnd-a ping -c1 -W1 10.199.88.2 >/dev/null 2>&1; }
dsjs() { req '{"op":"direct_status"}' | python3 -c "import json,sys; d=json.load(sys.stdin)['data']; print($1)"; }
check "direct: no path without redirect" bash -c "$(declare -f d_ping); ! d_ping"
PHYS=$(for i in /sys/class/net/*; do [[ -e $i/device ]] && basename "$i" && break; done || true)
if [[ -n "$PHYS" ]]; then
  check "direct: physical NIC refused without force" bash -c "$(declare -f req); SOCK=$SOCK; req '{\"op\":\"direct_configure\",\"config\":{\"vm\":\"x\",\"outer_iface\":\"$PHYS\",\"tap\":\"mnd-tap\"}}' | grep -q 'physical NIC'"
fi
req '{"op":"direct_configure","config":{"vm":"mnd-vm","outer_iface":"mnd-out","tap":"mnd-tap","mac":"52:54:00:88:00:02","ips":["10.199.88.2"]}}' | must
check "direct: programs on outer + tap" test "$(dsjs "sorted(d['attached'])")" = "['mnd-out:mn_direct', 'mnd-tap:mn_direct_out']"
check "direct: guest MAC from config" test "$(dsjs "d['entries'][0]['mac']")" = "52:54:00:88:00:02"
check "direct: idle without the lease" bash -c "$(declare -f d_ping); ! d_ping"
check "direct: idle hits counted" test "$(dsjs "d['idle']")" -gt 0
enforce
check "direct: active under the lease" test "$(dsjs "d['active']")" = True
check "direct: ping across the redirect (both ways)" d_ping
check "direct: redirected in and out" test "$(dsjs "d['redirected_in'] > 0 and d['redirected_out'] > 0")" = True
observe
check "direct: lease dropped, path gone" bash -c "$(declare -f d_ping); ! d_ping"
req '{"op":"direct_configure","config":{"vm":"mnd-vm","enabled":false}}' | must
check "direct: disabled, programs detached" test "$(dsjs "(d['entries'], d['attached'])")" = "([], [])"
ip netns del mnd-a 2>/dev/null || true
ip netns del mnd-b 2>/dev/null || true

# Attribution: exec events carry workload{kind,name} from the cgroup path.
ATTR_CG=/sys/fs/cgroup/mnsmoke-attr.service
mkdir -p "$ATTR_CG"
sleep 6 # cgroup id cache rescans at most every 5s
bash -c "echo \$\$ > $ATTR_CG/cgroup.procs && exec /bin/true"
sleep 0.5
check "attribution: exec workload from cgroup" bash -c "$(declare -f req); SOCK=$SOCK; req '{\"op\":\"proc_events\",\"kind\":\"exec\",\"limit\":5000}' | python3 -c 'import json,sys; d=json.load(sys.stdin)[\"data\"]; sys.exit(0 if any((r.get(\"workload\") or {}).get(\"name\")==\"mnsmoke-attr.service\" and r[\"workload\"][\"kind\"]==\"service\" for r in d) else 1)'"
rmdir "$ATTR_CG" 2>/dev/null || true

echo
echo "passed=$PASS failed=$FAIL  (log: $WORK/bpfd.log)"
grep -E "WARN|ERROR" "$WORK/bpfd.log" | head -20 || true
if [[ $FAIL -ne 0 ]]; then
  echo "--- interfaces"; req '{"op":"list_interfaces"}'
fi
[[ $FAIL -eq 0 ]]
