#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Render the README cards (docs/social/readme/*.html) to JPEGs in docs/ux/.
# Needs Google Chrome and macOS `sips` (both already on a Mac); nothing is installed.
#   ./docs/social/readme/build.sh
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="$HERE/../../ux"
mkdir -p "$OUT"
CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
[[ -x "$CHROME" ]] || { echo "Google Chrome not found (set CHROME=...)" >&2; exit 1; }
TMP="$(mktemp -d "${TMPDIR:-/tmp}/machina-readme.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
render() { # <html> <jpg> <height>
  "$CHROME" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=2 \
    --window-size="1600,$3" --screenshot="$TMP/$2.png" "file://$HERE/$1" >/dev/null 2>&1
  sips -s format jpeg -s formatOptions 90 "$TMP/$2.png" --out "$OUT/$2" >/dev/null
  echo "wrote docs/ux/$2 ($(du -k "$OUT/$2" | cut -f1) KB)"
}
render architecture.html readme-architecture.jpg 580
render capabilities.html readme-capabilities.jpg 590
render vs-openstack.html readme-vs-openstack.jpg 800
render hero.html readme-hero.jpg 680
render replace-openstack.html readme-replace-openstack.jpg 880
render database.html readme-database.jpg 700
render ec2.html readme-ec2.jpg 720
