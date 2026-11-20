#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Send Machina deploy / live UX report email (best-effort).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

HOST="${1:-unknown}"
shift || true

python3 "${ROOT}/scripts/send-deploy-report.py" --host "${HOST}" "$@" || {
  echo "warn: deploy report email not sent (SMTP missing or send failed)" >&2
}
