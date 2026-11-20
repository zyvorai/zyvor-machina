#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Generate PDFs from docs/client-presentations/*.html (hyper2kvm slide-deck format).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
WEB="${ROOT}/web"

if [[ ! -d "${WEB}/node_modules/playwright" ]]; then
  echo "  › Installing web deps (playwright)…"
  (cd "${WEB}" && npm ci --no-audit --no-fund)
fi

if ! (cd "${WEB}" && node scripts/run-playwright.mjs install chromium >/dev/null 2>&1); then
  (cd "${WEB}" && npx playwright install chromium)
fi

echo "  › Machina client presentation PDFs"
node "${SCRIPT_DIR}/generate-client-presentation-pdfs.mjs" "$@"
