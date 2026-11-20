#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

#
# Stacks you describe, against real libvirt VMs, driven through the
# controller: draft from a sentence, plan (quota, placement, cost), propose
# and approve a stack.deploy, check the VMs, labels and policy, delete a VM
# and the policy behind the stack's back and watch drift report and converge
# put them back, scale a group up with an update, then undo the deploy.
# The VMs have blank disks; nothing boots an OS.
# Creates stk-rv-* VMs and removes them, their disks and controller rows on exit.
#
#   printf '%s\n' "$PASS" | bash scripts/fleet/stack-realvm.sh
#
# The machina password is read from stdin (MACHINA_USER defaults to $USER).
set -uo pipefail

read -r MACHINA_PASS
MACHINA_USER="${MACHINA_USER:-$USER}"
MACHINA_URL="${MACHINA_URL:-https://127.0.0.1:5092}"
POOL=/var/lib/libvirt/images
DB="${CONTROLLER_DB:-/var/lib/machina/controller.db}"
NAME=stk-rv
W="$(mktemp -d /tmp/stk-realvm.XXXXXX)"
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
domstate() { sudo -n virsh domstate "$1" 2>/dev/null | head -1; }
stack_vms() { sudo -n virsh list --all --name 2>/dev/null | grep "^$NAME-" | sort; }
stack_id() { capi GET /stacks | jq -r --arg n "$NAME" '.[] | select(.name == $n) | .id' | head -1; }
stack_status() { capi GET "/stacks/$1" | jq -r .status; }
wait_status() {
    local id=$1 want=$2 s
    for _ in $(seq 100); do
        s=$(stack_status "$id")
        [[ "$s" == "$want" ]] && return 0
        [[ "$s" == failed || "$s" == rolled_back ]] && return 1
        sleep 3
    done
    return 1
}
wait_task() {
    local id=$1
    for _ in $(seq 100); do
        case "$(capi GET "/tasks/$id" | jq -r .status)" in completed) return 0 ;; failed) return 1 ;; esac
        sleep 3
    done
    return 1
}

cleanup() {
    local id
    id=$(stack_id 2>/dev/null)
    [[ -n "$id" ]] && capi DELETE "/stacks/$id" >/dev/null 2>&1 && sleep 10
    for v in $(stack_vms); do
        sudo -n virsh destroy "$v" >/dev/null 2>&1
        sudo -n virsh undefine "$v" --nvram >/dev/null 2>&1 || sudo -n virsh undefine "$v" >/dev/null 2>&1
    done
    sudo -n sh -c "rm -f $POOL/$NAME-*"
    if [[ -f "$DB" ]]; then
        sudo -n sqlite3 "$DB" "DELETE FROM vms WHERE name LIKE '$NAME-%';
            DELETE FROM vm_network_policies WHERE name LIKE 'stack-$NAME-%';
            DELETE FROM backup_schedules WHERE name LIKE 'stack-$NAME-%';
            DELETE FROM stacks WHERE name = '$NAME';" 2>/dev/null
    fi
    rm -rf "$W"
    echo "cleanup: stack, VMs, disks and controller rows removed"
}

if [[ -n "$(stack_vms)" ]]; then
    echo "$NAME-* VMs already exist; refusing to touch them" >&2
    exit 1
fi
trap cleanup EXIT

(umask 077 && : >"$JAR")
code=$(MACHINA_USER="$MACHINA_USER" MACHINA_PASS="$MACHINA_PASS" jq -n '{username: env.MACHINA_USER, password: env.MACHINA_PASS}' |
    curl -sk -c "$JAR" -o /dev/null -w '%{http_code}' -X POST "$MACHINA_URL/api/v1/auth/login" \
        -H 'Content-Type: application/json' --data-binary @-)
[[ "$code" == 200 ]] && ok "login" || { bad "login (HTTP $code)"; exit 1; }
[[ -z "$(stack_id)" ]] || { bad "a stack named $NAME already exists"; trap - EXIT; exit 1; }

