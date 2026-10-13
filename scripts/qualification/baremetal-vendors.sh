#!/usr/bin/env bash
set -euo pipefail
: "${MACHINA_URL:?set MACHINA_URL}"
: "${MACHINA_TOKEN:?set MACHINA_TOKEN}"
: "${METAL_SERVER_ID:?set METAL_SERVER_ID}"
vendor="${BMC_VENDOR:-unknown}"
case "$vendor" in dell-idrac|hpe-ilo|supermicro|lenovo-xcc|openbmc) ;; *) echo "unsupported BMC_VENDOR=$vendor" >&2; exit 2;; esac
api() { curl -fsS -H "Authorization: Bearer $MACHINA_TOKEN" -H 'Content-Type: application/json' "$@"; }
api "$MACHINA_URL/api/v1/baremetal/servers" > /tmp/metal-servers.json
api "$MACHINA_URL/api/v1/baremetal/servers/$METAL_SERVER_ID/power-state" > /tmp/metal-power-before.json
api -X POST -d '{"action":"off","dry_run":false}' "$MACHINA_URL/api/v1/baremetal/servers/$METAL_SERVER_ID/power" > /tmp/metal-off.json
api -X POST -d '{"action":"on","dry_run":false}' "$MACHINA_URL/api/v1/baremetal/servers/$METAL_SERVER_ID/power" > /tmp/metal-on.json
api -X POST "$MACHINA_URL/api/v1/baremetal/servers/$METAL_SERVER_ID/bmc-info/refresh" > /tmp/metal-refresh.json
api "$MACHINA_URL/api/v1/baremetal/servers/$METAL_SERVER_ID/bmc-info" > /tmp/metal-bmc-info.json
echo "PASS: physical BMC smoke completed for $vendor; archive /tmp/metal-*.json as claims evidence"
