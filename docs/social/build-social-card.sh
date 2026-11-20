#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Render the Machina social cards to JPEG:
#   machina-social-card.html (1600x900, LinkedIn and X)
#   machina-share-card.html  (1200x630, GitHub social preview and README hero)
# Needs Google Chrome and macOS `sips` (both already on a Mac); nothing is installed.
#   ./docs/social/build-social-card.sh
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
[[ -x "$CHROME" ]] || { echo "Google Chrome not found (set CHROME=...)" >&2; exit 1; }
TMP="$(mktemp -d "${TMPDIR:-/tmp}/machina-card.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
render() { # <name> <width> <height>
  "$CHROME" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=2 \
    --window-size="$2,$3" --screenshot="$TMP/$1.png" "file://$HERE/$1.html" >/dev/null 2>&1
  sips -s format jpeg -s formatOptions 90 "$TMP/$1.png" --out "$HERE/$1.jpg" >/dev/null
  echo "wrote docs/social/$1.jpg ($(sips -g pixelWidth -g pixelHeight "$HERE/$1.jpg" | awk '/pixel/{printf "%s ", $2}')px, $(du -k "$HERE/$1.jpg" | cut -f1) KB)"
}
render machina-social-card 1600 900
render machina-share-card 1200 630
render fluxvm-results-card 1600 900
render fluxvm-results-share 1200 630
