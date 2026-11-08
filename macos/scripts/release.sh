#!/bin/bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Build, sign, notarize and package Machina.app as a DMG (macOS only).
#
#   macos/scripts/release.sh                      # unsigned local DMG (ad-hoc signature, no notarization)
#   SIGN_IDENTITY="Developer ID Application: Name (TEAMID)" NOTARY_PROFILE=machina-notary macos/scripts/release.sh
#
# One-time notarization setup (stores credentials in the login keychain, nothing is kept in this repo):
#   xcrun notarytool store-credentials machina-notary --apple-id YOU@example.com --team-id TEAMID
#
# Or notarize with an App Store Connect API key (Developer role or higher) instead of a stored profile:
#   ASC_API_KEY=<key id> ASC_API_KEY_PATH=~/Downloads/AuthKey_<key id>.p8 [ASC_API_ISSUER=<issuer uuid>]
#   (leave ASC_API_ISSUER unset for an individual key)
#
# Env: MACHINA_VERSION (default 0.1.0), SIGN_IDENTITY, NOTARY_PROFILE or ASC_API_*, SKIP_NOTARIZE=1.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="${MACHINA_VERSION:-0.1.0}"
SIGN_IDENTITY="${SIGN_IDENTITY:--}"
DIST="$ROOT/build/dist"; APP="$ROOT/build/Machina.app"; DMG="$DIST/Machina-$VERSION.dmg"

MACHINA_VERSION="$VERSION" SIGN_IDENTITY="$SIGN_IDENTITY" "$ROOT/scripts/bundle.sh" release
codesign --verify --strict --verbose=2 "$APP"

rm -rf "$DIST"; mkdir -p "$DIST/stage"
cp -R "$APP" "$DIST/stage/"; ln -s /Applications "$DIST/stage/Applications"
hdiutil create -volname "Machina $VERSION" -srcfolder "$DIST/stage" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$DIST/stage"
[ "$SIGN_IDENTITY" = "-" ] || codesign --force --sign "$SIGN_IDENTITY" --timestamp "$DMG"
hdiutil verify "$DMG" >/dev/null && echo "DMG verified: $DMG"

AUTH=()
if [ -n "${NOTARY_PROFILE:-}" ]; then
  AUTH=(--keychain-profile "$NOTARY_PROFILE")
elif [ -n "${ASC_API_KEY:-}" ] && [ -n "${ASC_API_KEY_PATH:-}" ]; then
  [ -f "${ASC_API_KEY_PATH/#\~/$HOME}" ] || { echo "ASC_API_KEY_PATH not found: $ASC_API_KEY_PATH" >&2; exit 1; }
  AUTH=(--key "${ASC_API_KEY_PATH/#\~/$HOME}" --key-id "$ASC_API_KEY")
  [ -z "${ASC_API_ISSUER:-}" ] || AUTH+=(--issuer "$ASC_API_ISSUER")
fi

if [ "$SIGN_IDENTITY" = "-" ] || [ "${SKIP_NOTARIZE:-0}" = 1 ] || [ ${#AUTH[@]} -eq 0 ]; then
  echo "Not notarized (needs SIGN_IDENTITY=Developer ID Application and NOTARY_PROFILE or ASC_API_KEY/ASC_API_KEY_PATH). Gatekeeper will block this DMG on other Macs."
  exit 0
fi
xcrun notarytool submit "$DMG" "${AUTH[@]}" --wait
xcrun stapler staple "$DMG"
spctl --assess --type open --context context:primary-signature --verbose=2 "$DMG" || true
echo "Notarized and stapled: $DMG"
