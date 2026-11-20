#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# e2e-platform-test.sh — Machina platform control plane E2E (controller :5093)
#
# Usage:
#   ./scripts/e2e-platform-test.sh [BASE_URL]
#   ./scripts/e2e-platform-test.sh http://212.8.252.194:5093
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

E2E_PASS=0
E2E_FAIL=0
E2E_PLATFORM_BASE="${1:-http://127.0.0.1:5093}"
E2E_PLATFORM_BASE="${E2E_PLATFORM_BASE%/}"

# shellcheck source=lib/e2e-platform-common.sh
source "${SCRIPT_DIR}/lib/e2e-platform-common.sh"
# shellcheck source=lib/e2e-platform-smoke.sh
source "${SCRIPT_DIR}/lib/e2e-platform-smoke.sh"
# shellcheck source=lib/e2e-platform.sh
source "${SCRIPT_DIR}/lib/e2e-platform.sh"

echo "══════════════════════════════════════════"
echo "  Machina Platform E2E"
echo "  Base: ${E2E_PLATFORM_BASE}"
echo "══════════════════════════════════════════"

e2e_platform_smoke_run || true
e2e_platform_run || true

if e2e_platform_summary; then
  exit 0
fi
exit 1
