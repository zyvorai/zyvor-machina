#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

#
# Autopilot capacity against real libvirt VMs, driven through the controller:
# a cloud VPC + subnet, a Debian launch template, and an instance group behind
# a native load balancer. Scale in and watch the member leave the load
# balancer, drain, then sleep (managed save); scale out and watch it wake and
# rejoin. Seed hourly history and check a rightsizing resize through an
# approval action, its verification and undo; seed a daily rush and check
# predictive scaling raises the group ahead of it.
# Creates ap-rv cloud resources and asg-* VMs and removes them on exit.
# The subnet is an isolated libvirt network on a private /24; nothing is
# bridged to the host uplink.
#
#   printf '%s\n' "$PASS" | bash scripts/fleet/autopilot-realvm.sh
#
# The machina password is read from stdin (MACHINA_USER defaults to $USER).
set -uo pipefail

read -r MACHINA_PASS
MACHINA_USER="${MACHINA_USER:-$USER}"
MACHINA_URL="${MACHINA_URL:-https://127.0.0.1:5092}"
POOL=/var/lib/libvirt/images
DB="${CONTROLLER_DB:-/var/lib/machina/controller.db}"
NAME=ap-rv
CIDR="${AP_RV_CIDR:-10.231.0.0/16}"
SUBNET_CIDR="${AP_RV_SUBNET:-10.231.1.0/24}"
IMAGE="${AP_RV_IMAGE:-debian-13}"
LB_PORT="${AP_RV_LB_PORT:-18431}"
W="$(mktemp -d /tmp/ap-realvm.XXXXXX)"
JAR="$W/jar"
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
db() { sudo -n sqlite3 "$DB" "$@"; }
hex() { tr -d '-' <<<"$1"; }
saved() { echo "$2" > "$W/$1"; }
get() { cat "$W/$1" 2>/dev/null; }
member() { capi GET "/cloud/instance-groups/$GID" | jq -r --argjson s "$1" ".members[] | select(.slot == \$s) | .$2 // empty"; }
vm_field() { capi GET "/vms/$1" | jq -r ".$2 // empty"; }
lb_enabled() { capi GET "/load-balancers/$LB/members" | jq -r --arg v "$1" '[.[] | select(.vm_id == $v)][0].enabled // "absent"'; }
domstate() { sudo -n virsh domstate "$1" 2>/dev/null | head -1; }
managed_save() { sudo -n virsh dominfo "$1" 2>/dev/null | awk -F: '/Managed save/ {gsub(/ /, "", $2); print $2}'; }
policy() { capi GET "/cloud/instance-groups/$GID" | jq -c '.group.policy_json | fromjson'; }
patch_policy() {
    capi PATCH "/cloud/instance-groups/$GID" -o "$W/patch" -w '%{http_code}' \
        -d "$(jq -nc --argjson p "$1" '{policy: $p, paused: false}')"
}
wait_for() {
    local tries=$1; shift
    for _ in $(seq "$tries"); do "$@" >/dev/null 2>&1 && return 0; sleep 5; done
    return 1
}

