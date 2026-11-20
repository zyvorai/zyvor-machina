#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# scripts/deploy-remote.sh — rsync sources to remote, compile & install ONLY on remote
#
# Nothing is built on your laptop: install.sh runs cargo/npm on the SSH host (--quick uses
# make release web there). Locals only need rsync + ssh (no Rust/Node locally).
set -euo pipefail

# Indexed array required before REST+= / "${REST[@]}" under `set -u` (bash 5.x empty-array quirk).
declare -a REST=()

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=lib/deploy-common.sh
source "$SCRIPT_DIR/lib/deploy-common.sh"

SSH_PORT="${SSH_PORT:-22}"
SSH_KEY=""
HEALTH_URL="${HEALTH_URL:-https://127.0.0.1:5092/api/v1/health}"
STRICT="${STRICT:-0}"
# Default matches VM-style layout: rsync here → build on server → install to /usr/local + systemd
REMOTE_DIR="${REMOTE_DIR:-~/.deployment/machina}"
# Limit parallel rustc link jobs on memory-tight hypervisors (k3s + other cloud workloads + Machina).
REMOTE_CARGO_BUILD_JOBS="${REMOTE_CARGO_BUILD_JOBS:-1}"

info() { deploy_ui_info_b "$@"; }
ok()   { deploy_ui_info "$@"; }
warn() { deploy_ui_warn "$@"; }
die()  { deploy_ui_error "$@"; }
hr()   { deploy_ui_hr; }
phase() { deploy_ui_phase "$@"; }
tip()  { deploy_ui_note "$@"; }

banner_deploy() {
    local host="$1" user="$2" rdir="$3" mode="$4"
    local ver="${MACHINA_VERSION:-dev}" commit="${MACHINA_COMMIT:-?}"
    deploy_ui_banner "Remote deploy → ${user}@${host}" "${ver} · ${commit}"
    deploy_ui_kv "🎯" "SSH target" "${user}@${host}"
    deploy_ui_kv "📁" "Remote tree" "$rdir"
    deploy_ui_kv "📋" "Plan" "$mode"
    deploy_ui_kv "💚" "Health" "${HEALTH_URL}"
    deploy_ui_note "Build runs on the server (sources rsync'd — not compiled locally)"
}

elapsed_fmt() { machina_elapsed_fmt "$1"; }

# Keepalives for long remote cargo/npm/install.sh runs; ControlMaster=no avoids stale sockets.
DEPLOY_SSH_OPTS=(
    -o StrictHostKeyChecking=no
    -o ConnectTimeout=30
    -o ServerAliveInterval=15
    -o ServerAliveCountMax=120
    -o TCPKeepAlive=yes
    -o ControlMaster=no
    -p "$SSH_PORT"
)
SSH_OPTS=("${DEPLOY_SSH_OPTS[@]}")
RSYNC_RSH="ssh ${SSH_OPTS[*]}"
# Non-root: TTY for sudo password; cleared when passwordless sudo works.
DEPLOY_SSH_TTY_OPTS=()

usage() {
    cat <<'EOF'
deploy-remote.sh USER@HOST | USER HOST [PASSWORD] [--sync-only|--quick|--install-only|--bins-only|--e2e|--e2e-libvirt-desktop|--platform|--platform-bind ADDR|--cleanup|--prune-sources|--dry-run]

--platform-bind ADDR  Bind controller/agent to ADDR (default 127.0.0.1 behind the
                      daemon proxy; use 0.0.0.0 for multi-host fleets). The daemon
                      --bind is intentionally NOT forwarded to the platform.
        [--skip-platform-e2e|--skip-daemon-e2e]
        [--e2e-auth pam|ldap|oidc|auto]
        [--remote-build|--remote-check] [--bind ADDR] [--open-firewall|--disable-firewalld]
        [--no-start] [--deps-only] [extra install.sh args...]

Prefer: ./scripts/deploy remote USER@HOST [flags]  |  ./scripts/deploy status

deploy-remote.sh check [USER@HOST | USER HOST]

Flow: rsync → ~/.deployment/machina (REMOTE_DIR) → build on server → install → systemd.
Full install: install.sh enables + restarts the daemon (--no-start skips). Post-install curl/API verification is skipped on the remote (--no-tests). install.sh also ensures mkosi (v16+): distro package if recent, else pipx from GitHub, else optional git clone (MACHINA_MKOSI_FROM_CLONE=1), else /opt/mkosi-venv; host build tools (bubblewrap, dosfstools, …) best-effort. Default disk workflow in the Create VM UI.
Quick: make release web (incremental cargo) then install.sh --skip-build + try-restart — one compile pass, not two.
Install-only (--install-only): rsync + install.sh --skip-build + restart — no cargo/npm (~1–2 min). Requires prior build on the host (target/ is kept across rsyncs).
Prune (--prune-sources): after install, remove deploy-tree sources; keep target/, web/dist/, web/node_modules/ for the next incremental build.
Memory-tight hosts: swap + optional nginx disable via scripts/host-tune-memory.sh; remote cargo uses CARGO_BUILD_JOBS=${REMOTE_CARGO_BUILD_JOBS:-1} (override with REMOTE_CARGO_BUILD_JOBS=4).
Open the UI at https://HOST:5092 (install.sh generates a self-signed cert; replace with your CA for browsers).

--remote-build   After rsync+chown, run `make release` on the SSH host only (no sudo install.sh).
                 Use this to surface Rust / pam-sys / Axum compile errors quickly. Requires Rust + build deps on the server (e.g. after `sudo install.sh --deps-only` once).
--remote-check   Same but `make check` (faster compile check).

Auth: SSH keys/agent by default; optional PASSWORD arg or SSHPASS env → sshpass.
        --ssh-key PATH   Use this private key for rsync/ssh (IdentityFile; IdentitiesOnly=yes).

Examples:
  deploy-remote.sh sus@185.165.240.5 --bind 0.0.0.0 --open-firewall
  deploy-remote.sh sus 185.165.240.5 --quick
  deploy-remote.sh 185.165.240.5 sus --quick    # HOST USER (auto-swapped)
  VSPASS=max deploy-remote.sh sus 185.165.240.5 --quick --e2e --platform
  VSPASS=max deploy-remote.sh sus 212.8.252.194 --platform --e2e --bind 0.0.0.0
  VSPASS=max deploy-remote.sh sus 212.8.252.194 --platform --e2e --e2e-auth ldap --bind 0.0.0.0 --disable-firewalld
  deploy-remote.sh sus 212.8.252.194 --install-only --platform --prune-sources
  deploy-remote.sh sus@host --remote-check    # fast compile smoke after rsync
  deploy-remote.sh sus@host --remote-build   # full release build on server, then exit
  # Full install passes --no-tests to install.sh (no post-install curl suite on the server).
  (USER then HOST is canonical; HOST USER is swapped when HOST is an IPv4 address.)
  SYNC_ONLY=1 deploy-remote.sh sus@host
  deploy-remote.sh check    deploy-remote.sh check sus@host

Env: DEPLOY_HOST DEPLOY_USER SSH_PORT SSHPASS REMOTE_DIR HEALTH_URL STRICT SYNC_ONLY
     E2E_AUTH_MODE E2E_LDAP_USER E2E_LDAP_PASS (post-deploy --e2e login; default auto)

After each rsync, the script runs sudo chown on the deploy tree so interrupted
sudo builds cannot leave root-owned target/ (cargo EACCES on --quick).

Output uses ANSI colors when stdout is a TTY. Set NO_COLOR=1 to disable.
EOF
    exit 0
}

