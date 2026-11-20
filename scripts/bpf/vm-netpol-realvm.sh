#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

#
# VM network policy against two real libvirt VMs (Debian cloud image on the
# `default` NAT network): observe first, then a short enforcement lease,
# then the TLS-intercepting proxy (terminatingTLS + header rewrites,
# originatingTLS), flow history, quarantine and threat feeds on the host,
# then the fleet phase through the controller: Fleet Cloud project
# isolation, an egress allowlist, a project egress IP over IPv4 and IPv6 (seen
# from a TEST-NET netns on the host; IPv6 is skipped on a host with its own IPv6
# default route), a project change approved by a second, temporary admin and
# scale to zero (managed save, woken by a request from the host and from the
# other VM).
# Creates np-client / np-server and deletes them, the policies, the proxy
# secret, the temporary admin and the base image on exit; the edge is always
# returned to observe.
#
# Only run on a disposable test host: while the lease is held the edge
# enforces on every tap that has policy state.
#
#   printf '%s\n' "$PASS" | sudo -n true && bash scripts/bpf/vm-netpol-realvm.sh
#
# The machina password is read from stdin (MACHINA_USER defaults to $USER).
set -uo pipefail

read -r MACHINA_PASS
export MACHINA_USER="${MACHINA_USER:-$USER}" MACHINA_PASS MACHINA_URL="${MACHINA_URL:-https://127.0.0.1:5092}" NO_COLOR=1
HERE="$(cd "$(dirname "$0")/../.." && pwd)"
JAR="${MACHINA_COOKIE_JAR:-$HOME/.machina/cli-session}"
M="$HERE/machinactl"
LEASE="${LEASE:-300}"
SECRET_DIR=/etc/machina/netpol-secrets/default/np-intercept
IMG_URL="${IMG_URL:-https://cloud.debian.org/images/cloud/bookworm/latest/debian-12-genericcloud-amd64.qcow2}"
POOL=/var/lib/libvirt/images
BASE="$POOL/np-realvm-base.qcow2"
W="$(mktemp -d /tmp/np-realvm.XXXXXX)"
VMS=(np-client np-server)
P=0; F=0
exec 3>&2

ok() { echo "PASS  $1"; P=$((P+1)); }
bad() { echo "FAIL  $1"; F=$((F+1)); }
check() { local n=$1; shift; if "$@" >/dev/null 2>&1; then ok "$n"; else bad "$n"; fi; }

bpfd() {
    sudo -n python3 - "$1" <<'PY'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect('/run/machina-bpf/bpfd.sock')
s.sendall(sys.argv[1].encode() + b'\n'); b = b''
while not b.endswith(b'\n'):
    c = s.recv(1 << 20)
    if not c: break
    b += c
r = json.loads(b)
print(json.dumps(r.get('data', r)))
sys.exit(0 if r.get('ok', True) is not False else 1)
PY
}
observe() { bpfd '{"op":"set_mode","mode":"observe"}' >/dev/null; }

cleanup() {
    observe
    if [[ -n "${FLEET:-}" ]]; then
        "$M" netpol project reset np-red --fleet >/dev/null 2>&1
        "$M" netpol project reset np-blue --fleet >/dev/null 2>&1
        "$M" netpol project assign - np-client np-server --fleet >/dev/null 2>&1
        [[ -n "${OVERLAY_ON:-}" ]] && "$M" netpol overlay disable --fleet >/dev/null 2>&1
        "$M" netpol sync --fleet >/dev/null 2>&1
        sudo -n iptables -D FORWARD -i virbr0 -o np-outv -j ACCEPT 2>/dev/null
        sudo -n iptables -D FORWARD -i np-outv -o virbr0 -j ACCEPT 2>/dev/null
        sudo -n ip6tables -D FORWARD -i virbr0 -o np-outv -j ACCEPT 2>/dev/null
        sudo -n ip6tables -D FORWARD -i np-outv -o virbr0 -j ACCEPT 2>/dev/null
        sudo -n ip -6 addr del fd00:6e70::1/64 dev virbr0 2>/dev/null
        [[ -n "${V6_FWD:-}" ]] && sudo -n sysctl -qw "net.ipv6.conf.all.forwarding=$V6_FWD" "net.ipv6.conf.virbr0.disable_ipv6=$V6_BR"
        [[ -s "$W/aid" ]] && curl -sk -o /dev/null -b "$JAR" -X DELETE "$MACHINA_URL/api/v1/platform/controller/api/v1/users/$(cat "$W/aid")"
        sudo -n ip netns pids np-out 2>/dev/null | xargs -r sudo -n kill 2>/dev/null
        sudo -n ip netns del np-out 2>/dev/null
        sudo -n ip link del np-outv 2>/dev/null
    fi
    for v in "${VMS[@]}"; do "$M" vm release "$v" >/dev/null 2>&1; done
    "$M" netpol delete np-realvm >/dev/null 2>&1
    "$M" netpol delete np-realvm-proxy >/dev/null 2>&1
    "$M" netpol threat rm np-lab >/dev/null 2>&1
    sudo -n rm -rf "$SECRET_DIR"
    for v in "${VMS[@]}"; do
        sudo -n virsh destroy "$v" >/dev/null 2>&1
        sudo -n virsh managedsave-remove "$v" >/dev/null 2>&1
        sudo -n virsh undefine "$v" --nvram >/dev/null 2>&1 || sudo -n virsh undefine "$v" >/dev/null 2>&1
        sudo -n rm -f "$POOL/$v.qcow2" "$POOL/$v-seed.iso"
        "$M" vm label "$v" app- >/dev/null 2>&1
    done
    sudo -n rm -f "$BASE"
    # The controller's inventory sync does not prune when a host has no VMs left.
    if [[ -n "${FLEET:-}" && -f "${CONTROLLER_DB:-/var/lib/machina/controller.db}" ]]; then
        sudo -n sqlite3 "${CONTROLLER_DB:-/var/lib/machina/controller.db}" "DELETE FROM vms WHERE name IN ('np-client', 'np-server')" 2>/dev/null
    fi
    rm -rf "$W"
    echo "cleanup: VMs, policy and image removed; edge mode $(bpfd '{"op":"vm_edge_status"}' | python3 -c 'import json,sys; print("enforce" if json.load(sys.stdin).get("enforcing") else "observe")')"
}
trap cleanup EXIT

echo "== VMs =="
for v in "${VMS[@]}"; do
    sudo -n virsh dominfo "$v" >/dev/null 2>&1 && { echo "$v already exists; refusing to touch it" >&2; trap - EXIT; exit 1; }
done
ssh-keygen -q -t ed25519 -N '' -f "$W/key"
sudo -n curl -fsSL -o "$BASE" "$IMG_URL" || { bad "download $IMG_URL"; exit 1; }
for v in "${VMS[@]}"; do
    cat > "$W/$v-user" <<EOF
