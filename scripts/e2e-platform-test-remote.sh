#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Run Machina platform E2E tests against a remote deploy from your laptop.
#
# Usage:
#   ./scripts/e2e-platform-test-remote.sh USER HOST
#   ./scripts/e2e-platform-test-remote.sh sus 212.8.252.194
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

USER="${1:?usage: $0 USER HOST [e2e-platform flags...]}"
HOST="${2:?usage: $0 USER HOST [e2e-platform flags...]}"
shift 2

BASE="http://${HOST}:5093"

exec "${SCRIPT_DIR}/e2e-platform-test.sh" "$BASE" "$@"
