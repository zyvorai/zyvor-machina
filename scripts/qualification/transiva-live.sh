#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

set -euo pipefail
: "${MACHINA_URL:?set MACHINA_URL}"
: "${MACHINA_TOKEN:?set MACHINA_TOKEN}"
api() { curl -fsS -H "Authorization: Bearer $MACHINA_TOKEN" "$@"; }
echo '[1/2] Transiva health through Machina'
api "$MACHINA_URL/api/v1/transiva/status" | tee /tmp/machina-transiva-status.json
echo '[2/2] provider discovery through Machina'
api "$MACHINA_URL/api/v1/transiva/providers" | tee /tmp/machina-transiva-providers.json
echo 'PASS: Machina -> Transiva gateway is reachable and authenticated'
