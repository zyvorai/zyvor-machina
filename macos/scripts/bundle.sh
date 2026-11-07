#!/bin/bash
# Build Machina.app from the SwiftPM executable (macOS only). Usage: macos/scripts/bundle.sh [debug|release]
set -euo pipefail
CFG="${1:-release}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# Direct swiftc build: SwiftPM's build backend stalled on some volumes; this is fast and has no package deps.
SRC="$ROOT/MachinaKit/Sources"; OBJ="$ROOT/build/obj"; mkdir -p "$OBJ"
FLAGS=(-swift-version 6 -target arm64-apple-macos14.0)
[ "$CFG" = release ] && FLAGS+=(-O) || FLAGS+=(-Onone -g)
swiftc -c -wmo "${FLAGS[@]}" -module-name MachinaKit -parse-as-library "$SRC"/MachinaKit/*.swift \
  -emit-module -emit-module-path "$OBJ/MachinaKit.swiftmodule" -o "$OBJ/MachinaKit.o"
swiftc "${FLAGS[@]}" -parse-as-library -module-name Machina -I "$OBJ" "$SRC"/Machina/*.swift "$OBJ/MachinaKit.o" -o "$OBJ/Machina"
BIN="$OBJ/Machina"
APP="$ROOT/build/Machina.app"
rm -rf "$APP"; mkdir -p "$APP/Contents/MacOS"
cp "$BIN" "$APP/Contents/MacOS/Machina"
cat > "$APP/Contents/Info.plist" <<P
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.zyvor.machina</string>
<key>CFBundleName</key><string>Machina</string>
<key>CFBundleExecutable</key><string>Machina</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>LSMinimumSystemVersion</key><string>14.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
P
codesign --force --sign - "$APP" >/dev/null
echo "Built $APP"
