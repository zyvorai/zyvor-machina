#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Run Machina full E2E against a remote deploy from your laptop.
#
# Usage:
#   ./scripts/e2e-full-test-remote.sh USER HOST
#   VSPASS=max ./scripts/e2e-full-test-remote.sh sus 212.8.252.194
#   ./scripts/e2e-full-test-remote.sh sus 212.8.252.194 --platform-only --auth ldap
#
# Env: E2E_AUTH_MODE, E2E_LDAP_USER, E2E_LDAP_PASS (see e2e-full-test.sh)
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

USER="${1:?usage: $0 USER HOST [e2e-full-test flags...]}"
HOST="${2:?usage: $0 USER HOST [e2e-full-test flags...]}"
shift 2

BASE="https://${HOST}:5092"
PASS="${VSPASS:-${SSHPASS:-}}"

if [[ -z "$PASS" ]]; then
  read -rsp "Password for ${USER}@${BASE}: " PASS
  echo
fi

# Password is passed via VSPASS (env), not as a positional arg — CLI args are
# world-readable via `ps` on shared hosts, env vars set this way are not.
exec env VSPASS="$PASS" E2E_SSH_HOST="$HOST" E2E_INSTALL_REMOTE=1 \
  E2E_AUTH_MODE="${E2E_AUTH_MODE:-auto}" \
  E2E_LDAP_USER="${E2E_LDAP_USER:-}" \
  E2E_LDAP_PASS="${E2E_LDAP_PASS:-}" \
  "${SCRIPT_DIR}/e2e-full-test.sh" "$BASE" "$USER" "$@"
