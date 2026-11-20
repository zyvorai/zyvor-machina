#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

#
# Chaos game days against real libvirt VMs, driven through the controller:
# latency, loss and a partition from a peer VM on the target's tap (checked
# from the host and from the peer while each step runs), a disk throttle seen
# in libvirt, a kill the reconcile loop recovers from, a simulated host
# failure, an automatic abort when probes fail, a manual abort, and no fault
# left behind after any of them.
# Creates ch-a (serves HTTP on 8080) and ch-b on the libvirt default NAT
# network and deletes them, their disks and the base image on exit.
#
#   printf '%s\n' "$PASS" | bash scripts/fleet/chaos-realvm.sh
#
# The machina password is read from stdin (MACHINA_USER defaults to $USER).
set -uo pipefail

read -r MACHINA_PASS
MACHINA_USER="${MACHINA_USER:-$USER}"
MACHINA_URL="${MACHINA_URL:-https://127.0.0.1:5092}"
IMG_URL="${IMG_URL:-https://cloud.debian.org/images/cloud/bookworm/latest/debian-12-genericcloud-amd64.qcow2}"
POOL=/var/lib/libvirt/images
BASE="$POOL/ch-realvm-base.qcow2"
DB="${CONTROLLER_DB:-/var/lib/machina/controller.db}"
W="$(mktemp -d /tmp/ch-realvm.XXXXXX)"
JAR="$W/jar"
VMS=(ch-a ch-b)
EXPS=(ch-net ch-kill ch-abort ch-manual ch-guard)
P=0; F=0

ok() { echo "PASS  $1"; P=$((P+1)); }
bad() { echo "FAIL  $1"; F=$((F+1)); }
check() { local n=$1; shift; if "$@" >/dev/null 2>&1; then ok "$n"; else bad "$n"; fi; }
capi() {
    local method=$1 path=$2
    shift 2
    curl -sk -b "$JAR" -X "$method" -H 'Content-Type: application/json' "$@" \
        "$MACHINA_URL/api/v1/platform/controller/api/v1$path"
}
db() { sudo -n sqlite3 -cmd ".timeout 10000" "$DB" "$@"; }
names_sql() { printf "'%s'," "$@" | sed 's/,$//'; }

cleanup() {
    for e in "${EXPS[@]}"; do
        id=$(capi GET /chaos/experiments 2>/dev/null | jq -r --arg n "$e" '.[] | select(.name == $n) | .id' 2>/dev/null)
        [[ -n "$id" ]] && capi DELETE "/chaos/experiments/$id" >/dev/null 2>&1
    done
    for v in "${VMS[@]}"; do
        sudo -n virsh destroy "$v" >/dev/null 2>&1
        sudo -n virsh undefine "$v" --nvram >/dev/null 2>&1 || sudo -n virsh undefine "$v" >/dev/null 2>&1
        sudo -n rm -f "$POOL/$v.qcow2" "$POOL/$v-seed.iso"
    done
    sudo -n rm -f "$BASE"
    db "DELETE FROM chaos_experiments WHERE name IN ($(names_sql "${EXPS[@]}"));
        DELETE FROM vms WHERE name IN ($(names_sql "${VMS[@]}"));" 2>/dev/null
    rm -rf "$W"
    echo "cleanup: experiments, VMs, disks, controller rows and base image removed"
    echo
    echo "passed=$P failed=$F"
}

for v in "${VMS[@]}"; do
    sudo -n virsh dominfo "$v" >/dev/null 2>&1 && { echo "$v already exists; refusing to touch it" >&2; exit 1; }
done
trap cleanup EXIT

(umask 077 && : >"$JAR")
code=$(MACHINA_USER="$MACHINA_USER" MACHINA_PASS="$MACHINA_PASS" jq -n '{username: env.MACHINA_USER, password: env.MACHINA_PASS}' |
    curl -sk -c "$JAR" -o /dev/null -w '%{http_code}' -X POST "$MACHINA_URL/api/v1/auth/login" \
        -H 'Content-Type: application/json' --data-binary @-)
