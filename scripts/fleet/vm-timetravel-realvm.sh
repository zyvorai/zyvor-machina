#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

#
# Time travel against a real libvirt VM, driven through the controller:
# a restore point, a live disk fork (own MAC, address, hostname and
# machine-id; source never stops), a rewind refused while a fork depends on
# a later point, detaching the fork, rewinding the source (data written after
# the point is gone), a memory fork on the isolated network (RAM carried
# across, no route out), and restore-point pruning by merge.
# Creates tt-src, tt-fork and tt-mem and deletes them, their disk files and
# the base image on exit.
#
#   printf '%s\n' "$PASS" | bash scripts/fleet/vm-timetravel-realvm.sh
#
# The machina password is read from stdin (MACHINA_USER defaults to $USER).
set -uo pipefail

read -r MACHINA_PASS
export MACHINA_USER="${MACHINA_USER:-$USER}" MACHINA_PASS MACHINA_URL="${MACHINA_URL:-https://127.0.0.1:5092}" NO_COLOR=1
HERE="$(cd "$(dirname "$0")/../.." && pwd)"
JAR="${MACHINA_COOKIE_JAR:-$HOME/.machina/cli-session}"
M="$HERE/machinactl"
IMG_URL="${IMG_URL:-https://cloud.debian.org/images/cloud/bookworm/latest/debian-12-genericcloud-amd64.qcow2}"
POOL=/var/lib/libvirt/images
BASE="$POOL/tt-realvm-base.qcow2"
DB="${CONTROLLER_DB:-/var/lib/machina/controller.db}"
W="$(mktemp -d /tmp/tt-realvm.XXXXXX)"
VMS=(tt-src tt-fork tt-mem)
P=0; F=0
exec 3>&2

ok() { echo "PASS  $1"; P=$((P+1)); }
bad() { echo "FAIL  $1"; F=$((F+1)); }
check() { local n=$1; shift; if "$@" >/dev/null 2>&1; then ok "$n"; else bad "$n"; fi; }
capi() {
    local method=$1 path=$2
    shift 2
    curl -sk -b "$JAR" -X "$method" -H 'Content-Type: application/json' "$@" \
        "$MACHINA_URL/api/v1/platform/controller/api/v1$path"
}
names_sql() { printf "'%s'," "${VMS[@]}" | sed 's/,$//'; }

cleanup() {
    for v in "${VMS[@]}"; do
        sudo -n virsh destroy "$v" >/dev/null 2>&1
        sudo -n virsh managedsave-remove "$v" >/dev/null 2>&1
        sudo -n virsh undefine "$v" --nvram >/dev/null 2>&1 || sudo -n virsh undefine "$v" >/dev/null 2>&1
        sudo -n sh -c "rm -f $POOL/$v.qcow2 $POOL/$v-seed.iso $POOL/$v-seed.fk.iso $POOL/$v-*.rp-*.qcow2 $POOL/$v-*.rw-*.qcow2 $POOL/$v-*.fk-*.qcow2 $POOL/$v.*.mem"
    done
    sudo -n rm -f "$BASE"
    if [[ -f "$DB" ]]; then
        local n; n=$(names_sql)
        sudo -n sqlite3 "$DB" "DELETE FROM vm_restore_points WHERE vm_id IN (SELECT id FROM vms WHERE name IN ($n));
            DELETE FROM vm_forks WHERE fork_vm_id IN (SELECT id FROM vms WHERE name IN ($n)) OR source_vm_id IN (SELECT id FROM vms WHERE name IN ($n));
            DELETE FROM vms WHERE name IN ($n);" 2>/dev/null
    fi
    rm -rf "$W"
    echo "cleanup: VMs, disk layers, controller rows and base image removed"
}
trap cleanup EXIT

echo "== VM =="
for v in "${VMS[@]}"; do
    sudo -n virsh dominfo "$v" >/dev/null 2>&1 && { echo "$v already exists; refusing to touch it" >&2; trap - EXIT; exit 1; }
done
ssh-keygen -q -t ed25519 -N '' -f "$W/key"
sudo -n curl -fsSL -o "$BASE" "$IMG_URL" || { bad "download $IMG_URL"; exit 1; }
cat > "$W/user" <<EOF
#cloud-config
hostname: tt-src
users:
  - name: np
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    ssh_authorized_keys: ["$(cat "$W/key.pub")"]
runcmd:
  - [sh, -c, "mkdir -p /srv && apt-get update -qq && apt-get install -y -qq qemu-guest-agent && systemctl start qemu-guest-agent"]
