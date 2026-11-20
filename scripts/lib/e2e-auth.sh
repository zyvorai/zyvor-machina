# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# shellcheck shell=bash
# Auth mode helpers for Machina E2E (PAM, LDAP, OIDC, auto-detect).

# E2E_AUTH_MODE: pam | ldap | oidc | auto (default auto)
# PAM:     E2E_USER + E2E_PASSWORD (or positional args)
# LDAP:    E2E_LDAP_USER + E2E_LDAP_PASS (falls back to E2E_USER/E2E_PASSWORD)
# OIDC:    password login skipped — use browser SSO for UI tests

e2e_auth_normalize_mode() {
  local m="${1:-auto}"
  m="$(printf '%s' "$m" | tr '[:upper:]' '[:lower:]')"
  case "$m" in
    pam|ldap|oidc|auto) echo "$m" ;;
    *) echo "auto" ;;
  esac
}

e2e_auth_mode_requested() {
  e2e_auth_normalize_mode "${E2E_AUTH_MODE:-auto}"
}

e2e_auth_fetch_providers() {
  ${E2E_CURL} "${E2E_BASE}/api/v1/auth/providers" --max-time 15 2>/dev/null || true
}

e2e_auth_providers_enabled() {
  local providers="$1" key="$2"
  echo "$providers" | python3 -c "
import json, sys
try:
    o = json.load(sys.stdin)
    print('1' if (o.get('$key') or {}).get('enabled') else '0')
except Exception:
    print('0')
" 2>/dev/null || echo "0"
}

# Sets E2E_AUTH_EFFECTIVE and E2E_AUTH_PROVIDERS (exported for callers).
e2e_auth_resolve_mode() {
  E2E_AUTH_PROVIDERS="$(e2e_auth_fetch_providers)"
  export E2E_AUTH_PROVIDERS
  local requested
  requested="$(e2e_auth_mode_requested)"
  if [[ "$requested" != "auto" ]]; then
    E2E_AUTH_EFFECTIVE="$requested"
    export E2E_AUTH_EFFECTIVE
    return 0
  fi
  local pam_on ldap_on oidc_on
  pam_on="$(e2e_auth_providers_enabled "$E2E_AUTH_PROVIDERS" pam)"
  ldap_on="$(e2e_auth_providers_enabled "$E2E_AUTH_PROVIDERS" ldap)"
  oidc_on="$(e2e_auth_providers_enabled "$E2E_AUTH_PROVIDERS" oidc)"
  if [[ "$ldap_on" == "1" ]]; then
    if [[ -n "${E2E_LDAP_USER:-}" || "$E2E_USER" == *@* ]]; then
      E2E_AUTH_EFFECTIVE="ldap"
    elif [[ "$pam_on" == "1" ]]; then
      E2E_AUTH_EFFECTIVE="pam"
    else
      E2E_AUTH_EFFECTIVE="ldap"
    fi
  elif [[ "$pam_on" == "1" ]]; then
    E2E_AUTH_EFFECTIVE="pam"
  elif [[ "$oidc_on" == "1" ]]; then
    E2E_AUTH_EFFECTIVE="oidc"
  else
    E2E_AUTH_EFFECTIVE="pam"
  fi
  export E2E_AUTH_EFFECTIVE
}

e2e_auth_effective_mode() {
  if [[ -z "${E2E_AUTH_EFFECTIVE:-}" ]]; then
    e2e_auth_resolve_mode
  fi
  echo "${E2E_AUTH_EFFECTIVE}"
}

e2e_auth_apply_credentials() {
  local mode
  mode="$(e2e_auth_effective_mode)"
  case "$mode" in
    ldap)
      E2E_LOGIN_USER="${E2E_LDAP_USER:-$E2E_USER}"
      E2E_LOGIN_PASSWORD="${E2E_LDAP_PASS:-$E2E_PASSWORD}"
      ;;
    pam)
      E2E_LOGIN_USER="$E2E_USER"
      E2E_LOGIN_PASSWORD="$E2E_PASSWORD"
      ;;
    oidc)
      E2E_LOGIN_USER=""
      E2E_LOGIN_PASSWORD=""
      ;;
  esac
  export E2E_LOGIN_USER E2E_LOGIN_PASSWORD
}

e2e_auth_expected_source() {
  local mode
  mode="$(e2e_auth_effective_mode)"
  case "$mode" in
    pam|ldap) echo "$mode" ;;
    *) echo "" ;;
  esac
}

e2e_auth_login_skipped() {
  [[ "$(e2e_auth_effective_mode)" == "oidc" ]]
}

e2e_auth_banner_line() {
  local mode providers_hint
  mode="$(e2e_auth_effective_mode)"
  providers_hint=""
  if [[ "${E2E_AUTH_MODE:-auto}" == "auto" && -n "${E2E_AUTH_PROVIDERS:-}" ]]; then
    providers_hint=" (providers: $(echo "$E2E_AUTH_PROVIDERS" | tr -d '\n' | head -c 120))"
  fi
  echo "  Auth:       ${E2E_AUTH_MODE:-auto} → effective ${mode}${providers_hint}"
}

e2e_export_playwright_live_env() {
  local base="$1" user="$2" pass="$3"
  export PLAYWRIGHT_LIVE_URL="$base"
  export PLAYWRIGHT_LIVE_AUTH="${E2E_AUTH_MODE:-auto}"
  export PLAYWRIGHT_LIVE_USER="${E2E_USER:-$user}"
  export PLAYWRIGHT_LIVE_PASS="${E2E_PASSWORD:-$pass}"
  export E2E_AUTH_MODE="${E2E_AUTH_MODE:-auto}"
  if [[ -n "${E2E_LDAP_USER:-}" ]]; then
    export PLAYWRIGHT_LIVE_LDAP_USER="$E2E_LDAP_USER"
  fi
  if [[ -n "${E2E_LDAP_PASS:-}" ]]; then
    export PLAYWRIGHT_LIVE_LDAP_PASS="$E2E_LDAP_PASS"
  fi
}
