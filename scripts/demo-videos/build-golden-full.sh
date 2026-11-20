#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Full-process golden demos: title + entire Playwright recording + outro.
# (Unlike build-golden.sh highlight cuts — this keeps every step on screen.)
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

W=1920
H=1080
FPS=30
CRF=18
PRESET=slow
FADE=0.35

mkdir -p seg out

src_of() { ls raw/"$1"/*.webm | head -1; }

make_title() {
  local out="$1" dur="$2" png="$3"
  local fout
  fout=$(python3 -c "print(max(0.1, ${dur} - ${FADE}))")
  ffmpeg -y -loop 1 -t "${dur}" -i "png/${png}.png" \
    -vf "fade=t=in:st=0:d=${FADE},fade=t=out:st=${fout}:d=${FADE}" \
    -r "${FPS}" -c:v libx264 -preset "${PRESET}" -crf "${CRF}" -pix_fmt yuv420p -movflags +faststart "seg/${out}" -loglevel error
  echo "  title: ${out} (${dur}s)"
}

# Full raw webm → 1080p H.264 with a process caption bar for the whole take
full_take() {
  local segdir="$1" out="$2" cap="$3"
  local src
  src="$(src_of "${segdir}")"
  local tmp="seg/_full_${out}"
  ffmpeg -y -i "${src}" \
    -vf "scale=${W}:${H}:force_original_aspect_ratio=decrease:flags=lanczos,pad=${W}:${H}:(ow-iw)/2:(oh-ih)/2,fps=${FPS},format=yuv420p,fade=t=in:st=0:d=${FADE}" \
    -an -c:v libx264 -preset "${PRESET}" -crf "${CRF}" -pix_fmt yuv420p -movflags +faststart "${tmp}" -loglevel error
  ffmpeg -y -i "${tmp}" -loop 1 -i "png/${cap}.png" \
    -filter_complex "[0:v][1:v] overlay=(W-w)/2:H-h-60:shortest=1" \
    -c:v libx264 -preset "${PRESET}" -crf "${CRF}" -pix_fmt yuv420p -movflags +faststart "seg/${out}" -loglevel error
  rm -f "${tmp}"
  local d
  d=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "seg/${out}")
  echo "  full: ${out} (${d}s) from ${src}"
}

concat_to() {
  local outfile="$1"; shift
  local listfile="seg/_list_golden_full.txt"
  : > "${listfile}"
  for f in "$@"; do echo "file '${f}.mp4'" >> "${listfile}"; done
  ffmpeg -y -f concat -safe 0 -i "${listfile}" -c copy "out/${outfile}" -loglevel error
  echo "== built out/${outfile} =="
  ffprobe -v error -show_entries format=duration -of csv=p=0 "out/${outfile}"
}

# Captions that describe the whole flow (shown for entire take)
# Reuse create caption — accurate for both OS reels.
echo "== Linux full process =="
make_title "gfl00.mp4" 4.0 "g-linux-title"
full_take "seg-golden-linux" "gfl01.mp4" "cap-g-full-linux"
make_title "gfl02.mp4" 3.5 "g-linux-outro"
concat_to "machina-golden-linux-full.mp4" gfl00 gfl01 gfl02

echo "== Windows full process =="
make_title "gfw00.mp4" 4.0 "g-win-title"
full_take "seg-golden-windows" "gfw01.mp4" "cap-g-full-win"
make_title "gfw02.mp4" 3.5 "g-win-outro"
concat_to "machina-golden-windows-full.mp4" gfw00 gfw01 gfw02


cp -f out/machina-golden-linux-full.mp4 out/machina-golden-windows-full.mp4 ~/Desktop/
# Also refresh the short names to point at full cuts (user asked for full process)
cp -f out/machina-golden-linux-full.mp4 ~/Desktop/machina-golden-linux.mp4
cp -f out/machina-golden-windows-full.mp4 ~/Desktop/machina-golden-windows.mp4
cp -f out/machina-golden-linux-full.mp4 out/machina-golden-linux.mp4
cp -f out/machina-golden-windows-full.mp4 out/machina-golden-windows.mp4

ls -lah out/machina-golden-*-full.mp4 ~/Desktop/machina-golden-*.mp4
echo "== Done (full process on Desktop) =="
