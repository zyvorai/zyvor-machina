#!/bin/sh
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

#
# Idempotent first-install secrets for Machina. Sourced by the package scripts (and the offline bundle installer).
# Creates /etc/default/machina-platform (0600) with the controller's JWT secret, the controller↔agent token and a
# bootstrap admin password, and records the daemon's controller-proxy credential there too. Existing values are never changed, so
# upgrades and reinstalls keep every credential. POSIX sh only (dash-safe); needs no openssl.

PLATFORM_ENV="${MACHINA_PLATFORM_ENV:-/etc/default/machina-platform}"
INITIAL_PW_FILE="${MACHINA_INITIAL_PW_FILE:-/etc/machina/INITIAL_ADMIN_PASSWORD}"

_rand_hex() { # $1 = bytes
    head -c "$1" /dev/urandom | od -An -tx1 | tr -d ' \n'
}

_rand_pw() { # 24 URL-safe characters
    head -c 64 /dev/urandom | od -An -tx1 | tr -d ' \n' | head -c 24
}

# Appends KEY=VALUE only when KEY is not already set in FILE.
_ensure() { # file key value
    grep -q "^$2=" "$1" 2>/dev/null || printf '%s=%s\n' "$2" "$3" >>"$1"
}

machina_bootstrap_platform_env() {
    mkdir -p /etc/machina /etc/default
    [ -f "$PLATFORM_ENV" ] || (umask 077 && : >"$PLATFORM_ENV")
    chmod 600 "$PLATFORM_ENV"
    _ensure "$PLATFORM_ENV" MACHINA_JWT_SECRET "$(_rand_hex 48)"
    _ensure "$PLATFORM_ENV" MACHINA_AGENT_TOKEN "$(_rand_hex 32)"
    _ensure "$PLATFORM_ENV" MACHINA_CONTROLLER_ID "ctrl-$(hostname -s 2>/dev/null || echo primary)"
    if ! grep -q '^MACHINA_ADMIN_PASSWORD=' "$PLATFORM_ENV"; then
        pw="$(_rand_pw)"
        printf 'MACHINA_ADMIN_PASSWORD=%s\n' "$pw" >>"$PLATFORM_ENV"
        # Keep a root-only copy so the password is not lost if the install output scrolls away.
        (umask 077 && printf 'username: admin\npassword: %s\n' "$pw" >"$INITIAL_PW_FILE")
        echo "machina: first sign-in: run 'sudo machinactl show-login' (or read $INITIAL_PW_FILE, root only) — change the password after first login" >&2
    fi
    machina_sync_daemon_env
}

# The daemon proxies platform calls to the controller as 'admin'. Its unit reads /etc/default/machina-platform as well as its own
# file, so the credential is kept in the platform file, which belongs to no package (a package must not edit another package's
# conffile). Existing values are left alone; call this after the admin password exists.
machina_sync_daemon_env() {
    pw="$(sed -n 's/^MACHINA_ADMIN_PASSWORD=//p' "$PLATFORM_ENV" 2>/dev/null | head -1)"
    [ -n "$pw" ] || return 0
    _ensure "$PLATFORM_ENV" MACHINA_PLATFORM_AUTH "admin:$pw"
    _ensure "$PLATFORM_ENV" MACHINA_PLATFORM_CONTROLLER_URL "http://127.0.0.1:5093"
}
