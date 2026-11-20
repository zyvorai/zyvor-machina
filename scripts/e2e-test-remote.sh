#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Run Machina E2E tests against a remote deploy from your laptop.
#
# Usage:
#   ./scripts/e2e-test-remote.sh USER HOST
#   VSPASS=max ./scripts/e2e-test-remote.sh sus 185.165.240.5
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"

USER="${1:?usage: $0 USER HOST [e2e-test flags...]}"
HOST="${2:?usage: $0 USER HOST [e2e-test flags...]}"
shift 2

BASE="https://${HOST}:5092"
PASS="${VSPASS:-${SSHPASS:-}}"

if [[ -z "$PASS" ]]; then
  read -rsp "Password for ${USER}@${BASE}: " PASS
  echo
fi

# Password is passed via VSPASS (env), not as a positional arg — CLI args are
# world-readable via `ps` on shared hosts, env vars set this way are not.
exec env VSPASS="$PASS" E2E_SSH_HOST="$HOST" \
  E2E_AUTH_MODE="${E2E_AUTH_MODE:-auto}" \
  E2E_LDAP_USER="${E2E_LDAP_USER:-}" \
  E2E_LDAP_PASS="${E2E_LDAP_PASS:-}" \
  "${SCRIPT_DIR}/e2e-test.sh" "$BASE" "$USER" --ssh-host "$HOST" "$@"
