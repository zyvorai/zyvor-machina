#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Emergency recovery after Zeus EmergencyIsolation lockdown blocked SSH/API.
# Run from the host console / IPMI / provider VNC (sudo is used throughout).
#
# Ubuntu typically has NO firewalld — prefer UFW, then iptables/nft.
set -euo pipefail

SUDO="${SUDO:-sudo}"
if [[ "$(id -u)" -eq 0 ]]; then
  SUDO=""
fi

run() { $SUDO "$@"; }

echo "== Zeus lockdown recovery (Ubuntu / no-firewalld) =="

# --- UFW (default on Ubuntu when present) ---
if command -v ufw >/dev/null 2>&1; then
  echo "UFW detected — disabling deny-all Emergency Isolation..."
  # Fastest restore: turn UFW off (Machina ports were often never allow-listed).
  run ufw --force disable || true
  run ufw default allow incoming || true
  run ufw default allow outgoing || true
  run ufw allow 22/tcp || true
  run ufw allow 5092/tcp || true
  run ufw allow 5093/tcp || true
  run ufw allow 50051/tcp || true
  run ufw status verbose || true
fi

# --- iptables (backend when UFW/firewalld absent) ---
if command -v iptables >/dev/null 2>&1; then
  echo "Resetting iptables INPUT/OUTPUT policies to ACCEPT..."
  run iptables -P INPUT ACCEPT || true
  run iptables -P FORWARD ACCEPT || true
  run iptables -P OUTPUT ACCEPT || true
  # Drop EmergencyIsolation ACCEPT-from-CIDR / DROP residue on INPUT.
  # Keep established so we do not brick a live console session mid-flush.
  run iptables -F INPUT 2>/dev/null || true
  run iptables -F OUTPUT 2>/dev/null || true
  run iptables -A INPUT -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT 2>/dev/null \
    || run iptables -A INPUT -m state --state ESTABLISHED,RELATED -j ACCEPT 2>/dev/null \
    || true
  run iptables -A INPUT -i lo -j ACCEPT 2>/dev/null || true
  run iptables -A INPUT -p tcp --dport 22 -j ACCEPT 2>/dev/null || true
  run iptables -A INPUT -p tcp --dport 5092 -j ACCEPT 2>/dev/null || true
  run iptables -A INPUT -p tcp --dport 5093 -j ACCEPT 2>/dev/null || true
  run iptables -A INPUT -p tcp --dport 50051 -j ACCEPT 2>/dev/null || true
  echo "iptables -L -n (head):"
  run iptables -L INPUT -n | head -20 || true
fi

# --- nftables ---
if command -v nft >/dev/null 2>&1; then
  if run nft list ruleset 2>/dev/null | grep -qE 'drop|reject|Emergency|zeus'; then
    echo "nftables has restrictive rules — flushing filter table (emergency)..."
    run nft flush table inet filter 2>/dev/null || run nft flush ruleset 2>/dev/null || true
  else
    echo "nftables present — inspect with: sudo nft list ruleset"
  fi
fi

# --- firewalld (RHEL/Fedora only; skip on Ubuntu) ---
if command -v firewall-cmd >/dev/null 2>&1 && run systemctl is-active --quiet firewalld 2>/dev/null; then
  echo "firewalld active — restoring public zone + Machina ports..."
  run firewall-cmd --set-default-zone=public || true
  run firewall-cmd --permanent --set-default-zone=public || true
  for p in 22 5092 5093 50051; do
    run firewall-cmd --add-port=${p}/tcp --permanent || true
    run firewall-cmd --add-port=${p}/tcp || true
  done
  run firewall-cmd --reload || true
fi

echo "Restarting Machina services..."
run systemctl restart machina-agent machina-controller machina-daemon 2>/dev/null || true
sleep 2
run systemctl is-active machina-daemon machina-controller machina-agent 2>/dev/null || true

echo "Listening ports:"
run ss -lntp 2>/dev/null | grep -E ':22|:5092|:5093|:50051' \
  || run netstat -lntp 2>/dev/null | grep -E ':22|:5092' \
  || true

echo "Done. From your laptop: curl -sk https://HOST:5092/ and ssh USER@HOST"
