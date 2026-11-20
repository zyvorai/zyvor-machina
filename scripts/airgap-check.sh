#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0
#
# Air-gap evidence: list established TCP connections from Machina processes to public
# addresses. Read-only (ss). Exit 0 when every peer is private, loopback or link-local,
# 1 when a public peer is found, 2 on a usage or tooling error.
#
#   sudo ./scripts/airgap-check.sh [--allow ADDR]... [--procs REGEX]
set -euo pipefail

procs='machina-(daemon|controller|agent|bpfd)'
allow=()
while [ $# -gt 0 ]; do
  case "$1" in
    --allow) [ $# -ge 2 ] || { echo "--allow needs an address" >&2; exit 2; }; allow+=("$2"); shift 2 ;;
    --procs) [ $# -ge 2 ] || { echo "--procs needs a regex" >&2; exit 2; }; procs="$2"; shift 2 ;;
    -h|--help) sed -n 5,9p "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

command -v ss >/dev/null || { echo "ss (iproute2) is required" >&2; exit 2; }
command -v python3 >/dev/null || { echo "python3 is required" >&2; exit 2; }
[ "$(id -u)" -eq 0 ] || echo "note: not root, process names may be hidden and connections missed" >&2

ss -H -tnp state established 2>/dev/null | python3 -I -c '
import ipaddress, re, sys
procs = re.compile(sys.argv[1])
allow = set(sys.argv[2:])
bad = []
for line in sys.stdin:
    f = line.split()
    if len(f) < 5:
        continue
    # Recv-Q Send-Q Local:Port Peer:Port Process
    peer, who = f[3], " ".join(f[4:])
    m = procs.search(who)
    if not m:
        continue
    host = peer.rsplit(":", 1)[0].strip("[]").split("%")[0]
    try:
        ip = ipaddress.ip_address(host)
    except ValueError:
        continue
    if getattr(ip, "ipv4_mapped", None):
        ip = ip.ipv4_mapped
    if host in allow or str(ip) in allow:
        continue
    if ip.is_loopback or ip.is_link_local or ip.is_private:
        continue
    bad.append((peer, m.group(0)))
for peer, name in bad:
    print(f"PUBLIC {name} -> {peer}")
if bad:
    sys.exit(1)
print("ok: no Machina process has an established connection to a public address")
' "$procs" ${allow[@]+"${allow[@]}"}
