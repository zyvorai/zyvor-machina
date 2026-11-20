#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Post-deploy / manual sweep: sync inventory, remove stale e2e VMs, restart agent, start shutoff VMs.
set -euo pipefail

CONTROLLER="${MACHINA_CONTROLLER_URL:-http://127.0.0.1:5093}"
# MACHINA_PLATFORM_AUTH (daemon->controller proxy credential) is NOT the controller's
# own admin account, and "admin:admin" only authenticates when the controller was
# started with MACHINA_ALLOW_DEV_SECRETS=1. On a real install the bootstrap admin
# password is the random MACHINA_ADMIN_PASSWORD generated into /etc/default/machina-platform
# by install-platform.sh — without reading it, every curl_api call below 401s and the
# whole sweep (VM cleanup, sync, autostart) silently no-ops behind `|| warn ...`.
AUTH="${MACHINA_PLATFORM_AUTH:-}"
if [ -z "$AUTH" ] && [ -r /etc/default/machina-platform ]; then
  # shellcheck disable=SC1091
  . /etc/default/machina-platform
  [ -n "${MACHINA_ADMIN_PASSWORD:-}" ] && AUTH="${MACHINA_ADMIN_USER:-admin}:${MACHINA_ADMIN_PASSWORD}"
fi
AUTH="${AUTH:-admin:admin}"

info() { echo "ℹ️  $*"; }
ok() { echo "✅ $*"; }
warn() { echo "⚠️  $*"; }

curl_api() {
  curl -sf -u "$AUTH" -H 'Content-Type: application/json' "$@"
}

info "Platform sweep on $(hostname)"

systemctl enable machina-agent machina-controller 2>/dev/null || true
systemctl start libvirtd 2>/dev/null || true
systemctl restart machina-agent 2>/dev/null || warn "machina-agent restart failed"
sleep 2

curl_api -X POST "$CONTROLLER/api/v1/hosts/sync-all" -d '{}' >/dev/null || warn "sync-all failed"

# Remove libvirt e2e-* domains first (E2E leftovers).
while read -r dom; do
  [ -n "$dom" ] || continue
  [[ "$dom" == e2e-* ]] || [[ "$dom" == e2e_* ]] || [[ "$dom" == ux-e2e-* ]] || [[ "$dom" == ux-screenshot-* ]] || continue
  warn "Undefining libvirt domain: $dom"
  sudo virsh destroy "$dom" 2>/dev/null || true
  sudo virsh undefine "$dom" --nvram --managed-save --snapshots-metadata --remove-all-storage 2>/dev/null \
    || sudo virsh undefine "$dom" 2>/dev/null || true
done < <(sudo virsh list --all --name 2>/dev/null | grep -E '^(e2e[-_]|ux-e2e-|ux-screenshot-)' || true)

# Delete managed e2e-* platform VM records.
while read -r id name; do
  [ -n "$id" ] || continue
  warn "Removing e2e VM record: $name ($id)"
  curl_api -X POST "$CONTROLLER/api/v1/vms/${id}/delete" -d '{"confirmed":true}' >/dev/null 2>&1 || true
done < <(curl_api "$CONTROLLER/api/v1/vms" | python3 -c "
import sys, json
d = json.load(sys.stdin)
for v in d:
    n = v.get('name') or ''
    if n.startswith('e2e-') or n.startswith('e2e_') or n.startswith('ux-e2e-') or n.startswith('ux-screenshot-'):
        print(v['id'], n)
" 2>/dev/null || true)

sleep 5
curl_api -X POST "$CONTROLLER/api/v1/hosts/sync-all" -d '{}' >/dev/null 2>&1 || true
sleep 2

curl_api -X POST "$CONTROLLER/api/v1/vms/prune-missing" -d '{}' >/dev/null 2>&1 || true
curl_api -X POST "$CONTROLLER/api/v1/hosts/sync-all" -d '{}' >/dev/null || true

while read -r dom; do
  [ -n "$dom" ] || continue
  [[ "$dom" == e2e-* ]] || [[ "$dom" == e2e_* ]] || [[ "$dom" == ux-e2e-* ]] || [[ "$dom" == ux-screenshot-* ]] && continue
  st=$(sudo virsh domstate "$dom" 2>/dev/null || echo unknown)
  if [ "$st" = 'shut off' ]; then
    sudo virsh autostart "$dom" 2>/dev/null || true
    sudo virsh start "$dom" 2>/dev/null || true
  fi
done < <(sudo virsh list --all --name 2>/dev/null | grep -v '^$' || true)

sleep 2
curl_api -X POST "$CONTROLLER/api/v1/hosts/sync-all" -d '{}' >/dev/null || true

ok "Sweep complete"
curl_api "$CONTROLLER/api/v1/vms" | python3 -c "
import sys, json
d = json.load(sys.stdin)
print(f'  VMs: {len(d)}')
for v in d:
    err = (v.get('last_error') or '')[:40]
    print(f\"    {v.get('name')}: {v.get('observed_state')} / {v.get('lifecycle_phase')} {err}\")
" 2>/dev/null || true

curl_api "$CONTROLLER/api/v1/hosts" | python3 -c "
import sys, json
d = json.load(sys.stdin)
for h in d:
    print(f\"  Host {h.get('hostname')}: {h.get('state')}\")
" 2>/dev/null || true