echo "== draft =="
capi POST /stacks/draft -d "{\"name\":\"$NAME\",\"prompt\":\"two web servers and a postgres database they reach on 5432, dev\"}" > "$W/draft"
check "draft has a web and a db group" jq -e '[.template.instances[].name] | index("web") and index("db")' "$W/draft"
check "draft says who wrote it ($(jq -r .source "$W/draft"))" jq -e '.source == "llm" or .source == "rules"' "$W/draft"
check "draft lets web reach db on 5432" jq -e 'any(.template.policies[]; .from == "web" and .to == "db" and (.ports | index(5432)))' "$W/draft"
check "draft comes with a plan" jq -e '.plan.vms | length >= 2' "$W/draft"

template() {
    jq -nc --argjson n "$1" '{
        instances: [
            {name: "web", count: $n, cpu_cores: 1, memory: "512Mi", anti_affinity: true},
            {name: "db", cpu_cores: 1, memory: "512Mi"}
        ],
        policies: [{from: "web", to: "db", ports: [5432]}]
    }'
}

echo "== plan =="
capi POST /stacks/plan -d "$(jq -nc --arg n "$NAME" --argjson t "$(template 2)" '{name: $n, template: $t}')" > "$W/plan"
check "plan has no errors" jq -e '.errors == []' "$W/plan"
check "plan passes quota and placement" jq -e '.quota.ok and .placement.ok' "$W/plan"
check "plan has 3 VMs with names" jq -e '[.vms[].name] | sort == ["stk-rv-db","stk-rv-web-1","stk-rv-web-2"]' "$W/plan"
check "plan has a monthly cost" jq -e '.monthly_usd > 0' "$W/plan"
check "plan compiles the policy" jq -e '.policy_yaml | test("stack-stk-rv-db")' "$W/plan"
capi POST /stacks/plan -d "$(jq -nc --arg n "$NAME" --argjson t "$(template 0)" '{name: $n, template: $t}')" > "$W/bad"
check "a group of 0 VMs is an error" jq -e '.errors | length > 0' "$W/bad"
code=$(capi POST /stacks/propose -o "$W/blocked" -w '%{http_code}' -d "$(jq -nc --arg n "$NAME" --argjson t "$(template 0)" '{name: $n, template: $t}')")
[[ "$code" == 409 ]] && jq -e '.error_code == "stack_plan_blocked"' "$W/blocked" >/dev/null && ok "a blocked plan can't be proposed" || bad "a blocked plan can't be proposed (HTTP $code)"

echo "== approve and deploy =="
capi POST /stacks/propose -d "$(jq -nc --arg n "$NAME" --argjson t "$(template 2)" '{name: $n, template: $t, prompt: "realvm"}')" > "$W/action"
AID=$(jq -r .id "$W/action")
check "proposal queued as stack.deploy" jq -e '.action_type == "stack.deploy" and .status == "pending"' "$W/action"
[[ -z "$(stack_id)" ]] && ok "nothing deployed before approval" || bad "nothing deployed before approval"
capi POST "/ai/actions/$AID/execute" > "$W/exec"
check "approve and execute" jq -e '.error == null' "$W/exec"
SID=""
for _ in $(seq 20); do SID=$(stack_id); [[ -n "$SID" ]] && break; sleep 2; done
[[ -n "$SID" ]] && ok "stack row created ($SID)" || { bad "stack row created"; exit 1; }
wait_status "$SID" created && ok "stack created" || bad "stack created ($(capi GET "/stacks/$SID" | jq -c '{status, last_error}'))"
for v in stk-rv-web-1 stk-rv-web-2 stk-rv-db; do
    check "$v running" bash -c "[[ \"\$(sudo -n virsh domstate $v 2>/dev/null | head -1)\" == running ]]"
done
labelled=$(sudo -n sqlite3 "$DB" "SELECT count(*) FROM vms WHERE name LIKE 'stk-rv-%' AND labels LIKE '%\"machina.io/stack\":\"stk-rv\"%' AND labels LIKE '%\"machina.io/stack-group\":%'")
[[ "$labelled" == 3 ]] && ok "VMs carry the stack and group labels" || bad "VMs carry the stack and group labels ($labelled of 3)"
check "policy stack-stk-rv-db exists" bash -c "curl -sk -b '$JAR' '$MACHINA_URL/api/v1/platform/controller/api/v1/vm-network-policies/stack-stk-rv-db' | jq -e '.name == \"stack-stk-rv-db\" or .policy.name == \"stack-stk-rv-db\"'"
capi GET "/stacks/$SID/drift" > "$W/drift"
check "drift: in sync" jq -e '.in_sync == true and .open == 0' "$W/drift"