EOF
printf 'instance-id: tt-src-%s\nlocal-hostname: tt-src\n' "$$" > "$W/meta"
sudo -n cloud-localds "$POOL/tt-src-seed.iso" "$W/user" "$W/meta"
sudo -n qemu-img create -q -f qcow2 -F qcow2 -b "$BASE" "$POOL/tt-src.qcow2" 8G
sudo -n virt-install --name tt-src --memory 1024 --vcpus 1 --import --osinfo detect=on,require=off \
    --disk "path=$POOL/tt-src.qcow2,format=qcow2,bus=virtio" --disk "path=$POOL/tt-src-seed.iso,device=cdrom" \
    --network network=default,model=virtio --channel unix,target.type=virtio,target.name=org.qemu.guest_agent.0 \
    --graphics none --noautoconsole >/dev/null 2>&1 \
    && ok "virt-install tt-src" || { bad "virt-install tt-src"; exit 1; }

ip_of() { sudo -n virsh domifaddr "$1" --source lease 2>/dev/null | awk '/ipv4/ {sub(/\/.*/, "", $4); print $4; exit}'; }
mac_of() { sudo -n virsh domiflist "$1" 2>/dev/null | awk 'NR > 2 && $5 {print $5; exit}'; }
net_of() { sudo -n virsh domiflist "$1" 2>/dev/null | awk 'NR > 2 && $3 {print $3; exit}'; }
vssh() { local h=$1; shift; ssh -i "$W/key" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=5 -o LogLevel=ERROR "np@$h" "$@"; }
for _ in $(seq 90); do SIP=$(ip_of tt-src); [[ -n "$SIP" ]] && break; sleep 2; done
[[ -n "${SIP:-}" ]] && ok "DHCP lease: tt-src $SIP" || { bad "DHCP lease"; exit 1; }
src() { vssh "$SIP" "$@"; }
gping() { sudo -n virsh qemu-agent-command "$1" '{"execute":"guest-ping"}' >/dev/null 2>&1; }
for _ in $(seq 150); do src true 2>/dev/null && gping tt-src && break; sleep 2; done
check "ssh and guest agent up in tt-src" bash -c "$(declare -f vssh src gping); W='$W' SIP='$SIP'; src true && gping tt-src"

# Runs a shell command in a guest through the guest agent; prints stdout.
gexec() {
    local pid out
    pid=$(sudo -n virsh qemu-agent-command "$1" "$(jq -nc --arg c "$2" '{execute: "guest-exec", arguments: {path: "/bin/sh", arg: ["-c", $c], "capture-output": true}}')" 2>/dev/null | jq -r .return.pid) || return 1
    [[ "$pid" =~ ^[0-9]+$ ]] || return 1
    for _ in $(seq 40); do
        out=$(sudo -n virsh qemu-agent-command "$1" "{\"execute\":\"guest-exec-status\",\"arguments\":{\"pid\":$pid}}" 2>/dev/null)
        if [[ "$(jq -r .return.exited <<<"$out")" == true ]]; then
            jq -r '.return["out-data"] // "" | @base64d' <<<"$out"
            return "$(jq -r '.return.exitcode // 1' <<<"$out")"
        fi
        sleep 0.5
    done
    return 1
}

echo "== controller =="
tt_json() { NP_JSON=1 "$M" vm restore-points "$1" 2>/dev/null; }
for _ in $(seq 60); do tt_json tt-src | jq -e .vm_id >/dev/null && break; sleep 3; done
check "controller inventory has tt-src" bash -c "NP_JSON=1 '$M' vm restore-points tt-src | jq -e .vm_id"

echo "== restore point =="
src "echo alpha | sudo tee /srv/a >/dev/null && sync"
check "restore point (before b)" "$M" vm restore-point tt-src --note "before b" --wait
check "one point, filesystems quiesced through the guest agent" bash -c "NP_JSON=1 '$M' vm restore-points tt-src | jq -e '(.points | length) == 1 and .points[0].quiesced and .points[0].note == \"before b\"'"
check "tt-src kept running" bash -c "[[ \$(sudo -n virsh domstate tt-src) == running ]]"
check "tt-src writes to a new overlay on the frozen disk" bash -c "sudo -n virsh domblklist tt-src | grep -q 'tt-src-vda.rp-'"
src "echo bravo | sudo tee /srv/b >/dev/null && sync"