[[ "${1:-}" == -h || "${1:-}" == --help ]] && usage

machina_build_metadata "$REPO"

# Reuse last target: ./scripts/deploy remote --quick
if [[ $# -gt 0 && "${1:-}" == -* ]] && machina_load_deploy_last "$REPO"; then
    set -- "${USER}@${HOST}" "$@"
    ok "Using .deploy-last → ${USER}@${HOST}"
fi

# Run a remote script under bash (not login zsh — avoids nomatch on globs during make/install).
ssh_r_bash() {
    local remote="$1"
    shift
    local -a ssh_args=("${SSH_OPTS[@]}")
    ((${#DEPLOY_SSH_TTY_OPTS[@]})) && ssh_args+=("${DEPLOY_SSH_TTY_OPTS[@]}")
    if [[ -n "${SSHPASS:-}" ]] && command -v sshpass &>/dev/null; then
        printf '%s\n' "$@" | SSHPASS="$SSHPASS" sshpass -e ssh "${ssh_args[@]}" "$remote" "exec bash -s"
    else
        printf '%s\n' "$@" | ssh "${ssh_args[@]}" "$remote" "exec bash -s"
    fi
}

ssh_r() {
    # Two-arg one-liner → bash -s on remote. Multi-arg / explicit bash -s / heredoc → raw ssh.
    if [[ $# -eq 2 && "$2" != bash && "$2" != "bash -s" && "$2" != "exec bash -s" ]]; then
        ssh_r_bash "$1" "$2"
        return
    fi
    local -a ssh_args=("${SSH_OPTS[@]}")
    if [[ -n "${SSHPASS:-}" ]] && command -v sshpass &>/dev/null; then
        SSHPASS="$SSHPASS" sshpass -e ssh "${ssh_args[@]}" "$@"
    else
        ssh "${ssh_args[@]}" "$@"
    fi
}

# Checksum compare without preserving mtimes: a changed file lands with the
# current time, so cargo (and build.rs rerun-if-changed) never mistakes a
# source edited before the last remote build for an up-to-date one.
rsync_r() {
    if [[ -n "${SSHPASS:-}" ]] && command -v sshpass &>/dev/null; then
        SSHPASS="$SSHPASS" sshpass -e rsync -rlpcz --delete -e "$RSYNC_RSH" "$@"
    else
        rsync -rlpcz --delete -e "$RSYNC_RSH" "$@"
    fi
}

check_body() {
    EXIT_CODE=0
    deploy_ui_highlight "🩺 Local machina health snapshot"
    hr
    unit_line() {
        local n="$1" a e
        a=$(systemctl is-active "$n" 2>/dev/null) || a="unknown"
        e=$(systemctl is-enabled "$n" 2>/dev/null) || e="unknown"
        printf '  %-28s active=%-12s enabled=%s\n' "$n" "$a" "$e"
    }
    if ! command -v systemctl &>/dev/null; then
        warn "systemctl not found — skip unit checks"
    else
        deploy_ui_highlight "⚙️  Systemd units"
        unit_line libvirtd.service
        unit_line machina-daemon.service
        unit_line machina-backup.timer
        local ad al
        ad=$(systemctl is-active machina-daemon 2>/dev/null || true)
        al=$(systemctl is-active libvirtd 2>/dev/null || true)
        [[ "$al" == active ]] || { warn "libvirtd not active"; [[ "$STRICT" == 1 ]] && EXIT_CODE=1; }
        [[ "$al" == active ]] && ok "libvirtd active"
        [[ "$ad" == active ]] || { warn "machina-daemon not active"; [[ "$STRICT" == 1 ]] && EXIT_CODE=1; }
        [[ "$ad" == active ]] && ok "machina-daemon active"
    fi
    if command -v systemctl &>/dev/null; then
        deploy_ui_highlight "📋 machina-daemon — systemctl status"
        systemctl status machina-daemon --no-pager 2>/dev/null || warn "cannot read machina-daemon status"
        deploy_ui_highlight "📋 libvirtd — systemctl status"
        systemctl status libvirtd --no-pager 2>/dev/null || warn "cannot read libvirtd status"
    fi
    deploy_ui_highlight "💚 API health — $HEALTH_URL"
    if command -v curl &>/dev/null; then
        curl -sfk --connect-timeout 3 "$HEALTH_URL" >/dev/null 2>&1 && ok "GET $HEALTH_URL" || {
            warn "cannot reach $HEALTH_URL"; [[ "$STRICT" == 1 ]] && EXIT_CODE=1; }
    else
        warn "curl missing — skip HTTP check"
    fi
    printf '\n'
    exit "$EXIT_CODE"
}

check_remote() {
    local r="$1"
    deploy_ui_highlight "🩺 Remote health check"
    hr
    info "SSH target → $r"
    ssh_r "$r" env STRICT="$STRICT" HEALTH_URL="$HEALTH_URL" bash -s <<'EOS'
run() {
    EXIT_CODE=0
    unit_line() {
        local n="$1" a e
        a=$(systemctl is-active "$n" 2>/dev/null) || a="unknown"
        e=$(systemctl is-enabled "$n" 2>/dev/null) || e="unknown"
        printf '  %-28s active=%-12s enabled=%s\n' "$n" "$a" "$e"
    }
    info() { printf 'ℹ️  %s\n' "$*"; }
    ok()   { printf '✅ %s\n' "$*"; }
    warn() { printf '⚠️  %s\n' "$*"; }
    if ! command -v systemctl &>/dev/null; then warn "no systemctl"; exit 1; fi
    printf '\n⚙️  Systemd units\n'
    unit_line libvirtd.service
    unit_line machina-daemon.service
    unit_line machina-backup.timer
    local ad al
    ad=$(systemctl is-active machina-daemon 2>/dev/null || true)
    al=$(systemctl is-active libvirtd 2>/dev/null || true)
    [[ "$al" == active ]] || warn "libvirtd not active"
    [[ "$ad" == active ]] || warn "machina-daemon not active"
    [[ "$al" == active ]] && ok "libvirtd active"
    [[ "$ad" == active ]] && ok "machina-daemon active"
    printf '\n📋 systemctl status machina-daemon\n'
    systemctl status machina-daemon --no-pager 2>/dev/null || warn "cannot read machina-daemon status"
    printf '\n📋 systemctl status libvirtd\n'
    systemctl status libvirtd --no-pager 2>/dev/null || warn "cannot read libvirtd status"
    printf '\n💚 HTTPS %s\n' "$HEALTH_URL"
    command -v curl &>/dev/null && curl -sfk --connect-timeout 3 "$HEALTH_URL" >/dev/null && ok "GET $HEALTH_URL" || warn "cannot reach $HEALTH_URL"
    printf '\n'
}
run
EOS
}

MODE=deploy
[[ "${1:-}" == check ]] && { MODE=check; shift; }

SKIP_INSTALL=false
QUICK=false
CLEANUP=false
BIND=""
OPEN_FW=false
DISABLE_FW=false
NO_START=false
DEPS_ONLY=false
REMOTE_BUILD=false
REMOTE_CHECK=false
DRY_RUN=false
RUN_E2E=false
prune_remote_deploy_tree() {
    local remote="$1"
    info "Pruning deploy sources on remote (keeping target/, web/dist/, web/node_modules/)"
    ssh_r_bash "$remote" "
set -euo pipefail
cd ${REMOTE_DIR}
for ent in * .[!.]* ..?*; do
  [ -e \"\$ent\" ] || continue
  case \"\$ent\" in
    target|web) ;;
    *) rm -rf \"\$ent\" ;;
  esac
done
if [ -d web ]; then
  cd web
  for ent in * .[!.]* ..?*; do
    [ -e \"\$ent\" ] || continue
    case \"\$ent\" in
      dist|node_modules) ;;
      *) rm -rf \"\$ent\" ;;
    esac
  done
fi
du -sh ${REMOTE_DIR}/target ${REMOTE_DIR}/web/dist 2>/dev/null || true
" || warn "prune deploy tree failed (non-fatal)"
    ok "Deploy tree pruned — next --quick will rsync sources and incremental-build"
}

INSTALL_ONLY=false
PRUNE_SOURCES=false
INSTALL_PLATFORM=false
SKIP_PLATFORM_E2E=false
SKIP_DAEMON_E2E=false
SKIP_LIVE_UX=false
RUN_LIBVIRT_DESKTOP_E2E=false
E2E_AUTH_MODE="${E2E_AUTH_MODE:-auto}"

parse_flags() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --sync-only) SKIP_INSTALL=true; shift ;;
            --quick) QUICK=true; shift ;;
            --install-only|--bins-only) INSTALL_ONLY=true; shift ;;
            --prune-sources) PRUNE_SOURCES=true; shift ;;
            --e2e) RUN_E2E=true; shift ;;
            --e2e-libvirt-desktop) RUN_E2E=true; RUN_LIBVIRT_DESKTOP_E2E=true; shift ;;
            --platform) INSTALL_PLATFORM=true; shift ;;
            --skip-platform-e2e) SKIP_PLATFORM_E2E=true; shift ;;
            --skip-daemon-e2e) SKIP_DAEMON_E2E=true; shift ;;
            --skip-live-ux) SKIP_LIVE_UX=true; shift ;;
            --e2e-auth) E2E_AUTH_MODE="${2:?pam|ldap|oidc|auto}"; shift 2 ;;
            --cleanup) CLEANUP=true; shift ;;
            --open-firewall) OPEN_FW=true; shift ;;
            --disable-firewalld) DISABLE_FW=true; shift ;;
            --no-start) NO_START=true; shift ;;
            --deps-only) DEPS_ONLY=true; shift ;;
            --remote-build) REMOTE_BUILD=true; SKIP_INSTALL=true; shift ;;
            --remote-check) REMOTE_CHECK=true; SKIP_INSTALL=true; shift ;;
            --dry-run) DRY_RUN=true; shift ;;
            --bind) shift; BIND="${1:?}"; [[ "$BIND" =~ ^[A-Za-z0-9_.:-]+$ ]] || die "invalid --bind address: '$BIND'"; shift ;;
            --platform-bind) shift; PLATFORM_BIND="${1:?}"; [[ "$PLATFORM_BIND" =~ ^[A-Za-z0-9_.:-]+$ ]] || die "invalid --platform-bind address: '$PLATFORM_BIND'"; shift ;;
            --ssh-key) shift; SSH_KEY="${1:?}"; shift ;;
            *) REST+=("$1"); shift ;;
        esac
    done
}