echo "== drift and converge =="
capi PUT "/stacks/$SID/auto-heal" -d '{"enabled":false}' >/dev/null
WEB2=$(capi GET "/stacks/$SID" | jq -r '[.. | objects | select(.kind? == "instance" and .name? == "stk-rv-web-2") | .id][0]')
t=$(capi POST "/vms/$WEB2/delete" -d '{"confirmed":true}' | jq -r .task_id)
wait_task "$t" && ok "deleted stk-rv-web-2 behind the stack's back" || bad "delete stk-rv-web-2"
capi DELETE /vm-network-policies/stack-stk-rv-db >/dev/null
capi GET "/stacks/$SID/drift" > "$W/drift"
check "drift reports the missing VM" jq -e 'any(.items[]; .name == "stk-rv-web-2" and (.detail | test("missing")) and (.fixed | not))' "$W/drift"
check "drift reports the missing policy" jq -e 'any(.items[]; .name == "stack-stk-rv-db" and (.fixed | not))' "$W/drift"
check "drift: 2 open" jq -e '.in_sync == false and .open == 2' "$W/drift"
capi POST "/stacks/$SID/converge" > "$W/conv"
check "converge recreated the VM and the policy" jq -e '([.items[] | select(.fixed)] | length) == 2 and .open == 0' "$W/conv"
check "stk-rv-web-2 running again" bash -c "[[ \"\$(sudo -n virsh domstate stk-rv-web-2 2>/dev/null | head -1)\" == running ]]"
check "policy back" bash -c "curl -sk -b '$JAR' -o /dev/null -w '%{http_code}' '$MACHINA_URL/api/v1/platform/controller/api/v1/vm-network-policies/stack-stk-rv-db' | grep -q 200"
capi GET "/stacks/$SID/drift" > "$W/drift"
check "drift: in sync after converge" jq -e '.in_sync == true' "$W/drift"
capi PUT "/stacks/$SID/auto-heal" -d '{"enabled":true}' >/dev/null

echo "== update =="
capi POST /stacks/plan -d "$(jq -nc --arg n "$NAME" --arg s "$SID" --argjson t "$(template 3)" '{name: $n, template: $t, stack_id: $s}')" > "$W/uplan"
check "update plan creates stk-rv-web-3 only" jq -e '.diff.create == ["stk-rv-web-3"] and .diff.delete == []' "$W/uplan"
code=$(capi PUT "/stacks/$SID" -o "$W/upd" -w '%{http_code}' -d "$(jq -nc --argjson t "$(template 3)" '{template: $t}')")
[[ "$code" == 200 || "$code" == 202 ]] && ok "update accepted" || bad "update accepted (HTTP $code: $(head -c 300 "$W/upd"))"
wait_status "$SID" created && ok "update finished" || bad "update finished ($(capi GET "/stacks/$SID" | jq -c '{status, last_error}'))"
check "stk-rv-web-3 running" bash -c "[[ \"\$(sudo -n virsh domstate stk-rv-web-3 2>/dev/null | head -1)\" == running ]]"
check "4 stack VMs" bash -c "[[ \$(sudo -n virsh list --all --name | grep -c '^stk-rv-') == 4 ]]"
capi GET "/stacks/$SID/drift" > "$W/drift"
check "drift: in sync after update" jq -e '.in_sync == true' "$W/drift"

echo "== undo =="
capi POST "/ai/actions/$AID/undo" > "$W/undo"
gone=0
for _ in $(seq 60); do
    if [[ -z "$(stack_id)" && -z "$(stack_vms)" ]]; then gone=1; break; fi
    sleep 3
done
[[ $gone == 1 ]] && ok "undo deleted the stack and its VMs" || bad "undo deleted the stack and its VMs ($(head -c 300 "$W/undo"))"
check "undo removed the policy" bash -c "[[ \$(curl -sk -b '$JAR' -o /dev/null -w '%{http_code}' '$MACHINA_URL/api/v1/platform/controller/api/v1/vm-network-policies/stack-stk-rv-db') == 404 ]]"

echo "passed=$P failed=$F"
[[ $F == 0 ]]
