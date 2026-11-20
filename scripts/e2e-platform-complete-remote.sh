#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Comprehensive platform E2E on a remote host: API + live browser (PAM on :5092).
#
# Usage:
#   VSPASS='…' ./scripts/e2e-platform-complete-remote.sh sus 175.110.114.93
#   E2E_INCLUDE_MOCK=1 VSPASS='…' ./scripts/e2e-platform-complete-remote.sh sus HOST
#
# Optional env:
#   E2E_AUTH_MODE          — pam | ldap | oidc | auto (default auto)
#   E2E_LDAP_USER/PASS     — UPN login when --auth ldap or LDAP enabled on host
#   E2E_INCLUDE_MOCK=1     — also run mocked Playwright (npm run test:e2e) locally
#   E2E_SKIP_FULL=1        — skip e2e-full-test-remote
#   E2E_SKIP_UX_FLOW=1     — skip e2e-platform-ux-flow-remote
#   E2E_SKIP_LIVE_UX=1     — skip live-ux-wiring manifest
#   E2E_SKIP_LIVE_SPECS=1  — skip live create/delete/platform-live specs
#   E2E_LIBVIRT_DESKTOP=1  — run libvirt ubuntu-desktop full E2E (GuestKit + VNC)
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

USER="${1:?usage: $0 USER HOST}"
HOST="${2:?HOST}"
PASS="${VSPASS:-${SSHPASS:-}}"
REPORT="${ROOT}/docs/e2e-last-run.json"

if [[ -z "$PASS" ]]; then
  read -rsp "Password for ${USER}@${HOST}: " PASS
  echo
fi

export VSPASS="$PASS"
export E2E_AUTH_MODE="${E2E_AUTH_MODE:-auto}"
export E2E_LDAP_USER="${E2E_LDAP_USER:-}"
export E2E_LDAP_PASS="${E2E_LDAP_PASS:-}"
export MACHINA_E2E_BYPASS_SECRET="${MACHINA_E2E_BYPASS_SECRET:-}"
export E2E_PLATFORM_USER="${E2E_PLATFORM_USER:-sus}"
export E2E_PLATFORM_PASS="${E2E_PLATFORM_PASS:-$PASS}"
BASE="https://${HOST}:5092"
STARTED="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

declare -a PHASE_NAMES=()
declare -a PHASE_STATUS=()

record_phase() {
  PHASE_NAMES+=("$1")
  PHASE_STATUS+=("$2")
}

run_phase() {
  local name="$1"
  shift
  echo ""
  echo "══════════════════════════════════════════"
  echo "  Phase: ${name}"
  echo "══════════════════════════════════════════"
  if "$@"; then
    record_phase "$name" "passed"
    echo "  ✅ ${name}"
    return 0
  fi
  record_phase "$name" "failed"
  echo "  ❌ ${name}"
  return 1
}

FAILED=0

# A: full API + daemon (KVM-only)
if [[ "${E2E_SKIP_FULL:-0}" != "1" ]]; then
  run_phase "full-api-daemon" \
    "${SCRIPT_DIR}/e2e-full-test-remote.sh" "$USER" "$HOST" \
    || FAILED=$((FAILED + 1))
fi

# B: UX API flow (wizard + delete cleanup)
if [[ "${E2E_SKIP_UX_FLOW:-0}" != "1" ]]; then
  run_phase "ux-api-flow" \
    "${SCRIPT_DIR}/e2e-platform-ux-flow-remote.sh" "$USER" "$HOST" \
    || FAILED=$((FAILED + 1))
fi

# C: live UX wiring manifest (~84 routes)
if [[ "${E2E_SKIP_LIVE_UX:-0}" != "1" ]]; then
  run_phase "live-ux-wiring" \
    env VSPASS="$PASS" "${SCRIPT_DIR}/e2e-live-ux-remote.sh" "$USER" "$HOST" \
    || FAILED=$((FAILED + 1))
fi