cleanup() {
    local gid sid lb gh
    gid=$(get gid); sid=$(get sid); lb=$(get lb)
    if [[ -n "$gid" ]]; then
        capi PATCH "/cloud/instance-groups/$gid" -d "$(jq -nc --argjson p "$(get policy0)" '{policy: $p, paused: true}')" >/dev/null 2>&1
        gh=$(hex "$gid")
        for v in $(sudo -n virsh list --all --name 2>/dev/null | grep "^asg-$gh-"); do
            sudo -n virsh destroy "$v" >/dev/null 2>&1
            sudo -n virsh undefine "$v" --managed-save --nvram >/dev/null 2>&1 \
                || sudo -n virsh undefine "$v" --managed-save >/dev/null 2>&1
            sudo -n sh -c "rm -f $POOL/$v.qcow2 $POOL/$v-*.iso $POOL/$v-cloud-init*"
        done
        db "DELETE FROM lb_members WHERE vm_id IN (SELECT vm_id FROM cloud_group_members WHERE group_id = X'$gh');
            DELETE FROM ai_actions WHERE action_type = 'vm.resize' AND json_extract(object_ref, '\$.name') LIKE 'asg-$gh-%';
            DELETE FROM metric_hourly WHERE subject = 'group:$gh' OR subject IN (SELECT vm_id FROM cloud_group_members WHERE group_id = X'$gh');
            DELETE FROM metric_samples WHERE subject = 'group:$gh';
            DELETE FROM cloud_group_members WHERE group_id = X'$gh';
            DELETE FROM vms WHERE name LIKE 'asg-$gh-%';
            DELETE FROM cloud_instance_groups WHERE id = X'$gh';" 2>/dev/null
    fi
    [[ -n "$lb" ]] && capi DELETE "/load-balancers/$lb" >/dev/null 2>&1
    db "DELETE FROM cloud_launch_templates WHERE name = '$NAME';" 2>/dev/null
    if [[ -n "$sid" ]]; then
        sudo -n virsh net-destroy "mc-$sid" >/dev/null 2>&1
        sudo -n virsh net-undefine "mc-$sid" >/dev/null 2>&1
        db "DELETE FROM cloud_ip_allocations WHERE subnet_id = X'$(hex "$sid")';
            DELETE FROM cloud_subnets WHERE id = X'$(hex "$sid")';
            DELETE FROM networks WHERE name = 'mc-$sid';" 2>/dev/null
    fi
    db "DELETE FROM cloud_vpcs WHERE name = '$NAME';" 2>/dev/null
    rm -rf "$W"
    echo "cleanup: group, VMs, disks, load balancer, subnet network and controller rows removed"
    echo
    echo "passed=$P failed=$F"
}

if [[ -n "$(db "SELECT 1 FROM cloud_vpcs WHERE name = '$NAME' UNION SELECT 1 FROM cloud_launch_templates WHERE name = '$NAME'" 2>/dev/null)" ]]; then
    echo "$NAME cloud resources already exist; refusing to touch them" >&2
    exit 1
fi
trap cleanup EXIT

(umask 077 && : >"$JAR")
code=$(MACHINA_USER="$MACHINA_USER" MACHINA_PASS="$MACHINA_PASS" jq -n '{username: env.MACHINA_USER, password: env.MACHINA_PASS}' |
    curl -sk -c "$JAR" -o /dev/null -w '%{http_code}' -X POST "$MACHINA_URL/api/v1/auth/login" \
        -H 'Content-Type: application/json' --data-binary @-)
[[ "$code" == 200 ]] && ok "login" || { bad "login (HTTP $code)"; exit 1; }

PROJECT=$(capi GET /project-registry | jq -r '[.[] | select(.enabled)] | (map(select(.name == "default")) + .)[0].id // empty')
HOST=$(capi GET /hosts | jq -r '[.[] | select(.state == "online")][0].id // empty')
[[ -n "$PROJECT" && -n "$HOST" ]] && ok "project and host" || { bad "project and host ($PROJECT / $HOST)"; exit 1; }

echo "== VPC, subnet, template, load balancer =="
VPC=$(capi POST "/cloud/projects/$PROJECT/vpcs" -d "$(jq -nc --arg n "$NAME" --arg c "$CIDR" --arg h "$HOST" '{name: $n, cidr: $c, host_id: $h}')" | jq -r '.id // empty')
[[ -n "$VPC" ]] && ok "VPC created" || { bad "VPC created"; exit 1; }
SID=$(capi POST "/cloud/vpcs/$VPC/subnets" -d "$(jq -nc --arg n "$NAME" --arg c "$SUBNET_CIDR" '{name: $n, cidr: $c}')" | jq -r '.id // empty')
saved sid "$SID"
subnet_ready() { [[ "$(capi GET "/cloud/vpcs/$VPC/subnets" | jq -r --arg s "$SID" '.[] | select(.id == $s) | .status')" == ready ]]; }
wait_for 36 subnet_ready && ok "subnet ready" || { bad "subnet ready ($(capi GET "/cloud/vpcs/$VPC/subnets" | jq -c '.[0] | {status, last_error}'))"; exit 1; }
check "subnet is an isolated libvirt network" bash -c "sudo -n virsh net-dumpxml mc-$SID | grep -q '<ip ' && ! sudo -n virsh net-dumpxml mc-$SID | grep -q '<forward'"

