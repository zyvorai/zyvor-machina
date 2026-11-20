#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Build separate Linux + Windows golden→VM demo reels from Playwright raw/*.webm
#
# Calibrate extract_clip starts after each record (mark() ≠ video-relative).
# Defaults below are conservative mid-clip holds; override by editing starts.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

W=1920
H=1080
FPS=30
CRF=18
PRESET=slow
FADE=0.35

mkdir -p seg out png

src_of() { ls raw/"$1"/*.webm | head -1; }

dur_of() {
  ffprobe -v error -show_entries format=duration -of csv=p=0 "$(src_of "$1")"
}

make_title() {
  local out="$1" dur="$2" png="$3"
  local fout
  fout=$(python3 -c "print(${dur} - ${FADE})")
  ffmpeg -y -loop 1 -t "${dur}" -i "png/${png}.png" \
    -vf "fade=t=in:st=0:d=${FADE},fade=t=out:st=${fout}:d=${FADE}" \
    -r "${FPS}" -c:v libx264 -preset "${PRESET}" -crf "${CRF}" -pix_fmt yuv420p -movflags +faststart "seg/${out}" -loglevel error
  echo "  title: ${out} (${dur}s)"
}

extract_clip() {
  local segdir="$1" start="$2" dur="$3" out="$4" cap="$5"
  local src="$(src_of "${segdir}")"
  local tmp="seg/_raw_${out}"
  local fout
  fout=$(python3 -c "print(${dur} - ${FADE})")
  ffmpeg -y -ss "${start}" -t "${dur}" -i "${src}" \
    -vf "scale=${W}:${H}:force_original_aspect_ratio=decrease:flags=lanczos,pad=${W}:${H}:(ow-iw)/2:(oh-ih)/2,fps=${FPS},format=yuv420p,fade=t=in:st=0:d=${FADE},fade=t=out:st=${fout}:d=${FADE}" \
    -an -c:v libx264 -preset "${PRESET}" -crf "${CRF}" -pix_fmt yuv420p -movflags +faststart "${tmp}" -loglevel error
  ffmpeg -y -i "${tmp}" -loop 1 -i "png/${cap}.png" \
    -filter_complex "[0:v][1:v] overlay=(W-w)/2:H-h-60:shortest=1" \
    -c:v libx264 -preset "${PRESET}" -crf "${CRF}" -pix_fmt yuv420p -movflags +faststart "seg/${out}" -loglevel error
  rm -f "${tmp}"
  echo "  extract: ${out} [${start}, +${dur}]"
}

concat_to() {
  local outfile="$1"; shift
  local listfile="seg/_list_golden.txt"
  : > "${listfile}"
  for f in "$@"; do echo "file '${f}.mp4'" >> "${listfile}"; done
  ffmpeg -y -f concat -safe 0 -i "${listfile}" -c copy "out/${outfile}" -loglevel error
  echo "== built out/${outfile} =="
  ffprobe -v error -show_entries format=duration -of csv=p=0 "out/${outfile}"
}

LINUX_DUR=$(python3 -c "print(max(20.0, float('$(dur_of seg-golden-linux)') - 2))")
WIN_DUR=$(python3 -c "print(max(20.0, float('$(dur_of seg-golden-windows)') - 2))")
echo "raw durations: linux=$(dur_of seg-golden-linux)s windows=$(dur_of seg-golden-windows)s"

# Heuristic mid-points (re-calibrate after record if needed)
L1=$(python3 -c "print(max(2.0, ${LINUX_DUR} * 0.12))")
L2=$(python3 -c "print(max(4.0, ${LINUX_DUR} * 0.28))")
L3=$(python3 -c "print(max(6.0, ${LINUX_DUR} * 0.48))")
L4=$(python3 -c "print(max(8.0, ${LINUX_DUR} * 0.72))")

W1=$(python3 -c "print(max(2.0, ${WIN_DUR} * 0.10))")
W2=$(python3 -c "print(max(5.0, ${WIN_DUR} * 0.32))")
W3=$(python3 -c "print(max(8.0, ${WIN_DUR} * 0.55))")
W4=$(python3 -c "print(max(10.0, ${WIN_DUR} * 0.78))")

echo "== Linux golden reel =="
make_title "gl00.mp4" 3.5 "g-linux-title"
extract_clip "seg-golden-linux" "$L1" 3.0 "gl01.mp4" "cap-g-disks"
extract_clip "seg-golden-linux" "$L2" 3.2 "gl02.mp4" "cap-g-create"
extract_clip "seg-golden-linux" "$L3" 3.5 "gl03.mp4" "cap-g-tmpl-linux"
extract_clip "seg-golden-linux" "$L4" 4.0 "gl04.mp4" "cap-g-spawn-linux"
make_title "gl05.mp4" 3.2 "g-linux-outro"
concat_to "machina-golden-linux.mp4" gl00 gl01 gl02 gl03 gl04 gl05

echo "== Windows golden reel =="
make_title "gw00.mp4" 3.5 "g-win-title"
extract_clip "seg-golden-windows" "$W1" 3.2 "gw01.mp4" "cap-g-win-show"
extract_clip "seg-golden-windows" "$W2" 3.5 "gw02.mp4" "cap-g-create"
extract_clip "seg-golden-windows" "$W3" 3.5 "gw03.mp4" "cap-g-tmpl-win"
extract_clip "seg-golden-windows" "$W4" 4.0 "gw04.mp4" "cap-g-spawn-win"
make_title "gw05.mp4" 3.2 "g-win-outro"
concat_to "machina-golden-windows.mp4" gw00 gw01 gw02 gw03 gw04 gw05

cp -f out/machina-golden-linux.mp4 out/machina-golden-windows.mp4 ~/Desktop/ 2>/dev/null || true
ls -lah out/machina-golden-*.mp4
echo "== Done =="