[[ "$code" == 200 ]] && ok "login" || { bad "login (HTTP $code)"; exit 1; }

echo "== VMs =="
ssh-keygen -q -t ed25519 -N '' -f "$W/key"
sudo -n curl -fsSL -o "$BASE" "$IMG_URL" || { bad "download $IMG_URL"; exit 1; }
for v in "${VMS[@]}"; do
    cat > "$W/user-$v" <<EOF
#cloud-config
hostname: $v
users:
  - name: np
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    ssh_authorized_keys: ["$(cat "$W/key.pub")"]
write_files:
  - path: /etc/systemd/system/ch-http.service
    content: |
      [Unit]
      After=network-online.target
      [Service]
      ExecStart=/usr/bin/python3 -m http.server 8080 --directory /srv
      Restart=always
      [Install]
      WantedBy=multi-user.target
runcmd:
  - [sh, -c, "mkdir -p /srv && echo $v > /srv/index.html && systemctl daemon-reload && systemctl enable --now ch-http"]
EOF
    printf 'instance-id: %s-%s\nlocal-hostname: %s\n' "$v" "$$" "$v" > "$W/meta-$v"
    sudo -n cloud-localds "$POOL/$v-seed.iso" "$W/user-$v" "$W/meta-$v"
    sudo -n qemu-img create -q -f qcow2 -F qcow2 -b "$BASE" "$POOL/$v.qcow2" 8G
    sudo -n virt-install --name "$v" --memory 768 --vcpus 1 --import --osinfo detect=on,require=off \
        --disk "path=$POOL/$v.qcow2,format=qcow2,bus=virtio" --disk "path=$POOL/$v-seed.iso,device=cdrom" \
        --network network=default,model=virtio --graphics none --noautoconsole >/dev/null 2>&1 \
        && ok "virt-install $v" || { bad "virt-install $v"; exit 1; }
done

ip_of() { sudo -n virsh domifaddr "$1" --source lease 2>/dev/null | awk '/ipv4/ {sub(/\/.*/, "", $4); print $4; exit}'; }
tap_of() { sudo -n virsh domiflist "$1" 2>/dev/null | awk 'NR > 2 && $1 {print $1; exit}'; }
vssh() { local h=$1; shift; ssh -i "$W/key" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=5 -o LogLevel=ERROR "np@$h" "$@"; }
for _ in $(seq 90); do AIP=$(ip_of ch-a); BIP=$(ip_of ch-b); [[ -n "$AIP" && -n "$BIP" ]] && break; sleep 2; done
[[ -n "${AIP:-}" && -n "${BIP:-}" ]] && ok "DHCP leases: ch-a $AIP, ch-b $BIP" || { bad "DHCP leases"; exit 1; }
http_a() { curl -s -m3 "http://$AIP:8080/" | grep -q ch-a; }
for _ in $(seq 150); do http_a && vssh "$BIP" true 2>/dev/null && break; sleep 2; done
check "ch-a serves HTTP" http_a
check "ssh into ch-b" vssh "$BIP" true
b_reaches_a() { vssh "$BIP" "curl -s -m3 http://$AIP:8080/" 2>/dev/null | grep -q ch-a; }
check "ch-b reaches ch-a before any fault" b_reaches_a

echo "== controller =="
vm_id() { capi GET /vms | jq -r --arg n "$1" '.[] | select(.name == $n) | .id // empty'; }
vm_field() { capi GET "/vms/$1" | jq -r ".$2 // empty"; }
for _ in $(seq 60); do A=$(vm_id ch-a); B=$(vm_id ch-b); [[ -n "$A" && -n "$B" ]] && break; sleep 3; done
[[ -n "${A:-}" && -n "${B:-}" ]] && ok "controller inventory has ch-a and ch-b" || { bad "controller inventory"; exit 1; }
for _ in $(seq 60); do [[ -n "$(vm_field "$B" guest_ip)" && "$(vm_field "$A" observed_state)" == running ]] && break; sleep 3; done
check "controller knows ch-b's address" bash -c "[[ -n '$(vm_field "$B" guest_ip)' ]]"
if [[ "$(vm_field "$A" desired_state)" != running ]]; then
    capi POST "/vms/$A/start" -d '{}' >/dev/null
    for _ in $(seq 20); do [[ "$(vm_field "$A" desired_state)" == running ]] && break; sleep 1; done
