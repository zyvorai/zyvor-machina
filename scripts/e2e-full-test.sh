#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# e2e-full-test.sh — unified Machina E2E (daemon + platform proxy + controller lifecycle)
#
# Usage:
#   ./scripts/e2e-full-test.sh https://212.8.252.194:5092 sus PASS
#   ./scripts/e2e-full-test.sh --platform-only https://HOST:5092 sus
#   ./scripts/e2e-full-test.sh --skip-libvirt https://HOST:5092 sus
#
# Flags:
#   --skip-install-smoke   Skip systemd/curl install smoke (local only unless E2E_INSTALL_REMOTE=1)
#   --skip-daemon-e2e      Skip libvirt daemon E2E (e2e-test.sh)
#   --skip-host-health     Skip SMART/nbd checklist checks
#   --skip-ui-proxy        Skip daemon platform proxy tests
#   --skip-platform-smoke  Skip read-only controller smoke
#   --skip-platform-lifecycle  Skip VM create/snapshot lifecycle on controller
#   --platform-only        Skip daemon E2E + install smoke; run platform phases only
#   --skip-libvirt         Forwarded to e2e-test.sh
#   --skip-preflight       Forwarded to e2e-test.sh
#
# Env:
#   VSPASS / E2E_PASSWORD — PAM password for daemon login
#   E2E_AUTH_MODE — pam | ldap | oidc | auto (default auto)
#   E2E_LDAP_USER / E2E_LDAP_PASS — Active Directory UPN login
#   E2E_PLATFORM_USER/PASS — controller Basic auth (default admin/admin)
#   E2E_PLATFORM_DIRECT    — override direct controller URL (default http://HOST:5093)
#   E2E_INSTALL_REMOTE=1   — run install smoke via SSH to E2E_SSH_HOST
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=lib/e2e-common.sh
source "${SCRIPT_DIR}/lib/e2e-common.sh"
# shellcheck source=lib/e2e-platform-common.sh
source "${SCRIPT_DIR}/lib/e2e-platform-common.sh"
# shellcheck source=lib/e2e-host-health.sh
source "${SCRIPT_DIR}/lib/e2e-host-health.sh"
# shellcheck source=lib/e2e-ui-platform.sh
source "${SCRIPT_DIR}/lib/e2e-ui-platform.sh"
# shellcheck source=lib/e2e-platform-smoke.sh
source "${SCRIPT_DIR}/lib/e2e-platform-smoke.sh"
# shellcheck source=lib/e2e-platform.sh
source "${SCRIPT_DIR}/lib/e2e-platform.sh"

E2E_BASE="https://localhost:5092"
E2E_USER="sus"
E2E_PASSWORD="${VSPASS:-}"
E2E_CURL="curl -sk"
E2E_PASS=0
E2E_FAIL=0
E2E_SSH_HOST="${E2E_SSH_HOST:-}"

SKIP_INSTALL_SMOKE=0
SKIP_DAEMON_E2E=0
SKIP_HOST_HEALTH=0
SKIP_UI_PROXY=0
SKIP_PLATFORM_SMOKE=0
SKIP_PLATFORM_LIFECYCLE=0
PLATFORM_ONLY=0
DAEMON_EXTRA=()
E2E_AUTH_MODE="${E2E_AUTH_MODE:-auto}"

usage() {
  sed -n '2,28p' "$0" | sed 's/^# \{0,1\}//'
}

