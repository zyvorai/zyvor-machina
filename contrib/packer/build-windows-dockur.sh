#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Build Windows golden qcow2 via dockur/windows (Podman + KVM).
# Usage: ./build-windows-dockur.sh {win10|win11|windows-server-2022|windows-server-2025} [workdir]
# Output: workdir/output-{guest}/{guest}.qcow2 (same layout as build-linux-image.sh)
set -euo pipefail

GUEST="${1:-win11}"
WORKDIR="${2:-work}"

case "$GUEST" in
  win11) DOCKUR_VERSION="${DOCKUR_VERSION:-11}" ;;
  win10) DOCKUR_VERSION="${DOCKUR_VERSION:-10}" ;;
  windows-server-2022) DOCKUR_VERSION="${DOCKUR_VERSION:-2022}" ;;
  windows-server-2025) DOCKUR_VERSION="${DOCKUR_VERSION:-2025}" ;;
  *)
    echo "[machina] unknown dockur guest: $GUEST (expected win10, win11, windows-server-2022, or windows-server-2025)" >&2
    exit 1
    ;;
esac

mkdir -p "$WORKDIR"
STORAGE_DIR="$(cd "$WORKDIR" && pwd)/storage"
OUT_DIR="$(cd "$WORKDIR" && pwd)/output-${GUEST}"
OUT_QCOW2="${OUT_DIR}/${GUEST}.qcow2"
DISK_QCOW2="${STORAGE_DIR}/data.qcow2"

DOCKUR_IMAGE="${MACHINA_DOCKUR_IMAGE:-docker.io/dockurr/windows}"
DISK_SIZE="${MACHINA_DOCKUR_DISK_SIZE:-64G}"
DISK_FMT="${MACHINA_DOCKUR_DISK_FMT:-qcow2}"
RAM_SIZE="${MACHINA_DOCKUR_RAM_SIZE:-4G}"
CPU_CORES="${MACHINA_DOCKUR_CPU_CORES:-2}"
WIN_USERNAME="${MACHINA_DOCKUR_USERNAME:-Docker}"
WIN_PASSWORD="${MACHINA_DOCKUR_PASSWORD:-admin}"
MAX_WAIT_SECS="${MACHINA_DOCKUR_MAX_WAIT_SECS:-10800}"
POLL_SECS="${MACHINA_DOCKUR_POLL_SECS:-15}"

CONTAINER_NAME="machina-dockur-${GUEST}-$$"

log() { echo "[machina] $*"; }
die() { echo "[machina] ERROR: $*" >&2; exit 1; }

podman_bin() {
  if command -v podman >/dev/null 2>&1; then
    echo podman
    return 0
  fi
  return 1
}

# dockur prints "Windows started successfully" even while booting the *installer* ISO.
# Prefer the windows.boot marker + large disk; fall back to hard-disk Boot Manager
# with a large qcow2 (installer DVD lines may still linger in the log window).
install_complete_in_logs() {
  local blob="$1"
  grep -qiE 'Windows Boot Manager' <<<"$blob" \
    && grep -qiE 'Windows started successfully|Windows is running' <<<"$blob" \
    && ! grep -qiE 'UEFI QEMU DVD-ROM' <<<"$blob"
}

disk_looks_installed() {
  [ -f "$DISK_QCOW2" ] || return 1
  local size
  size="$(stat -c%s "$DISK_QCOW2" 2>/dev/null || echo 0)"
  # Fresh installs land well above 5 GiB once the WIM is applied
  [ "$size" -gt 5000000000 ] || return 1
  # Strong signal: dockur creates windows.boot when setup finished
  if [ -f "${STORAGE_DIR}/windows.boot" ]; then
    return 0
  fi
  return 1
}

# HD boot + large disk for several consecutive polls (OOBE / first boot).
hd_boot_stable() {
  local blob="$1"
  [ -f "$DISK_QCOW2" ] || return 1
  local size
  size="$(stat -c%s "$DISK_QCOW2" 2>/dev/null || echo 0)"
  [ "$size" -gt 8000000000 ] || return 1
  grep -qiE 'Windows Boot Manager' <<<"$blob"
}

log "Golden Forge (dockur): guest=${GUEST} VERSION=${DOCKUR_VERSION}"
log "Workdir: $(cd "$WORKDIR" && pwd)"
log "Image: ${DOCKUR_IMAGE}"

if ! PODMAN=$(podman_bin); then
  die "podman not found — install podman on the hypervisor"
