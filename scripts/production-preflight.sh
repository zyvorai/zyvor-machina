#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0
set -euo pipefail

# A deliberately strict, opt-in production profile. It is safe to run in CI or
# as systemd ExecStartPre. Nothing is mutated.
profile="${MACHINA_SECURITY_PROFILE:-development}"
[[ "$profile" == production ]] || exit 0

fail=0
bad() { echo "machina production preflight: $*" >&2; fail=1; }
need_file() { [[ -r "${!1:-}" ]] || bad "$1 must name a readable file"; }

[[ -n "${MACHINA_JWT_SECRET:-}" && ${#MACHINA_JWT_SECRET} -ge 32 ]] || bad "MACHINA_JWT_SECRET must be set and at least 32 characters"
[[ "${MACHINA_SKIP_AUTH:-0}" != 1 ]] || bad "MACHINA_SKIP_AUTH=1 is forbidden"
[[ "${MACHINA_ALLOW_DEV_SECRETS:-0}" != 1 ]] || bad "MACHINA_ALLOW_DEV_SECRETS=1 is forbidden"
master="${MACHINA_API_KEY_MASTER_KEY:-}"
if [[ -n "${MACHINA_API_KEY_MASTER_KEY_FILE:-}" ]]; then
  [[ -r "$MACHINA_API_KEY_MASTER_KEY_FILE" ]] || bad "MACHINA_API_KEY_MASTER_KEY_FILE must be readable"
  master="$(tr -d '[:space:]' <"$MACHINA_API_KEY_MASTER_KEY_FILE" 2>/dev/null || true)"
fi
[[ "$master" =~ ^[0-9a-fA-F]{64}$ ]] || bad "a 32-byte hex API-key master key is required (env or file)"
case "${MACHINA_BPF_REQUIRED:-true}" in 1|true|TRUE|yes) ;; *) bad "MACHINA_BPF_REQUIRED must be true" ;; esac

# Controller-to-agent mTLS. A production controller must not silently fall back
# to token/plaintext transport for already-enrolled nodes.
need_file MACHINA_AGENT_CA
need_file MACHINA_AGENT_CLIENT_CERT
need_file MACHINA_AGENT_CLIENT_KEY

if (( fail )); then
  exit 78 # EX_CONFIG
fi

echo "machina production preflight: OK"