POSITIONAL=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    --skip-install-smoke) SKIP_INSTALL_SMOKE=1 ;;
    --skip-daemon-e2e) SKIP_DAEMON_E2E=1 ;;
    --skip-host-health) SKIP_HOST_HEALTH=1 ;;
    --skip-ui-proxy) SKIP_UI_PROXY=1 ;;
    --skip-platform-smoke) SKIP_PLATFORM_SMOKE=1 ;;
    --skip-platform-lifecycle) SKIP_PLATFORM_LIFECYCLE=1 ;;
    --platform-only) PLATFORM_ONLY=1; SKIP_DAEMON_E2E=1; SKIP_INSTALL_SMOKE=1 ;;
    --skip-libvirt|--skip-preflight|--libvirt-only)
      DAEMON_EXTRA+=("$1") ;;
    --ssh-host)
      DAEMON_EXTRA+=("$1" "${2:?}"); shift ;;
    --skip-dhcp-check) DAEMON_EXTRA+=("$1") ;;
    --auth) E2E_AUTH_MODE="${2:?}"; shift ;;
    --) shift; break ;;
    -*) echo "Unknown option: $1" >&2; usage >&2; exit 1 ;;
    *) POSITIONAL+=("$1") ;;
  esac
  shift
done
while [[ $# -gt 0 ]]; do POSITIONAL+=("$1"); shift; done

if [[ ${#POSITIONAL[@]} -ge 1 ]]; then E2E_BASE="${POSITIONAL[0]}"; fi
if [[ ${#POSITIONAL[@]} -ge 2 ]]; then E2E_USER="${POSITIONAL[1]}"; fi
if [[ ${#POSITIONAL[@]} -ge 3 ]]; then E2E_PASSWORD="${POSITIONAL[2]}"; fi

if [[ -z "${E2E_PASSWORD}" ]]; then
  read -rsp "Password for ${E2E_USER}@${E2E_BASE}: " E2E_PASSWORD
  echo
fi
export E2E_PASSWORD
export E2E_AUTH_MODE

[[ -z "${E2E_SSH_HOST}" ]] && E2E_SSH_HOST="$(e2e_host_from_base)"
E2E_PLATFORM_BASE="${E2E_PLATFORM_DIRECT:-http://${E2E_SSH_HOST}:5093}"
E2E_PLATFORM_BASE="${E2E_PLATFORM_BASE%/}"

PHASE_FAIL=0
phase_ok()   { echo "  ✅ phase: $*"; }
phase_fail() { echo "  ❌ phase FAILED: $*"; PHASE_FAIL=$((PHASE_FAIL + 1)); }

e2e_init_cookie
trap e2e_cleanup EXIT

echo "══════════════════════════════════════════"
echo "  Machina Full E2E"
echo "  Daemon:     ${E2E_BASE} (${E2E_USER})"
echo "  Controller: ${E2E_PLATFORM_BASE}"
e2e_auth_banner_line
echo "══════════════════════════════════════════"

# Phase 1: install smoke
if [[ "$SKIP_INSTALL_SMOKE" -eq 0 ]]; then
  e2e_hdr "PHASE 1: PLATFORM INSTALL SMOKE"
  if [[ "${E2E_INSTALL_REMOTE:-0}" == "1" ]]; then
    if "${SCRIPT_DIR}/e2e-platform-install-smoke-remote.sh" "${E2E_USER}" "${E2E_SSH_HOST}"; then
      phase_ok "install smoke (remote)"
    else
      phase_fail "install smoke (remote)"
    fi
  elif [[ -f /usr/local/bin/machina-controller ]] || systemctl list-unit-files machina-controller.service &>/dev/null; then
    if "${SCRIPT_DIR}/e2e-platform-install-smoke.sh"; then
      phase_ok "install smoke (local)"
    else
      phase_fail "install smoke (local)"
    fi
  else
    e2e_warn "platform not installed locally — skip install smoke (set E2E_INSTALL_REMOTE=1 for remote)"
  fi
fi

# Phase 2: daemon E2E
if [[ "$SKIP_DAEMON_E2E" -eq 0 ]]; then
  e2e_hdr "PHASE 2: DAEMON E2E"
  # Password goes via VSPASS (env), not a positional arg — CLI args are
  # world-readable via `ps` on shared hosts, env vars set this way are not.
  if env VSPASS="$E2E_PASSWORD" E2E_SSH_HOST="$E2E_SSH_HOST" E2E_AUTH_MODE="$E2E_AUTH_MODE" \
    E2E_LDAP_USER="${E2E_LDAP_USER:-}" E2E_LDAP_PASS="${E2E_LDAP_PASS:-}" \
    "${SCRIPT_DIR}/e2e-test.sh" "$E2E_BASE" "$E2E_USER" \
    --ssh-host "$E2E_SSH_HOST" --auth "$E2E_AUTH_MODE" "${DAEMON_EXTRA[@]}"; then
    phase_ok "daemon E2E"
  else
    phase_fail "daemon E2E"
  fi
else
  e2e_hdr "PHASE 2: DAEMON E2E (skipped)"
  if ! e2e_login; then
    phase_fail "daemon login (required for later phases)"
  fi
fi

# Phase 3: host health (when daemon E2E skipped — otherwise covered by e2e-test.sh)
if [[ "$SKIP_HOST_HEALTH" -eq 0 && "$SKIP_DAEMON_E2E" -eq 1 ]]; then
  e2e_hdr "PHASE 3: HOST HEALTH"
  e2e_host_health_run || true
  if [[ "$E2E_FAIL" -gt 0 ]]; then
    phase_fail "host health"
  else
    phase_ok "host health"
  fi
fi

# Phase 4: UI platform proxy
if [[ "$SKIP_UI_PROXY" -eq 0 ]]; then
  e2e_hdr "PHASE 4: UI PLATFORM PROXY"
  if ! e2e_login; then
    phase_fail "UI platform proxy login"
  else
    local_before_fail=$E2E_FAIL
    e2e_ui_platform_run || true
    if [[ "$E2E_FAIL" -gt "$local_before_fail" ]]; then
      phase_fail "UI platform proxy"
    else
      phase_ok "UI platform proxy"
    fi
  fi
fi

# Phase 5: platform controller
PLATFORM_FAIL_BASE=$E2E_FAIL
PLATFORM_PASS_BASE=$E2E_PASS
if [[ "$SKIP_PLATFORM_SMOKE" -eq 0 || "$SKIP_PLATFORM_LIFECYCLE" -eq 0 ]]; then
  e2e_hdr "PHASE 5: PLATFORM CONTROLLER"
fi

if [[ "$SKIP_PLATFORM_SMOKE" -eq 0 ]]; then
  echo ""
  echo "--- platform smoke (read-only) ---"
  e2e_platform_smoke_run || true
fi

if [[ "$SKIP_PLATFORM_LIFECYCLE" -eq 0 ]]; then
  echo ""
  echo "--- platform lifecycle ---"
  e2e_platform_run || true
fi

if [[ "$SKIP_PLATFORM_SMOKE" -eq 0 || "$SKIP_PLATFORM_LIFECYCLE" -eq 0 ]]; then
  platform_new_fail=$((E2E_FAIL - PLATFORM_FAIL_BASE))
  platform_new_pass=$((E2E_PASS - PLATFORM_PASS_BASE))
  echo ""
  echo "══════════════════════════════════════════"
  echo "  Platform E2E: ${platform_new_pass} passed, ${platform_new_fail} failed"
  echo "══════════════════════════════════════════"
  if [[ "$platform_new_fail" -gt 0 ]]; then
    echo "❌ ${platform_new_fail} platform test(s) FAILED"
    phase_fail "platform controller"
  else
    echo "✅ All platform tests passed"
    phase_ok "platform controller"
  fi
fi

echo ""
echo "══════════════════════════════════════════"
echo "  Full E2E summary"
echo "  Daemon checks:    ${E2E_PASS} passed, ${E2E_FAIL} failed"
echo "  Phase failures:   ${PHASE_FAIL}"
echo "══════════════════════════════════════════"

if [[ "$PHASE_FAIL" -eq 0 && "$E2E_FAIL" -eq 0 ]]; then
  echo "✅ Full E2E passed"
  exit 0
fi
echo "❌ Full E2E FAILED"
exit 1
