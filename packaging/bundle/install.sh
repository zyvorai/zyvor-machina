#!/bin/sh
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

#
# Offline installer for the Machina release bundle. No network, no compiler: it verifies the bundle, copies binaries
# to /usr/local, installs the systemd units, creates first-run secrets and starts the services. It never installs OS
# packages (the host may be air-gapped); it checks for what Machina needs and says exactly what is missing.
#
#   sudo ./install.sh [--all | --controller | --agent | --daemon] [--no-start] [--bind 0.0.0.0]
#   sudo ./install.sh --agent --join https://CTL:5094 --token join-... [--ca-sha256 FP] [--no-expose]
#       (--token-file F reads the token from a file, so it is not on the command line)
#   sudo ./install.sh --uninstall [--purge]
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"
ROLES=""; START=1; BIND=""; UNINSTALL=0; PURGE=0; JOIN=""; TOKEN=""; CA_FP=""; EXPOSE=1
while [ $# -gt 0 ]; do
    case "$1" in
        --all) ROLES="daemon controller agent" ;;
        --daemon) ROLES="$ROLES daemon" ;;
        --controller) ROLES="$ROLES controller" ;;
        --agent) ROLES="$ROLES agent" ;;
        --no-start) START=0 ;;
        --bind) BIND="$2"; shift ;;
        --uninstall) UNINSTALL=1 ;;
        --purge) PURGE=1 ;;
        --join) JOIN="$2"; shift ;;
        --token) TOKEN="$2"; shift ;;
        --token-file) TOKEN="$(cat "$2")" || exit 1; shift ;;
        --ca-sha256) CA_FP="$2"; shift ;;
        --no-expose) EXPOSE=0 ;;
        -h|--help) sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
    shift
done
[ -n "$ROLES" ] || ROLES="daemon controller agent"
if [ -n "$JOIN" ]; then
    [ -n "$TOKEN" ] || { echo "--join needs --token (or --token-file)" >&2; exit 2; }
    has_role_early() { case " $ROLES " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }
    has_role_early agent || { echo "--join needs the agent role (--agent or --all)" >&2; exit 2; }
fi
[ "$(id -u)" = 0 ] || { echo "run as root (sudo ./install.sh)" >&2; exit 1; }

say() { printf '  %s\n' "$*"; }
die() { echo "error: $*" >&2; exit 1; }
has_role() { case " $ROLES " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }
systemd_up() { [ -d /run/systemd/system ] && command -v systemctl >/dev/null 2>&1; }

if [ "$UNINSTALL" = 1 ]; then
    echo "Removing Machina"
    for u in machina-agent machina-bpfd machina-controller machina-daemon machina-backup.timer; do
        systemd_up && { systemctl stop "$u" 2>/dev/null || true; systemctl disable "$u" 2>/dev/null || true; }
    done
    rm -f /usr/local/bin/machina-daemon /usr/local/bin/machina-controller /usr/local/bin/machina-agent /usr/local/bin/machina-bpfd /usr/local/bin/machinactl
    rm -f /usr/lib/systemd/system/machina-*.service /usr/lib/systemd/system/machina-backup.timer
    rm -rf /usr/local/share/machina/web
    systemd_up && systemctl daemon-reload || true
    if [ "$PURGE" = 1 ]; then
        rm -rf /etc/machina /etc/default/machina-platform /etc/default/machina-daemon /var/lib/machina
        say "purged configuration and data"
    else
        say "kept /etc/machina, /etc/default/machina-* and /var/lib/machina (use --purge to delete them)"
    fi
    exit 0
fi

echo "Machina $(cat "$HERE/VERSION" 2>/dev/null || echo '?') — installing: $ROLES"

# 1. The bundle is intact.
if command -v sha256sum >/dev/null 2>&1; then
    ( cd "$HERE" && sha256sum -c MANIFEST.sha256 --quiet ) || die "bundle checksum mismatch — re-download or re-copy it"
    say "bundle verified"
fi

