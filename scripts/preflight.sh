#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

#
# machina-preflight: can this machine run Machina? Read-only by default; every failed check says what to do.
#
#   preflight.sh [--role all|controller|daemon|agent] [--controller URL] [--json] [--fix [--yes]]
#
# --role        what the machine will run (default all). A node that only runs the agent needs KVM, libvirt and QEMU,
#               not the controller's ports.
# --controller  also test that this machine can reach the controller (GET <URL>/api/v1/health).
# --json        one JSON object on stdout instead of text (used by the installer and the web UI).
# --fix         install missing libvirt/QEMU with the distribution's package manager and start libvirt. Asks first
#               unless --yes. Nothing else is ever changed.
#
# Exit status: 0 = ready (warnings allowed), 1 = at least one FAIL, 2 = bad usage.
set -u

ROLE=all
CONTROLLER=""
JSON=0
FIX=0
YES=0
while [ $# -gt 0 ]; do
    case "$1" in
        --role) ROLE="${2:-}"; shift 2 ;;
        --controller) CONTROLLER="${2:-}"; shift 2 ;;
        --json) JSON=1; shift ;;
        --fix) FIX=1; shift ;;
        --yes|-y) YES=1; shift ;;
        -h|--help) sed -n '4,17p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done
case "$ROLE" in all|controller|daemon|agent) ;; *) echo "--role must be all, controller, daemon or agent" >&2; exit 2 ;; esac

wants() { [ "$ROLE" = all ] || [ "$ROLE" = "$1" ]; }
# the agent and the daemon both talk to libvirt
wants_virt() { wants agent || wants daemon; }

RESULTS=()   # status|name|detail|fix
add() { RESULTS+=("$1|$2|$3|${4:-}"); }
pass() { add PASS "$1" "$2"; }
warn() { add WARN "$1" "$2" "${3:-}"; }
fail() { add FAIL "$1" "$2" "${3:-}"; }

PM=""
for p in apt-get dnf zypper pacman; do command -v "$p" >/dev/null 2>&1 && { PM="$p"; break; }; done
ARCH="$(uname -m)"

pkg_hint() {
    case "$PM" in
        apt-get) case "$ARCH" in aarch64) q=qemu-system-arm ;; *) q=qemu-system-x86 ;; esac
                 echo "sudo apt-get update && sudo apt-get install -y libvirt-daemon-system libvirt-clients $q qemu-utils" ;;
        dnf)     echo "sudo dnf install -y libvirt libvirt-client qemu-kvm" ;;
        zypper)  echo "sudo zypper install -y libvirt qemu-kvm" ;;
        pacman)  echo "sudo pacman -S --needed libvirt qemu-base" ;;
        *)       echo "install libvirt and QEMU/KVM with your package manager" ;;
    esac
}

run_checks() {
# ── platform ────────────────────────────────────────────────────────────
if [ "$(uname -s)" != Linux ]; then
    fail "operating system" "$(uname -s) is not supported" "Machina's hypervisor side runs on Linux (build and run it on a Linux host)."
else
    . /etc/os-release 2>/dev/null || true
    pass "operating system" "${PRETTY_NAME:-Linux} ($ARCH)"
fi
if [ "$ARCH" != x86_64 ] && [ "$ARCH" != aarch64 ]; then
    warn "cpu architecture" "$ARCH is not x86_64 or aarch64" "Only x86_64 and aarch64 builds are published."
fi

if command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ]; then
    pass "systemd" "$(systemctl --version 2>/dev/null | head -1)"
else
    fail "systemd" "not running as init" "Machina installs systemd services; use a host (or VM) that boots with systemd."
fi

glibc="$(ldd --version 2>/dev/null | head -1 | grep -o '[0-9]*\.[0-9]*$')"
if [ -n "$glibc" ]; then
    gmaj="${glibc%%.*}"; gmin="${glibc##*.}"
    if [ "$gmaj" -gt 2 ] || { [ "$gmaj" -eq 2 ] && [ "$gmin" -ge 34 ]; }; then
        pass "glibc" "$glibc (needs 2.34+)"
    else
        fail "glibc" "$glibc is older than 2.34" "Use Ubuntu 22.04+, Debian 12+, RHEL/Rocky/Alma 9+, or build from source."
    fi
fi

