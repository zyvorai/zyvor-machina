#!/bin/bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Build Machina.app from the SwiftPM executable (macOS only). Usage: macos/scripts/bundle.sh [debug|release]
set -euo pipefail
CFG="${1:-release}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# Direct swiftc build: SwiftPM's build backend stalled on some volumes; this is fast and has no package deps.
SRC="$ROOT/MachinaKit/Sources"; OBJ="$ROOT/build/obj"; mkdir -p "$OBJ"
FLAGS=(-swift-version 6 -target arm64-apple-macos15.0)
[ "$CFG" = release ] && FLAGS+=(-O) || FLAGS+=(-Onone -g)
swiftc -c -wmo "${FLAGS[@]}" -module-name MachinaKit -parse-as-library "$SRC"/MachinaKit/*.swift \
  -emit-module -emit-module-path "$OBJ/MachinaKit.swiftmodule" -o "$OBJ/MachinaKit.o"
swiftc "${FLAGS[@]}" -parse-as-library -module-name Machina -I "$OBJ" "$SRC"/Machina/*.swift "$OBJ/MachinaKit.o" -o "$OBJ/Machina"
BIN="$OBJ/Machina"
APP="$ROOT/build/Machina.app"
VERSION="${MACHINA_VERSION:-0.1.0}"
BUILD="${MACHINA_BUILD:-$(date +%Y%m%d%H%M)}"
SIGN_IDENTITY="${SIGN_IDENTITY:--}"      # "-" = ad-hoc; set to "Developer ID Application: ..." for distribution
rm -rf "$APP"; mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$OBJ/Machina" "$APP/Contents/MacOS/Machina"
ICON=""; if [ -f "$ROOT/Resources/Machina.icns" ]; then cp "$ROOT/Resources/Machina.icns" "$APP/Contents/Resources/"; ICON="<key>CFBundleIconFile</key><string>Machina</string>"; fi
# Localizations: every Resources/<lang>.lproj/Localizable.strings is copied and listed in Info.plist.
LOCS=""
for d in "$ROOT"/Resources/*.lproj; do
  [ -d "$d" ] || continue
  l="$(basename "$d" .lproj)"; mkdir -p "$APP/Contents/Resources/$l.lproj"; cp "$d"/* "$APP/Contents/Resources/$l.lproj/"
  plutil -lint "$d/Localizable.strings" >/dev/null
  LOCS="$LOCS<string>$l</string>"
done
cat > "$APP/Contents/Info.plist" <<P
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.zyvor.machina</string>
<key>CFBundleDevelopmentRegion</key><string>en</string>
<key>CFBundleLocalizations</key><array>$LOCS</array>
<key>CFBundleName</key><string>Machina</string>
<key>CFBundleDisplayName</key><string>Machina</string>
<key>CFBundleExecutable</key><string>Machina</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$VERSION</string>
<key>CFBundleVersion</key><string>$BUILD</string>
$ICON
<key>CFBundleURLTypes</key><array><dict>
  <key>CFBundleURLName</key><string>dev.zyvor.machina</string>
  <key>CFBundleURLSchemes</key><array><string>machina</string></array>
</dict></array>
<key>NSUserActivityTypes</key><array><string>dev.zyvor.machina.vm</string></array>
<key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
<key>LSMinimumSystemVersion</key><string>15.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSHumanReadableCopyright</key><string>Copyright 2026 Zyvor AI Labs</string>
</dict></plist>
P
if [ "$SIGN_IDENTITY" = "-" ]; then
  codesign --force --sign - "$APP" >/dev/null
else
  # Hardened runtime + secure timestamp are required for notarization.
  codesign --force --sign "$SIGN_IDENTITY" --options runtime --timestamp "$APP"
fi
echo "Built $APP (version $VERSION, signed: $([ "$SIGN_IDENTITY" = "-" ] && echo ad-hoc || echo "$SIGN_IDENTITY"))"
