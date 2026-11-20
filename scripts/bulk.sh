#!/bin/bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# machina bulk — batch operations on all VMs
# Usage:
#   ./scripts/bulk.sh start          # Start all stopped VMs
#   ./scripts/bulk.sh stop           # Force stop all running VMs
#   ./scripts/bulk.sh shutdown       # Graceful shutdown all running VMs
#   ./scripts/bulk.sh start vm1 vm2  # Start specific VMs
#   ./scripts/bulk.sh snapshot       # Snapshot all running VMs
#   ./scripts/bulk.sh snapshot-clean # Delete all snapshots named 'auto-*'
set -eo pipefail

API="${MACHINA_API:-https://localhost:5092/api/v1}"

info()  { echo "ℹ️  $*"; }
ok()    { echo "✅ $*"; }
warn()  { echo "⚠️  $*"; }
fail()  { echo "❌ $*"; }

# API auth: every daemon route below /health requires a bearer token (session
# cookie or Bearer header) — see daemon/src/auth.rs auth_middleware. Without a
# credential every call here 401s and every action silently "fails" for every
# VM (see scripts/backup.sh for the same requirement on the read side). There
# is no dedicated service token for bulk (write) operations, so this only
# honors an explicitly supplied token.
API_TOKEN="${MACHINA_API_TOKEN:-}"
# The header is fed to curl as a config read from its own stdin (`-K -`)
# rather than `-H ...` on the command line — see scripts/backup.sh's mcurl
# for why (argv is readable by any local user via `ps`/proc).
bcurl() {
    if [ -n "$API_TOKEN" ]; then
        printf 'header = "Authorization: Bearer %s"\n' "$API_TOKEN" | curl -sk -K - "$@"
    else
        curl -sk "$@"
    fi
}

usage() {
    echo "Usage: $0 <action> [vm1 vm2 ...]"
    echo ""
    echo "Actions:"
    echo "  start          Start all stopped VMs (or specific VMs)"
    echo "  stop           Force stop all running VMs (or specific VMs)"
    echo "  shutdown       Graceful shutdown all running VMs (or specific VMs)"
    echo "  pause          Pause all running VMs"
    echo "  resume         Resume all paused VMs"
    echo "  reboot         Reboot all running VMs"
    echo "  snapshot       Create timestamped snapshot of all running VMs"
    echo "  snapshot-clean Delete all snapshots matching 'auto-*'"
    echo "  status         Quick status of all VMs"
    echo ""
    echo "Environment:"
    echo "  MACHINA_API        API URL (default: https://localhost:5092/api/v1)"
    echo "  MACHINA_API_TOKEN  Bearer token (required unless MACHINA_SKIP_AUTH=1 on the daemon;"
    echo "                     every /vms endpoint requires auth, see auth_middleware)"
    exit 1
}

