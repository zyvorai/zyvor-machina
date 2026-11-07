#!/bin/bash
# Run MachinaKit tests with swiftc + Swift Testing (macOS only). Fast alternative to `swift test`,
# whose build backend stalled on some volumes. Usage: macos/scripts/test.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
KIT="$ROOT/MachinaKit"
OUT="$ROOT/build/test"; mkdir -p "$OUT"
DEV="$(xcode-select -p)"
FW="$DEV/Platforms/MacOSX.platform/Developer/Library/Frameworks"
LIB="$DEV/Platforms/MacOSX.platform/Developer/usr/lib"
PLUGINS="$DEV/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/host/plugins/testing"
FLAGS=(-swift-version 6 -target arm64-apple-macos15.0 -Onone -g)

# Swift Testing needs an entry point when linked outside SwiftPM.
cat > "$OUT/main.swift" <<'S'
@_spi(Experimental) @_spi(ForToolsIntegrationOnly) import Testing
await Testing.__swiftPMEntryPoint() as Never
S

swiftc -c -wmo "${FLAGS[@]}" -enable-testing -module-name MachinaKit -parse-as-library "$KIT"/Sources/MachinaKit/*.swift \
  -emit-module -emit-module-path "$OUT/MachinaKit.swiftmodule" -o "$OUT/MachinaKit.o"
swiftc "${FLAGS[@]}" -module-name MachinaKitTests -I "$OUT" -F "$FW" -plugin-path "$PLUGINS" \
  -Xlinker -rpath -Xlinker "$FW" -L "$LIB" -Xlinker -rpath -Xlinker "$LIB" \
  "$OUT/main.swift" "$KIT"/Tests/MachinaKitTests/*.swift "$OUT/MachinaKit.o" -o "$OUT/tests"
"$OUT/tests"