if [[ "$MODE" == check ]]; then
    case $# in
        0) check_body ;;
        1) [[ "$1" == *@* ]] || die "check: pass USER@HOST or two args USER HOST"; check_remote "$1" ;;
        2) check_remote "${1}@${2}" ;;
        *) die "check: too many arguments" ;;
    esac
    exit 0
fi

if [[ $# -eq 0 ]]; then
    if machina_load_deploy_last "$REPO"; then
        set -- "${USER}@${HOST}"
        ok "Using .deploy-last → ${USER}@${HOST}"
    elif [[ -n "${DEPLOY_HOST:-}" ]]; then
        set -- "${DEPLOY_USER:-root}" "$DEPLOY_HOST"
    fi
fi

if [[ $# -ge 1 && "$1" == *@* ]]; then
    REMOTE="$1"; USER="${1%%@*}"; HOST="${1#*@}"; shift
    parse_flags "$@"
elif [[ $# -ge 2 ]]; then
    # Common mistake: HOST USER (e.g. IP first). We only auto-fix when $1 looks like IPv4 and $2 does not.
    if [[ "$1" =~ ^[0-9]{1,3}(\.[0-9]{1,3}){3}$ ]] && [[ ! "$2" =~ ^[0-9]{1,3}(\.[0-9]{1,3}){3}$ ]]; then
        USER="$2"
        HOST="$1"
        deploy_ui_target_swap "$USER" "$HOST"
    else
        USER="$1"
        HOST="$2"
    fi
    shift 2
    [[ $# -gt 0 && "${1:-}" != -* ]] && { export SSHPASS="$1"; shift; }
    parse_flags "$@"
else
    die "need USER@HOST or USER HOST (deploy-remote.sh --help)"
fi

REMOTE="${USER}@${HOST}"

# USER/HOST/REMOTE_DIR are spliced verbatim (not re-quoted) into remote shell
# script text throughout this file (e.g. "cd $REMOTE_DIR" inside strings fed to
# `ssh ... exec bash -s`). Reject shell metacharacters up front so a malformed
# or hostile argument can't inject commands into the remote bash session.
[[ "$USER" =~ ^[A-Za-z0-9_.-]+$ ]] || die "invalid username: '$USER' (only letters, digits, '.', '_', '-' allowed)"
[[ "$HOST" =~ ^[A-Za-z0-9_.:-]+$ ]] || die "invalid host: '$HOST' (only letters, digits, '.', ':', '_', '-' allowed)"
[[ "$REMOTE_DIR" =~ ^[A-Za-z0-9_./~-]+$ ]] || die "invalid REMOTE_DIR: '$REMOTE_DIR' (unsafe characters)"

if [[ -n "$SSH_KEY" ]]; then
    [[ -f "$SSH_KEY" ]] || die "--ssh-key not found: $SSH_KEY"
    SSH_OPTS+=(-i "$SSH_KEY" -o IdentitiesOnly=yes)
    RSYNC_RSH="ssh ${SSH_OPTS[*]}"
    info "SSH auth: 🔑 key file ${SSH_KEY}"
fi

# Full install defaults: remote IPv4 targets must listen on 0.0.0.0 and open :5092 unless overridden.
if [[ "$HOST" =~ ^[0-9]{1,3}(\.[0-9]{1,3}){3}$ ]] && [[ "$HOST" != "127.0.0.1" ]]; then
    [[ -z "$BIND" ]] && BIND="0.0.0.0"
    if ! $OPEN_FW; then
        DISABLE_FW=true
    fi
    tip "Remote IPv4 deploy: --bind ${BIND:-0.0.0.0} + --disable-firewalld (pass --open-firewall to open ports instead)"
fi

declare -a INSTALL_ARGS=()
if ((${#REST[@]} > 0)); then
    INSTALL_ARGS=("${REST[@]}")
fi

[[ -f "$REPO/Cargo.toml" ]] || die "run from machina repo root (sources are rsync'd — not built here)"
[[ -n "${SSHPASS:-}" ]] && ! command -v sshpass &>/dev/null && die "install sshpass for password auth"
command -v rsync &>/dev/null || die "rsync required"

if [[ -n "${SSHPASS:-}" ]]; then
    info "SSH auth: 🔑 password (SSHPASS / sshpass)"
else
    info "SSH auth: 🗝️  keys or agent"
fi

deploy_ui_spinner_start "Connecting to ${REMOTE}…"
REMOTE_HOSTNAME="$(ssh_r_bash "$REMOTE" 'hostname')" || { deploy_ui_spinner_stop; die "cannot SSH to $REMOTE"; }
deploy_ui_spinner_stop
ok "Connected — 🖥️  ${REMOTE_HOSTNAME} (${REMOTE})"

if [[ "$USER" != "root" ]]; then
    if ssh_r_bash "$REMOTE" "sudo -n true" 2>/dev/null; then
        DEPLOY_SSH_TTY_OPTS=()
        ok "Passwordless sudo — 🔓 non-TTY SSH for long installs"
    else
        DEPLOY_SSH_TTY_OPTS=(-tt)
    fi
fi

MODE_LABEL="Full install — sudo install.sh (deps, Rust, npm, systemd)"
if $REMOTE_CHECK; then MODE_LABEL="Compile check — make check (no install)"; fi
if $REMOTE_BUILD; then MODE_LABEL="Compile — make release (no install)"; fi
if [[ "${SYNC_ONLY:-0}" == 1 ]] || ($SKIP_INSTALL && ! $REMOTE_BUILD && ! $REMOTE_CHECK); then
    MODE_LABEL="Sync only — rsync sources + ownership fix"
fi
if $QUICK; then MODE_LABEL="Quick — incremental make release web + install (--skip-build)"; fi
if $INSTALL_ONLY; then MODE_LABEL="Install-only — copy existing binaries, no cargo/npm"; fi
if $INSTALL_PLATFORM; then MODE_LABEL+=" + platform (PostgreSQL, controller :5093, agent)"; fi
if $PRUNE_SOURCES; then MODE_LABEL+=" + prune sources after install"; fi

TOTAL_STEPS=4
PLATFORM_PHASE=0
SNAPSHOT_PHASE=4
if $INSTALL_PLATFORM; then
    if $QUICK || $INSTALL_ONLY; then
        TOTAL_STEPS=6
        PLATFORM_PHASE=5
        SNAPSHOT_PHASE=6
    else
        TOTAL_STEPS=5
        PLATFORM_PHASE=4
        SNAPSHOT_PHASE=5
    fi
fi
if $REMOTE_BUILD || $REMOTE_CHECK; then TOTAL_STEPS=3
elif [[ "${SYNC_ONLY:-0}" == 1 ]] || ($SKIP_INSTALL && ! $REMOTE_BUILD && ! $REMOTE_CHECK); then TOTAL_STEPS=2
fi

OPTS_LINE=""
[[ -n "$BIND" ]] && OPTS_LINE+="--bind $BIND  "
$OPEN_FW && OPTS_LINE+="--open-firewall  "
$DISABLE_FW && OPTS_LINE+="--disable-firewalld  "
$NO_START && OPTS_LINE+="--no-start  "
$DEPS_ONLY && OPTS_LINE+="--deps-only  "
$CLEANUP && OPTS_LINE+="cleanup deploy dir after  "

DEPLOY_T0=$SECONDS
banner_deploy "$HOST" "$USER" "$REMOTE_DIR" "$MODE_LABEL"
[[ -n "${OPTS_LINE// /}" ]] && tip "Extra install.sh flags: ${OPTS_LINE%  }"

if $DRY_RUN; then
    deploy_ui_dry_run "$HOST" "$USER" "$REMOTE_DIR" "$QUICK"
    deploy_ui_note "Would rsync → chown → install on remote (cargo/npm on server)"
    $REMOTE_BUILD && deploy_ui_note "Mode: --remote-build (make release only)"
    $REMOTE_CHECK && deploy_ui_note "Mode: --remote-check (make check only)"
    exit 0
fi

phase 1 "$TOTAL_STEPS" "Synchronize sources to remote" "rsync · keeps remote target/, web/dist/, node_modules/ (not overwritten)"
ssh_r_bash "$REMOTE" "mkdir -p $REMOTE_DIR"
rsync_r \
    --exclude='target/' --exclude='node_modules/' --exclude='.git/' --exclude='web/dist/' \
    "$REPO/" "$REMOTE:$REMOTE_DIR/" || die "rsync failed"
ok "Sources synced → ${REMOTE}:${REMOTE_DIR}"

GUESTKIT_SRC="$(cd "$REPO/.." && pwd)/guestkit"
GUESTKIT_REMOTE="$(dirname "$REMOTE_DIR")/guestkit"
if [[ -f "$GUESTKIT_SRC/Cargo.toml" ]]; then
    tip "Syncing sibling GuestKit repo for controller path dependency"
    ssh_r_bash "$REMOTE" "mkdir -p $(dirname "$REMOTE_DIR")/guestkit"
    rsync_r \
        --exclude='target/' --exclude='.git/' \
        "$GUESTKIT_SRC/" "$REMOTE:$GUESTKIT_REMOTE/" || warn "guestkit rsync failed (controller build may fail)"
    ok "GuestKit synced → ${REMOTE}:${GUESTKIT_REMOTE}"
else
    warn "No sibling ../guestkit — ensure path ../../guestkit exists on remote for controller build"
fi

# If a previous run left root-owned files under the tree (e.g. interrupted sudo), cargo fails with EACCES.
phase 2 "$TOTAL_STEPS" "Ensure deploy tree is writable" "sudo chown → SSH user (idempotent)"
ssh_r_bash "$REMOTE" "cd $REMOTE_DIR && sudo chown -R \"\$(id -un):\$(id -gn)\" ." || warn "chown deploy tree failed (non-fatal if you are not sudo-capable)"

if $REMOTE_BUILD || $REMOTE_CHECK; then
    mk_target=release
    $REMOTE_CHECK && mk_target=check
    phase 3 "$TOTAL_STEPS" "Compile on remote (make ${mk_target})" "no install.sh — use full deploy to install binaries"
    ssh_r "$REMOTE" "bash -s" "$REMOTE_DIR" "$mk_target" "$REMOTE_CARGO_BUILD_JOBS" <<'EOS' || die "remote compile failed"
set -euo pipefail
REMOTE_DIR="${1:?}"
REMOTE_MAKE_TARGET="${2:?}"
REMOTE_CARGO_JOBS="${3:-1}"
cd "$REMOTE_DIR" || exit 1
export PATH="${HOME}/.cargo/bin:/usr/local/cargo/bin:/usr/local/bin:/usr/bin:${PATH}"
if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo not on PATH — install Rust + clang first (e.g. sudo ./install.sh --deps-only on the server)." >&2
    exit 1
fi
if [ -z "${LIBCLANG_PATH:-}" ]; then
    if command -v llvm-config >/dev/null 2>&1; then
        _maj="$(llvm-config --version 2>/dev/null | cut -d. -f1 || true)"
        if [ -n "$_maj" ] && [ -d "/usr/lib64/llvm${_maj}/lib64" ]; then
            export LIBCLANG_PATH="/usr/lib64/llvm${_maj}/lib64"
        fi
    fi
    if [ -z "${LIBCLANG_PATH:-}" ] && [ -d /usr/lib64/llvm20/lib64 ]; then
        export LIBCLANG_PATH=/usr/lib64/llvm20/lib64
    fi
fi
[ -n "${LIBCLANG_PATH:-}" ] && printf 'ℹ  LIBCLANG_PATH=%s\n' "$LIBCLANG_PATH"
export CARGO_BUILD_JOBS="$REMOTE_CARGO_JOBS"
printf 'ℹ  CARGO_BUILD_JOBS=%s\n' "$CARGO_BUILD_JOBS"
make "$REMOTE_MAKE_TARGET"
EOS
    ok "Remote compile finished — run without --remote-build/--remote-check to install"
    machina_save_deploy_last "$REPO" "$HOST" "$USER" "remote-${mk_target}"
    hr
    deploy_ui_celebrate "Compile finished in $(machina_elapsed_fmt $((SECONDS - DEPLOY_T0)))"
    tip "Next: ./scripts/deploy remote ${USER}@${HOST}   # full install on same tree"
    exit 0
fi

if [[ "${SYNC_ONLY:-0}" == 1 ]] || $SKIP_INSTALL; then
    machina_save_deploy_last "$REPO" "$HOST" "$USER" "sync-only"
    hr
    deploy_ui_celebrate "Sync finished in $(machina_elapsed_fmt $((SECONDS - DEPLOY_T0)))"
    tip "Sources live on the server under ${REMOTE_DIR} — run full deploy when ready."
    exit 0
fi

OPTS=" --no-tests"
[[ -n "$BIND" ]] && OPTS+=" --bind $BIND"
$OPEN_FW && OPTS+=" --open-firewall"
$DISABLE_FW && OPTS+=" --disable-firewalld"
$NO_START && OPTS+=" --no-start"
$DEPS_ONLY && OPTS+=" --deps-only"

REMOTE_INST=""
if ((${#INSTALL_ARGS[@]} > 0)); then
    for a in "${INSTALL_ARGS[@]}"; do REMOTE_INST+=" $(printf '%q' "$a")"; done
fi

# MACHINA_LICENSE_KEY is spliced into remote command strings below (three call sites).
# Shell-quote it (like REMOTE_INST above) so a key containing quotes/backticks/$(...)
# can't break out of the `export MACHINA_LICENSE_KEY="..."` string and inject commands
# into the `sudo -E bash install.sh` invocation on the remote host.
LICENSE_KEY_EXPORT=""
if [[ -n "${MACHINA_LICENSE_KEY:-}" ]]; then
    LICENSE_KEY_EXPORT="export MACHINA_LICENSE_KEY=$(printf '%q' "$MACHINA_LICENSE_KEY"); "
fi

QUICK_OPTS=" --no-tests --skip-build"
[[ -n "$BIND" ]] && QUICK_OPTS+=" --bind $BIND"
$OPEN_FW && QUICK_OPTS+=" --open-firewall"
$DISABLE_FW && QUICK_OPTS+=" --disable-firewalld"

if $QUICK || $INSTALL_ONLY; then
    if $INSTALL_ONLY; then
        phase 3 "$TOTAL_STEPS" "Install binaries only (no rebuild)" "install.sh --skip-build · uses existing target/release + web/dist"
        ssh_r_bash "$REMOTE" "
set -euo pipefail
cd $REMOTE_DIR
if [ ! -x target/release/machina-daemon ] || [ ! -f web/dist/index.html ]; then
  echo 'Missing target/release/machina-daemon or web/dist — run --quick once first' >&2
  exit 1
fi
${LICENSE_KEY_EXPORT}sudo -E bash install.sh${QUICK_OPTS}
for bin in machina-controller machina-agent; do
  if [ -x target/release/\$bin ]; then
    sudo install -m755 target/release/\$bin /usr/local/bin/\$bin
  fi
done
" || die "install-only failed"
    else
        phase 3 "$TOTAL_STEPS" "Build & install (quick path)" "deps-only → make release web (incremental) + install.sh --skip-build"
        # Ensure system deps (protobuf-compiler, libvirt-dev, etc.) are present before building.
        # install.sh --deps-only is idempotent and fast when deps are already installed.
        ssh_r_bash "$REMOTE" "
set -euo pipefail
export PATH=\"\${HOME}/.cargo/bin:/usr/local/cargo/bin:/usr/local/bin:/usr/bin:\${PATH}\"
export CARGO_BUILD_JOBS=${REMOTE_CARGO_BUILD_JOBS}
cd $REMOTE_DIR
sudo bash install.sh --deps-only --no-tests
make release web
${LICENSE_KEY_EXPORT}sudo -E bash install.sh${QUICK_OPTS}
for bin in machina-controller machina-agent; do
  if [ -x target/release/\$bin ]; then
    sudo install -m755 target/release/\$bin /usr/local/bin/\$bin
  fi
done
" || die "quick build failed"
    fi
    phase 4 "$TOTAL_STEPS" "Reload systemd & restart Machina services" "daemon-reload — restart daemon; controller+agent only if active"
    ssh_r_bash "$REMOTE" "
set -euo pipefail
sudo cp ${REMOTE_DIR}/contrib/machina-daemon.service ${REMOTE_DIR}/contrib/machina-controller.service ${REMOTE_DIR}/contrib/machina-agent.service /usr/lib/systemd/system/ 2>/dev/null || true
sudo systemctl daemon-reload
sudo systemctl restart machina-daemon
for svc in machina-controller machina-agent; do
  if systemctl is-enabled \"\$svc\" &>/dev/null || systemctl is-active \"\$svc\" &>/dev/null; then
    sudo systemctl restart \"\$svc\" || true
  fi
done
" || die "service restart failed"
else
    phase 3 "$TOTAL_STEPS" "Run installer on remote" "sudo install.sh — tooling, build, unit files, optional firewall"
    ssh_r_bash "$REMOTE" "
set -euo pipefail
cd $REMOTE_DIR
${LICENSE_KEY_EXPORT}sudo -E bash install.sh${OPTS}${REMOTE_INST}
" || die "install failed"
fi

if $INSTALL_PLATFORM; then
    phase "$PLATFORM_PHASE" "$TOTAL_STEPS" "Install platform control plane" "PostgreSQL + machina-controller :5093 + machina-agent"
    PLATFORM_OPTS=""
    # The daemon's --bind (auto 0.0.0.0 for IPv4 targets — the UI must be public)
    # is NOT forwarded to the platform installer: controller/agent stay on
    # 127.0.0.1 behind the daemon proxy unless --platform-bind is passed
    # explicitly (multi-host fleets).
    [[ -n "${PLATFORM_BIND:-}" ]] && PLATFORM_OPTS+=" --bind $PLATFORM_BIND"
    $OPEN_FW && PLATFORM_OPTS+=" --open-firewall"
    $DISABLE_FW && PLATFORM_OPTS+=" --disable-firewalld"
    PLATFORM_OPTS+=" --public-url http://${HOST}:5093"
    ssh_r_bash "$REMOTE" "
set -euo pipefail
cd $REMOTE_DIR
sudo bash scripts/install-platform.sh${PLATFORM_OPTS}
" || die "platform install failed"
    deploy_ui_highlight "Platform postflight — agent, inventory, e2e cleanup"
    ssh_r_bash "$REMOTE" "sudo bash $REMOTE_DIR/scripts/lib/platform-sweep-remote.sh" || warn "platform postflight had issues (non-fatal)"
fi

# Stale-binary guard: across the multiple install+restart cycles in a deploy, a
# service can end up still running the PREVIOUS binary (its /proc/<pid>/exe shows
# "(deleted)", or the on-disk binary is newer than the process). That silently
# ships stale code — a fix appears deployed but isn't live until a manual restart.
# Detect and force-restart so the running process always matches what we installed.
ssh_r_bash "$REMOTE" "
for svc in machina-controller machina-agent machina-daemon; do
  bin=/usr/local/bin/\$svc
  [ -x \"\$bin\" ] || continue
  systemctl is-active \"\$svc\" &>/dev/null || continue
  pid=\$(systemctl show -p MainPID --value \"\$svc\" 2>/dev/null)
  [ -n \"\$pid\" ] && [ \"\$pid\" != 0 ] || continue
  stale=0
  exe=\$(sudo readlink /proc/\$pid/exe 2>/dev/null || true)
  case \"\$exe\" in *'(deleted)'*) stale=1;; esac
  bmt=\$(stat -c %Y \"\$bin\" 2>/dev/null || echo 0)
  pst=\$(stat -c %Y /proc/\$pid 2>/dev/null || echo 0)
  [ \"\$bmt\" -gt \"\$pst\" ] && stale=1
  if [ \"\$stale\" = 1 ]; then
    echo \"↻ \$svc was running a stale binary (exe=\$exe) — restarting to load the new one\"
    sudo systemctl restart \"\$svc\" || true
  fi
done
" || warn "stale-binary guard had issues (non-fatal)"

phase "$SNAPSHOT_PHASE" "$TOTAL_STEPS" "Service snapshot" "machina-daemon + libvirtd + platform status"
ssh_r_bash "$REMOTE" "
for svc in machina-daemon libvirtd machina-controller machina-agent; do
  st=\$(systemctl is-active \$svc 2>/dev/null || echo unknown)
  if [ \"\$st\" = active ]; then
    echo \"✅ \$svc: running\"
  else
    echo \"⚠️  \$svc: \$st\"
  fi
  systemctl status \$svc --no-pager 2>/dev/null || true
  echo
done
if systemctl is-active machina-controller &>/dev/null; then
  curl -sf http://127.0.0.1:5093/api/v1/health && echo || echo '⚠️  platform health check failed'
  if [ -f $REMOTE_DIR/scripts/lib/platform-sweep-remote.sh ]; then
    echo '▶ Platform inventory sweep (sync + prune stale VMs)'
    sudo bash $REMOTE_DIR/scripts/lib/platform-sweep-remote.sh || echo '⚠️  platform sweep had issues'
  fi
fi
" || warn "service status check failed"

if $CLEANUP; then
    warn "Removing remote deploy tree $REMOTE_DIR"
    ssh_r "$REMOTE" "rm -rf $REMOTE_DIR" || warn "cleanup failed"
fi

if $PRUNE_SOURCES; then
    prune_remote_deploy_tree "$REMOTE"
fi

hr
info "Post-flight verification (health endpoint + systemd)"
sleep 1
check_remote "$REMOTE" || true

ELAPSED=$((SECONDS - DEPLOY_T0))
MODE_SAVE=full
$QUICK && MODE_SAVE=quick
$INSTALL_ONLY && MODE_SAVE=install-only
machina_save_deploy_last "$REPO" "$HOST" "$USER" "$MODE_SAVE"

deploy_ui_highlight "📋 Post-deploy checklist"
deploy_ui_checklist "machina-daemon" "$(ssh_r_bash "$REMOTE" 'systemctl is-active machina-daemon 2>/dev/null || echo unknown' | tr -d '\r')"
deploy_ui_checklist "libvirtd" "$(ssh_r_bash "$REMOTE" 'systemctl is-active libvirtd 2>/dev/null || echo unknown' | tr -d '\r')"
if $INSTALL_PLATFORM; then
    deploy_ui_checklist "machina-controller" "$(ssh_r_bash "$REMOTE" 'systemctl is-active machina-controller 2>/dev/null || echo unknown' | tr -d '\r')"
    deploy_ui_checklist "machina-agent" "$(ssh_r_bash "$REMOTE" 'systemctl is-active machina-agent 2>/dev/null || echo unknown' | tr -d '\r')"
    deploy_ui_kv "🗄️" "DB" "sqlite:///var/lib/machina/controller.db"
    deploy_ui_kv "🎛️" "Platform API" "http://${HOST}:5093/api/v1/health"
fi

deploy_ui_celebrate "Ship it!"
machina_print_success "$HOST" "$ELAPSED" "$USER" "$($INSTALL_PLATFORM && echo '--platform' || true)"
deploy_ui_kv "🔗" "SSH" "ssh ${USER}@${HOST}"
deploy_ui_kv "🌐" "UI" "https://${HOST}:5092/"
tip "Trust the browser once for the self-signed TLS cert, or terminate TLS upstream."
if $INSTALL_PLATFORM; then
    tip "Fast redeploy (no rebuild): ./scripts/deploy remote ${USER}@${HOST} --install-only --platform"
else
    tip "Add --platform to also install/update machina-controller + agent."
    tip "Quick redeploys now sync MACHINA_PLATFORM_AUTH from MACHINA_ADMIN_PASSWORD when platform env exists (fixes proxy 401)."
fi
tip "After first --quick, prune sources: add --prune-sources (keeps target/ + web/dist/ on server)"
tip "HOST USER also works: ./scripts/deploy-remote.sh ${HOST} ${USER} --install-only"

if $RUN_E2E; then
    if [[ -n "${VSPASS:-}" || -n "${SSHPASS:-}" || -n "${E2E_LDAP_PASS:-}" ]]; then
        # shellcheck source=lib/e2e-auth.sh
        source "${SCRIPT_DIR}/lib/e2e-auth.sh"
        export E2E_AUTH_MODE
        export E2E_USER="${USER}"
        export E2E_PASSWORD="${VSPASS:-${SSHPASS:-}}"
        if $INSTALL_PLATFORM && ! $SKIP_PLATFORM_E2E; then
            deploy_ui_highlight "🧪 Post-deploy full E2E (daemon + platform proxy + controller)"
            info "Waiting for agent gRPC :50051 before install smoke…"
            ssh_r_bash "$REMOTE" '
for i in $(seq 1 15); do
  if ss -ltn 2>/dev/null | grep -q ":50051"; then echo "agent gRPC ready"; exit 0; fi
  sleep 2
done
echo "agent gRPC not ready after 30s" >&2
exit 1
' || warn "agent gRPC wait timed out (install smoke may flake)"
            FULL_E2E_FLAGS=()
            if $SKIP_DAEMON_E2E; then FULL_E2E_FLAGS+=(--skip-daemon-e2e); fi
            FULL_E2E_OK=true
            API_E2E_SUMMARY="not run"
            if env VSPASS="$E2E_PASSWORD" E2E_AUTH_MODE="$E2E_AUTH_MODE" \
                E2E_LDAP_USER="${E2E_LDAP_USER:-}" E2E_LDAP_PASS="${E2E_LDAP_PASS:-}" \
                "${SCRIPT_DIR}/e2e-full-test-remote.sh" "$USER" "$HOST" --auth "$E2E_AUTH_MODE" "${FULL_E2E_FLAGS[@]}"; then
                deploy_ui_celebrate "Full E2E passed"
                API_E2E_SUMMARY="passed"
            else
                warn "Full E2E failed (deploy itself succeeded)"
                FULL_E2E_OK=false
                API_E2E_SUMMARY="failed"
            fi
            LIVE_E2E_OK=true
            VM_E2E_SUMMARY="not run"
            if ! $SKIP_LIVE_UX; then
                deploy_ui_highlight "🧪 Post-deploy live UX wiring (Playwright)"
                LIVE_PW="${VSPASS:-${SSHPASS:-${E2E_LDAP_PASS:-}}}"
                LIVE_BASE="https://${HOST}:5092"
                e2e_export_playwright_live_env "$LIVE_BASE" "$USER" "$LIVE_PW"
                if npm --prefix "${SCRIPT_DIR}/../web" run test:e2e:live-ux; then
                    deploy_ui_celebrate "Live UX wiring passed"
                else
                    warn "Live UX wiring failed (deploy itself succeeded)"
                    LIVE_E2E_OK=false
                fi
                deploy_ui_highlight "🧪 Post-deploy live VM create/delete (Playwright)"
                if npm --prefix "${SCRIPT_DIR}/../web" run test:e2e -- --workers=1 --timeout=300000 \
                    e2e/platform-live-access.spec.ts \
                    e2e/platform-live-vm-create.spec.ts \
                    e2e/platform-live-machine-finder-delete.spec.ts \
                    e2e/platform-live-vm-delete.spec.ts; then
                    deploy_ui_celebrate "Live VM lifecycle passed"
                    VM_E2E_SUMMARY="passed"
                else
                    warn "Live VM lifecycle failed (deploy itself succeeded)"
                    LIVE_E2E_OK=false
                    VM_E2E_SUMMARY="failed"
                fi
                $LIVE_E2E_OK || warn "One or more live Playwright phases failed"
            fi
            if $RUN_LIBVIRT_DESKTOP_E2E; then
                deploy_ui_highlight "🧪 Libvirt desktop E2E (GuestKit + lifecycle + VNC)"
                E2E_KEY="${SSH_KEY:-${HOME}/.ssh/id_ed25519}"
                if VSPASS="${VSPASS:-${SSHPASS:-}}" E2E_SSH_KEY="${E2E_KEY}" \
                    "${SCRIPT_DIR}/e2e-libvirt-desktop-full-remote.sh" "$USER" "$HOST" --ssh-key "${E2E_KEY}"; then
                    deploy_ui_celebrate "Libvirt desktop E2E passed"
                else
                    warn "Libvirt desktop E2E failed (deploy itself succeeded)"
                    LIVE_E2E_OK=false
                fi
            fi
            SERVICES_SUMMARY="$(ssh_r_bash "$REMOTE" 'for u in machina-daemon libvirtd machina-controller machina-agent; do printf "%s=%s\n" "$u" "$(systemctl is-active "$u" 2>/dev/null || echo unknown)"; done' | tr -d '\r')"
            OVERALL="PASS"
            if ! $FULL_E2E_OK || ! $LIVE_E2E_OK; then OVERALL="FAIL"; fi
            "${SCRIPT_DIR}/lib/send-deploy-report.sh" "$HOST" \
                --api-e2e-summary "$API_E2E_SUMMARY" \
                --vm-e2e-summary "$VM_E2E_SUMMARY" \
                --services-summary "$SERVICES_SUMMARY" \
                --overall "$OVERALL" || true
            if [[ "$STRICT" == "1" ]] && { ! $FULL_E2E_OK || ! $LIVE_E2E_OK; }; then
                die "STRICT=1: post-deploy E2E failed (API=${API_E2E_SUMMARY}, live=${LIVE_E2E_OK}, vm=${VM_E2E_SUMMARY})"
            fi
        elif ! $SKIP_DAEMON_E2E; then
            deploy_ui_highlight "🧪 Post-deploy E2E (daemon :5092)"
            if "${SCRIPT_DIR}/e2e-test-remote.sh" "$USER" "$HOST"; then
                deploy_ui_celebrate "Daemon E2E passed"
            else
                warn "Daemon E2E failed (deploy itself succeeded)"
            fi
        else
            warn "E2E skipped (--skip-daemon-e2e with --skip-platform-e2e or no platform install)"
        fi
    else
        warn "E2E skipped: set VSPASS/SSHPASS (PAM) or E2E_LDAP_* (LDAP), and optional --e2e-auth pam|ldap|auto"
    fi
fi
printf '\n'