ssh-keygen -q -t ed25519 -N '' -f "$W/key" >/dev/null
TID=$(capi POST "/cloud/projects/$PROJECT/launch-templates" -d "$(jq -nc --arg n "$NAME" --arg img "$IMAGE" --arg k "$(cat "$W/key.pub")" '{
    name: $n,
    vm: {api_version: "virt.zyvor.dev/v1", kind: "VirtualMachine", metadata: {name: "base"},
         spec: {cpu: {sockets: 1, cores: 2}, memory: "1Gi", template_ref: $img,
                storage: [{name: "root", size: "10Gi"}], cloud_init: {user: "debian", ssh_pubkey: $k}}}
}')" | jq -r '.id // empty')
[[ -n "$TID" ]] && ok "launch template saved" || { bad "launch template saved"; exit 1; }

LB=$(capi POST /load-balancers -d "$(jq -nc --arg n "$NAME" --arg h "$HOST" --arg p "$PROJECT" --argjson port "$LB_PORT" '{name: $n, host_id: $h, listener_port: $port, protocol: "tcp", project_id: $p}')" | jq -r '.id // empty')
saved lb "$LB"
[[ -n "$LB" ]] && ok "load balancer created" || { bad "load balancer created"; exit 1; }

echo "== group behind the load balancer =="
POLICY=$(jq -nc --arg lb "$LB" '{min: 0, max: 3, desired: 2, target_cpu: null, cooldown_secs: 30,
    scale_in: "sleep", load_balancer: {id: $lb, port: 80}, drain_secs: 20}')
saved policy0 "$POLICY"
GID=$(capi POST "/cloud/projects/$PROJECT/instance-groups" -d "$(jq -nc --arg n "$NAME" --arg t "$TID" --arg s "$SID" --argjson p "$(jq -c '.desired = 1' <<<"$POLICY")" '{name: $n, template_id: $t, subnet_id: $s, policy: $p}')" | jq -r '.id // empty')
saved gid "$GID"
[[ -n "$GID" ]] && ok "group created" || { bad "group created"; exit 1; }
code=$(capi POST "/cloud/projects/$PROJECT/instance-groups" -o "$W/badlb" -w '%{http_code}' \
    -d "$(jq -nc --arg t "$TID" --arg s "$SID" --argjson p "$(jq -c '.load_balancer.id = "00000000-0000-0000-0000-000000000001"' <<<"$POLICY")" '{name: "ap-rv-bad", template_id: $t, subnet_id: $s, policy: $p}')")
[[ "$code" == 4* ]] && ok "an unknown load balancer is refused (HTTP $code)" || bad "an unknown load balancer is refused (HTTP $code)"

up() {
    local v
    for s in $(seq 0 $(($1 - 1))); do
        v=$(member "$s" vm_id); [[ -n "$v" ]] || return 1
        [[ "$(vm_field "$v" observed_state)" == running && -n "$(vm_field "$v" guest_ip)" ]] || return 1
    done
}
echo "waiting for the first instance to boot (the $IMAGE image downloads on first use)..."
wait_for 240 up 1 && ok "first instance running with an address" || { bad "first instance running with an address ($(capi GET "/cloud/instance-groups/$GID" | jq -c '[.members[] | {slot, observed_state}]'); $(capi GET "/cloud/instance-groups/$GID" | jq -r .group.last_error))"; exit 1; }
code=$(patch_policy "$POLICY")
[[ "$code" == 200 ]] && ok "scale out 1 -> 2" || bad "scale out 1 -> 2 (HTTP $code $(cat "$W/patch"))"
wait_for 120 up 2 && ok "2 instances running with addresses" || { bad "2 instances running with addresses ($(capi GET "/cloud/instance-groups/$GID" | jq -c '[.members[] | {slot, observed_state}]'); $(capi GET "/cloud/instance-groups/$GID" | jq -r .group.last_error))"; exit 1; }
V0=$(member 0 vm_id); V1=$(member 1 vm_id)
N1=$(member 1 name)
IP1=$(vm_field "$V1" guest_ip)
case "$IP1" in 10.231.1.*) ok "member address from the subnet ($IP1)" ;; *) bad "member address from the subnet ($IP1)" ;; esac
both_in() { [[ "$(lb_enabled "$V0")" == true && "$(lb_enabled "$V1")" == true ]]; }
wait_for 24 both_in && ok "both members joined the load balancer" || bad "both members joined the load balancer ($(lb_enabled "$V0") $(lb_enabled "$V1"))"
check "load balancer rules carry the member" bash -c "sudo -n iptables-save 2>/dev/null | grep -q '$IP1:80'"