fi

if [ ! -e /dev/kvm ]; then
  die "/dev/kvm missing — KVM required for dockur/windows"
fi

mkdir -p "$STORAGE_DIR" "$OUT_DIR"
rm -f "$DISK_QCOW2" "${STORAGE_DIR}/data.img" "${STORAGE_DIR}/windows.boot"

log "Pulling ${DOCKUR_IMAGE} (may take a while on first run)"
"$PODMAN" pull "$DOCKUR_IMAGE"

log "Starting container ${CONTAINER_NAME}"
"$PODMAN" run -d --name "$CONTAINER_NAME" \
  -e "VERSION=${DOCKUR_VERSION}" \
  -e "DISK_FMT=${DISK_FMT}" \
  -e "DISK_SIZE=${DISK_SIZE}" \
  -e "RAM_SIZE=${RAM_SIZE}" \
  -e "CPU_CORES=${CPU_CORES}" \
  -e "USERNAME=${WIN_USERNAME}" \
  -e "PASSWORD=${WIN_PASSWORD}" \
  -e "AUTOLOGIN=Y" \
  --device /dev/kvm \
  --device /dev/net/tun \
  --cap-add NET_ADMIN \
  -v "${STORAGE_DIR}:/storage" \
  "$DOCKUR_IMAGE"

cleanup_container() {
  "$PODMAN" stop -t 120 "$CONTAINER_NAME" >/dev/null 2>&1 || true
  "$PODMAN" rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true
}

trap cleanup_container EXIT

log "Streaming container logs (install may take 30–90 minutes)"
elapsed=0
last_log_line=0
hd_boot_hits=0
while (( elapsed < MAX_WAIT_SECS )); do
  if ! "$PODMAN" inspect -f '{{.State.Running}}' "$CONTAINER_NAME" 2>/dev/null | grep -q true; then
    log "Container exited — checking disk artifact"
    break
  fi

  mapfile -t all_logs < <("$PODMAN" logs "$CONTAINER_NAME" 2>&1)
  for ((i = last_log_line; i < ${#all_logs[@]}; i++)); do
    echo "[dockur] ${all_logs[$i]}"
  done
  last_log_line=${#all_logs[@]}

  logs="$(printf '%s\n' "${all_logs[@]: -200}")"

  if disk_looks_installed; then
    size="$(stat -c%s "$DISK_QCOW2" 2>/dev/null || echo 0)"
    log "Install complete (windows.boot + disk ${size} bytes)"
    sleep 60
    break
  fi

  if install_complete_in_logs "$logs"; then
    size="$(stat -c%s "$DISK_QCOW2" 2>/dev/null || echo 0)"
    log "Install complete marker detected; disk size ${size} bytes"
    sleep 60
    break
  fi

  if hd_boot_stable "$logs"; then
    hd_boot_hits=$((hd_boot_hits + 1))
    log "HD boot seen (${hd_boot_hits}/8 consecutive polls)"
    # ~2 minutes of stable HD boot after a large disk → harvest
    if (( hd_boot_hits >= 8 )); then
      size="$(stat -c%s "$DISK_QCOW2" 2>/dev/null || echo 0)"
      log "Treating install as complete after stable HD boot; disk ${size} bytes"
      # Ensure windows.boot exists so vessel golden seeding skips re-download
      : >"${STORAGE_DIR}/windows.boot"
      sleep 30
      break
    fi
  else
    hd_boot_hits=0
  fi

  sleep "$POLL_SECS"
  elapsed=$((elapsed + POLL_SECS))
done

if (( elapsed >= MAX_WAIT_SECS )); then
  die "timed out after ${MAX_WAIT_SECS}s waiting for dockur install"
fi

cleanup_container
trap - EXIT

if [ ! -f "$DISK_QCOW2" ]; then
  die "expected disk at ${DISK_QCOW2} not found"
fi

log "Copying golden image to ${OUT_QCOW2}"
cp -f "$DISK_QCOW2" "$OUT_QCOW2"
chmod 644 "$OUT_QCOW2"

if command -v qemu-img >/dev/null 2>&1; then
  qemu-img info "$OUT_QCOW2" | head -5
fi

log "Default login: ${WIN_USERNAME} / ${WIN_PASSWORD} — rotate before production use"
log "Clone with virtio disk + UEFI; enable RDP via Machina API if needed"
log "Artifact: ${OUT_QCOW2}"
