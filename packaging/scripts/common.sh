# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Shared helpers for package maintainer scripts (POSIX sh).
have_systemd() { [ -d /run/systemd/system ] && command -v systemctl >/dev/null 2>&1; }

# Reload units and (re)start the given services unless the admin opted out or systemd is not running (containers, chroots).
machina_services() { # enable-and-start units...
    have_systemd || return 0
    systemctl daemon-reload || true
    for u in "$@"; do
        systemctl enable "$u" >/dev/null 2>&1 || true
        [ "${MACHINA_NO_START:-0}" = 1 ] || systemctl try-restart "$u" >/dev/null 2>&1 || systemctl start "$u" >/dev/null 2>&1 || true
    done
}
