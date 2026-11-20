#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Run live UX → API wiring verification against a remote Machina host.
#
# Usage:
#   VSPASS=max ./scripts/e2e-live-ux-remote.sh sus 212.8.252.194
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
# shellcheck source=lib/e2e-auth.sh
source "${SCRIPT_DIR}/lib/e2e-auth.sh"

USER="${1:?usage: $0 USER HOST}"
HOST="${2:?usage: $0 USER HOST}"
PASS="${VSPASS:-${SSHPASS:-}}"

if [[ -z "$PASS" ]]; then
  read -rsp "Password for ${USER}@${HOST}: " PASS
  echo
fi

BASE="https://${HOST}:5092"

echo "══════════════════════════════════════════"
echo "  Live UX wiring verification"
echo "  Target: ${BASE}"
echo "══════════════════════════════════════════"

cd "${ROOT}/web"
if ! npm run playwright -- install chromium >/dev/null 2>&1; then
  npm run test:e2e:install
fi

set +e
export E2E_USER="$USER"
export E2E_PASSWORD="$PASS"
export E2E_AUTH_MODE="${E2E_AUTH_MODE:-auto}"
e2e_export_playwright_live_env "$BASE" "$USER" "$PASS"
  npm run test:e2e:live-ux
PW_EXIT=$?
set -e

echo "Report: ${ROOT}/docs/ux-wiring-live-report.json"

"${ROOT}/scripts/lib/send-deploy-report.sh" "${HOST}" \
  --overall "$( [[ $PW_EXIT -eq 0 ]] && echo PASS || echo FAIL )" || true

if [[ "${STRICT:-0}" == "1" && $PW_EXIT -ne 0 ]]; then
  echo "STRICT=1: live UX wiring failed" >&2
  exit "$PW_EXIT"
fi
exit "$PW_EXIT"