#cloud-config
hostname: $v
users:
  - name: np
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    ssh_authorized_keys: ["$(cat "$W/key.pub")"]
write_files:
  - path: /srv/echo.py
    content: |
      import http.server, ssl, sys
      class H(http.server.BaseHTTPRequestHandler):
          def do_GET(self):
              b = (self.requestline + "\n" + str(self.headers)).encode()
              self.send_response(200)
              self.send_header("Content-Length", str(len(b)))
              self.end_headers()
              self.wfile.write(b)
          do_POST = do_GET
      s = http.server.ThreadingHTTPServer(("", int(sys.argv[1])), H)
      if len(sys.argv) > 2:
          c = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
          c.load_cert_chain(sys.argv[2], sys.argv[3])
          s.socket = c.wrap_socket(s.socket, server_side=True)
      s.serve_forever()
runcmd:
  - [mkdir, -p, /srv]
  - [sh, -c, "echo ok > /srv/ok; echo secret > /srv/secret"]
  - [systemd-run, --unit, np80, python3, -m, http.server, "80", --directory, /srv]
  - [systemd-run, --unit, np8080, python3, -m, http.server, "8080", --directory, /srv]
  - [systemd-run, --unit, np443, python3, /srv/echo.py, "443"]
  - [sh, -c, "apt-get update -qq && apt-get install -y -qq qemu-guest-agent && systemctl start qemu-guest-agent"]
EOF
    printf 'instance-id: %s-%s\nlocal-hostname: %s\n' "$v" "$$" "$v" > "$W/$v-meta"
    sudo -n cloud-localds "$POOL/$v-seed.iso" "$W/$v-user" "$W/$v-meta"
    sudo -n qemu-img create -q -f qcow2 -F qcow2 -b "$BASE" "$POOL/$v.qcow2" 8G
    sudo -n virt-install --name "$v" --memory 1024 --vcpus 1 --import --osinfo detect=on,require=off \
        --disk "path=$POOL/$v.qcow2,format=qcow2,bus=virtio" --disk "path=$POOL/$v-seed.iso,device=cdrom" \
        --network network=default,model=virtio --channel unix,target.type=virtio,target.name=org.qemu.guest_agent.0 \
        --graphics none --noautoconsole >/dev/null \
        && ok "virt-install $v" || bad "virt-install $v"
done

ip_of() { sudo -n virsh domifaddr "$1" --source lease 2>/dev/null | awk '/ipv4/ {sub(/\/.*/, "", $4); print $4; exit}'; }
for _ in $(seq 90); do
    CIP=$(ip_of np-client); SIP=$(ip_of np-server)
    [[ -n "$CIP" && -n "$SIP" ]] && break
    sleep 2
done
[[ -n "${CIP:-}" && -n "${SIP:-}" ]] && ok "DHCP leases: client $CIP server $SIP" || { bad "DHCP leases"; exit 1; }
vssh() { local h=$1; shift; ssh -i "$W/key" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=5 -o LogLevel=ERROR "np@$h" "$@"; }
cssh() { vssh "$CIP" "$@"; }
sssh() { vssh "$SIP" "$@"; }
for _ in $(seq 90); do cssh true 2>/dev/null && break; sleep 2; done
check "ssh into client" cssh true
for _ in $(seq 60); do curl -fs -m 2 "http://$SIP/ok" >/dev/null && curl -fs -m 2 "http://$SIP:8080/ok" >/dev/null && break; sleep 2; done
check "server answers on 80 and 8080" curl -fs -m 2 "http://$SIP:8080/ok"

# Test CA + server certificate for the proxy phase (shipped before any lease:
# the server's ingress policy keeps the host's ssh out while enforcing).
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 1 -subj /CN=np-test-ca \
    -addext basicConstraints=critical,CA:TRUE -keyout "$W/ca.key" -out "$W/ca.crt" 2>/dev/null
openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -subj /CN=np-server.test \
    -keyout "$W/tls.key" -out "$W/tls.csr" 2>/dev/null
printf 'subjectAltName=DNS:np-server.test,IP:%s\n' "$SIP" > "$W/san"
openssl x509 -req -in "$W/tls.csr" -CA "$W/ca.crt" -CAkey "$W/ca.key" -CAcreateserial -days 1 \
    -extfile "$W/san" -out "$W/tls.crt" 2>/dev/null
for _ in $(seq 60); do sssh true 2>/dev/null && break; sleep 2; done
cssh "cat > /tmp/ca.crt" < "$W/ca.crt"
sssh "cat > /tmp/tls.crt" < "$W/tls.crt"
sssh "cat > /tmp/tls.key" < "$W/tls.key"
sssh "sudo systemd-run --unit np8443 python3 /srv/echo.py 8443 /tmp/tls.crt /tmp/tls.key" >/dev/null 2>&1
tls_echo() { curl -fs -m 3 --cacert "$W/ca.crt" "https://$SIP:8443/hdr" | grep -q "GET /hdr"; }
for _ in $(seq 10); do tls_echo && break; sleep 1; done
check "server answers TLS on 8443" tls_echo

# HTTP status of a request from the client to the server ("000" = no answer).
get() { local path=$1; shift; cssh "curl -s -m 4 -o /dev/null -w '%{http_code}' $* http://$SIP$path" 2>/dev/null; }
is() { local want=$1; shift; [[ "$(get "$@")" == "$want" ]]; }
taps_programmed() {
    bpfd '{"op":"vm_edge_status"}' | python3 -c 'import json,sys; sys.exit(0 if len(json.load(sys.stdin).get("taps", [])) >= 2 else 1)'
}
flow_has() {
    local pat=$1; shift
    local vm=${FLOW_VM:-np-server}
    for _ in 1 2 3 4 5; do
        "$M" flow observe --vm "$vm" --last 300 "$@" > "$W/flows" 2>&1
        grep -q -- "$pat" "$W/flows" && return
        sleep 2
    done
    { echo "      flow observe --vm $vm $*:"; tail -n 8 "$W/flows"; echo "      unfiltered:"; "$M" flow observe --last 8 2>&1; } | sed 's/^/      /' >&3
    return 1
}

check "label client" "$M" vm label np-client app=np-client
check "label server" "$M" vm label np-server app=np-server
cat > "$W/policy.yaml" <<'Y'
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: np-realvm
spec:
  description: real-VM test - client may only GET /ok on the server's port 80
  endpointSelector:
    matchLabels: {app: np-server}
  ingress:
    - fromEndpoints:
        - matchLabels: {app: np-client}
      toPorts:
        - ports: [{port: "80", protocol: TCP}]
          rules:
            http: [{method: GET, path: /ok}]
