#!/bin/bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# machina demo — exercises the REST API to demonstrate all features
# Usage: ./scripts/demo.sh [API_URL]
set -eo pipefail

_script_dir="$(cd "$(dirname "$0")" && pwd)"
for _pkg_ui in "${_script_dir}/../.package-lib/package-ui.sh" "${_script_dir}/lib/package-ui.sh"; do
  if [[ -f "${_pkg_ui}" ]]; then
    # shellcheck source=/dev/null
    source "${_pkg_ui}"
    break
  fi
done

_default_api="https://localhost:5092/api/v1"
if [[ $# -eq 0 ]] && declare -F pkg_access_url >/dev/null 2>&1; then
  _default_api="$(pkg_access_url https 5092)/api/v1"
fi
API="${1:-${_default_api}}"

step_n=0
step() {
    step_n=$((step_n + 1))
    echo ""
    echo "📌 [$step_n] $*"
    echo "$(printf '%.0s-' {1..60})"
}

api() {
    local method="$1" path="$2"
    shift 2
    echo "  📤 $method ${path}"
    local result
    if [ "$method" = "GET" ]; then
        result=$(curl -sk "${API}${path}")
    elif [ "$method" = "DELETE" ]; then
        result=$(curl -sk -X DELETE "${API}${path}")
    else
        result=$(curl -sk -X "$method" "${API}${path}" -H 'Content-Type: application/json' "$@")
    fi
    echo "$result" | python3 -m json.tool 2>/dev/null || echo "$result" | head -20
    sleep 1
}

DEMO_VM="demo-vm-$$"
DEMO_FAIL=0

echo ""
echo "🚀 machina API Demo"
echo "  🔗 API: $API"
echo "  🖥️  Demo VM: $DEMO_VM"
echo ""

# ── Health & Node ──────────────────────────────────────────────────────

step "Health Check"
api GET /health

step "Host Information"
api GET /node

step "VM Templates"
api GET /templates

# ── VM Lifecycle ───────────────────────────────────────────────────────

step "List Current VMs"
api GET /vms

step "Create a Demo VM"
api POST /vms -d "{\"name\": \"${DEMO_VM}\", \"vcpus\": 1, \"memory_mb\": 512, \"disk_gb\": 5, \"network\": \"default\"}"

step "Get VM Details"
api GET /vms/${DEMO_VM}

step "Start VM"
api POST /vms/${DEMO_VM}/start
sleep 2

step "Get VM Metrics"
api GET /metrics/${DEMO_VM}

step "Console Info"
api GET /vms/console-info/${DEMO_VM}

step "Get Boot Configuration"
api GET /vms/${DEMO_VM}/boot

step "Check Managed Save Status"
api GET /vms/${DEMO_VM}/managed-save/status

step "Enable Autostart"
api POST /vms/${DEMO_VM}/autostart/true

# ── Snapshots ──────────────────────────────────────────────────────────

step "Create Snapshot"
api POST /vms/${DEMO_VM}/snapshots -d '{"name": "demo-snap", "description": "Demo snapshot"}'

step "List All Snapshots"
api GET /snapshots

step "Revert to Snapshot"
api POST /vms/${DEMO_VM}/snapshots/demo-snap/revert

# ── Networks ───────────────────────────────────────────────────────────

step "List Networks"
api GET /networks

# ── Storage ────────────────────────────────────────────────────────────

step "List Storage Pools"
api GET /storage/pools

# ── Advanced: Infrastructure ──────────────────────────────────────────

step "Hypervisor Capabilities"
echo "  📤 GET /capabilities"
curl -sk "${API}/capabilities" | python3 -c "
import json,sys
d=json.load(sys.stdin)
print(f'  Host arch: {d.get(\"host_arch\",\"?\")}')
print(f'  CPU model: {d.get(\"host_cpu_model\",\"?\")}')
print(f'  Guest types: {len(d.get(\"guests\",[]))}')
" 2>/dev/null || echo "  (capabilities endpoint)"
sleep 1

step "System Info (SMBIOS)"
echo "  📤 GET /sysinfo"
curl -sk "${API}/sysinfo" | head -10
echo "  ... (truncated)"
sleep 1

step "Node Devices"
echo "  📤 GET /devices"
curl -sk "${API}/devices" | python3 -c "
import json,sys
devs=json.load(sys.stdin)
types={}
for d in devs:
    t=d.get('capability_type','?')
    types[t]=types.get(t,0)+1
print(f'  Total: {len(devs)} devices')
for t,c in sorted(types.items()):
    print(f'    {t}: {c}')
" 2>/dev/null || echo "  (devices endpoint)"
sleep 1

step "Node Devices (filtered: net only)"
echo "  📤 GET /devices?capability=net"
curl -sk "${API}/devices?capability=net" | python3 -c "
import json,sys
devs=json.load(sys.stdin)
print(f'  Net devices: {len(devs)}')
for d in devs[:5]:
    print(f'    {d[\"name\"]}')
" 2>/dev/null || echo "  (filtered devices)"
sleep 1

step "Network Filters"
echo "  📤 GET /nwfilters"
curl -sk "${API}/nwfilters" | python3 -c "
import json,sys
f=json.load(sys.stdin)
print(f'  {len(f)} network filters')
for x in f[:5]:
    print(f'    {x[\"name\"]}')
if len(f)>5: print(f'    ... and {len(f)-5} more')
" 2>/dev/null || echo "  (nwfilters endpoint)"
sleep 1

step "Secrets"
api GET /secrets

# ── Security Validation ───────────────────────────────────────────────

step "Security: Migration URI Validation"
echo "  Testing SSRF prevention..."
echo "  📤 POST /vms/${DEMO_VM}/migrate with http://evil.com"
result=$(curl -sk -X POST "${API}/vms/${DEMO_VM}/migrate" \
    -H 'Content-Type: application/json' \
    -d '{"dest_uri":"http://evil.com","live":false}')
if echo "$result" | grep -qF "Invalid migration URI"; then
    echo "  ✅ BLOCKED: $result"
else
    echo "  ❌ NOT BLOCKED: $result"
    DEMO_FAIL=$((DEMO_FAIL + 1))
fi
sleep 1

step "Security: Volume Resize Validation"
echo "  Testing negative capacity prevention..."
echo "  📤 POST /storage/pools/default/volumes/x/resize with -1"
result=$(curl -sk -X POST "${API}/storage/pools/default/volumes/x/resize" \
    -H 'Content-Type: application/json' \
    -d '{"capacity_gb":-1}')
if echo "$result" | grep -qF "capacity_gb must be"; then
    echo "  ✅ BLOCKED: $result"
else
    echo "  ❌ NOT BLOCKED: $result"
    DEMO_FAIL=$((DEMO_FAIL + 1))
fi
sleep 1

# ── Prometheus ─────────────────────────────────────────────────────────

step "Prometheus Metrics"
echo "  📤 GET /prometheus"
curl -sk "${API}/prometheus" | head -15
echo "  ... (truncated)"
sleep 1

# ── Cleanup ────────────────────────────────────────────────────────────

step "Shutdown VM"
api POST /vms/${DEMO_VM}/shutdown
sleep 3

step "Delete Snapshot"
api DELETE /vms/${DEMO_VM}/snapshots/demo-snap

step "Delete Demo VM"
api DELETE /vms/${DEMO_VM}
rm -f /var/lib/libvirt/images/${DEMO_VM}.qcow2 2>/dev/null || true

step "Final VM List"
api GET /vms

echo ""
if [ "$DEMO_FAIL" -eq 0 ]; then
  echo "✅ Demo complete!"
else
  echo "❌ Demo complete with ${DEMO_FAIL} security validation(s) NOT BLOCKED (see above)"
fi
echo ""
if declare -F pkg_access_url >/dev/null 2>&1; then
  echo "  Web UI:  $(pkg_access_url https 5092) ($(pkg_primary_host_label))"
else
  echo "  Web UI:  https://localhost:5092"
fi
echo "  🔗 API:     ${API}/health"
echo ""
[ "$DEMO_FAIL" -eq 0 ]
