#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

set -uo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"
export PKG_INSTALL_ROOT="${ROOT}"
# shellcheck source=/dev/null
[[ -f "${ROOT}/.package-lib/package-ui.sh" ]] && source "${ROOT}/.package-lib/package-ui.sh"

[[ "${1:-}" == "-h" || "${1:-}" == "--help" ]] && {
  pkg_script_help "test-host.sh"
  exit 0
}

_PKG_SESSION_START=${SECONDS}
pkg_counters_reset
pkg_banner "Machina host test" "KVM · libvirt · hypervisor prerequisites"

SUDO=""
[[ "$(id -u)" -ne 0 ]] && command -v sudo &>/dev/null && SUDO=sudo

if egrep -q '(vmx|svm)' /proc/cpuinfo 2>/dev/null; then
  pkg_ok "CPU virtualization (vmx/svm)"
else
  pkg_warn "No vmx/svm in /proc/cpuinfo"
fi

if command -v virsh &>/dev/null; then
  if virsh list --all >/dev/null 2>&1; then
    pkg_ok "virsh → libvirt"
  elif sudo virsh list --all >/dev/null 2>&1; then
    pkg_warn "virsh needs sudo (add user to libvirt group)"
  else
    pkg_fail "virsh cannot connect — sudo systemctl start libvirtd"
  fi
else
  pkg_fail "virsh not found"
fi

if systemctl is-active libvirtd &>/dev/null || systemctl is-active virtqemud &>/dev/null; then
  pkg_ok "libvirt service active"
else
  pkg_warn "libvirtd not active"
fi

if [[ -x "${ROOT}/machina-daemon" ]]; then
  pkg_ok "machina-daemon binary in bundle"
else
  pkg_fail "machina-daemon missing in bundle"
fi

if [[ -f /etc/machina/config.toml ]]; then
  pkg_ok "/etc/machina/config.toml"
elif [[ -f ./config.toml.local ]]; then
  pkg_ok "./config.toml.local"
else
  pkg_warn "No Machina config — copy machina.toml.example"
fi

pkg_summary "Host readiness"
[[ "${_PKG_COUNTERS_FAIL}" -eq 0 ]]