echo "== scale in: drain, then sleep =="
code=$(patch_policy "$(jq -c '.desired = 1' <<<"$POLICY")")
[[ "$code" == 200 ]] && ok "desired 2 -> 1" || bad "desired 2 -> 1 (HTTP $code $(cat "$W/patch"))"
draining() { [[ -n "$(member 1 draining_since)" ]]; }
wait_for 18 draining && ok "slot 1 is draining" || bad "slot 1 is draining"
check "slot 1 left the load balancer while draining" bash -c "[[ \"\$(curl -sk -b '$JAR' '$MACHINA_URL/api/v1/platform/controller/api/v1/load-balancers/$LB/members' | jq -r --arg v '$V1' '[.[] | select(.vm_id == \$v)][0].enabled')\" == false ]]"
[[ "$(domstate "$N1")" == running ]] && ok "still running during the drain" || bad "still running during the drain ($(domstate "$N1"))"
asleep() { [[ "$(domstate "$N1")" == "shut off" && "$(managed_save "$N1")" == yes ]]; }
wait_for 36 asleep && ok "slot 1 asleep (managed save, after the drain)" || bad "slot 1 asleep ($(domstate "$N1"), managed save $(managed_save "$N1"))"
check "controller records it as sleeping" bash -c "[[ \"\$(curl -sk -b '$JAR' '$MACHINA_URL/api/v1/platform/controller/api/v1/vms/$V1' | jq -r .desired_state)\" == sleeping ]]"
wait_for 12 bash -c "[[ -z \"\$(curl -sk -b '$JAR' '$MACHINA_URL/api/v1/platform/controller/api/v1/cloud/instance-groups/$GID' | jq -r '.members[] | select(.slot == 1) | .draining_since // empty')\" ]]" \
    && ok "drain mark cleared" || bad "drain mark cleared"
[[ "$(lb_enabled "$V0")" == true ]] && ok "slot 0 still serves" || bad "slot 0 still serves"
[[ "$(domstate "$(member 0 name)")" == running ]] && ok "slot 0 untouched" || bad "slot 0 untouched"

echo "== scale out: wake and rejoin =="
code=$(patch_policy "$POLICY")
[[ "$code" == 200 ]] && ok "desired 1 -> 2" || bad "desired 1 -> 2 (HTTP $code)"
awake() { [[ "$(domstate "$N1")" == running && "$(managed_save "$N1")" == no ]]; }
wait_for 36 awake && ok "slot 1 restored from its saved memory" || bad "slot 1 restored ($(domstate "$N1"), managed save $(managed_save "$N1"))"
rejoined() { [[ "$(lb_enabled "$V1")" == true ]]; }
wait_for 36 rejoined && ok "slot 1 rejoined the load balancer" || bad "slot 1 rejoined the load balancer ($(lb_enabled "$V1"))"
[[ "$(vm_field "$V1" guest_ip)" == "$IP1" ]] && ok "same address after waking" || bad "same address after waking ($(vm_field "$V1" guest_ip))"

