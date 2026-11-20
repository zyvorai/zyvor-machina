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
  pkg_script_help "test-package.sh"
  exit 0
}

_PKG_SESSION_START=${SECONDS}
pkg_counters_reset
pkg_banner "Machina package test" "Daemon · dashboard assets"

[[ -x ./machina-daemon ]] && pkg_ok "machina-daemon" || pkg_fail "machina-daemon"
./machina-daemon --help 2>&1 | head -3 | while read -r l; do pkg_detail "${l}"; done || true

if [[ -x ./test-host.sh ]]; then
  ./test-host.sh || pkg_warn "test-host.sh — see HOST_SETUP.txt"
fi

_lan=$(pkg_access_url https 5092)
if curl -skf https://127.0.0.1:5092/health >/dev/null 2>&1; then
  pkg_ok "daemon health https://127.0.0.1:5092"
  pkg_detail "LAN URL: ${_lan} ($(pkg_primary_host_label))"
else
  pkg_skip "daemon not running (start machina-daemon after config)"
  pkg_detail "When running: ${_lan}"
fi

pkg_summary "Package test"
[[ "${_PKG_COUNTERS_FAIL}" -eq 0 ]]