# 2. This machine can run it.
[ "$(uname -m)" = x86_64 ] || die "this bundle is for x86_64 (this machine is $(uname -m))"
missing=""
need() { command -v "$1" >/dev/null 2>&1 || missing="$missing $1"; }
if has_role daemon || has_role agent; then need virsh; need qemu-img; fi
[ -z "$missing" ] || die "missing on this machine:$missing — install libvirt and QEMU first (Debian/Ubuntu: apt install libvirt-daemon-system qemu-system-x86 qemu-utils; RHEL: dnf install libvirt-daemon-kvm qemu-img). Nothing was changed."
if ! "$HERE/bin/machina-daemon" --help >/dev/null 2>&1; then
    die "machina-daemon will not start here ($("$HERE/bin/machina-daemon" --help 2>&1 | head -1)). The bundle needs a newer glibc/libvirt than this OS provides; use the bundle built for your distro. Nothing was changed."
fi

# 3. Install.
mkdir -p /usr/local/bin /etc/machina /var/lib/machina /usr/lib/systemd/system
has_role daemon && { install -m 0755 "$HERE/bin/machina-daemon" "$HERE/bin/machinactl" /usr/local/bin/; mkdir -p /usr/local/share/machina/web; cp -R "$HERE/web/." /usr/local/share/machina/web/; }
has_role controller && install -m 0755 "$HERE/bin/machina-controller" /usr/local/bin/
has_role agent && install -m 0755 "$HERE/bin/machina-agent" "$HERE/bin/machina-bpfd" /usr/local/bin/
for r in daemon controller agent; do
    has_role $r || continue
    case $r in
        daemon) us="machina-daemon machina-backup" ;;
        controller) us="machina-controller" ;;
        agent) us="machina-agent machina-bpfd" ;;
    esac
    for u in $us; do install -m 0644 "$HERE/units/$u.service" /usr/lib/systemd/system/; done
done
has_role daemon && install -m 0644 "$HERE/units/machina-backup.timer" /usr/lib/systemd/system/
if has_role daemon; then
    [ -f /etc/machina/config.toml ] || install -m 0644 "$HERE/etc/machina.toml" /etc/machina/config.toml
    [ -f /etc/default/machina-daemon ] || install -m 0600 "$HERE/etc/machina-daemon.default" /etc/default/machina-daemon
    [ -z "$BIND" ] || { tmp=/etc/machina/config.toml.$$; sed "s/^host = .*/host = \"$BIND\"/" /etc/machina/config.toml >"$tmp" && cat "$tmp" >/etc/machina/config.toml; rm -f "$tmp"; say "daemon will listen on $BIND"; }
fi
# shellcheck disable=SC1091
. "$HERE/lib/bootstrap-secrets.sh"
has_role controller && machina_bootstrap_platform_env
say "files installed under /usr/local, units under /usr/lib/systemd/system"

# 4. Start.
if [ "$START" = 1 ] && systemd_up; then
    systemctl daemon-reload
    for u in machina-bpfd machina-controller machina-agent machina-daemon; do
        [ -f "/usr/lib/systemd/system/$u.service" ] || continue
        case "$u" in machina-bpfd|machina-agent) has_role agent || continue ;; machina-controller) has_role controller || continue ;; machina-daemon) has_role daemon || continue ;; esac
        systemctl enable --now "$u" >/dev/null 2>&1 && say "started $u" || say "could not start $u — see: journalctl -u $u"
    done
    has_role daemon && systemctl enable --now machina-backup.timer >/dev/null 2>&1 || true
fi

# 5. Join the fleet (the agent role, when asked): the same call install.sh from a controller makes.
if [ -n "$JOIN" ]; then
    echo
    say "joining $JOIN"
    set -- join --controller "$JOIN" --token "$TOKEN"
    [ -z "$CA_FP" ] || set -- "$@" --ca-sha256 "$CA_FP"
    [ "$EXPOSE" = 0 ] || set -- "$@" --expose
    /usr/local/bin/machina-agent "$@" || die "the agent could not join $JOIN (token single use: mint a new one)"
    say "joined; check on the controller: machinactl host list"
fi

echo
echo "Done."
has_role daemon && echo "  Web UI:   https://$(hostname -f 2>/dev/null || hostname):5092  (self-signed certificate until you add your own)"
[ -f /etc/machina/INITIAL_ADMIN_PASSWORD ] && echo "  Sign in:  cat /etc/machina/INITIAL_ADMIN_PASSWORD   (root only — change it after first login)"
[ -z "$JOIN" ] && has_role agent && ! has_role controller && echo "  Enrol this host: machina-agent join --controller URL --token TOKEN"
exit 0
