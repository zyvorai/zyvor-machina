#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=/dev/null
[[ -f "${ROOT}/.package-lib/package-ui.sh" ]] && source "${ROOT}/.package-lib/package-ui.sh"
if [[ -f "${ROOT}/.package-lib/package-uninstall-lib.sh" ]]; then
  # shellcheck source=/dev/null
  source "${ROOT}/.package-lib/package-uninstall-lib.sh"
else
  # shellcheck source=package-uninstall-lib.sh
  source "$(dirname "$0")/package-uninstall-lib.sh"
fi

PRODUCT="Machina"
BINARIES=(machina-daemon machina)
PORTS=(5092)
LOCAL_CONFIGS=(config.toml.local)
SYSTEM_PATHS=(/etc/machina/config.toml)
SYSTEMD_UNITS=(machina-daemon.service)

package_uninstall_main "${PRODUCT}" "${ROOT}" "$@"
