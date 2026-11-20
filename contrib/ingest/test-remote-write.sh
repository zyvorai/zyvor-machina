#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Smoke-test POST /api/v1/metrics/ingest/remote-write (Snappy prometheus.WriteRequest).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
API_URL="${MACHINA_URL:-https://127.0.0.1:5092}"
TOKEN="${MACHINA_TOKEN:-}"

TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT
cargo run -q -p machina-core --example remote_write_smoke > "$TMP"

AUTH=()
[[ -n "$TOKEN" ]] && AUTH=(-H "Authorization: Bearer $TOKEN")

curl -sk "${AUTH[@]}" \
  -X POST \
  -H "Content-Type: application/x-protobuf" \
  --data-binary "@${TMP}" \
  "$API_URL/api/v1/metrics/ingest/remote-write" | jq .
