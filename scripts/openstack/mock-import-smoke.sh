#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

#
# Live smoke for OpenStack shim + importer + VM QoS via the daemon→controller proxy.
# Unit-level mock import: cargo test -p machina-controller openstack -- --nocapture
set -euo pipefail

BASE="${MACHINA_BASE_URL:-https://127.0.0.1:15092}"
USER="${MACHINA_USER:-sus}"
PASS="${MACHINA_PASS:-max}"
CTRL="${MACHINA_CONTROLLER_URL:-}" # optional direct :5093 (skips daemon cookie auth)
CURL=(curl -sk)
COOKIE_JAR="$(mktemp)"
trap 'rm -f "$COOKIE_JAR"' EXIT

if [[ -n "$CTRL" ]]; then
  echo "== Direct controller login =="
  ADMIN_USER="${MACHINA_ADMIN_USER:-admin}"
  ADMIN_PASS="${MACHINA_ADMIN_PASSWORD:?set MACHINA_ADMIN_PASSWORD for direct controller smoke}"
  TOKEN="$("${CURL[@]}" -X POST "${CTRL}/api/v1/auth/login" \
    -H 'Content-Type: application/json' \
    -d "{\"username\":\"${ADMIN_USER}\",\"password\":\"${ADMIN_PASS}\"}" \
    | python3 -c 'import sys,json; print(json.load(sys.stdin).get("token",""))')"
  [[ -n "$TOKEN" ]] || { echo "controller login failed"; exit 1; }
  AUTH=(-H "Authorization: Bearer ${TOKEN}")
  PROXY="$CTRL"
else
  echo "== Daemon PAM login (session cookie) =="
  "${CURL[@]}" -c "$COOKIE_JAR" -X POST "${BASE}/api/v1/auth/login" \
    -H 'Content-Type: application/json' \
    -d "{\"username\":\"${USER}\",\"password\":\"${PASS}\"}" \
    | python3 -c 'import sys,json; d=json.load(sys.stdin); assert d.get("status")=="ok", d; print("logged in as", d.get("username"))'
  AUTH=(-b "$COOKIE_JAR")
  PROXY="${BASE}/api/v1/platform/controller"
fi

echo "== OpenStack API discovery =="
"${CURL[@]}" "${AUTH[@]}" "${PROXY}/v3" | head -c 400
echo

echo "== OpenStack connections list =="
"${CURL[@]}" "${AUTH[@]}" "${PROXY}/api/v1/openstack/connections"
echo

echo "== VM QoS policies list =="
"${CURL[@]}" "${AUTH[@]}" "${PROXY}/api/v1/vm-qos/policies"
echo

echo "OK (live endpoints reachable). Full mock import: cargo test -p machina-controller openstack_import::tests::import_image_via_mock -- --nocapture"
