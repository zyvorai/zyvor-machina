#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Build machina-hardware-wow-reel from Playwright recording + title cards.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

W=1920
H=1080
FPS=30
CRF=18
PRESET=slow
FADE=0.35
PNG=png-hw

mkdir -p seg-hw out raw

src_of() { ls raw/"$1"/*.webm | head -1; }

make_title() {
  local out="$1" dur="$2" png="$3"
  local fout
  fout=$(python3 -c "print(${dur} - ${FADE})")
  ffmpeg -y -loop 1 -t "${dur}" -i "${PNG}/${png}.png" \
    -vf "fade=t=in:st=0:d=${FADE},fade=t=out:st=${fout}:d=${FADE}" \
    -r "${FPS}" -c:v libx264 -preset "${PRESET}" -crf "${CRF}" -pix_fmt yuv420p -movflags +faststart "seg-hw/${out}" -loglevel error
  echo "  title: ${out} (${dur}s)"
}

extract_clip() {
  local segdir="$1" start="$2" dur="$3" out="$4" cap="$5"
  local src="$(src_of "${segdir}")"
  local tmp="seg-hw/_raw_${out}"
  local fout
  fout=$(python3 -c "print(${dur} - ${FADE})")
  ffmpeg -y -ss "${start}" -t "${dur}" -i "${src}" \
    -vf "scale=${W}:${H}:force_original_aspect_ratio=decrease:flags=lanczos,pad=${W}:${H}:(ow-iw)/2:(oh-ih)/2,fps=${FPS},format=yuv420p,fade=t=in:st=0:d=${FADE},fade=t=out:st=${fout}:d=${FADE}" \
    -an -c:v libx264 -preset "${PRESET}" -crf "${CRF}" -pix_fmt yuv420p -movflags +faststart "${tmp}" -loglevel error
  ffmpeg -y -i "${tmp}" -loop 1 -i "${PNG}/${cap}.png" \
    -filter_complex "[0:v][1:v] overlay=(W-w)/2:H-h-60:shortest=1" \
    -c:v libx264 -preset "${PRESET}" -crf "${CRF}" -pix_fmt yuv420p -movflags +faststart "seg-hw/${out}" -loglevel error
  rm -f "${tmp}"
  echo "  extract: ${out} [${start}, +${dur}]"
}

concat_to() {
  local outfile="$1"; shift
  local listfile="seg-hw/_list.txt"
  : > "${listfile}"
  for f in "$@"; do echo "file '${f}.mp4'" >> "${listfile}"; done
  ffmpeg -y -f concat -safe 0 -i "${listfile}" -c copy "out/${outfile}" -loglevel error
  echo "== built out/${outfile} =="
  ffprobe -v error -show_entries format=duration -of csv=p=0 "out/${outfile}"
}

MARKS_FILE="${1:-}"
# Defaults are VIDEO-relative seconds (mark_process - (done_process - webm_duration)).
login_s=8.0
summary_s=66.0
display_s=74.5
storage_s=79.0
network_s=82.5
nic_s=87.5
hostdev_s=96.5
firmware_s=101.0
video_s=105.0
if [[ -n "${MARKS_FILE}" && -f "${MARKS_FILE}" ]]; then
  # shellcheck disable=SC1090
  source "${MARKS_FILE}"
fi

echo "== Title cards =="
make_title "k00.mp4" 3.8 "hw00-title"
make_title "k01t.mp4" 1.4 "hw01-login"
make_title "k03t.mp4" 1.4 "hw02-summary"
make_title "k05t.mp4" 1.4 "hw03-display"
make_title "k07t.mp4" 1.4 "hw04-storage"
make_title "k09t.mp4" 1.4 "hw05-network"
make_title "k11t.mp4" 1.4 "hw06-hostdev"
make_title "k13t.mp4" 1.4 "hw07-firmware"
make_title "k15.mp4" 3.5 "hw08-outro"

echo "== Clips =="
extract_clip "seg-hw-feats" "${login_s}"    2.4 "k02.mp4" "cap-hw-login"
extract_clip "seg-hw-feats" "${summary_s}"  3.2 "k04.mp4" "cap-hw-summary"
extract_clip "seg-hw-feats" "${display_s}"  2.8 "k06.mp4" "cap-hw-display"
extract_clip "seg-hw-feats" "${storage_s}"  3.2 "k08.mp4" "cap-hw-storage"
extract_clip "seg-hw-feats" "${network_s}"  2.4 "k10.mp4" "cap-hw-nic"
extract_clip "seg-hw-feats" "${nic_s}"      4.0 "k10b.mp4" "cap-hw-nic"
extract_clip "seg-hw-feats" "${hostdev_s}"  3.2 "k12.mp4" "cap-hw-hostdev"
extract_clip "seg-hw-feats" "${firmware_s}" 2.8 "k14.mp4" "cap-hw-firmware"
extract_clip "seg-hw-feats" "${video_s}"    3.5 "k14b.mp4" "cap-hw-display"

concat_to "machina-hardware-wow-reel.mp4" \
  k00 k01t k02 k03t k04 k05t k06 k07t k08 k09t k10 k10b k11t k12 k13t k14 k14b k15

cp out/machina-hardware-wow-reel.mp4 out/machina-hardware-wow-reel-linkedin-1080p.mp4
ffmpeg -y -i out/machina-hardware-wow-reel.mp4 -c:v libvpx-vp9 -b:v 0 -crf 32 -an \
  out/machina-hardware-wow-reel.webm -loglevel error

mkdir -p "$HOME/Desktop/Machina-Demo-Videos/machina-hardware"
cp -f out/machina-hardware-wow-reel.mp4 \
      out/machina-hardware-wow-reel-linkedin-1080p.mp4 \
      out/machina-hardware-wow-reel.webm \
      "$HOME/Desktop/Machina-Demo-Videos/machina-hardware/"
cp -f out/machina-hardware-wow-reel.mp4 "$HOME/Desktop/"
echo "== Done =="