echo "== rightsizing from history =="
now_h=$(( $(date +%s) / 3600 * 3600 ))
rows=""
for k in $(seq 72 160); do rows+="(X'$(hex "$V0")','cpu_percent',$((now_h - k * 3600)),6,10,12),(X'$(hex "$V0")','mem_ratio',$((now_h - k * 3600)),0.15,0.2,12),"; done
db "INSERT OR REPLACE INTO metric_hourly (subject, metric, hour, avg, max, n) VALUES ${rows%,};"
capi GET /rightsizing > "$W/recs"
check "recommendation for slot 0 (2 -> 1 vCPU, 1024 -> 512 MiB)" jq -e --arg v "$V0" '.recommendations[] | select(.vm_id == $v) | .suggested_vcpus == 1 and .suggested_memory_mib == 512 and .hours >= 72' "$W/recs"
capi POST /rightsizing/propose -d "$(jq -nc --arg v "$V0" '{vm_id: $v}')" > "$W/action"
AID=$(jq -r '.id // empty' "$W/action")
check "resize filed as an approval action" jq -e '.action_type == "vm.resize" and .status == "pending"' "$W/action"
code=$(capi POST /rightsizing/propose -o /dev/null -w '%{http_code}' -d "$(jq -nc --arg v "$V0" '{vm_id: $v}')")
[[ "$code" == 409 ]] && ok "a second proposal is refused while one waits" || bad "a second proposal is refused while one waits (HTTP $code)"
[[ "$(vm_field "$V0" vcpus)" == 2 ]] && ok "nothing resized before approval" || bad "nothing resized before approval"
capi POST "/ai/actions/$AID/execute" > "$W/exec"
check "approve and execute" jq -e '.error == null' "$W/exec"
N0=$(member 0 name)
config_small() { [[ "$(sudo -n virsh vcpucount "$N0" --config --current 2>/dev/null)" == 1 ]]; }
wait_for 24 config_small && ok "libvirt config has 1 vCPU" || bad "libvirt config has 1 vCPU"
mem_small() { [[ "$(vm_field "$V0" memory_mib)" == 512 ]]; }
wait_for 24 mem_small && ok "controller shows 512 MiB" || bad "controller shows 512 MiB ($(vm_field "$V0" memory_mib))"
capi POST "/ai/actions/$AID/verify" > "$W/verify"
if [[ "$(vm_field "$V0" vcpus)" == 1 ]]; then
    check "verification: ok (vCPU removed live)" jq -e '.status == "ok"' "$W/verify"
else
    check "verification: pending until restart (guest kept its vCPU live)" jq -e '.status == "pending" and (.detail | test("next restart"))' "$W/verify"
fi
check "no new suggestion right after the resize" bash -c "curl -sk -b '$JAR' '$MACHINA_URL/api/v1/platform/controller/api/v1/rightsizing' | jq -e --arg v '$V0' 'all(.recommendations[]; .vm_id != \$v)'"
capi POST "/ai/actions/$AID/undo" > "$W/undo"
check "undo accepted" jq -e '.error == null' "$W/undo"
restored() { [[ "$(vm_field "$V0" vcpus)" == 2 && "$(vm_field "$V0" memory_mib)" == 1024 ]]; }
wait_for 24 restored && ok "undo restores 2 vCPUs and 1024 MiB" || bad "undo restores 2 vCPUs and 1024 MiB ($(vm_field "$V0" vcpus) / $(vm_field "$V0" memory_mib))"

echo "== predictive scaling ahead of a daily rush =="
next_h=$(( now_h + 3600 ))
rows=""
for k in $(seq 1 192); do
    h=$(( next_h - k * 3600 ))
    if (( (next_h - h) % 86400 == 0 )); then v=400; else v=20; fi
    rows+="('group:$(hex "$GID")','cpu_sum',$h,$v,$v,12),"
done
db "INSERT OR REPLACE INTO metric_hourly (subject, metric, hour, avg, max, n) VALUES ${rows%,};"
code=$(patch_policy "$(jq -c '.target_cpu = 50 | .predictive = true' <<<"$POLICY")")
[[ "$code" == 200 ]] && ok "predictive on, 50% target" || bad "predictive on (HTTP $code $(cat "$W/patch"))"
capi GET "/cloud/instance-groups/$GID/forecast" > "$W/forecast"
check "forecast sees the rush next hour (daily pattern, 3 instances)" jq -e '.next_hour_peak.basis == "daily" and .next_hour_peak.needed == 3 and .next_hour_peak.demand > 200' "$W/forecast"
check "forecast covers the next 24 hours" jq -e '.forecast | length == 24' "$W/forecast"
prescaled() { [[ "$(policy | jq -r .desired)" == 3 ]]; }
wait_for 18 prescaled && ok "group raised to 3 ahead of the rush" || bad "group raised to 3 ahead of the rush ($(policy | jq -c '{desired, target_cpu, predictive}'))"
third() { [[ -n "$(member 2 vm_id)" ]]; }
wait_for 18 third && ok "slot 2 created for the rush" || bad "slot 2 created for the rush"

[[ $F -eq 0 ]]