echo "== live fork =="
check "fork tt-src -> tt-fork while it runs" "$M" vm fork tt-src tt-fork --wait
check "tt-src still running" bash -c "[[ \$(sudo -n virsh domstate tt-src) == running ]]"
check "tt-src still has b" bash -c "$(declare -f vssh src); W='$W' SIP='$SIP'; [[ \$(src cat /srv/b) == bravo ]]"
check "tt-fork running" bash -c "[[ \$(sudo -n virsh domstate tt-fork) == running ]]"
SMAC=$(mac_of tt-src); FMAC=$(mac_of tt-fork)
[[ -n "$FMAC" && "$FMAC" != "$SMAC" ]] && ok "tt-fork has its own MAC ($FMAC)" || bad "tt-fork has its own MAC ($FMAC vs $SMAC)"
for _ in $(seq 90); do FIP=$(ip_of tt-fork); [[ -n "$FIP" ]] && break; sleep 2; done
[[ -n "${FIP:-}" && "$FIP" != "$SIP" ]] && ok "tt-fork has its own address ($FIP)" || bad "tt-fork has its own address (${FIP:-none})"
fk() { vssh "$FIP" "$@"; }
for _ in $(seq 60); do fk true 2>/dev/null && break; sleep 2; done
check "ssh into tt-fork with the source's key" fk true
check "tt-fork has a and b (forked now)" bash -c "$(declare -f vssh fk); W='$W' FIP='$FIP'; [[ \$(fk cat /srv/a /srv/b | tr '\n' ' ') == 'alpha bravo ' ]]"
for _ in $(seq 30); do [[ "$(fk hostname 2>/dev/null)" == tt-fork ]] && break; sleep 2; done
check "tt-fork renamed itself (new cloud-init identity)" bash -c "$(declare -f vssh fk); W='$W' FIP='$FIP'; [[ \$(fk hostname) == tt-fork ]]"
check "tt-fork has a fresh machine-id" bash -c "$(declare -f vssh src fk); W='$W' SIP='$SIP' FIP='$FIP'; [[ \$(src cat /etc/machine-id) != \$(fk cat /etc/machine-id) ]]"
src "echo charlie | sudo tee /srv/c >/dev/null && sync"
fk "echo delta | sudo tee /srv/d >/dev/null && sync"
check "writes after the fork stay apart" bash -c "$(declare -f vssh src fk); W='$W' SIP='$SIP' FIP='$FIP'; ! fk test -e /srv/c && ! src test -e /srv/d"
check "controller records tt-fork as a fork of tt-src" bash -c "NP_JSON=1 '$M' vm restore-points tt-src | jq -e '(.forks | map(.name) | index(\"tt-fork\")) and (.points | length) == 2 and .points[1].kind == \"fork\"'"
check "tt-fork knows its source" bash -c "NP_JSON=1 '$M' vm restore-points tt-fork | jq -e '.fork_of.name == \"tt-src\"'"

echo "== rewind =="
refused() { ! "$M" vm rewind tt-src 1 2>"$W/err" && grep -q "depend on later restore points" "$W/err"; }
check "rewinding past the point tt-fork sits on is refused" refused
check "detach tt-fork" "$M" vm fork-detach tt-fork --wait
FDISK=$(sudo -n virsh domblklist tt-fork | awk '$1 == "vda" {print $2}')
check "tt-fork disk no longer has a backing file" bash -c "! sudo -n qemu-img info -U '$FDISK' | grep -q '^backing file:'"
check "tt-fork still has its data after detach" bash -c "$(declare -f vssh fk); W='$W' FIP='$FIP'; [[ \$(fk cat /srv/d) == delta ]]"
check "rewind tt-src to the first point" "$M" vm rewind tt-src 1 --wait
for _ in $(seq 60); do src true 2>/dev/null && break; sleep 2; done
check "tt-src back up after the rewind" src true
check "a survives the rewind" bash -c "$(declare -f vssh src); W='$W' SIP='$SIP'; [[ \$(src cat /srv/a) == alpha ]]"
check "b and c (written after the point) are gone" bash -c "$(declare -f vssh src); W='$W' SIP='$SIP'; ! src test -e /srv/b && ! src test -e /srv/c"
check "later restore points dropped" bash -c "NP_JSON=1 '$M' vm restore-points tt-src | jq -e '(.points | length) == 1'"
check "tt-fork unaffected by the source's rewind" bash -c "$(declare -f vssh fk); W='$W' FIP='$FIP'; [[ \$(fk cat /srv/b) == bravo ]]"