fi
check "ch-a is meant to stay running" bash -c "[[ '$(vm_field "$A" desired_state)' == running ]]"
HOST=$(vm_field "$A" host_id)
TAP=$(tap_of ch-a)
echo "      ch-a tap: $TAP"

create() {
    capi POST /chaos/experiments -o "$W/created" -w '%{http_code}' \
        -d "$(jq -nc --arg n "$1" --argjson s "$2" '{name: $n, description: "real-VM test", spec: $s}')"
}
eid() { capi GET /chaos/experiments | jq -r --arg n "$1" '.[] | select(.name == $n) | .id // empty'; }
run() { capi POST "/chaos/experiments/$(eid "$1")/run" -d "$(jq -nc --arg n "$1" '{confirm: $n}')" | jq -r '.run_id // empty'; }
run_status() { capi GET "/chaos/runs/$1" | jq -r .run.status; }
phase() { capi GET "/chaos/runs/$1" | jq -r '.live.phase // -1'; }
wait_phase() { for _ in $(seq 120); do [[ "$(phase "$1")" -ge "$2" ]] && return 0; [[ "$(run_status "$1")" != running ]] && return 1; sleep 1; done; return 1; }
wait_end() { for _ in $(seq "${2:-600}"); do [[ "$(run_status "$1")" != running ]] && return 0; sleep 1; done; return 1; }
report() { capi GET "/chaos/runs/$1" | jq -c "$2"; }
no_faults() { [[ "$(capi GET /chaos/faults | jq '[.items[] | select(.vm == "ch-a" or .vm == "ch-b")] | length')" == 0 ]]; }
no_netem() { ! sudo -n tc qdisc show dev "$(tap_of ch-a)" root | grep -q netem; }
no_table() { ! sudo -n nft list table bridge machina_chaos >/dev/null 2>&1; }
rtt() { ping -c4 -i0.3 -W3 "$AIP" 2>/dev/null | awk -F/ '/^rtt/ {printf "%d", $5}'; }

echo "== guards =="
code=$(create ch-guard "$(jq -nc --arg a "$A" '{targets: [$a], steps: [{kind: "latency", delay_ms: 20000, jitter_ms: 0, secs: 10}]}')")
[[ "$code" == 400 ]] && ok "delay over 10 s rejected" || bad "delay over 10 s rejected (HTTP $code $(cat "$W/created"))"
db "UPDATE vms SET tags = '[\"chaos=protected\"]' WHERE name = 'ch-b'"
code=$(create ch-guard "$(jq -nc --arg b "$B" '{targets: [$b], steps: [{kind: "loss", loss_pct: 10, secs: 10}]}')")
[[ "$code" == 403 ]] && ok "a chaos=protected VM can't be targeted" || bad "a chaos=protected VM can't be targeted (HTTP $code $(cat "$W/created"))"
db "UPDATE vms SET tags = '[]' WHERE name = 'ch-b'"

