#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Machina node bootstrap, served by the controller at /install.sh.
#
#   sudo bash install.sh --controller https://CTL:5094 --token join-... [--ca-sha256 FP] [--no-deps] [--no-expose]
#
# What it does, in order: checks it is root, installs libvirt/QEMU if missing (unless --no-deps), fetches the agent
# from the controller (/dist, checked against SHA256SUMS over a channel pinned to the controller's CA), installs
# and starts it, joins the fleet (own agent token + certificate, agent reachable by the controller) and tells you
# what happened. Everything it changes is listed as it goes; it is safe to run again.
set -euo pipefail

CONTROLLER=""; TOKEN=""; CA_FP=""; DEPS=1; EXPOSE=1
while [ $# -gt 0 ]; do
  case "$1" in
    --controller) CONTROLLER="${2:-}"; shift 2 ;;
    --token) TOKEN="${2:-}"; shift 2 ;;
    --ca-sha256) CA_FP="${2:-}"; shift 2 ;;
    --no-deps) DEPS=0; shift ;;
    --no-expose) EXPOSE=0; shift ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
: "${CONTROLLER:?--controller URL is required}"
: "${TOKEN:?--token is required}"
CONTROLLER="${CONTROLLER%/}"

say()  { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
ok()   { printf '\033[1;32m ok\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31mfail\033[0m %s\n' "$*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || die "run as root (sudo bash install.sh ...)"
command -v curl >/dev/null || die "curl is required"
[ "$(uname -s)" = Linux ] || die "Linux only"

WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
CURL=(curl -fsS --max-time 120)

# ── 1. the controller, verified ─────────────────────────────────────────
if [ -n "$CA_FP" ]; then
  say "Verifying the controller by its CA fingerprint"
  "${CURL[@]}" -k "$CONTROLLER/api/v1/pki/ca.pem" -o "$WORK/ca.pem" || die "cannot reach $CONTROLLER"
  have="$(openssl x509 -in "$WORK/ca.pem" -noout -fingerprint -sha256 | cut -d= -f2 | tr -d ':' | tr 'A-F' 'a-f')"
  want="$(printf %s "$CA_FP" | tr -d ': ' | tr 'A-F' 'a-f')"
  [ "$have" = "$want" ] || die "the controller's CA ($have) is not the one in the command ($want): not the controller you meant, or a stale command"
  ok "controller CA matches the pinned fingerprint"
  CURL+=(--cacert "$WORK/ca.pem")
fi

# ── 2. libvirt and QEMU ─────────────────────────────────────────────────
have_virt() { command -v virsh >/dev/null && { command -v qemu-system-x86_64 >/dev/null || command -v qemu-kvm >/dev/null || ls /usr/libexec/qemu-kvm >/dev/null 2>&1 || command -v qemu-system-aarch64 >/dev/null; }; }
if ! have_virt; then
  [ "$DEPS" = 1 ] || die "libvirt/QEMU are missing and --no-deps was given"
  say "Installing libvirt and QEMU"
  if command -v apt-get >/dev/null; then
    case "$(uname -m)" in aarch64) q=qemu-system-arm ;; *) q=qemu-system-x86 ;; esac
    DEBIAN_FRONTEND=noninteractive apt-get update -qq
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq libvirt-daemon-system libvirt-clients "$q" qemu-utils
  elif command -v dnf >/dev/null; then dnf install -y -q libvirt libvirt-client qemu-kvm
  elif command -v zypper >/dev/null; then zypper -n install libvirt qemu-kvm
  else die "no supported package manager: install libvirt and QEMU, then run again with --no-deps"; fi
  systemctl enable --now libvirtd >/dev/null 2>&1 || systemctl enable --now virtqemud.socket >/dev/null 2>&1 || true
  ok "libvirt and QEMU installed"
else
  ok "libvirt and QEMU are present"
fi
[ -e /dev/kvm ] || echo "warning: /dev/kvm is missing: VMs will not run here (enable virtualisation)" >&2

# ── 3. the agent ────────────────────────────────────────────────────────
if ! command -v machina-agent >/dev/null; then
  say "Fetching the agent from the controller"
  "${CURL[@]}" "$CONTROLLER/dist/SHA256SUMS" -o "$WORK/SHA256SUMS" || die "the controller has nothing published at /dist (run: machinactl dist publish on the controller)"
  for f in machina-agent machina-bpfd machina-agent.service machina-bpfd.service; do
    "${CURL[@]}" "$CONTROLLER/dist/$f" -o "$WORK/$f" || die "download of $f failed"
    want="$(awk -v f="$f" '$2 == f {print $1}' "$WORK/SHA256SUMS")"
    [ -n "$want" ] || die "$f is not listed in SHA256SUMS"
    got="$(sha256sum "$WORK/$f" | cut -d' ' -f1)"
    [ "$got" = "$want" ] || die "checksum mismatch for $f"
  done
  install -m 0755 "$WORK/machina-agent" /usr/local/bin/machina-agent
  install -m 0755 "$WORK/machina-bpfd" /usr/local/bin/machina-bpfd
  # package units say /usr/bin; the binaries were just put in /usr/local/bin
  sed 's#/usr/bin/#/usr/local/bin/#g' "$WORK/machina-agent.service" >/etc/systemd/system/machina-agent.service
  sed 's#/usr/bin/#/usr/local/bin/#g' "$WORK/machina-bpfd.service" >/etc/systemd/system/machina-bpfd.service
  systemctl daemon-reload
  systemctl enable machina-bpfd.service machina-agent.service >/dev/null 2>&1 || true
  ok "agent installed from the controller (checksums verified)"
else
  ok "machina-agent is already installed ($(command -v machina-agent))"
fi
mkdir -p /etc/machina /var/lib/machina
[ -f /etc/default/machina-platform ] || { umask 077; : >/etc/default/machina-platform; }

# ── 4. join ─────────────────────────────────────────────────────────────
say "Joining the fleet"
ARGS=(join --controller "$CONTROLLER" --token "$TOKEN")
[ -n "$CA_FP" ] && ARGS+=(--ca-sha256 "$CA_FP")
[ "$EXPOSE" = 1 ] && ARGS+=(--expose)
systemctl start machina-bpfd.service >/dev/null 2>&1 || true
machina-agent "${ARGS[@]}" 2>&1 | sed -e 's/--token [^ ]*/--token <hidden>/' | tail -12
systemctl is-active --quiet machina-agent || systemctl start machina-agent
sleep 2
if systemctl is-active --quiet machina-agent; then
  ok "machina-agent is running; this node is joining ${CONTROLLER}"
  echo "    Watch it appear in the web UI (Hosts), or follow: journalctl -u machina-agent -f"
else
  die "machina-agent did not start: journalctl -u machina-agent -n 50"
fi
