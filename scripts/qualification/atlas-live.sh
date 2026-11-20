#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

set -euo pipefail
: "${MACHINA_URL:?set MACHINA_URL}"
: "${MACHINA_TOKEN:?set MACHINA_TOKEN}"
: "${ATLAS_URL:?set ATLAS_URL}"
curlf() { curl -fsS --retry 3 --retry-delay 1 "$@"; }
auth=(-H "Authorization: Bearer $MACHINA_TOKEN")
curlf "$ATLAS_URL/health" >/tmp/atlas-health.json
curlf "${auth[@]}" "$MACHINA_URL/api/v1/atlas/status" >/tmp/machina-atlas-status.json
curlf "${auth[@]}" "$MACHINA_URL/api/v1/atlas/pools" >/tmp/machina-atlas-pools.json
grep -Eqi 'ok|healthy|ready' /tmp/atlas-health.json
grep -Eqi 'reachable[^a-z]*(true|1)|healthy|ready|connected' /tmp/machina-atlas-status.json
python3 - <<'PY'
import json
j=json.load(open('/tmp/machina-atlas-pools.json'))
assert isinstance(j,(list,dict))
PY
echo 'PASS: live Atlas gateway status + pool path answers through Machina'
