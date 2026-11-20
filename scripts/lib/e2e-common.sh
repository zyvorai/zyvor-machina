# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# shellcheck shell=bash
# Shared helpers for Machina E2E tests (sourced by e2e-test.sh).

# shellcheck source=e2e-auth.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-auth.sh"

e2e_ok()   { echo "  ✅ $*"; (( E2E_PASS++ )) || true; }
e2e_fail() { echo "  ❌ $*"; (( E2E_FAIL++ )) || true; }
e2e_warn() { echo "  ⚠️  $*"; }
e2e_hdr()  { echo ""; echo "=== $* ==="; }

e2e_host_from_base() {
  echo "${E2E_BASE}" | sed -E 's#https?://([^/:]+).*#\1#'
}

e2e_init_cookie() {
  E2E_COOKIE="$(mktemp -t machina_e2e_cookie.XXXXXX)"
  export E2E_COOKIE
}

e2e_cleanup() {
  rm -f "${E2E_COOKIE:-}"
  echo ""
}

e2e_assert_json_key() {
  local resp="$1" key="$2" label="$3"
  if echo "$resp" | grep -q "\"${key}\""; then
    e2e_ok "$label"
  else
    e2e_fail "$label — got: $resp"
  fi
}

e2e_assert_http() {
  local got="$1" want="$2" label="$3"
  if [[ "$got" == "$want" ]]; then
    e2e_ok "$label (HTTP $got)"
  else
    e2e_fail "$label — expected HTTP $want, got $got"
  fi
}

e2e_assert_json_true() {
  local resp="$1" key="$2" label="$3"
  if echo "$resp" | grep -qE "\"${key}\"[[:space:]]*:[[:space:]]*true"; then
    e2e_ok "$label"
  else
    e2e_fail "$label — got: $resp"
  fi
}

e2e_login() {
  e2e_auth_resolve_mode
  local mode
  mode="$(e2e_auth_effective_mode)"
  e2e_hdr "LOGIN (${E2E_AUTH_MODE:-auto} → ${mode})"

  if e2e_auth_login_skipped; then
    e2e_warn "OIDC-only host — password login skipped (set E2E_AUTH_MODE=pam|ldap or disable OIDC-only)"
    e2e_fail "password login unavailable for auth mode oidc"
    return 1
  fi

  e2e_auth_apply_credentials
  if [[ -z "${E2E_LOGIN_USER:-}" || -z "${E2E_LOGIN_PASSWORD:-}" ]]; then
    e2e_fail "missing credentials for auth mode ${mode} (set E2E_USER/E2E_PASSWORD or E2E_LDAP_USER/E2E_LDAP_PASS)"
    return 1
  fi

  local r expected_source login_payload
  # Feed the JSON body (password included) to curl over stdin rather than as a -d
  # argument: -d "...password..." would put the plaintext password in this
  # process's argv, visible to any local user running `ps` while curl runs.
  login_payload="{\"username\":\"${E2E_LOGIN_USER}\",\"password\":\"${E2E_LOGIN_PASSWORD}\"}"
  r="$(printf '%s' "$login_payload" | ${E2E_CURL} -c "$E2E_COOKIE" -X POST "${E2E_BASE}/api/v1/auth/login" \
    -H "Content-Type: application/json" \
    --data-binary @-)"
  echo "  $r"
  e2e_assert_json_key "$r" "status" "login returns status"
  if ! echo "$r" | grep -q '"status":"ok"'; then
    local hint="login failed — check username/password"
    case "$mode" in
      ldap) hint="LDAP auth rejected — set E2E_LDAP_USER (UPN) and E2E_LDAP_PASS, or pass --auth ldap" ;;
      pam) hint="PAM auth rejected — set E2E_USER/E2E_PASSWORD, or pass --auth pam" ;;
      auto) hint="login failed — try --auth pam|ldap or set E2E_LDAP_* for Active Directory" ;;
    esac
    e2e_fail "$hint"
    return 1
  fi

  expected_source="$(e2e_auth_expected_source)"
  if [[ -n "$expected_source" ]]; then
    if echo "$r" | grep -q "\"auth_source\":\"${expected_source}\""; then
      e2e_ok "auth_source ${expected_source}"
    else
      e2e_fail "auth_source — expected ${expected_source}, got: $r"
      return 1
    fi
  elif echo "$r" | grep -q '"auth_source"'; then
    e2e_ok "auth_source present"
  fi
  return 0
}

e2e_summary() {
  echo ""
  echo "══════════════════════════════════════════"
  echo "  Results: ${E2E_PASS} passed, ${E2E_FAIL} failed"
  echo "══════════════════════════════════════════"
  if [[ "${E2E_FAIL}" -eq 0 ]]; then
    echo "✅ All tests passed"
    return 0
  fi
  echo "❌ ${E2E_FAIL} test(s) FAILED"
  return 1
}
