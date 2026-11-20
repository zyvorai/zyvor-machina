#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

#
# Preemptible instances against real libvirt VMs, driven through the
# controller: a reserve just above the host's free memory preempts only the
# lowest-priority VM (managed save, not a stop), traffic does not wake it,
# lowering the reserve brings it back from its saved memory, and a preempted
# VM made regular wakes on traffic like any sleeping VM.
# Creates pr-low (priority 0) and pr-high (priority 50) on the libvirt default
# NAT network and deletes them, their disks and the base image on exit; the
# preemption settings found at the start are put back.
#
#   printf '%s\n' "$PASS" | bash scripts/fleet/preempt-realvm.sh
#
# The machina password is read from stdin (MACHINA_USER defaults to $USER).
set -uo pipefail

read -r MACHINA_PASS
MACHINA_USER="${MACHINA_USER:-$USER}"
MACHINA_URL="${MACHINA_URL:-https://127.0.0.1:5092}"
IMG_URL="${IMG_URL:-https://cloud.debian.org/images/cloud/bookworm/latest/debian-12-genericcloud-amd64.qcow2}"
POOL=/var/lib/libvirt/images
BASE="$POOL/pr-realvm-base.qcow2"
DB="${CONTROLLER_DB:-/var/lib/machina/controller.db}"
W="$(mktemp -d /tmp/pr-realvm.XXXXXX)"
JAR="$W/jar"
VMS=(pr-low pr-high)
P=0; F=0
SAVED=""

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
settings() { capi PUT /preemption/settings -o /dev/null -w '%{http_code}' -d "$(jq -nc --argjson e "$1" --argjson r "$2" '{enabled: $e, reserve_pct: $r}')"; }