# D: targeted live Playwright (create, delete, routes, host)
if [[ "${E2E_SKIP_LIVE_SPECS:-0}" != "1" ]]; then
  # USER/PASS/E2E_LDAP_USER/E2E_LDAP_PASS are passed as real environment variables
  # (via `env`) rather than spliced as literal quoted text into the bash -c script:
  # a password containing a single quote would otherwise break out of the '...'
  # quoting below and inject arbitrary shell syntax into this locally-run command.
  run_phase "live-playwright-specs" env \
    _E2E_LIVE_USER="$USER" \
    _E2E_LIVE_PASS="$PASS" \
    _E2E_LIVE_LDAP_USER="${E2E_LDAP_USER:-}" \
    _E2E_LIVE_LDAP_PASS="${E2E_LDAP_PASS:-}" \
    bash -c "
    set -euo pipefail
    cd '${ROOT}/web'
    if ! npm run playwright -- install chromium >/dev/null 2>&1; then
      npm run test:e2e:install
    fi
    # shellcheck source=lib/e2e-auth.sh
    source '${SCRIPT_DIR}/lib/e2e-auth.sh'
    export E2E_USER=\"\$_E2E_LIVE_USER\"
    export E2E_PASSWORD=\"\$_E2E_LIVE_PASS\"
    export E2E_AUTH_MODE='${E2E_AUTH_MODE:-auto}'
    export E2E_LDAP_USER=\"\$_E2E_LIVE_LDAP_USER\"
    export E2E_LDAP_PASS=\"\$_E2E_LIVE_LDAP_PASS\"
    e2e_export_playwright_live_env '${BASE}' \"\$_E2E_LIVE_USER\" \"\$_E2E_LIVE_PASS\"
    npm run test:e2e -- --workers=1 --timeout=300000 \
      e2e/platform-live.spec.ts \
      e2e/platform-live-vm-create.spec.ts \
      e2e/platform-live-machine-finder-delete.spec.ts \
      e2e/platform-live-vm-delete.spec.ts \
      e2e/live-host.spec.ts
  " || FAILED=$((FAILED + 1))
fi

# E: libvirt desktop full E2E (GuestKit, lifecycle, SSH, VNC screenshots)
if [[ "${E2E_LIBVIRT_DESKTOP:-0}" == "1" ]]; then
  SSH_KEY="${E2E_SSH_KEY:-${HOME}/.ssh/id_ed25519}"
  run_phase "libvirt-desktop-e2e" \
    env VSPASS="$PASS" E2E_SSH_KEY="$SSH_KEY" \
    "${SCRIPT_DIR}/e2e-libvirt-desktop-full-remote.sh" "$USER" "$HOST" --ssh-key "$SSH_KEY" \
    || FAILED=$((FAILED + 1))
fi

# F: optional mocked CI suite (local preview)
if [[ "${E2E_INCLUDE_MOCK:-0}" == "1" ]]; then
  run_phase "mock-playwright-ci" bash -c "
    set -euo pipefail
    cd '${ROOT}/web'
    npm run test:e2e
  " || FAILED=$((FAILED + 1))
fi

# G: feature-by-feature matrix (mock build + live lab)
if [[ "${E2E_FEATURE_MATRIX:-0}" == "1" ]]; then
  run_phase "feature-matrix" \
    env VSPASS="$PASS" PLAYWRIGHT_LIBVIRT_VM_ID="${PLAYWRIGHT_LIBVIRT_VM_ID:-}" \
    "${SCRIPT_DIR}/e2e-feature-matrix-remote.sh" "$USER" "$HOST" \
    || FAILED=$((FAILED + 1))
fi

ENDED="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
UX_REPORT="${ROOT}/docs/ux-wiring-live-report.json"
UX_FAILED=""
if [[ -f "$UX_REPORT" ]]; then
  UX_FAILED="$(python3 -c "
import json
try:
    r=json.load(open('${UX_REPORT}'))
    print(r.get('failed', 0))
except Exception:
    print('')
" 2>/dev/null || true)"
fi

mkdir -p "$(dirname "$REPORT")"
python3 -c "
import json
names = '''${PHASE_NAMES[*]}'''.split()
statuses = '''${PHASE_STATUS[*]}'''.split()
phases = [{'name': n, 'status': s} for n, s in zip(names, statuses)]
doc = {
    'host': '${HOST}',
    'user': '${USER}',
    'base_url': '${BASE}',
    'started_at': '${STARTED}',
    'ended_at': '${ENDED}',
    'overall': 'passed' if ${FAILED} == 0 else 'failed',
    'phase_failures': ${FAILED},
    'phases': phases,
    'ux_wiring_report': '${UX_REPORT}',
    'ux_wiring_failed': '${UX_FAILED}' or None,
}
with open('${REPORT}', 'w') as f:
    json.dump(doc, f, indent=2)
print('Report:', '${REPORT}')
"

echo ""
echo "══════════════════════════════════════════"
echo "  Platform complete E2E"
echo "  Host:    ${HOST}"
echo "  Failed:  ${FAILED} phase(s)"
echo "  Report:  ${REPORT}"
echo "══════════════════════════════════════════"

[[ "$FAILED" -eq 0 ]]
