#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# host-tune-memory.sh — extra swap on /data + optional nginx 80/443 disable
#
# Use on memory-tight hypervisors (k3s + other cloud workloads + Machina) before cargo builds.
#
#   sudo ./scripts/host-tune-memory.sh
#   sudo ./scripts/host-tune-memory.sh --swap-gb 32 --disable-nginx
#   sudo ./scripts/host-tune-memory.sh --swap-gb 16 --no-disable-nginx
#
set -euo pipefail

SWAP_GB=32
SWAP_DIR=/data/swap
SWAP_FILE="${SWAP_DIR}/machina-extra.swap"
DISABLE_NGINX=false

usage() {
  sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'
  exit 0
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage ;;
    --swap-gb) SWAP_GB="${2:?}"; shift 2 ;;
    --disable-nginx) DISABLE_NGINX=true; shift ;;
    --no-disable-nginx) DISABLE_NGINX=false; shift ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

if [[ $EUID -ne 0 ]]; then
  echo "Run as root: sudo $0" >&2
  exit 1
fi

if [[ ! -d /data ]]; then
  echo "/data not mounted — pick another SWAP_DIR or mount /data first" >&2
  exit 1
fi

avail_kb=$(df -k /data | awk 'NR==2 {print $4}')
need_kb=$((SWAP_GB * 1024 * 1024))
if (( avail_kb < need_kb + 1024 * 1024 )); then
  echo "Need ~${SWAP_GB}G free on /data (have $((avail_kb / 1024 / 1024))G)" >&2
  exit 1
fi

mkdir -p "$SWAP_DIR"
if [[ ! -f "$SWAP_FILE" ]]; then
  echo "Creating ${SWAP_GB}G swap at $SWAP_FILE ..."
  if ! fallocate -l "${SWAP_GB}G" "$SWAP_FILE" 2>/dev/null; then
    dd if=/dev/zero of="$SWAP_FILE" bs=1M count=$((SWAP_GB * 1024)) status=progress
  fi
  chmod 600 "$SWAP_FILE"
  mkswap "$SWAP_FILE"
fi

grep -qF "$SWAP_FILE" /etc/fstab || echo "$SWAP_FILE none swap sw 0 0" >> /etc/fstab
swapon "$SWAP_FILE" 2>/dev/null || true
echo "Swap:"
swapon --show

if $DISABLE_NGINX; then
  echo "Disabling nginx public sites (80/443) ..."
  for f in /etc/nginx/conf.d/v9s.conf; do
    [[ -f "$f" && ! -f "${f}.disabled" ]] && mv "$f" "${f}.disabled"
  done
  systemctl stop nginx 2>/dev/null || true
  systemctl disable nginx 2>/dev/null || true
  if ss -tlnp 2>/dev/null | grep -qE ':80 |:443 '; then
    echo "Warning: something still listens on 80/443" >&2
    ss -tlnp | grep -E ':80 |:443 ' || true
  else
    echo "OK: ports 80/443 are free"
  fi
fi

echo "Memory:"
free -h
echo "Machina: https://<host>:5092/ (not nginx 80/443)"
