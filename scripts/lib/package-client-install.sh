#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

set -euo pipefail
# shellcheck source=/dev/null
[[ -f "$(dirname "$0")/package-ui.sh" ]] && source "$(dirname "$0")/package-ui.sh"
[[ -f "$(dirname "$0")/.package-lib/package-ui.sh" ]] && source "$(dirname "$0")/.package-lib/package-ui.sh"

pkg_banner "Machina host dependencies" "libvirt · qemu-kvm"
SUDO=""
[[ "$(id -u)" -ne 0 ]] && command -v sudo &>/dev/null && SUDO=sudo
if command -v dnf &>/dev/null; then
  $SUDO dnf install -y libvirt libvirt-devel qemu-kvm 2>&1 | tail -6 || true
  pkg_ok "dnf packages (libvirt/qemu)"
elif command -v apt-get &>/dev/null; then
  $SUDO apt-get update -qq
  qemu_pkg=qemu-system-x86
  [ "$(uname -m)" = "aarch64" ] && qemu_pkg=qemu-system-arm
  $SUDO apt-get install -y libvirt-daemon-system "$qemu_pkg" qemu-utils 2>&1 | tail -6 || true
  pkg_ok "apt packages (libvirt/qemu)"
else
  pkg_warn "Install libvirt and qemu-kvm manually"
fi
pkg_summary "Dependencies"