# ── virtualisation (agent / daemon) ─────────────────────────────────────
if wants_virt; then
    if grep -qE '(vmx|svm)' /proc/cpuinfo 2>/dev/null || [ "$ARCH" = aarch64 ]; then
        pass "cpu virtualisation" "hardware virtualisation flags present"
    else
        fail "cpu virtualisation" "no vmx/svm flag in /proc/cpuinfo" "Enable VT-x/AMD-V in the BIOS, or turn on nested virtualisation for this VM (cloud: choose a bare-metal or nested-virt instance)."
    fi
    if [ -e /dev/kvm ]; then
        if [ -r /dev/kvm ] && [ -w /dev/kvm ] || [ "$(id -u)" = 0 ]; then
            pass "/dev/kvm" "present"
        else
            warn "/dev/kvm" "present but not accessible to $(id -un)" "Run as root, or add the user to the kvm group."
        fi
    else
        fail "/dev/kvm" "missing" "Load the KVM module (sudo modprobe kvm_intel or kvm_amd) and check virtualisation is enabled."
    fi

    if command -v virsh >/dev/null 2>&1; then
        pass "libvirt client" "$(virsh --version 2>/dev/null | head -1) (virsh)"
        if systemctl is-active --quiet libvirtd 2>/dev/null || systemctl is-active --quiet virtqemud 2>/dev/null \
            || systemctl is-active --quiet libvirtd.socket 2>/dev/null || systemctl is-active --quiet virtqemud.socket 2>/dev/null; then
            pass "libvirt service" "running"
        else
            fail "libvirt service" "not running" "sudo systemctl enable --now libvirtd   (on newer distributions: virtqemud.socket)"
        fi
    else
        fail "libvirt" "virsh not found" "$(pkg_hint)"
    fi

    qemu=""
    for b in qemu-kvm qemu-system-x86_64 qemu-system-aarch64; do
        p="$(command -v "$b" 2>/dev/null || true)"
        [ -z "$p" ] && for d in /usr/libexec /usr/bin /usr/local/bin; do [ -x "$d/$b" ] && { p="$d/$b"; break; }; done
        [ -n "$p" ] && { qemu="$p"; break; }
    done
    if [ -n "$qemu" ]; then
        pass "QEMU" "$("$qemu" --version 2>/dev/null | head -1)"
    else
        fail "QEMU" "no qemu-kvm / qemu-system-* found" "$(pkg_hint)"
    fi
    command -v qemu-img >/dev/null 2>&1 && pass "qemu-img" "found" || warn "qemu-img" "not found" "Needed for disk images: install qemu-utils (Debian/Ubuntu) or qemu-img (RHEL/Fedora)."
fi

# ── kernel features used by the eBPF data plane ─────────────────────────
if wants agent || wants all; then
    if [ -r /sys/kernel/btf/vmlinux ]; then
        pass "kernel BTF" "present"
    else
        warn "kernel BTF" "/sys/kernel/btf/vmlinux missing" "The eBPF features need a kernel built with BTF (CONFIG_DEBUG_INFO_BTF); VMs still run without them."
    fi
    if [ "$(stat -fc %T /sys/fs/cgroup 2>/dev/null)" = cgroup2fs ]; then
        pass "cgroup v2" "unified hierarchy"
    else
        warn "cgroup v2" "not the unified hierarchy" "Boot with systemd.unified_cgroup_hierarchy=1 for VM sandboxing and accounting."
    fi
fi

# ── resources ───────────────────────────────────────────────────────────
mem_kb="$(awk '/MemTotal/ {print $2}' /proc/meminfo 2>/dev/null)"
if [ -n "$mem_kb" ]; then
    mem_gb=$((mem_kb / 1024 / 1024))
    if [ "$mem_gb" -ge 4 ]; then pass "memory" "${mem_gb} GiB"; else warn "memory" "${mem_gb} GiB" "4 GiB is the practical minimum for the control plane; VMs need more."; fi
fi
for dir in /var/lib; do
    free_gb=$(( $(df -Pk "$dir" 2>/dev/null | awk 'NR==2 {print $4}') / 1024 / 1024 ))
    if [ "$free_gb" -ge 20 ]; then pass "disk $dir" "${free_gb} GiB free"
    elif [ "$free_gb" -ge 5 ]; then warn "disk $dir" "${free_gb} GiB free" "Keep 20 GiB or more for images and VM disks."
    else fail "disk $dir" "${free_gb} GiB free" "Free some space or mount a larger volume at /var/lib."; fi
done

