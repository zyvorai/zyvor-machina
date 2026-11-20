#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Launch a dedicated Chrome with remote debugging for page-sweep.js
set -euo pipefail
PORT="${MACHINA_CDP_PORT:-9222}"
DATA="${MACHINA_CHROME_PROFILE:-/tmp/machina-chrome-regression}"
CHROME="${MACHINA_CHROME_BIN:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"

if [[ ! -x "$CHROME" ]]; then
  CHROME="$(command -v google-chrome || command -v chromium || true)"
fi
if [[ -z "${CHROME:-}" || ! -x "$CHROME" ]]; then
  echo "Chrome/Chromium not found. Set MACHINA_CHROME_BIN." >&2
  exit 1
fi

if curl -sf "http://127.0.0.1:${PORT}/json/version" >/dev/null 2>&1; then
  echo "CDP already listening on :${PORT}"
  exit 0
fi

mkdir -p "$DATA"
exec "$CHROME" \
  --user-data-dir="$DATA" \
  --remote-debugging-port="$PORT" \
  --ignore-certificate-errors \
  --no-first-run \
  --no-default-browser-check \
  --disable-background-networking \
  about:blank
