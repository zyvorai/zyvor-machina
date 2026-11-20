#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# UX-critical platform flow on a live host (API paths used by Create VM wizard + guest tabs).
# Usage: ./scripts/e2e-platform-ux-flow-remote.sh USER HOST
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
USER="${1:?USER}"
HOST="${2:?HOST}"
BASE="http://${HOST}:5093"
export E2E_PLATFORM_BASE="${BASE%/}"
export E2E_PLATFORM_USER="${E2E_PLATFORM_USER:-admin}"
export E2E_PLATFORM_PASS="${E2E_PLATFORM_PASS:-admin}"
# When set on the controller (see contrib/machina-platform.default), curl sends X-Machina-E2E.
export MACHINA_E2E_BYPASS_SECRET="${MACHINA_E2E_BYPASS_SECRET:-}"

# shellcheck source=lib/e2e-platform-common.sh
source "${SCRIPT_DIR}/lib/e2e-platform-common.sh"

PASS=0
FAIL=0
ok() { echo "  ✅ $*"; PASS=$((PASS + 1)); }
bad() { echo "  ❌ $*"; FAIL=$((FAIL + 1)); }

echo "=== UX flow smoke @ ${E2E_PLATFORM_BASE} ==="

http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/health")"
[[ "$http" == "200" ]] && ok "health" || bad "health HTTP $http"

e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/templates/seed" >/dev/null || true

r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/templates/marketplace")"
marketplace_has() {
  echo "$r" | python3 -c "import json,sys; names={t.get('name','') for t in json.load(sys.stdin)}; sys.exit(0 if sys.argv[1] in names else 1)" "$1" 2>/dev/null
}
marketplace_has 'fedora-44' && ok "marketplace has fedora-44" || bad "missing fedora-44"
marketplace_has 'ubuntu-25.10' && ok "marketplace has ubuntu-25.10" || bad "missing ubuntu-25.10"
marketplace_has 'ubuntu-26.04' && ok "marketplace has ubuntu-26.04" || bad "missing ubuntu-26.04"
marketplace_has 'windows-server-2025' && ok "marketplace has windows-server-2025" || bad "missing windows-server-2025"
marketplace_has 'fedora-40' && bad "stale fedora-40 still listed" || ok "marketplace retired fedora-40"

http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/templates/ubuntu-24.04/1.0.0/readiness")"
[[ "$http" == "200" ]] && ok "template readiness" || bad "readiness HTTP $http"

r="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/networks")"
echo "$r" | grep -q 'default' && ok "networks include default" || bad "no default network in API"

vm_name="ux-flow-$$"
r="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/from-template" \
  -H "Content-Type: application/json" \
  -d "{\"template_ref\":\"ubuntu-24.04@1.0.0\",\"name\":\"${vm_name}\",\"memory\":\"1Gi\",\"template_vars\":{\"hostname\":\"${vm_name}\",\"name\":\"${vm_name}\"},\"cloud_init_user\":\"ubuntu\"}")"
tid="$(echo "$r" | python3 -c "import json,sys; print(json.load(sys.stdin).get('task_id',''))" 2>/dev/null || true)"
[[ -n "$tid" ]] && ok "wizard-equivalent create queued $tid" || { bad "create failed: $r"; exit 1; }

deadline=$((SECONDS + 300))
st=""
while (( SECONDS < deadline )); do
  t="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/tasks/${tid}")"
  st="$(echo "$t" | python3 -c "import json,sys; print(json.load(sys.stdin).get('status',''))" 2>/dev/null || true)"
  [[ "$st" == "completed" ]] && { ok "vm.apply completed"; break; }
  [[ "$st" == "failed" ]] && { bad "vm.apply failed: $t"; break; }
  sleep 5
done
[[ "$st" == "completed" ]] || { [[ -n "$st" ]] && bad "task stuck: $st"; }

vms="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/vms")"
vm_id="$(echo "$vms" | python3 -c "
import json,sys
for v in json.load(sys.stdin):
    if v.get('name')=='${vm_name}':
        print(v.get('id',''))
        break
" 2>/dev/null || true)"
[[ -n "$vm_id" ]] && ok "VM listed $vm_id" || bad "VM not in list"

if [[ -n "$vm_id" ]]; then
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/guest/health")"
  [[ "$http" == "200" ]] && ok "guest health endpoint" || bad "guest health HTTP $http"
  http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/domain-xml")"
  [[ "$http" == "200" ]] && ok "domain-xml endpoint" || bad "domain-xml HTTP $http"

  del="$(e2e_platform_curl -X POST "${E2E_PLATFORM_BASE}/api/v1/vms/${vm_id}/delete" \
    -H "Content-Type: application/json" -d '{"confirmed":true}')"
  del_tid="$(echo "$del" | python3 -c "import json,sys; print(json.load(sys.stdin).get('task_id',''))" 2>/dev/null || true)"
  [[ -n "$del_tid" ]] && ok "vm.delete queued $del_tid" || bad "delete failed: $del"

  if [[ -n "$del_tid" ]]; then
    deadline=$((SECONDS + 180))
    del_st=""
    while (( SECONDS < deadline )); do
      t="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/tasks/${del_tid}")"
      del_st="$(echo "$t" | python3 -c "import json,sys; print(json.load(sys.stdin).get('status',''))" 2>/dev/null || true)"
      [[ "$del_st" == "completed" ]] && { ok "vm.delete completed"; break; }
      [[ "$del_st" == "failed" ]] && { bad "vm.delete failed: $t"; break; }
      sleep 3
    done
    [[ "$del_st" == "completed" ]] || { [[ -n "$del_st" ]] && bad "delete task stuck: $del_st"; }
  fi

  vms_after="$(e2e_platform_curl "${E2E_PLATFORM_BASE}/api/v1/vms")"
  if echo "$vms_after" | python3 -c "
import json,sys
vms=json.load(sys.stdin)
sys.exit(0 if not any(v.get('id')=='${vm_id}' for v in vms) else 1)
" 2>/dev/null; then
    ok "VM removed from list"
  else
    bad "VM still listed after delete"
  fi
fi

http="$(e2e_platform_http_code "${E2E_PLATFORM_BASE}/api/v1/guestkit/status")"
[[ "$http" == "200" ]] && ok "guestkit status endpoint" || bad "guestkit status HTTP $http"

echo ""
echo "UX flow: ${PASS} passed, ${FAIL} failed"
[[ "$FAIL" -eq 0 ]]
