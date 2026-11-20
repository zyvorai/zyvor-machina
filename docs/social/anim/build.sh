#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Regenerate the animated SVGs (gen.mjs) and render GIF fallbacks for places that do not play SVG animation.
# Needs node, ffmpeg and Google Chrome. Frames are made by pausing the CSS animations at each timestamp in headless Chrome.
#   ./docs/social/anim/build.sh [scene ...]      FPS=5 WIDTH=900 to tune size
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
FPS="${FPS:-5}"; WIDTH="${WIDTH:-960}"
[[ -x "$CHROME" ]] || { echo "Google Chrome not found (set CHROME=...)" >&2; exit 1; }
node "$HERE/gen.mjs"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/machina-anim.XXXXXX")"; trap 'rm -rf "$TMP"' EXIT
scenes=("$@"); [[ ${#scenes[@]} -gt 0 ]] || scenes=(deploy-single-host deploy-fleet deploy-postgres ec2-launch openstack-vs-machina)
for s in "${scenes[@]}"; do
  dur="$(grep -o 'dur [0-9.]*' <<<"$(node "$HERE/gen.mjs" | grep "$s.svg")" | cut -d' ' -f2)"
  n=$(awk -v d="$dur" -v f="$FPS" 'BEGIN{printf "%d", d*f}')
  mkdir -p "$TMP/$s"
  for ((i=0;i<n;i++)); do
    ms=$(awk -v i="$i" -v f="$FPS" 'BEGIN{printf "%d", i*1000/f}')
    { echo '<!doctype html><meta charset=utf-8><style>html,body{margin:0;background:#000}svg{display:block}</style>'
      cat "$HERE/$s.svg"
      echo "<script>document.getAnimations().forEach(a=>{a.pause();a.currentTime=$ms})</script>"; } > "$TMP/$s/f$i.html"
    printf '%s\n' "$i"
  done | xargs -P 4 -I{} sh -c '"$0" --headless=new --disable-gpu --hide-scrollbars --window-size=1600,720 --screenshot="$1/f{}.png" "file://$1/f{}.html" >/dev/null 2>&1' "$CHROME" "$TMP/$s"
  ffmpeg -loglevel error -y -framerate "$FPS" -i "$TMP/$s/f%d.png" \
    -vf "scale=$WIDTH:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=96[p];[b][p]paletteuse=dither=bayer:bayer_scale=4" -loop 0 "$HERE/$s.gif"
  echo "wrote docs/social/anim/$s.gif ($(du -k "$HERE/$s.gif" | cut -f1) KB, $n frames)"
done