cleanup() {
    if [[ -n "$SAVED" ]]; then
        capi PUT /preemption/settings -d "$SAVED" >/dev/null 2>&1
        echo "preemption settings restored: $SAVED"
    fi
    for v in "${VMS[@]}"; do
        sudo -n virsh destroy "$v" >/dev/null 2>&1
        sudo -n virsh managedsave-remove "$v" >/dev/null 2>&1
        sudo -n virsh undefine "$v" --nvram >/dev/null 2>&1 || sudo -n virsh undefine "$v" >/dev/null 2>&1
        sudo -n rm -f "$POOL/$v.qcow2" "$POOL/$v-seed.iso"
    done
    sudo -n rm -f "$BASE"
    db "DELETE FROM vm_sleep_events WHERE vm_id IN (SELECT id FROM vms WHERE name IN ($(names_sql "${VMS[@]}")));
        DELETE FROM vms WHERE name IN ($(names_sql "${VMS[@]}"));" 2>/dev/null
    rm -rf "$W"
    echo "cleanup: VMs, disks, controller rows and base image removed"
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
SAVED=$(capi GET /preemption | jq -c '.settings // empty')
[[ -n "$SAVED" ]] && ok "preemption overview ($SAVED)" || { bad "preemption overview"; exit 1; }
PRE=$(capi GET /preemption | jq '[.vms[] | select(.preempted_at == null and .observed_state == "running")] | length')
[[ "$PRE" == 0 ]] || { bad "other preemptible VMs are running ($PRE); refusing to squeeze them"; exit 1; }

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
runcmd:
  - [sh, -c, "date +%s%N > /var/tmp/boot-mark"]
EOF
    printf 'instance-id: %s-%s\nlocal-hostname: %s\n' "$v" "$$" "$v" > "$W/meta-$v"
    sudo -n cloud-localds "$POOL/$v-seed.iso" "$W/user-$v" "$W/meta-$v"
    sudo -n qemu-img create -q -f qcow2 -F qcow2 -b "$BASE" "$POOL/$v.qcow2" 8G
    sudo -n virt-install --name "$v" --memory 1024 --vcpus 1 --import --osinfo detect=on,require=off \
        --disk "path=$POOL/$v.qcow2,format=qcow2,bus=virtio" --disk "path=$POOL/$v-seed.iso,device=cdrom" \
        --network network=default,model=virtio --graphics none --noautoconsole >/dev/null 2>&1 \
        && ok "virt-install $v" || { bad "virt-install $v"; exit 1; }
done

ip_of() { sudo -n virsh domifaddr "$1" --source lease 2>/dev/null | awk '/ipv4/ {sub(/\/.*/, "", $4); print $4; exit}'; }
vssh() { local h=$1; shift; ssh -i "$W/key" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=5 -o LogLevel=ERROR "np@$h" "$@"; }
domstate() { sudo -n virsh domstate "$1" 2>/dev/null | head -1; }
managed_save() { sudo -n virsh dominfo "$1" 2>/dev/null | awk -F: '/Managed save/ {gsub(/ /, "", $2); print $2}'; }
wait_for() { local tries=$1; shift; for _ in $(seq "$tries"); do "$@" >/dev/null 2>&1 && return 0; sleep 5; done; return 1; }
for _ in $(seq 90); do LIP=$(ip_of pr-low); HIP=$(ip_of pr-high); [[ -n "$LIP" && -n "$HIP" ]] && break; sleep 2; done
[[ -n "${LIP:-}" && -n "${HIP:-}" ]] && ok "DHCP leases: pr-low $LIP, pr-high $HIP" || { bad "DHCP leases"; exit 1; }
for _ in $(seq 150); do vssh "$LIP" test -f /var/tmp/boot-mark 2>/dev/null && break; sleep 2; done
check "ssh into pr-low" vssh "$LIP" true
MARK=$(vssh "$LIP" cat /var/tmp/boot-mark 2>/dev/null)
vssh "$LIP" 'nohup sh -c "while :; do date +%s > /var/tmp/alive; sleep 1; done" >/dev/null 2>&1 &'

echo "== controller =="
vm_id() { capi GET /vms | jq -r --arg n "$1" '.[] | select(.name == $n) | .id // empty'; }
vm_field() { capi GET "/vms/$1" | jq -r ".$2 // empty"; }
for _ in $(seq 60); do L=$(vm_id pr-low); H=$(vm_id pr-high); [[ -n "$L" && -n "$H" ]] && break; sleep 3; done
[[ -n "${L:-}" && -n "${H:-}" ]] && ok "controller inventory has pr-low and pr-high" || { bad "controller inventory"; exit 1; }
for v in "$L" "$H"; do
    for _ in $(seq 60); do [[ -n "$(vm_field "$v" guest_ip)" && "$(vm_field "$v" observed_state)" == running ]] && break; sleep 3; done
    [[ "$(vm_field "$v" desired_state)" != running ]] && capi POST "/vms/$v/start" -d '{}' >/dev/null
done
for _ in $(seq 20); do [[ "$(vm_field "$L" desired_state)" == running && "$(vm_field "$H" desired_state)" == running ]] && break; sleep 1; done
check "both meant to stay running" bash -c "[[ '$(vm_field "$L" desired_state)$(vm_field "$H" desired_state)' == runningrunning ]]"
HOST=$(vm_field "$L" host_id)
db "UPDATE vms SET memory_mib = 1024 WHERE name IN ($(names_sql "${VMS[@]}"))"

echo "== guards =="
code=$(capi PUT "/vms/$L/preemptible" -o /dev/null -w '%{http_code}' -d '{"preemptible":true,"priority":101}')
[[ "$code" == 400 ]] && ok "priority over 100 rejected" || bad "priority over 100 rejected (HTTP $code)"
code=$(settings true 95)
[[ "$code" == 400 ]] && ok "reserve over 90% rejected" || bad "reserve over 90% rejected (HTTP $code)"
code=$(capi PUT "/vms/$L/preemptible" -o /dev/null -w '%{http_code}' -d '{"preemptible":true,"priority":0}')
[[ "$code" == 200 ]] && ok "pr-low preemptible, priority 0" || bad "flag pr-low (HTTP $code)"
code=$(capi PUT "/vms/$H/preemptible" -o /dev/null -w '%{http_code}' -d '{"preemptible":true,"priority":50}')
[[ "$code" == 200 ]] && ok "pr-high preemptible, priority 50" || bad "flag pr-high (HTTP $code)"
check "overview lists both" bash -c "[[ \$(curl -sk -b '$JAR' '$MACHINA_URL/api/v1/platform/controller/api/v1/preemption' | jq '[.vms[] | select(.name == \"pr-low\" or .name == \"pr-high\")] | length') == 2 ]]"

host_json() { capi GET /preemption | jq -c --arg h "$HOST" '.hosts[] | select(.id == $h)'; }
# One point above the free share: the shortfall stays under one 1 GiB VM.
squeeze() {
    local hj total free
    hj=$(host_json)
    total=$(jq -r .total_mib <<<"$hj"); free=$(jq -r .free_mib <<<"$hj")
    echo "      host: ${free} MiB free of ${total} MiB" >&2
    echo $(( free * 100 / total + 1 ))
}
preempted() { [[ "$(domstate "$1")" == "shut off" && "$(managed_save "$1")" == yes ]]; }
running() { [[ "$(domstate "$1")" == running && "$(managed_save "$1")" == no ]]; }
in_wake_set() { sudo -n nft list table inet machina_wake 2>/dev/null | grep -q "daddr $1 "; }
pre_at() { capi GET /preemption | jq -r --arg n "$1" '.vms[] | select(.name == $n) | .preempted_at // empty'; }
last_event() { capi GET /preemption | jq -r --arg n "$1" '[.events[] | select(.vm == $n)][0] | "\(.kind)|\(.reason)"'; }

echo "== pressure =="
check "host reported fresh" bash -c "[[ \$(curl -sk -b '$JAR' '$MACHINA_URL/api/v1/platform/controller/api/v1/preemption' | jq -r --arg h '$HOST' '.hosts[] | select(.id == \$h) | .fresh') == true ]]"
R=$(squeeze)
echo "      reserve set to ${R}%"
code=$(settings true "$R")
[[ "$code" == 200 ]] && ok "reserve ${R}% saved" || bad "reserve saved (HTTP $code)"
wait_for 24 preempted pr-low && ok "pr-low preempted (managed save)" || bad "pr-low preempted ($(domstate pr-low), managed save $(managed_save pr-low))"
settings false "$R" >/dev/null
check "pr-high, the higher priority, still running" running pr-high
check "controller: pr-low sleeping" bash -c "[[ '$(vm_field "$L" desired_state)' == sleeping ]]"
for _ in $(seq 10); do [[ -n "$(pre_at pr-low)" ]] && break; sleep 2; done
check "controller: pr-low marked preempted" bash -c "[[ -n '$(pre_at pr-low)' ]]"
EV=$(last_event pr-low); echo "      event: $EV"
check "event says why" bash -c "[[ '$EV' == sleep\|preempted:*below\ ${R}%\ free\ memory ]]"
check "pr-low is not in the wake set" bash -c "$(declare -f in_wake_set); ! in_wake_set $LIP"
ping -c 15 -i 1 -W 1 "$LIP" >/dev/null 2>&1
curl -s -m 3 "http://$LIP:22/" >/dev/null 2>&1
sleep 10
check "traffic to pr-low does not wake it" preempted pr-low

echo "== capacity freed =="
code=$(settings true 0)
[[ "$code" == 200 ]] && ok "reserve 0% saved" || bad "reserve 0% saved (HTTP $code)"
wait_for 30 running pr-low && ok "pr-low resumed" || bad "pr-low resumed ($(domstate pr-low), managed save $(managed_save pr-low))"
for _ in $(seq 10); do [[ -z "$(pre_at pr-low)" && "$(vm_field "$L" desired_state)" == running ]] && break; sleep 2; done
check "controller: pr-low running, no longer preempted" bash -c "[[ -z '$(pre_at pr-low)' && '$(vm_field "$L" desired_state)' == running ]]"
EV=$(last_event pr-low); echo "      event: $EV"
check "event: capacity freed" bash -c "[[ '$EV' == wake\|capacity\ freed ]]"
for _ in $(seq 30); do vssh "$LIP" true 2>/dev/null && break; sleep 2; done
check "same boot after resume (memory restored, not rebooted)" bash -c "[[ '$(vssh "$LIP" cat /var/tmp/boot-mark 2>/dev/null)' == '$MARK' ]]"
check "guest clock loop still alive" bash -c "a=\$(ssh -i '$W/key' -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR np@$LIP cat /var/tmp/alive); sleep 3; b=\$(ssh -i '$W/key' -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR np@$LIP cat /var/tmp/alive); [[ \$b -gt \$a ]]"
check "pr-high untouched throughout" running pr-high

echo "== made regular while preempted =="
echo "      waiting out the host's settle time"
sleep 65
R=$(squeeze)
settings true "$R" >/dev/null
wait_for 24 preempted pr-low && ok "pr-low preempted again at ${R}%" || bad "pr-low preempted again ($(domstate pr-low))"
settings false "$R" >/dev/null
code=$(capi PUT "/vms/$L/preemptible" -o /dev/null -w '%{http_code}' -d '{"preemptible":false,"priority":0}')
[[ "$code" == 200 ]] && ok "pr-low made regular" || bad "pr-low made regular (HTTP $code)"
check "controller: no longer marked preempted" bash -c "[[ '$(db "SELECT preempted_at IS NULL AND preemptible = 0 FROM vms WHERE name = 'pr-low'")' == 1 ]]"
wait_for 6 in_wake_set "$LIP" && ok "pr-low joined the wake set" || bad "pr-low joined the wake set"
ping -c 3 -W 1 "$LIP" >/dev/null 2>&1
wait_for 12 running pr-low && ok "traffic woke pr-low" || bad "traffic woke pr-low ($(domstate pr-low))"
check "overview no longer lists pr-low" bash -c "[[ -z \$(curl -sk -b '$JAR' '$MACHINA_URL/api/v1/platform/controller/api/v1/preemption' | jq -r '.vms[] | select(.name == \"pr-low\") | .id') ]]"
check "pr-high still running" running pr-high