echo "== network steps =="
SPEC=$(jq -nc --arg a "$A" --arg b "$B" --arg url "http://$AIP:8080/" '{
    targets: [$a],
    steps: [
        {kind: "latency", delay_ms: 300, jitter_ms: 0, secs: 25},
        {kind: "loss", loss_pct: 20, secs: 15},
        {kind: "partition", cidrs: [], peers: [$b], secs: 25}
    ],
    probes: [{kind: "http", url: $url, expect_status: 200}],
    abort: {min_success_pct: 30, window_secs: 20},
    baseline_secs: 10, recovery_secs: 10
}')
code=$(create ch-net "$SPEC")
[[ "$code" == 200 ]] && ok "create ch-net" || bad "create ch-net (HTTP $code $(cat "$W/created"))"
code=$(capi POST "/chaos/experiments/$(eid ch-net)/run" -o "$W/r" -w '%{http_code}' -d '{"confirm":"wrong"}')
[[ "$code" == 400 ]] && ok "run refused without the typed name" || bad "run refused without the typed name (HTTP $code)"
R=$(run ch-net)
[[ -n "$R" ]] && ok "run ch-net" || bad "run ch-net"
code=$(capi POST "/chaos/experiments/$(eid ch-net)/run" -o "$W/r" -w '%{http_code}' -d '{"confirm":"ch-net"}')
[[ "$code" == 409 ]] && ok "a second run of the same experiment is refused" || bad "a second run is refused (HTTP $code)"
wait_phase "$R" 1 && sleep 4
T=$(rtt); echo "      rtt to ch-a during latency: ${T:-?} ms"
check "latency step: rtt to ch-a at least 280 ms" bash -c "[[ ${T:-0} -ge 280 ]]"
has_netem() { sudo -n tc qdisc show dev "$(tap_of ch-a)" root | grep -q netem; }
check "latency step: netem on ch-a's tap" has_netem
check "latency step: fault listed with its host" bash -c "capi() { curl -sk -b '$JAR' '$MACHINA_URL/api/v1/platform/controller/api/v1/chaos/faults'; }; capi | jq -e '.items[] | select(.vm == \"ch-a\" and .delay_ms == 300 and (.host | length) > 0)'"
check "latency step: ch-b unaffected (only traffic to ch-a)" bash -c "[[ \$(ping -c3 -i0.3 -W2 $BIP | awk -F/ '/^rtt/ {printf \"%d\", \$5}') -lt 100 ]]"
wait_phase "$R" 3 && sleep 4
check "partition step: ch-b can't reach ch-a" bash -c "$(declare -f vssh b_reaches_a); W='$W' BIP='$BIP' AIP='$AIP'; ! b_reaches_a"
check "partition step: the host still reaches ch-a" http_a
check "partition step: bridge table installed" sudo -n nft list table bridge machina_chaos
wait_end "$R" 300
check "ch-net passed" bash -c "[[ '$(run_status "$R")' == passed ]]"
check "report: 5 phases" bash -c "[[ '$(report "$R" '.run.report.phases | length')' == 5 ]]"
LAT=$(report "$R" '.run.report.phases[1].p50_ms'); BASEP=$(report "$R" '.run.report.phases[0].p50_ms')
echo "      probe p50: baseline ${BASEP} ms, latency step ${LAT} ms"
check "report: latency step p50 at least 300 ms" bash -c "[[ ${LAT:-0} != null && ${LAT:-0} -ge 300 ]]"
check "report: a p95 finding for the latency step" bash -c "capi() { curl -sk -b '$JAR' '$MACHINA_URL/api/v1/platform/controller/api/v1/chaos/runs/$R'; }; capi | jq -e '.run.report.findings | map(select(test(\"p95 latency\"))) | length > 0'"
check "after the run: no faults listed" no_faults
check "after the run: no netem on the tap" no_netem
check "after the run: no partition table" no_table
check "after the run: ch-b reaches ch-a again" b_reaches_a

