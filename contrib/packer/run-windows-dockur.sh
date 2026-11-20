#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Run a Windows guest via dockur/windows in Podman (preferred) or Docker.
# Usage: ./run-windows-dockur.sh {win10|win11|windows-server-2022|windows-server-2025} [container-name]
# Optional: MACHINA_DOCKUR_GOLDEN=/var/lib/libvirt/images/win11.qcow2
#           MACHINA_DOCKUR_PUBLISH=1 (default) maps 127.0.0.1:8006 and :3389
set -euo pipefail

GUEST="${1:-win11}"
NAME="${2:-machina-${GUEST}}"

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

DOCKUR_IMAGE="${MACHINA_DOCKUR_IMAGE:-docker.io/dockurr/windows}"
DISK_SIZE="${MACHINA_DOCKUR_DISK_SIZE:-64G}"
DISK_FMT="${MACHINA_DOCKUR_DISK_FMT:-qcow2}"
RAM_SIZE="${MACHINA_DOCKUR_RAM_SIZE:-4G}"
CPU_CORES="${MACHINA_DOCKUR_CPU_CORES:-2}"
WIN_USERNAME="${MACHINA_DOCKUR_USERNAME:-Docker}"
WIN_PASSWORD="${MACHINA_DOCKUR_PASSWORD:-admin}"
PUBLISH="${MACHINA_DOCKUR_PUBLISH:-1}"
GOLDEN="${MACHINA_DOCKUR_GOLDEN:-/var/lib/libvirt/images/${GUEST}.qcow2}"
STORAGE_DIR="${MACHINA_DOCKUR_STORAGE:-/var/lib/machina/vessel-windows/${GUEST}}"

log() { echo "[machina] $*"; }
die() { echo "[machina] ERROR: $*" >&2; exit 1; }

runtime_bin() {
  if command -v podman >/dev/null 2>&1; then
    echo podman
    return 0
  fi
  if command -v docker >/dev/null 2>&1; then
    echo docker
    return 0
  fi
  return 1
}

if ! RT=$(runtime_bin); then
  die "neither podman nor docker found on PATH"
fi

if [ ! -e /dev/kvm ]; then
  die "/dev/kvm missing — KVM required for dockur/windows"
fi

mkdir -p "$STORAGE_DIR"
# Optional full dockur /storage seed (data.qcow2 + windows.* sidecars from a prior install).
SEED_DIR="${MACHINA_DOCKUR_SEED_DIR:-}"
if [ -z "$SEED_DIR" ] && [ -d "/var/lib/machina/dockur-seeds/${GUEST}" ]; then
  SEED_DIR="/var/lib/machina/dockur-seeds/${GUEST}"
fi

seed_file() {
  local src="$1" dst="$2"
  if [ -f "$src" ]; then
    if ! ln -f "$src" "$dst" 2>/dev/null; then
      cp --reflink=auto -f "$src" "$dst" 2>/dev/null || cp -a "$src" "$dst"
    fi
  fi
}

if [ -n "$SEED_DIR" ] && [ -f "${SEED_DIR}/data.qcow2" ]; then
  log "Seeding storage from ${SEED_DIR}"
  seed_file "${SEED_DIR}/data.qcow2" "${STORAGE_DIR}/data.qcow2"
  for f in windows.boot windows.base windows.ver windows.rom windows.vars windows.mac; do
    seed_file "${SEED_DIR}/${f}" "${STORAGE_DIR}/${f}"
  done
  [ -f "${STORAGE_DIR}/windows.boot" ] || : >"${STORAGE_DIR}/windows.boot"
  log "Seeded ${STORAGE_DIR}/data.qcow2 ($(stat -c%s "${STORAGE_DIR}/data.qcow2") bytes)"
elif [ -f "$GOLDEN" ]; then
  log "Using golden disk ${GOLDEN}"
  # protected_hardlinks can block ln when the daemon user does not own the golden.
  if ! ln -f "$GOLDEN" "${STORAGE_DIR}/data.qcow2"; then
    log "hardlink failed — copying (reflink if available)"
    cp --reflink=auto -f "$GOLDEN" "${STORAGE_DIR}/data.qcow2" \
      || cp -a "$GOLDEN" "${STORAGE_DIR}/data.qcow2"
  fi
  # dockur re-downloads Windows unless it sees an installed disk marker.
  : >"${STORAGE_DIR}/windows.boot"
  log "Seeded ${STORAGE_DIR}/data.qcow2 ($(stat -c%s "${STORAGE_DIR}/data.qcow2") bytes) — tip: set MACHINA_DOCKUR_SEED_DIR for full windows.* sidecars"
else
  log "No golden at ${GOLDEN} — dockur will install ${GUEST} into ${STORAGE_DIR}"
fi

if "$RT" inspect "$NAME" >/dev/null 2>&1; then
  die "container ${NAME} already exists — stop/remove it first, or pick another name"
fi

log "Pulling ${DOCKUR_IMAGE} via ${RT}"
"$RT" pull "$DOCKUR_IMAGE"

PUBLISH_ARGS=()
if [ "$PUBLISH" = "1" ]; then
  PUBLISH_ARGS+=(-p 127.0.0.1:8006:8006 -p 127.0.0.1:3389:3389/tcp -p 127.0.0.1:3389:3389/udp)
fi

log "Starting ${NAME} (${GUEST} VERSION=${DOCKUR_VERSION})"
"$RT" run -d --name "$NAME" \
  -e "VERSION=${DOCKUR_VERSION}" \
  -e "DISK_FMT=${DISK_FMT}" \
  -e "DISK_SIZE=${DISK_SIZE}" \
  -e "RAM_SIZE=${RAM_SIZE}" \
  -e "CPU_CORES=${CPU_CORES}" \
  -e "USERNAME=${WIN_USERNAME}" \
  -e "PASSWORD=${WIN_PASSWORD}" \
  -e "AUTOLOGIN=Y" \
  --label "machina.io/windows-dockur=1" \
  --label "machina.io/guest=${GUEST}" \
  --device /dev/kvm \
  --device /dev/net/tun \
  --cap-add NET_ADMIN \
  -v "${STORAGE_DIR}:/storage" \
  "${PUBLISH_ARGS[@]}" \
  --stop-timeout 120 \
  "$DOCKUR_IMAGE"

CID="$("$RT" inspect -f '{{.Id}}' "$NAME" 2>/dev/null || true)"
log "engine=${RT}"
log "container=${NAME}"
log "id=${CID}"
log "web=http://127.0.0.1:8006"
log "rdp=127.0.0.1:3389"
log "login=${WIN_USERNAME} / ${WIN_PASSWORD}"
log "Artifact storage: ${STORAGE_DIR}"