Y
check "apply policy" "$M" netpol apply -f "$W/policy.yaml"
for _ in $(seq 30); do
    "$M" netpol endpoints 2>/dev/null | grep -q "np-server.*$SIP" && break
    sleep 2; "$M" netpol apply -f "$W/policy.yaml" >/dev/null 2>&1
done
check "server endpoint has its address" bash -c "'$M' netpol endpoints | grep -q 'np-server.*$SIP'"
check "edge programmed the VM taps" taps_programmed
# Learn and replay below judge this run's traffic, not earlier runs'.
check "flow history cleared" bpfd '{"op":"vm_flow_edges_reset"}'

echo "== observe =="
observe
check "observe: GET /ok 200" is 200 /ok
check "observe: GET /secret still 200 (audited)" is 200 /secret
check "observe: port 8080 still open" is 200 :8080/ok
sleep 2
check "observe: AUDIT flow for 8080" flow_has 8080 --verdict AUDIT --port 8080

echo "== enforce (lease ${LEASE}s) =="
check "take enforcement lease" bpfd "{\"op\":\"set_mode\",\"mode\":\"enforce\",\"lease_secs\":$LEASE}"
check "enforce: GET /ok 200" is 200 /ok
check "enforce: GET /secret 403 from L7 rule" is 403 /secret
check "enforce: POST /ok 403" is 403 /ok -X POST
check "enforce: port 8080 dropped" is 000 :8080/ok
check "enforce: host (not np-client) cannot reach server :80" bash -c "! curl -fs -m 3 -o /dev/null http://$SIP/ok"
check "enforce: client without policy keeps egress" cssh "ping -c1 -W2 192.168.122.1"
sleep 2
check "enforce: DROPPED flow for 8080" flow_has 8080 --verdict DROPPED --port 8080
check "enforce: L7 flow records the request" flow_has /secret --port 80

echo "== just-in-time access =="
check "jit: grant np-client → np-server :8080 for 8s" "$M" netpol jit grant --from np-client --to np-server --port 8080 --for 8s --reason realvm
check "jit: 8080 open during the grant" is 200 :8080/ok
check "jit: still only for np-client (host dropped on 8080)" bash -c "! curl -fs -m 3 -o /dev/null http://$SIP:8080/ok"
check "jit: listed" bash -c "'$M' netpol jit | grep -q 'np-client.*np-server.*8080/tcp'"
sleep 10
check "jit: 8080 dropped again after expiry" is 000 :8080/ok
check "jit: grant removed" bash -c "! '$M' netpol get | grep -q jit-np-client"

echo "== proxy: TLS interception + header rewrites =="
check "proxy secret installed" bash -c "sudo -n mkdir -p '$SECRET_DIR' && sudo -n install -m600 '$W/tls.crt' '$W/tls.key' '$W/ca.crt' '$SECRET_DIR/'"
cat > "$W/proxy.yaml" <<'Y'
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: np-realvm-proxy
specs:
  - description: client HTTPS to the server is intercepted and rewritten
    endpointSelector:
      matchLabels: {app: np-client}
    egress:
      - toEndpoints:
          - matchLabels: {app: np-server}
        toPorts:
          - ports: [{port: "443", protocol: TCP}]
            terminatingTLS:
              secret: {name: np-intercept}
            rules:
              http:
                - path: /hdr
                  headerMatches:
                    - {name: X-Team, value: blue, mismatch: REPLACE}
                    - {name: X-Debug, value: "0", mismatch: DELETE}
                    - {name: X-Via, value: machina, mismatch: ADD}
          - ports: [{port: "8443", protocol: TCP}]
            originatingTLS:
              secret: {name: np-intercept}
              trustedCA: ca.crt
            rules:
              http: [{path: /hdr}]
  - description: the server admits the client (not the host) on 443 / 8443
    endpointSelector:
      matchLabels: {app: np-server}
    ingress:
      - fromEndpoints:
          - matchLabels: {app: np-client}
        toPorts:
          - ports: [{port: "443", protocol: TCP}, {port: "8443", protocol: TCP}]
Y
check "apply proxy policy" "$M" netpol apply -f "$W/proxy.yaml"
proxy_up() {
    bpfd '{"op":"vm_edge_status"}' | python3 -c 'import json,sys; sys.exit(0 if json.load(sys.stdin).get("proxy","").startswith("listening") else 1)'
}
for _ in $(seq 10); do proxy_up && break; sleep 1; done
check "bpfd proxy listening" proxy_up
https() { local path=$1; shift; cssh "curl -s -m 6 --cacert /tmp/ca.crt --resolve np-server.test:443:$SIP $* https://np-server.test$path" 2>/dev/null; }
https /hdr -H "'X-Team: red'" -H "'X-Debug: 1'" > "$W/hdr.out"
check "intercepted HTTPS reaches the server" grep -q "GET /hdr" "$W/hdr.out"
check "REPLACE: X-Team rewritten to blue" grep -q "X-Team: blue" "$W/hdr.out"
check "REPLACE: original X-Team gone" bash -c "! grep -q 'X-Team: red' '$W/hdr.out'"
check "DELETE: X-Debug removed" bash -c "! grep -qi 'X-Debug' '$W/hdr.out'"
check "ADD: X-Via added" grep -q "X-Via: machina" "$W/hdr.out"
denied_other() { [[ "$(https /other -o /dev/null -w "'%{http_code}'")" == 403 ]]; }
check "intercepted path outside the rule: 403" denied_other
policy_cert() { cssh "curl -sv -m 6 -o /dev/null --cacert /tmp/ca.crt --resolve np-server.test:443:$SIP https://np-server.test/hdr" 2>&1 | grep -q "issuer: CN=np-test-ca"; }
check "client sees the policy certificate" policy_cert
check "server sees the client identity (host itself is refused on 443)" bash -c "! curl -s -m 3 -o /dev/null http://$SIP:443/hdr"
cssh "curl -s -m 6 http://$SIP:8443/hdr" > "$W/orig.out" 2>/dev/null
check "originatingTLS: plain client request reaches the TLS server" grep -q "GET /hdr" "$W/orig.out"
FLOW_VM=np-client check "flow records the intercepted request" flow_has "tls intercepted" --port 443
FLOW_VM=np-client check "flow records the rewrite" flow_has "X-Via added" --port 443

echo "== back to observe =="
observe
check "observe again: port 8080 open" is 200 :8080/ok
check "observe again: GET /secret 200" is 200 /secret