echo "== memory fork =="
for _ in $(seq 60); do gping tt-src && break; sleep 2; done
src "echo 'in memory only' | sudo tee /dev/shm/marker >/dev/null"
check "memory fork tt-src -> tt-mem" "$M" vm fork tt-src tt-mem --memory --wait
check "tt-mem running" bash -c "[[ \$(sudo -n virsh domstate tt-mem) == running ]]"
check "tt-mem is on the isolated network" bash -c "[[ \$(sudo -n virsh domiflist tt-mem | awk 'NR > 2 && \$3 {print \$3; exit}') == machina-fork ]]"
[[ "$(mac_of tt-mem)" == "$SMAC" ]] && ok "tt-mem kept the source's MAC (its RAM still uses it)" || bad "tt-mem kept the source's MAC"
check "tt-src still running and reachable" src true
check "tt-mem has the tmpfs file (RAM carried across)" bash -c "$(declare -f gexec); [[ \$(gexec tt-mem 'cat /dev/shm/marker') == 'in memory only' ]]"
check "tt-mem cannot reach the libvirt network" bash -c "$(declare -f gexec); ! gexec tt-mem 'ping -c1 -W2 192.168.122.1'"
check "controller lists tt-mem as a memory fork" bash -c "NP_JSON=1 '$M' vm restore-points tt-src | jq -e '.forks | map(select(.name == \"tt-mem\" and .memory and .isolated)) | length == 1'"

echo "== pruning =="
MEM_ID=$(tt_json tt-mem | jq -r .vm_id)
task_done() {
    local id=$1
    for _ in $(seq 120); do
        case "$(capi GET "/tasks/$id" | jq -r .status)" in completed) return 0 ;; failed) return 1 ;; esac
        sleep 1
    done
    return 1
}
del=$(capi POST "/vms/$MEM_ID/delete" -d '{"confirmed":true}' | jq -r .task_id)
check "delete tt-mem through the controller" task_done "$del"
check "keep 2 restore points, every 5 min" bash -c "'$M' vm restore-points tt-src --every 5 --keep 2 | grep -q 'every 5 min, keep 2'"
chain_len() { sudo -n qemu-img info -U --backing-chain "$(sudo -n virsh domblklist tt-src | awk '$1 == "vda" {print $2}')" | grep -c '^image:'; }
BEFORE=$(chain_len)
check "restore point 3" "$M" vm restore-point tt-src --wait
check "restore point 4" "$M" vm restore-point tt-src --wait
AFTER=$(chain_len)
check "oldest points merged away: 2 left" bash -c "NP_JSON=1 '$M' vm restore-points tt-src | jq -e '(.points | length) == 2'"
[[ "$AFTER" -le "$BEFORE" ]] && ok "backing chain did not grow ($BEFORE -> $AFTER images)" || bad "backing chain grew ($BEFORE -> $AFTER images)"
check "tt-src still running after the merges" bash -c "[[ \$(sudo -n virsh domstate tt-src) == running ]]"
check "data intact after the merges" bash -c "$(declare -f vssh src); W='$W' SIP='$SIP'; [[ \$(src cat /srv/a) == alpha ]]"
src "echo echo | sudo tee /srv/e >/dev/null && sync"
check "rewind to the newest point" "$M" vm rewind tt-src -1 --wait
for _ in $(seq 60); do src true 2>/dev/null && break; sleep 2; done
check "e (written after it) is gone, a is still there" bash -c "$(declare -f vssh src); W='$W' SIP='$SIP'; ! src test -e /srv/e && [[ \$(src cat /srv/a) == alpha ]]"

echo
echo "passed=$P failed=$F"
[[ "$F" == 0 ]]
