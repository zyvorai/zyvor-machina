# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# shellcheck shell=bash
# Stop and disable host firewalls so Machina API/console ports are reachable remotely.

disable_firewalld() {
    local log_file="${1:-/dev/null}"
    local did=false

    {
        echo "WARNING: disabling the host firewall entirely (firewalld/ufw stop+disable+mask)."
        echo "  This opens ALL ports on this host, not just the Machina ports."
        echo "  Intended for lab / E2E hosts only — for production hosts prefer"
        echo "  --open-firewall, which opens only the specific port Machina needs."
    } | tee -a "$log_file" >&2

    if command -v systemctl &>/dev/null; then
        if systemctl list-unit-files 2>/dev/null | grep -q '^firewalld\.service'; then
            systemctl stop firewalld >>"$log_file" 2>&1 || true
            systemctl disable firewalld >>"$log_file" 2>&1 || true
            systemctl mask firewalld >>"$log_file" 2>&1 || true
            did=true
        fi
        if systemctl list-unit-files 2>/dev/null | grep -q '^ufw\.service'; then
            systemctl stop ufw >>"$log_file" 2>&1 || true
            systemctl disable ufw >>"$log_file" 2>&1 || true
            did=true
        fi
    fi

    if command -v ufw &>/dev/null; then
        ufw disable >>"$log_file" 2>&1 || true
        did=true
    fi

    if $did; then
        return 0
    fi
    return 1
}
