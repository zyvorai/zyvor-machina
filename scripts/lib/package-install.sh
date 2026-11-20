#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Machina — automatic client install (extracted tarball).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"
export PKG_INSTALL_ROOT="${ROOT}"
# shellcheck source=/dev/null
[[ -f "${ROOT}/.package-lib/package-ui.sh" ]] && source "${ROOT}/.package-lib/package-ui.sh"

pkg_parse_install_args "$@"

_PKG_SESSION_START=${SECONDS}
pkg_counters_reset
pkg_install_welcome "Machina"
pkg_bundle_sanity_check || true
pkg_banner "Machina" "libvirt / KVM hypervisor · client bundle"
pkg_step_init 6

pkg_step "Host dependencies (libvirt, QEMU, tools)"
if [[ -x ./install-client-deps.sh ]]; then
  pkg_sudo ./install-client-deps.sh && pkg_step_done || { pkg_warn "deps had issues"; pkg_step_done; }
else
  pkg_fail "install-client-deps.sh missing"
  exit 1
fi

pkg_step "Host preflight"
if [[ -x ./test-host.sh ]]; then
  ./test-host.sh || pkg_warn "test-host.sh — see HOST_SETUP.txt"
else
  pkg_skip "test-host.sh not bundled"
fi
pkg_step_done

pkg_step "Configuration"
pkg_sudo mkdir -p /etc/machina /var/lib/machina 2>/dev/null || true
if [[ -f machina.toml.example ]] && [[ ! -f /etc/machina/config.toml ]]; then
  pkg_sudo cp machina.toml.example /etc/machina/config.toml
  pkg_ok "/etc/machina/config.toml created"
else
  pkg_env_bootstrap machina.toml.example config.toml.local
fi
pkg_step_done

pkg_step "Verify bundle"
[[ -x ./machina-daemon ]] && pkg_ok "machina-daemon" || { pkg_fail "machina-daemon missing"; exit 1; }
[[ -d ./web/dist ]] && pkg_ok "web dashboard assets" || pkg_warn "web/dist missing"
pkg_step_done

pkg_step "Production install (systemd, TLS, firewall)"
pkg_maybe_run_full_install
pkg_step_done

pkg_step "Smoke test"
[[ -x ./test-package.sh ]] && ./test-package.sh || pkg_warn "test-package.sh issues"
pkg_step_done

pkg_install_finish "Machina" https 5092 "" \
  "Help: cat HELP.txt · ./install.sh --help" \
  "Service: sudo systemctl status machina-daemon" \
  "Logs: sudo journalctl -u machina-daemon -f"
