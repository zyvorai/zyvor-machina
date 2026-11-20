#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Build / refresh Ubuntu desktop golden qcow2 on a hypervisor (GNOME + GDM autologin for VNC).
#
# Strategy: start from official Ubuntu server cloud image, virt-customize in desktop
# packages (no official desktop cloudimg exists). Output is a backing golden for linked clones.
#
# Usage (on hypervisor):
#   sudo ./scripts/build-ubuntu-desktop-golden.sh
#   FORCE=1 sudo ./scripts/build-ubuntu-desktop-golden.sh
#
# Usage (from laptop):
#   ./scripts/build-ubuntu-desktop-golden-remote.sh sus 212.8.252.194
#
set -euo pipefail

GOLDEN="${GOLDEN:-/var/lib/libvirt/images/ubuntu-24.04-desktop.qcow2}"
SERVER_BASE="${SERVER_BASE:-/var/lib/libvirt/images/ubuntu-24.04-server-cloudimg-amd64.img}"
UBUNTU_RELEASE="${UBUNTU_RELEASE:-24.04}"
DESKTOP_DISK_GIB="${DESKTOP_DISK_GIB:-25}"
DOWNLOAD_URL="${DOWNLOAD_URL:-https://cloud-images.ubuntu.com/releases/${UBUNTU_RELEASE}/release/ubuntu-${UBUNTU_RELEASE}-server-cloudimg-amd64.img}"

log() { echo "== $*"; }

if [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
  echo "Run as root (sudo)" >&2
  exit 1
fi

if [[ -f "$GOLDEN" && "${FORCE:-0}" != "1" ]]; then
  log "Golden desktop image already present: $GOLDEN"
  qemu-img info "$GOLDEN" | head -5
  exit 0
fi

for cmd in curl qemu-img virt-customize; do
  command -v "$cmd" >/dev/null || { echo "Missing $cmd (install guestfs-tools / qemu-utils)" >&2; exit 1; }
done

mkdir -p "$(dirname "$GOLDEN")" "$(dirname "$SERVER_BASE")"

if [[ ! -f "$SERVER_BASE" || "${FORCE_BASE:-0}" == "1" ]]; then
  log "Downloading Ubuntu ${UBUNTU_RELEASE} server cloud image"
  tmp="${SERVER_BASE}.part"
  rm -f "$tmp"
  curl -fL --retry 3 --retry-delay 5 -o "$tmp" "$DOWNLOAD_URL"

  # Verify against Ubuntu's published SHA256SUMS before this becomes the base
  # of every VM built from this golden image — an unverified download (MITM,
  # truncated transfer, compromised mirror) would otherwise be baked in
  # silently. Best-effort: warn (don't block a hypervisor with no outbound
  # access to the sums file) rather than hard-fail, but never skip silently.
  sums_url="$(dirname "$DOWNLOAD_URL")/SHA256SUMS"
  img_name="$(basename "$DOWNLOAD_URL")"
  if sums_tmp="$(mktemp)" && curl -fsSL --retry 3 --retry-delay 5 -o "$sums_tmp" "$sums_url" 2>/dev/null; then
    # SHA256SUMS lines look like "<hash> *filename" (binary mode marker) or
    # "<hash>  filename" — accept either, with or without the leading '*'.
    expected="$(awk -v f="$img_name" '{n=$2; sub(/^\*/,"",n); if (n==f) {print $1; exit}}' "$sums_tmp")"
    rm -f "$sums_tmp"
    if [[ -z "$expected" ]]; then
      echo "WARNING: ${img_name} not listed in ${sums_url} — proceeding unverified" >&2
    else
      actual="$(sha256sum "$tmp" | awk '{print $1}')"
      if [[ "$actual" != "$expected" ]]; then
        rm -f "$tmp"
        echo "ERROR: checksum mismatch for ${img_name}: expected ${expected}, got ${actual}" >&2
        exit 1
      fi
      log "Checksum verified against ${sums_url}"
    fi
  else
    rm -f "${sums_tmp:-}"
    echo "WARNING: could not fetch ${sums_url} — proceeding unverified" >&2
  fi

  mv "$tmp" "$SERVER_BASE"
  chmod 644 "$SERVER_BASE"
fi

log "Converting server cloud image to standalone qcow2 (${DESKTOP_DISK_GIB}Gi)"
rm -f "$GOLDEN" "${GOLDEN}.part"
qemu-img convert -O qcow2 "$SERVER_BASE" "${GOLDEN}.part"
mv "${GOLDEN}.part" "$GOLDEN"
qemu-img resize "$GOLDEN" "${DESKTOP_DISK_GIB}G"

log "Expanding root partition to fill disk (growpart + resize2fs)"
virt-customize -a "$GOLDEN" \
  --run-command "growpart /dev/sda 1 2>/dev/null || true" \
  --run-command "resize2fs /dev/sda1 2>/dev/null || true"

log "Installing desktop environment (virt-customize — may take several minutes)"
virt-customize -a "$GOLDEN" \
  --install "ubuntu-desktop-minimal,cloud-init,spice-vdagent,dbus-x11,policykit-1" \
  --run-command "systemctl set-default graphical.target" \
  --run-command "systemctl enable gdm 2>/dev/null || systemctl enable gdm3 2>/dev/null || true" \
  --run-command "mkdir -p /etc/gdm3 && printf '%s\n' '[daemon]' 'AutomaticLogin=ubuntu' 'AutomaticLoginEnable=true' > /etc/gdm3/custom.conf" \
  --run-command "mkdir -p /etc/cloud/cloud.cfg.d && printf '%s\n' 'datasource_list: [ NoCloud, ConfigDrive, None ]' > /etc/cloud/cloud.cfg.d/99-machina-datasource.cfg" \
  --run-command "update-grub 2>/dev/null || true" \
  --run-command "grub-install /dev/sda 2>/dev/null || grub-install --target=i386-pc /dev/sda 2>/dev/null || true"

chmod 644 "$GOLDEN"
log "Done — golden desktop image: $GOLDEN"
qemu-img info "$GOLDEN" | head -8