[ $# -eq 0 ] && usage

ACTION="$1"
shift

# Check daemon
bcurl -f "$API/health" > /dev/null 2>&1 || { fail "Daemon not reachable at $API"; exit 1; }

# Get VM list
get_vms() {
    bcurl -f "$API/vms" 2>/dev/null
}

get_vm_names_by_state() {
    local state="$1"
    get_vms | python3 -c "import json,sys; state=sys.argv[1]; [print(v['name']) for v in json.load(sys.stdin) if v['state']==state]" "$state" 2>/dev/null
}

# If specific VMs given, use those; otherwise use state-based selection
resolve_targets() {
    local default_state="$1"
    if [ $# -gt 1 ]; then
        shift
        echo "$@" | tr ' ' '\n'
    else
        get_vm_names_by_state "$default_state"
    fi
}

do_action() {
    local action="$1" verb="$2" endpoint="$3"
    shift 3
    local targets
    targets=$(resolve_targets "$@")

    if [ -z "$targets" ]; then
        info "No VMs to $verb"
        return
    fi

    echo "📋 ${verb^} VMs"

    while read -r name; do
        [ -z "$name" ] && continue
        result=$(bcurl -X POST "$API/vms/$name/$endpoint" 2>/dev/null)
        if echo "$result" | grep -qF "status"; then
            ok "  $name"
        else
            fail "  $name: $result"
        fi
    done <<< "$targets"
}

DATE=$(date +%Y%m%d-%H%M%S)

case "$ACTION" in
    start)
        targets=$(resolve_targets "shutoff" "$@")
        if [ -z "$targets" ]; then
            info "No stopped VMs to start"
            exit 0
        fi
        echo "📋 Starting VMs"
        while read -r name; do
            [ -z "$name" ] && continue
            result=$(bcurl -X POST "$API/vms/$name/start" 2>/dev/null)
            if echo "$result" | grep -qF "started"; then
                ok "  $name"
            else
                fail "  $name: $(echo "$result" | python3 -c "import json,sys; print(json.load(sys.stdin).get('error','unknown'))" 2>/dev/null || echo "$result")"
            fi
        done <<< "$targets"
        ;;

    stop)
        targets=$(resolve_targets "running" "$@")
        if [ -z "$targets" ]; then
            info "No running VMs to stop"
            exit 0
        fi
        echo "⛔ Force stopping VMs"
        while read -r name; do
            [ -z "$name" ] && continue
            result=$(bcurl -X POST "$API/vms/$name/stop" 2>/dev/null)
            if echo "$result" | grep -qF "status"; then
                ok "  $name"
            else
                fail "  $name"
            fi
        done <<< "$targets"
        ;;

    shutdown)
        targets=$(resolve_targets "running" "$@")
        if [ -z "$targets" ]; then
            info "No running VMs to shutdown"
            exit 0
        fi
        echo "📋 Shutting down VMs"
        while read -r name; do
            [ -z "$name" ] && continue
            result=$(bcurl -X POST "$API/vms/$name/shutdown" 2>/dev/null)
            if echo "$result" | grep -qF "status"; then
                ok "  $name"
            else
                fail "  $name"
            fi
        done <<< "$targets"
        ;;

    pause)
        targets=$(resolve_targets "running" "$@")
        if [ -z "$targets" ]; then
            info "No running VMs to pause"
            exit 0
        fi
        echo "📋 Pausing VMs"
        while read -r name; do
            [ -z "$name" ] && continue
            result=$(bcurl -X POST "$API/vms/$name/pause" 2>/dev/null)
            if echo "$result" | grep -qF "status"; then
                ok "  $name"
            else
                fail "  $name"
            fi
        done <<< "$targets"
        ;;

    resume)
        targets=$(resolve_targets "paused" "$@")
        if [ -z "$targets" ]; then
            info "No paused VMs to resume"
            exit 0
        fi
        echo "📋 Resuming VMs"
        while read -r name; do
            [ -z "$name" ] && continue
            result=$(bcurl -X POST "$API/vms/$name/resume" 2>/dev/null)
            if echo "$result" | grep -qF "status"; then
                ok "  $name"
            else
                fail "  $name"
            fi
        done <<< "$targets"
        ;;

    reboot)
        targets=$(resolve_targets "running" "$@")
        if [ -z "$targets" ]; then
            info "No running VMs to reboot"
            exit 0
        fi
        echo "📋 Rebooting VMs"
        while read -r name; do
            [ -z "$name" ] && continue
            result=$(bcurl -X POST "$API/vms/$name/reboot" 2>/dev/null)
            if echo "$result" | grep -qF "status"; then
                ok "  $name"
            else
                fail "  $name"
            fi
        done <<< "$targets"
        ;;

    snapshot)
        targets=$(get_vm_names_by_state "running")
        if [ -z "$targets" ]; then
            info "No running VMs to snapshot"
            exit 0
        fi
        SNAP_NAME="auto-$DATE"
        echo "📋 Creating snapshots ($SNAP_NAME)"
        while read -r name; do
            [ -z "$name" ] && continue
            result=$(bcurl -X POST "$API/vms/$name/snapshots" \
                -H 'Content-Type: application/json' \
                -d "{\"name\": \"$SNAP_NAME\", \"description\": \"Auto backup $DATE\"}" 2>/dev/null)
            if echo "$result" | grep -qF "created"; then
                ok "  $name -> $SNAP_NAME"
            else
                fail "  $name: $(echo "$result" | python3 -c "import json,sys; print(json.load(sys.stdin).get('error','unknown'))" 2>/dev/null || echo "$result")"
            fi
        done <<< "$targets"
        ;;

    snapshot-clean)
        SNAPS=$(bcurl -f "$API/snapshots" 2>/dev/null)
        if [ -z "$SNAPS" ] || [ "$SNAPS" = "[]" ]; then
            info "No snapshots found"
            exit 0
        fi
        echo "📋 Cleaning auto-* snapshots"
        echo "$SNAPS" | python3 -c "
import json, sys
for s in json.load(sys.stdin):
    if s['name'].startswith('auto-'):
        print(f'{s[\"vm_name\"]} {s[\"name\"]}')
" 2>/dev/null | while read -r vm snap; do
            [ -z "$vm" ] && continue
            result=$(bcurl -X DELETE "$API/vms/$vm/snapshots/$snap" 2>/dev/null)
            if echo "$result" | grep -qF "deleted"; then
                ok "  $vm/$snap"
            else
                fail "  $vm/$snap"
            fi
        done
        ;;

    status)
        get_vms | python3 -c "
import json, sys
vms = json.load(sys.stdin)
running = [v for v in vms if v['state'] == 'running']
stopped = [v for v in vms if v['state'] == 'shutoff']
other = [v for v in vms if v['state'] not in ('running', 'shutoff')]
print(f'Total: {len(vms)}  Running: {len(running)}  Stopped: {len(stopped)}  Other: {len(other)}')
print()
for v in vms:
    state = v['state']
    icon = '🟢' if state == 'running' else '🔴' if state == 'shutoff' else '🟡'
    print(f'  {icon} {state:12s} {v[\"name\"]:40s} {v[\"vcpus\"]:>2d} vCPU  {v[\"memory_mb\"]:>5d} MB')
" 2>/dev/null
        ;;

    *)
        fail "Unknown action: $ACTION"
        usage
        ;;
esac