# ── ports ───────────────────────────────────────────────────────────────
port_check() { # port label service
    local port="$1" label="$2" svc="$3" line who
    line="$(ss -ltnpH "sport = :$port" 2>/dev/null | head -1)"
    if [ -z "$line" ]; then
        pass "port $port" "free ($label)"
    elif echo "$line" | grep -q 'machina' || systemctl is-active --quiet "$svc" 2>/dev/null; then
        pass "port $port" "in use by Machina ($label)"
    else
        who="$(echo "$line" | grep -o 'users:(("[^"]*"' | head -1 | sed 's/users:(("//; s/"$//')"
        if [ -n "$who" ]; then
            fail "port $port" "already in use by $who" "Stop that service or change Machina's port ($label)."
        else
            warn "port $port" "in use by a program I cannot name (run as root to see which)" "If it is not Machina, stop it or change Machina's port ($label)."
        fi
    fi
}
if command -v ss >/dev/null 2>&1; then
    wants daemon && port_check 5092 "web UI and API" machina-daemon
    wants controller && port_check 5093 "controller" machina-controller
    wants agent && { port_check 50051 "agent" machina-agent; port_check 50052 "agent console" machina-agent; }
fi

# ── time and network ────────────────────────────────────────────────────
if command -v timedatectl >/dev/null 2>&1; then
    if [ "$(timedatectl show -p NTPSynchronized --value 2>/dev/null)" = yes ]; then
        pass "clock" "synchronised"
    else
        warn "clock" "not NTP-synchronised" "Certificates and tokens depend on the clock: sudo timedatectl set-ntp true"
    fi
fi
if command -v firewall-cmd >/dev/null 2>&1 && firewall-cmd --state >/dev/null 2>&1; then
    pass "firewall" "firewalld active (open 5092 for the UI; the installer can do it with --open-firewall)"
elif command -v ufw >/dev/null 2>&1 && ufw status 2>/dev/null | grep -q 'Status: active'; then
    pass "firewall" "ufw active (open 5092 for the UI; the installer can do it with --open-firewall)"
fi
if [ -n "$CONTROLLER" ]; then
    code="$(curl -ksS -m 8 -o /dev/null -w '%{http_code}' "${CONTROLLER%/}/api/v1/health" 2>/dev/null || true)"
    if [ "$code" = 200 ]; then
        pass "controller reachable" "$CONTROLLER answered"
    else
        fail "controller reachable" "$CONTROLLER did not answer (HTTP ${code:-none})" "Check the URL, that the controller listens on a network address (install with --bind), and the firewall between the machines."
    fi
fi

}
run_checks

# ── --fix: install libvirt + QEMU, start libvirt ────────────────────────
if [ "$FIX" = 1 ]; then
    missing=0
    for r in "${RESULTS[@]}"; do case "$r" in FAIL\|libvirt*|FAIL\|QEMU*) missing=1 ;; esac; done
    if [ "$missing" = 1 ] && [ "$PM" != "" ]; then
        cmd="$(pkg_hint)"; cmd="${cmd//sudo /}"
        if [ "$(id -u)" != 0 ]; then echo "--fix needs root" >&2; exit 2; fi
        go=0
        if [ "$YES" = 1 ]; then go=1; else
            printf 'Install libvirt and QEMU now?\n  %s\nProceed? [y/N] ' "$cmd" >&2; read -r a; case "$a" in y|Y|yes) go=1 ;; esac
        fi
        if [ "$go" = 1 ]; then
            sh -c "$cmd" >&2 && { systemctl enable --now libvirtd >&2 2>/dev/null || systemctl enable --now virtqemud.socket >&2 2>/dev/null || true; }
            RESULTS=()
            run_checks
        fi
    fi
fi

# ── report ──────────────────────────────────────────────────────────────
nfail=0; nwarn=0
for r in "${RESULTS[@]}"; do case "${r%%|*}" in FAIL) nfail=$((nfail+1)) ;; WARN) nwarn=$((nwarn+1)) ;; esac; done

if [ "$JSON" = 1 ]; then
    python3 - "$nfail" "$nwarn" "${RESULTS[@]}" <<'PY'
import json, sys
nfail, nwarn, rows = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3:]
checks = []
for r in rows:
    s, name, detail, fix = (r.split("|", 3) + [""])[:4]
    checks.append({"status": s, "name": name, "detail": detail, "fix": fix})
print(json.dumps({"ok": nfail == 0, "failed": nfail, "warnings": nwarn, "checks": checks}))
PY
else
    for r in "${RESULTS[@]}"; do
        IFS='|' read -r s name detail fix <<<"$r"
        case "$s" in PASS) mark="✅" ;; WARN) mark="⚠️ " ;; *) mark="❌" ;; esac
        printf '%s %-22s %s\n' "$mark" "$name" "$detail"
        [ -n "$fix" ] && [ "$s" != PASS ] && printf '     fix: %s\n' "$fix"
    done
    echo
    if [ "$nfail" = 0 ]; then
        echo "Ready. ${nwarn} warning(s)."
    else
        echo "Not ready: ${nfail} problem(s), ${nwarn} warning(s). Fix the ❌ lines above${PM:+, or run: sudo machina-preflight --fix}."
    fi
fi
[ "$nfail" = 0 ]
