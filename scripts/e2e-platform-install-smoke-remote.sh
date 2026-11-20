#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Run platform install smoke on a remote host via SSH.
#
# Usage:
#   ./scripts/e2e-platform-install-smoke-remote.sh USER HOST
#   ./scripts/e2e-platform-install-smoke-remote.sh sus 212.8.252.194
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

USER="${1:?usage: $0 USER HOST}"
HOST="${2:?usage: $0 USER HOST}"

REMOTE="${USER}@${HOST}"
SSH_OPTS=(-o ConnectTimeout=15 -o BatchMode=yes -o StrictHostKeyChecking=accept-new)

echo "Platform install smoke → ${REMOTE}"
ssh "${SSH_OPTS[@]}" "$REMOTE" 'bash -s' < "${SCRIPT_DIR}/e2e-platform-install-smoke.sh"