echo "== disk, kill, host failure =="
SPEC=$(jq -nc --arg a "$A" --arg h "$HOST" --arg url "http://$AIP:8080/" '{
    targets: [$a],
    steps: [
        {kind: "disk", read_iops: 20, write_iops: 20, secs: 20},
        {kind: "host_failure", host_id: $h},
        {kind: "kill", recover_secs: 300}
    ],
    probes: [{kind: "vm_running", vm: $a}, {kind: "http", url: $url, expect_status: 200}],
    abort: {min_success_pct: 0, window_secs: 20},
    baseline_secs: 5, recovery_secs: 20
}')
code=$(create ch-kill "$SPEC")
[[ "$code" == 200 ]] && ok "create ch-kill" || bad "create ch-kill (HTTP $code $(cat "$W/created"))"
R=$(run ch-kill)
wait_phase "$R" 1 && sleep 4
iops() { sudo -n virsh blkdeviotune ch-a vda 2>/dev/null | awk '/^read_iops_sec[ :]/ {print $NF}'; }
check "disk step: libvirt read IOPS limit is 20" bash -c "[[ '$(iops)' == 20 ]]"
wait_phase "$R" 3
DOWN=no
for _ in $(seq 30); do [[ "$(sudo -n virsh domstate ch-a)" != running ]] && { DOWN=yes; break; }; sleep 1; done
[[ "$DOWN" == yes ]] && ok "kill step: ch-a was destroyed" || bad "kill step: ch-a was destroyed"
check "disk limit lifted after its step" bash -c "[[ '$(iops)' == 0 ]]"
wait_end "$R" 600
echo "      ch-kill: $(run_status "$R"); $(report "$R" '.run.report.findings')"
check "ch-kill passed" bash -c "[[ '$(run_status "$R")' == passed ]]"
REC=$(report "$R" '.run.report.phases[3].recovered_s')
echo "      ch-a back after ${REC} s"
check "report: kill step recovered" bash -c "[[ '$REC' != null && -n '$REC' ]]"
check "report: host failure notes from the impact analysis" bash -c "[[ '$(report "$R" '.run.report.phases[2].notes | length')' -ge 1 ]]"
check "ch-a running again" bash -c "[[ \$(sudo -n virsh domstate ch-a) == running ]]"
for _ in $(seq 60); do http_a && break; sleep 2; done
check "ch-a serving again" http_a
check "disk limit still lifted after the reboot" bash -c "[[ '$(iops)' == 0 ]]"

echo "== automatic abort =="
SPEC=$(jq -nc --arg a "$A" --arg url "http://$AIP:8080/" '{
    targets: [$a],
    steps: [{kind: "loss", loss_pct: 100, secs: 120}],
    probes: [{kind: "http", url: $url, expect_status: 200}],
    abort: {min_success_pct: 50, window_secs: 8},
    baseline_secs: 6, recovery_secs: 5
}')
code=$(create ch-abort "$SPEC")
[[ "$code" == 200 ]] && ok "create ch-abort" || bad "create ch-abort (HTTP $code $(cat "$W/created"))"
T0=$(date +%s)
R=$(run ch-abort)
wait_end "$R" 90
EL=$(( $(date +%s) - T0 ))
echo "      ch-abort ended after ${EL} s: $(capi GET "/chaos/runs/$R" | jq -r .run.abort_reason)"
check "ch-abort aborted by its probes" bash -c "[[ '$(run_status "$R")' == aborted ]]"
check "it stopped well before the 120 s step" bash -c "[[ $EL -lt 60 ]]"
check "after the abort: no faults listed" no_faults
check "after the abort: no netem on the tap" no_netem
check "after the abort: ch-a reachable" http_a

echo "== manual abort =="
SPEC=$(jq -nc --arg a "$A" '{targets: [$a], steps: [{kind: "latency", delay_ms: 200, jitter_ms: 0, secs: 120}], baseline_secs: 2, recovery_secs: 2}')
create ch-manual "$SPEC" >/dev/null
R=$(run ch-manual)
wait_phase "$R" 1 && sleep 2
check "manual: latency applied" has_netem
code=$(capi POST "/chaos/runs/$R/abort" -o "$W/ab" -w '%{http_code}' -d '{}')
[[ "$code" == 200 ]] && ok "abort accepted" || bad "abort accepted (HTTP $code $(cat "$W/ab"))"
wait_end "$R" 30
check "manual: run aborted" bash -c "[[ '$(run_status "$R")' == aborted ]]"
check "manual: netem gone right away" no_netem
code=$(capi POST "/chaos/runs/$R/abort" -o "$W/ab" -w '%{http_code}' -d '{}')
[[ "$code" == 409 ]] && ok "aborting an ended run is a conflict" || bad "aborting an ended run (HTTP $code)"