echo "== flow history, learn, replay, detection =="
edges() { "$M" flow edges --vm "$1" -o json 2>/dev/null; }
edge_has() {
    local vm=$1 filter=$2
    for _ in 1 2 3 4 5; do
        edges "$vm" | jq -e "[.items[] | select($filter)] | length > 0" >/dev/null && return
        sleep 2
    done
    edges "$vm" | jq -c '.items[] | select(.port < 2000 or .port > 2030)
        | {s: (.src_vm // .src), d: (.dst_vm // .dst), p: .port, dir: .direction, v: .verdict, l7: [.l7[]? | "\(.request) ×\(.count)"]}' \
        | head -12 | sed 's/^/      /' >&3
    return 1
}
check "history: client → server :80 with GET /ok" edge_has np-server \
    '.src_vm == "np-client" and .dst_vm == "np-server" and .port == 80 and any(.l7[]?; .request | test("^GET .*/ok$"))'
check "history: denied 8080 edge kept" edge_has np-server '.port == 8080 and .verdict == "DROPPED"'
check "history: proxied 443 has status and latency" edge_has np-client \
    '.port == 443 and any(.l7[]?; (.status["2xx"] // 0) > 0 and .latency_n > 0)'
"$M" netpol learn --vm np-server > "$W/learned.yaml" 2> "$W/learned.err"
check "learn: policy for np-server" grep -q "app: np-server" "$W/learned.yaml"
grep -q "app: np-server" "$W/learned.yaml" || sed 's/^/      /' "$W/learned.err" "$W/learned.yaml"
learned_80() { grep -q 'app: np-client' "$W/learned.yaml" && grep -qE "port: ['\"]?80['\"]?$" "$W/learned.yaml"; }
check "learn: admits np-client on 80" learned_80
check "learn: learned YAML validates" "$M" netpol validate -f "$W/learned.yaml"
"$M" netpol replay -f "$W/learned.yaml" > "$W/replay.out" 2>&1
check "replay: learned policy breaks nothing observed" grep -q "Safe:" "$W/replay.out"
grep -q "Safe:" "$W/replay.out" || sed 's/^/      /' "$W/replay.out"
cat > "$W/narrow.yaml" <<'Y'
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: np-realvm-proxy
specs:
  - endpointSelector:
      matchLabels: {app: np-client}
    egress:
      - toEndpoints: [{matchLabels: {app: np-server}}]
        toPorts: [{ports: [{port: "8443", protocol: TCP}]}]
  - endpointSelector:
      matchLabels: {app: np-server}
    ingress:
      - fromEndpoints: [{matchLabels: {app: np-client}}]
        toPorts: [{ports: [{port: "8443", protocol: TCP}]}]
Y
"$M" netpol replay -f "$W/narrow.yaml" > "$W/narrow.out" 2>&1
narrow_breaks_443() { grep -q "Would break" "$W/narrow.out" && grep -q "np-server tcp/443" "$W/narrow.out"; }
check "replay: dropping 443 from np-realvm-proxy would break client → server :443" narrow_breaks_443
narrow_breaks_443 || sed 's/^/      /' "$W/narrow.out"
cssh "for p in \$(seq 2000 2030); do timeout 1 bash -c '</dev/tcp/$SIP/'\$p 2>/dev/null; done; true"
alert_has() {
    for _ in 1 2 3 4 5; do
        "$M" flow alerts 2>/dev/null | grep -q "$1" && return
        sleep 2
    done
    return 1
}
check "detect: port scan from np-client" alert_has port_scan
persisted() {
    for _ in $(seq 40); do
        sudo -n test -s /var/lib/machina/bpf/flow-history.json && return
        sleep 2
    done
    return 1
}
check "history persisted to disk" persisted

echo "== quarantine =="
# An established client → server SSH session that ticks twice a second.
scp -q -i "$W/key" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR "$W/key" "np@$CIP:/tmp/k"
cssh "rm -f /tmp/ticks; setsid nohup ssh -i /tmp/k -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ServerAliveInterval=1 -o ServerAliveCountMax=600 np@$SIP 'while :; do echo t; sleep 0.5; done' > /tmp/ticks 2>/dev/null < /dev/null & disown"
ticks() { cssh "wc -l < /tmp/ticks" 2>/dev/null; }
growing() { local a b; a=$(ticks); sleep 2; b=$(ticks); [[ -n "$a" && "$b" -gt "$a" ]]; }
check "quarantine: long-lived client → server session is flowing" growing
check "quarantine np-server for 10m (host SSH allowed)" "$M" vm quarantine np-server --for 10m --allow-host-ssh --reason "realvm test"
check "quarantine: listed with time left" bash -c "'$M' netpol quarantines | grep -q 'np-server.*ingress host tcp/22'"
check "quarantine: open session cut" bash -c "! { a=\$(ssh -i '$W/key' -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR np@$CIP 'wc -l < /tmp/ticks'); sleep 3; b=\$(ssh -i '$W/key' -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR np@$CIP 'wc -l < /tmp/ticks'); [ \"\$b\" -gt \"\$a\" ]; }"
check "quarantine: new client → server :80 dropped (observe mode)" is 000 /ok
check "quarantine: host → server :80 dropped" bash -c "! curl -fs -m 3 -o /dev/null http://$SIP/ok"
check "quarantine: host SSH exception works" sssh true
check "quarantine: server egress cut" bash -c "! ssh -i '$W/key' -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR np@$SIP 'ping -c1 -W2 192.168.122.1'"
check "quarantine: client unaffected" cssh "ping -c1 -W2 192.168.122.1"
check "quarantine: DROPPED flows say quarantine" flow_has quarantine --verdict DROPPED
check "release" "$M" vm release np-server
check "released: client → server :80 back" is 200 /ok
check "released: nothing listed" bash -c "! '$M' netpol quarantines 2>/dev/null | grep -q np-server"

check "quarantine np-server for 6s" "$M" vm quarantine np-server --for 6s
check "short quarantine: :80 dropped" is 000 /ok
sleep 7
check "short quarantine lifted by the kernel at the deadline" is 200 /ok
gone() { for _ in $(seq 15); do "$M" netpol quarantines 2>/dev/null | grep -q np-server || return 0; sleep 1; done; return 1; }
check "short quarantine forgotten by bpfd" gone

check "quarantine np-server for 10m before a bpfd restart" "$M" vm quarantine np-server --for 10m --allow-host-ssh
check "restart machina-bpfd" sudo -n systemctl restart machina-bpfd
for _ in $(seq 20); do bpfd '{"op":"vm_quarantines"}' 2>/dev/null | grep -q np-server && break; sleep 1; done
check "restart: quarantine restored" bash -c "'$M' netpol quarantines | grep -q np-server"
held() { for _ in $(seq 10); do is 000 /ok && return; sleep 1; done; return 1; }
check "restart: still dropping" held
check "release after restart" "$M" vm release np-server
back() { for _ in $(seq 10); do is 200 /ok && return; sleep 1; done; return 1; }
check "restart: traffic back after release" back

# A VM with no policy at all: its taps are programmed for the quarantine only.
"$M" netpol delete np-realvm >/dev/null 2>&1
"$M" netpol delete np-realvm-proxy >/dev/null 2>&1
check "no policies: client reaches the gateway" cssh "ping -c1 -W2 192.168.122.1"
check "quarantine np-client with no policy" "$M" vm quarantine np-client --for 5m --allow-host-ssh
check "no policies: client egress cut" bash -c "! ssh -i '$W/key' -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR np@$CIP 'ping -c1 -W2 192.168.122.1'"
check "no policies: host SSH exception works" cssh true
check "release np-client" "$M" vm release np-client
check "no policies: client egress back" cssh "ping -c1 -W2 192.168.122.1"

echo "== DNS threat feeds =="
web() { cssh "curl -s -m 5 -o /dev/null -w '%{http_code}' http://$THREAT_DOMAIN/" 2>/dev/null; }
reach() { [[ "$(web)" != 000 ]]; }
THREAT_DOMAIN="${THREAT_DOMAIN:-example.com}"
check "client reaches $THREAT_DOMAIN before any feed" reach
check "set a blocking feed for $THREAT_DOMAIN" "$M" netpol threat set np-lab --domain "$THREAT_DOMAIN" --block
check "feed listed, both VMs watched" bash -c "'$M' netpol threat | grep -q 'np-lab.*1.*block' && '$M' netpol threat | grep -Eq 'watching DNS of [2-9]'"
cssh "getent ahostsv4 www.$THREAT_DOMAIN; getent ahostsv4 $THREAT_DOMAIN" >/dev/null 2>&1
check "threat_domain alert for np-client" alert_has threat_domain
blocked() { for _ in $(seq 10); do "$M" netpol threat | grep -q "BLOCKED" && return; sleep 1; done; return 1; }
check "answer addresses blocked" blocked
check "observe: still reachable" reach
FLOW_VM=np-client check "observe: AUDIT flow says threat-domain" flow_has threat-domain --verdict AUDIT
check "take a 60s enforcement lease" bpfd '{"op":"set_mode","mode":"enforce","lease_secs":60}'
check "enforce: $THREAT_DOMAIN dropped" bash -c "[[ \"\$(ssh -i '$W/key' -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR np@$CIP \"curl -s -m 5 -o /dev/null -w '%{http_code}' http://$THREAT_DOMAIN/\")\" == 000 ]]"
check "enforce: gateway still reachable" cssh "ping -c1 -W2 192.168.122.1"
FLOW_VM=np-client check "enforce: DROPPED flow says threat-domain" flow_has threat-domain --verdict DROPPED
observe
check "restart machina-bpfd" sudo -n systemctl restart machina-bpfd
for _ in $(seq 20); do bpfd '{"op":"vm_threat_feeds"}' 2>/dev/null | grep -q np-lab && break; sleep 1; done
check "restart: feed restored" bash -c "'$M' netpol threat | grep -q np-lab"
check "remove the feed" "$M" netpol threat rm np-lab
check "removed: nothing listed" bash -c "! '$M' netpol threat 2>/dev/null | grep -q np-lab"
check "removed: $THREAT_DOMAIN reachable" reach

# ---- fleet: projects, egress allowlist, egress IP, evidence --------------------
# The controller owns the edge here (no daemon policies are left). The outside
# world for the egress IP is a netns on the host with TEST-NET-2 addresses:
# libvirt's NAT shows it 198.51.100.1, the project egress IP 198.51.100.77.
echo "== fleet: projects =="
FLEET=1
FM() { "$M" netpol "$@" --fleet; }
fsync() { FM sync >/dev/null 2>&1; }
fleet_sees() { FM endpoints 2>/dev/null | grep -q "np-server.*$SIP" && FM endpoints 2>/dev/null | grep -q "np-client.*$CIP"; }
for _ in $(seq 60); do fleet_sees && break; sleep 3; done
check "controller inventory has both VMs with their addresses" fleet_sees
check "assign np-server to np-red" FM project assign np-red np-server
check "assign np-client to np-blue" FM project assign np-blue np-client
check "isolate np-red" FM project isolate np-red
fsync
owned() { bpfd '{"op":"vm_edge_status"}' | python3 -c 'import json,sys; d=json.load(sys.stdin); sys.exit(0 if d.get("owner")=="controller" and len(d.get("taps",[]))>=1 else 1)'; }
for _ in $(seq 20); do owned && break; sleep 2; fsync; done
check "controller owns the edge, server tap programmed" owned
node_addrs() { bpfd '{"op":"vm_edge_status"}' | python3 -c 'import json,sys; d=json.load(sys.stdin); d=d.get("data",d); a=d.get("node_addrs",[]); sys.exit(0 if any(x.startswith("192.168.122.1/") for x in a) else 1)'; }
check "host reports its addresses, libvirt bridge included" node_addrs
preview_only() {
    local out; out=$(FM project open np-red --preview 2>&1) || return 1
    grep -q "would break" <<<"$out" && "$M" netpol projects --fleet | grep -q 'project-isolation-np-red-'
}
check "preview: opening np-red is replayed, not applied" preview_only
check "generated policy listed" bash -c "'$M' netpol projects --fleet | grep -q 'project-isolation-np-red-'"
observe
check "observe: client → server still 200 (audited)" is 200 /ok
sleep 2
check "observe: AUDIT flow for the cross-project request" flow_has "$CIP" --verdict AUDIT --port 80
check "take a 240s enforcement lease" bpfd '{"op":"set_mode","mode":"enforce","lease_secs":240}'
until_is() { local want=$1; shift; for _ in $(seq 15); do is "$want" "$@" && return; sleep 1; done; return 1; }
check "enforce: np-blue client → np-red server dropped" until_is 000 /ok
check "enforce: host → server allowed" curl -fs -m 3 -o /dev/null "http://$SIP/ok"
srv_to_client() { [[ "$(sssh "curl -s -m 4 -o /dev/null -w '%{http_code}' http://$CIP/ok" 2>/dev/null)" == 200 ]]; }
check "enforce: np-red server → np-blue client allowed" srv_to_client
FM project assign np-red np-client >/dev/null; fsync
check "enforce: same project (client moved to np-red) allowed" until_is 200 /ok
FM project assign np-blue np-client >/dev/null; fsync
check "enforce: moved back, dropped again" until_is 000 /ok
FM project isolate np-red --no-host >/dev/null; fsync
host_dropped() { for _ in $(seq 15); do curl -fs -m 2 -o /dev/null "http://$SIP/ok" || return 0; sleep 1; done; return 1; }
check "enforce: --no-host drops the host" host_dropped
FM project isolate np-red >/dev/null; fsync
host_back() { for _ in $(seq 15); do curl -fs -m 2 -o /dev/null "http://$SIP/ok" && return; sleep 1; done; return 1; }
check "enforce: host allowed again" host_back

echo "== fleet: egress allowlist =="
cweb() { cssh "curl -sk -m 5 -o /dev/null -w '%{http_code}' $1" 2>/dev/null; }
reaches() { for _ in $(seq 10); do [[ "$(cweb "$1")" != 000 ]] && return; sleep 1; done; return 1; }
blocked_to() { for _ in $(seq 10); do [[ "$(cweb "$1")" == 000 ]] && return; sleep 1; done; return 1; }
check "renew the enforcement lease (180s)" bpfd '{"op":"set_mode","mode":"enforce","lease_secs":180}'
check "allow example.com:80 for np-blue" FM egress np-blue allow example.com --port 80
check "allow 1.1.1.1:443 for np-blue" FM egress np-blue allow 1.1.1.1 --port 443
fsync
check "enforce: DNS through the host works" cssh "getent ahostsv4 example.com"
check "enforce: allowed domain reachable" reaches http://example.com/
check "enforce: allowed IP and port reachable" reaches https://1.1.1.1/
check "enforce: allowed IP, other port dropped" blocked_to http://1.1.1.1/
check "enforce: unlisted destination dropped" blocked_to https://9.9.9.9/
check "enforce: host gateway still reachable" cssh "ping -c1 -W2 192.168.122.1"
check "enforce: server (np-red) egress unaffected" bash -c "[[ \"\$(ssh -i '$W/key' -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR np@$SIP \"curl -sk -m 5 -o /dev/null -w '%{http_code}' https://9.9.9.9/\")\" != 000 ]]"
FLOW_VM=np-client check "enforce: DROPPED flow to 9.9.9.9" flow_has 9.9.9.9 --verdict DROPPED
check "unrestrict np-blue" FM egress np-blue unrestrict
fsync
check "unrestricted: 9.9.9.9 reachable again" reaches https://9.9.9.9/
observe

echo "== fleet: egress IP =="
ON=np-out OV=np-outv
outside_up() {
    sudo -n ip netns add $ON && sudo -n ip link add $OV type veth peer name ${OV}p && sudo -n ip link set ${OV}p netns $ON \
        && sudo -n ip addr add 198.51.100.1/24 dev $OV && sudo -n ip addr add 198.51.100.77/32 dev $OV \
        && sudo -n ip addr add 10.199.90.1/24 dev $OV && sudo -n ip link set $OV up \
        && sudo -n ip netns exec $ON ip addr add 198.51.100.2/24 dev ${OV}p \
        && sudo -n ip netns exec $ON ip addr add 10.199.90.2/24 dev ${OV}p \
        && sudo -n ip netns exec $ON ip link set ${OV}p up && sudo -n ip netns exec $ON ip link set lo up || return 1
    if command -v iptables >/dev/null; then
        sudo -n iptables -I FORWARD -i virbr0 -o $OV -j ACCEPT
        sudo -n iptables -I FORWARD -i $OV -o virbr0 -j ACCEPT
    fi
    sudo -n ip netns exec $ON python3 -c "
import socket
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(('0.0.0.0', 18095)); s.listen(8)
while True:
    c, a = s.accept(); c.sendall(a[0].encode()); c.close()
" >/dev/null 2>&1 &
    sleep 1
}
check "outside netns with a peer-address server" outside_up
seen() { vssh "$1" "python3 -c \"import socket; s=socket.create_connection(('$2',18095),4); print(s.recv(64).decode())\"" 2>/dev/null; }
check "before: client seen as libvirt NAT address" [ "$(seen "$CIP" 198.51.100.2)" = 198.51.100.1 ]
HN=$(NP_JSON=1 FM egress 2>/dev/null | jq -r '.items[0].hostname // empty')
check "set np-blue egress IP 198.51.100.77 (plus IPv6) on $HN" FM egress np-blue ip "$HN" "198.51.100.77,2001:db8:77::77"
fsync
egress_active() { for _ in $(seq 20); do NP_JSON=1 FM egress 2>/dev/null | jq -e '.items[0].active and (.items[0].rules | length) == 1' >/dev/null && return; sleep 1; fsync; done; return 1; }
check "host reports the rule active" egress_active
check "nft rule names the client and the egress IP" bash -c "sudo -n nft list table ip machina_egress | grep -q '$CIP' && sudo -n nft list table ip machina_egress | grep -q 'snat to 198.51.100.77'"
check "client seen as the egress IP" [ "$(seen "$CIP" 198.51.100.2)" = 198.51.100.77 ]
check "private destination keeps libvirt NAT" [ "$(seen "$CIP" 10.199.90.2)" = 10.199.90.1 ]
check "server (np-red) not rewritten" [ "$(seen "$SIP" 198.51.100.2)" = 198.51.100.1 ]
check "remove the egress IP" FM egress np-blue ip "$HN" -
fsync
no_egress_table() { for _ in $(seq 20); do sudo -n nft list table ip machina_egress >/dev/null 2>&1 || return 0; sleep 1; fsync; done; return 1; }
check "removed: nft table gone" no_egress_table
check "removed: client seen as libvirt NAT address again" [ "$(seen "$CIP" 198.51.100.2)" = 198.51.100.1 ]

# IPv6: a ULA on virbr0 and the client, the outside netns on 2001:db8:78::/64.
# ULA destinations are never rewritten, so the peer has a documentation prefix.
if [[ -n "$(ip -6 route show default 2>/dev/null)" ]]; then
    echo "SKIP  IPv6 egress IP: this host has its own IPv6 default route"
else
V6_FWD=$(sysctl -n net.ipv6.conf.all.forwarding) V6_BR=$(sysctl -n net.ipv6.conf.virbr0.disable_ipv6)
outside6_up() {
    sudo -n sysctl -qw net.ipv6.conf.virbr0.disable_ipv6=0 net.ipv6.conf.all.forwarding=1 \
        && sudo -n ip -6 addr add fd00:6e70::1/64 dev virbr0 nodad \
        && sudo -n ip -6 addr add 2001:db8:78::1/64 dev $OV nodad && sudo -n ip -6 addr add 2001:db8:77::77/128 dev $OV nodad \
        && sudo -n ip netns exec $ON ip -6 addr add 2001:db8:78::2/64 dev ${OV}p nodad \
        && sudo -n ip netns exec $ON ip -6 route add default via 2001:db8:78::1 || return 1
    if command -v ip6tables >/dev/null; then
        sudo -n ip6tables -I FORWARD -i virbr0 -o $OV -j ACCEPT
        sudo -n ip6tables -I FORWARD -i $OV -o virbr0 -j ACCEPT
    fi
    sudo -n ip netns exec $ON python3 -c "
import socket
s = socket.socket(socket.AF_INET6); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
s.bind(('::', 18095)); s.listen(8)
while True:
    c, a = s.accept(); c.sendall(a[0].encode()); c.close()
" >/dev/null 2>&1 &
    cssh "dev=\$(ip -o -4 route show default | awk '{print \$5}'); sudo ip -6 addr add fd00:6e70::10/64 dev \$dev nodad && sudo ip -6 route add default via fd00:6e70::1"
}
check "IPv6: ULA on virbr0 and the client, outside netns on 2001:db8:78::/64" outside6_up
# virbr0's link-local address is still in DAD for a moment after IPv6 is enabled.
seen_as() { for _ in $(seq 10); do [[ "$(seen "$CIP" "$1")" == "$2" ]] && return; sleep 1; done; return 1; }
check "IPv6 before: client seen as its own ULA" seen_as 2001:db8:78::2 fd00:6e70::10
check "IPv6: guest agent reports the client's ULA" bash -c "sudo -n virsh domifaddr np-client --source agent | grep -q fd00:6e70::10"
check "set the dual-stack egress IP again" FM egress np-blue ip "$HN" "198.51.100.77,2001:db8:77::77"
egress6() { for _ in $(seq 60); do sudo -n nft list table ip6 machina_egress 2>/dev/null | grep -q 'fd00:6e70::10' && return; sleep 3; fsync; done; return 1; }
check "IPv6: ip6 machina_egress names the client's ULA (from the guest agent)" egress6
check "IPv6: nft rule snats to 2001:db8:77::77" bash -c "sudo -n nft list table ip6 machina_egress | grep -q 'snat to 2001:db8:77::77'"
check "IPv6: client seen as the egress IP" [ "$(seen "$CIP" 2001:db8:78::2)" = 2001:db8:77::77 ]
check "IPv6: IPv4 rule still applies" [ "$(seen "$CIP" 198.51.100.2)" = 198.51.100.77 ]
check "IPv6: remove the egress IP" FM egress np-blue ip "$HN" -
fsync
no_egress6() { for _ in $(seq 20); do sudo -n nft list table ip6 machina_egress >/dev/null 2>&1 || return 0; sleep 1; fsync; done; return 1; }
check "IPv6 removed: ip6 table gone" no_egress6
check "IPv6 removed: client seen as its ULA again" [ "$(seen "$CIP" 2001:db8:78::2)" = fd00:6e70::10 ]
fi

echo "== fleet: learned addresses and overlay =="
learned() { bpfd '{"op":"vm_edge_status"}' | python3 -c "import json,sys; d=json.load(sys.stdin); d=d.get('data',d); sys.exit(0 if '$CIP' in d.get('learned',{}).get('np-client',[]) else 1)"; }
check "bpfd learned the client's address from its tap traffic" learned
if ! command -v wg >/dev/null || ip link show machina-wg >/dev/null 2>&1; then
    echo "SKIP  overlay: wg missing or machina-wg already in use"
else
OVERLAY_ON=1
check "enable the overlay" FM overlay enable
ov_mapped() {
    for _ in $(seq 20); do
        NP_JSON=1 FM overlay 2>/dev/null | jq -e --arg c "$CIP" --arg s "$SIP" '.items[0] | .interface == "machina-wg" and (.public_key | length) == 44
          and ([.mappings[] | select(.local == $c or .local == $s) | .fleet | startswith("100.96.")] == [true, true])
          and all(.mappings[]; .fleet | startswith("100.96.") or startswith("fd6d:6163:6869:"))' >/dev/null && return
        sleep 1; fsync
    done
    return 1
}
check "overlay up: public key, both VMs mapped to fleet addresses" ov_mapped
check "overlay: nft maps the client" bash -c "sudo -n nft list table ip machina_overlay | grep -q '$CIP'"
check "overlay: fleet range unreachable without a peer" bash -c "ip route show type unreachable | grep -q '^unreachable 100.96.0.0/12'"
check "overlay: unmapped new connections from peers dropped" bash -c "sudo -n nft list table ip machina_overlay | grep -q 'ct status ! dnat drop'"
check "disable the overlay" FM overlay disable
ov_gone() { for _ in $(seq 20); do ! ip link show machina-wg >/dev/null 2>&1 && ! sudo -n nft list table ip machina_overlay >/dev/null 2>&1 && ! ip route show type unreachable | grep -q 100.96.0.0/12 && return; sleep 1; done; return 1; }
check "overlay off: interface, tables and routes removed" ov_gone
OVERLAY_ON=
fi

echo "== fleet: evidence =="
check "export JSON evidence" FM evidence -o json --out "$W/ev.json"
check "evidence digest verifies" "$M" netpol evidence verify "$W/ev.json"
check "evidence is signed by the controller CA" bash -c "'$M' netpol evidence verify '$W/ev.json' 2>/dev/null | grep -q 'ok: signed (ecdsa-p256-sha256)'"
tampered_sig() { jq -c '.signature.value = "MEUCIQ"' "$W/ev.json" >"$W/ev-bad.json" && ! "$M" netpol evidence verify "$W/ev-bad.json" >/dev/null 2>&1; }
check "evidence: a replaced signature is rejected" tampered_sig
check "project evidence for np-red" FM evidence --project np-red --probes tcp/80,tcp/22 -o json --out "$W/ev-red.json"
check "project evidence: only np-red against the others" jq -e '([.matrix[].from] | unique) as $f | ($f | index("np-red")) and ($f | index("np-blue") | not) and ([.matrix[].to] | index("(other projects)"))' "$W/ev-red.json"
check "project evidence: probes as asked" jq -e '.matrix_probes == ["TCP/80", "TCP/22"]' "$W/ev-red.json"
check "evidence archive lists" FM evidence archive
check "evidence: np-red isolated" jq -e '.projects[] | select(.project == "np-red") | .isolated' "$W/ev.json"
check "evidence: np-blue → np-red segmented" jq -e '.matrix[] | select(.from == "np-blue" and .to == "np-red") | (.allowed | length) == 0' "$W/ev.json"
check "evidence: host → np-red allowed" jq -e '.matrix[] | select(.from == "host" and .to == "np-red") | (.allowed | length) > 0' "$W/ev.json"
check "evidence: dropped client → server connection listed" bash -c "jq -r '.denied[] | \"\(.src) \(.dst) \(.port)\"' '$W/ev.json' | grep -Eq '(np-client|$CIP).*(np-server|$SIP) 80'"
check "evidence: Markdown export" bash -c "'$M' netpol evidence -o md --fleet | grep -q '## Reachability matrix'"

# Approval: with [vm_netpol] project_approval the requester proposes, a second
# admin approves. The second admin is a temporary controller user.
echo "== fleet: approval =="
CTRL="${MACHINA_CONTROLLER_URL:-http://127.0.0.1:5093}"
# Through the daemon proxy the controller sees its service account, not $MACHINA_USER.
propose_open() { NP_JSON=1 FM project open np-red --propose 2>/dev/null | jq -r '.pending | select(.id) | "\(.id) \(.requested_by)"'; }
red_isolated() { "$M" netpol projects --fleet | grep -q 'project-isolation-np-red-'; }
read -r AID REQ <<<"$(propose_open)"
check "propose: opening np-red waits for a second admin" [ -n "$AID" ]
check "propose: np-red still isolated" red_isolated
check "propose: the requester cannot approve it" bash -c "! '$M' netpol project approve '$AID' --fleet"
check "reject the proposal" FM project reject "$AID"
check "rejected: np-red still isolated" red_isolated
approver() {
    local u="np-approver-$$"
    (umask 077; openssl rand -hex 16 >"$W/apw") || return 1
    jq -n --arg u "$u" --rawfile p "$W/apw" '{username: $u, password: ($p | rtrimstr("\n")), role: "admin"}' >"$W/au.json"
    curl -sk -b "$JAR" -X POST -H 'Content-Type: application/json' --data-binary @"$W/au.json" \
        "$MACHINA_URL/api/v1/platform/controller/api/v1/users" | jq -er .id >"$W/aid" || return 1
    jq 'del(.role)' "$W/au.json" | curl -sf -X POST -H 'Content-Type: application/json' --data-binary @- "$CTRL/api/v1/auth/login" \
        | jq -er '"Authorization: Bearer " + .token' >"$W/ah"
}
check "a temporary second admin" approver
read -r AID REQ <<<"$(propose_open)"
check "propose again" [ -n "$AID" ]
check "second admin approves" curl -sf -o /dev/null -H @"$W/ah" -H 'Content-Type: application/json' -d '{}' -X POST "$CTRL/api/v1/ai/actions/$AID/execute"
red_open() { for _ in $(seq 15); do red_isolated || return 0; sleep 1; fsync; done; return 1; }
check "approved: np-red open" red_open
approval_in_evidence() {
    FM evidence -o json --out "$W/ev2.json" >/dev/null 2>&1 \
        && jq -e --arg u "np-approver-$$" --arg r "$REQ" \
            '[.approvals[] | select(.action_type == "vm_netpol.project" and .approved_by == $u and .requested_by == $r)] | length > 0' "$W/ev2.json"
}
check "approval in evidence (requester and approver)" approval_in_evidence
check "re-isolate np-red" FM project isolate np-red

echo "== fleet: scale to zero =="
sp_json() { NP_JSON=1 "$M" vm sleep-policy np-server 2>/dev/null; }
saved() {
    for _ in $(seq 90); do
        [[ "$(sudo -n virsh domstate np-server 2>/dev/null)" == "shut off" ]] \
            && sudo -n virsh dominfo np-server | grep -Eq 'Managed save: +yes' && return
        sleep 1
    done
    { echo "      state: $(sudo -n virsh domstate np-server --reason 2>&1)"
      sudo -n virsh dominfo np-server 2>&1 | grep -i 'managed'
      bpfd '{"op":"vm_wake_status"}' | jq -c '.wakes[-3:]'; } | sed 's/^/      /' >&3
    return 1
}
wake_listed() {
    for _ in $(seq 60); do
        bpfd '{"op":"vm_wake_status"}' | jq -e --arg s "$SIP" '.entries[] | select(.vm == "np-server") | .addresses | index($s)' >/dev/null && return
        sleep 1
    done
    return 1
}
desired() { for _ in $(seq 90); do [[ "$(sp_json | jq -r .desired_state)" == "$1" ]] && return; sleep 2; done; return 1; }
# The quarantine phase's client → server SSH session keeps retransmitting,
# which would (rightly) wake the server.
cssh "pkill -x ssh" >/dev/null 2>&1
check "sleep np-server" "$M" vm sleep np-server
check "np-server managed-saved (shut off with a managed save)" saved
check "controller: np-server desired sleeping" desired sleeping
check "bpfd wake set lists np-server with its address" wake_listed
check "host neighbor entry for np-server pinned while asleep" bash -c "ip neigh show '$SIP' | grep -q PERMANENT"
check "fleet summary lists np-server and the RAM handed back" bash -c "'$M' vm sleeping | grep -q np-server"
asleep_quiet() {
    sleep 20
    [[ "$(sudo -n virsh domstate np-server 2>/dev/null)" == "shut off" ]] && return
    bpfd '{"op":"vm_wake_status"}' | jq -c '.wakes[-2:]' | sed 's/^/      /' >&3
    return 1
}
check "stays asleep with no traffic for it (20 s)" asleep_quiet
T0=$(date +%s%N)
woke_http() { curl -fs -m 40 "http://$SIP/ok" | grep -q ok; }
check "first request to the sleeping VM wakes it and is answered" woke_http
echo "      first request answered after $(( ($(date +%s%N) - T0) / 1000000 )) ms"
check "agent logged the restore, woken by the host's request" bash -c "sudo -n journalctl -u machina-agent --since '-3min' --no-pager | grep 'wake: restored sleeping vm' | tail -1 | grep -q 'via=\"host\"'"
check "controller: np-server desired running again" desired running
unlisted() {
    for _ in $(seq 60); do
        bpfd '{"op":"vm_wake_status"}' | jq -e '[.entries[] | select(.vm == "np-server")] | length == 0' >/dev/null && return
        sleep 2
    done
    return 1
}
check "woken VM leaves the wake set" unlisted
check "neighbor pin released on wake" bash -c "! ip neigh show '$SIP' | grep -q PERMANENT"
check "wake (traffic) in the VM's sleep history" bash -c "NP_JSON=1 '$M' vm sleep-policy np-server | jq -e '[.events[] | select(.kind == \"wake\" and .reason == \"traffic\")] | length > 0'"
check "sleep np-server again" "$M" vm sleep np-server
check "managed-saved again" saved
check "listed again" wake_listed
cssh "curl -s -m 40 -o /dev/null http://$SIP/ok" >/dev/null 2>&1 &
PEER_PID=$!
woke_peer() { for _ in $(seq 40); do [[ "$(sudo -n virsh domstate np-server 2>/dev/null)" == running ]] && return; sleep 1; done; return 1; }
check "a request from a VM on the same bridge wakes it" woke_peer
check "the peer wake came in through bridge ARP" bash -c "sudo -n journalctl -u machina-agent --since '-2min' --no-pager | grep 'wake: restored sleeping vm' | tail -1 | grep -q 'via=\"arp\"'"
wait "$PEER_PID"
check "controller: running after the peer wake" desired running
check "auto-sleep policy: 15 min" "$M" vm sleep-policy np-server 15
check "policy reads back 15 min" bash -c "NP_JSON=1 '$M' vm sleep-policy np-server | jq -e '.effective_minutes == 15'"
check "a policy under 5 minutes is refused" bash -c "! '$M' vm sleep-policy np-server 2"
check "auto-sleep policy: never" "$M" vm sleep-policy np-server never

echo "passed=$P failed=$F"
[[ $F -eq 0 ]]
