#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# scripts/install-platform.sh — machina-controller + machina-agent on a KVM host.
#
# Run after machina-daemon install/build (expects target/release binaries in repo root):
#   sudo bash scripts/install-platform.sh [--bind ADDR] [--open-firewall] [--public-url URL]
#
set -euo pipefail

INSTALLER_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_FILE="$(mktemp /tmp/machina-platform-install-XXXXXX.log)"
chmod 600 "$LOG_FILE"

# Localhost by default: the web UI reaches the controller through the daemon's
# authenticated proxy on :5092, and the controller dials the agent itself — no
# platform port needs to be public on a single-host install. Pass --bind ADDR
# (e.g. 0.0.0.0) only for multi-host fleets whose agents/controllers must be
# reachable across the network.
BIND_HOST="127.0.0.1"
OPEN_FIREWALL=false
DB_MODE=""   # sqlite (default) | pod | package | external; see scripts/db/machina-db.sh
DB_URL=""
DB_MIGRATE=false   # --database-migrate: copy the existing SQLite data into the new PostgreSQL database
DISABLE_FIREWALL=false
PUBLIC_URL=""
# shellcheck source=lib/disable-firewalld.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib/disable-firewalld.sh"
SKIP_AUTH="${MACHINA_SKIP_AUTH:-0}"

info()  { echo "ℹ️  $*"; }
ok()    { echo "✅ $*"; }
warn()  { echo "⚠️  $*"; }
fail()  { echo "❌ $*"; exit 1; }
step()  { echo ""; echo "➡️  $*"; }

log_cmd() { "$@" >>"$LOG_FILE" 2>&1; }

usage() {
  cat <<'EOF'
install-platform.sh [--bind ADDR] [--open-firewall|--disable-firewalld] [--public-url URL] [--require-auth]
                    [--database sqlite|pod|package|external [--database-url postgres://...] [--database-migrate]]

Installs machina-controller (:5093) and machina-agent (:50051).
Controller binds 127.0.0.1 by default (UI uses the daemon proxy on :5092);
pass --bind 0.0.0.0 only for multi-host fleets needing a network-reachable
control plane.
Requires root and pre-built target/release/{machina-controller,machina-agent}.

Env: MACHINA_SKIP_AUTH=0 to disable dev auth bypass in /etc/default/machina-platform
EOF
  exit 0
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bind) BIND_HOST="${2:?}"; shift 2 ;;
    --open-firewall) OPEN_FIREWALL=true; shift ;;
    --disable-firewalld) DISABLE_FIREWALL=true; shift ;;
    --public-url) PUBLIC_URL="${2:?}"; shift 2 ;;
    --require-auth) SKIP_AUTH=0; shift ;;
    --database) DB_MODE="${2:?}"; shift 2 ;;
    --database-url) DB_URL="${2:?}"; shift 2 ;;
    --database-migrate) DB_MIGRATE=true; shift ;;
    -h|--help) usage ;;
    *) warn "Unknown arg: $1"; shift ;;
  esac
done

[[ "$(id -u)" -eq 0 ]] || fail "Run as root: sudo bash scripts/install-platform.sh"

detect_os() {
  if [[ -f /etc/os-release ]]; then
    # shellcheck source=/dev/null
    . /etc/os-release
    OS_ID="${ID:-unknown}"
    OS_NAME="${PRETTY_NAME:-$OS_ID}"
  else
    fail "/etc/os-release not found"
  fi
  case "$OS_ID" in
    fedora) PKG_MANAGER=dnf; OS_FAMILY=fedora ;;
    rhel|centos|rocky|almalinux|alma) PKG_MANAGER=dnf; OS_FAMILY=rhel ;;
    ubuntu|debian|linuxmint|pop) PKG_MANAGER=apt; OS_FAMILY=debian ;;
    opensuse*|sles) PKG_MANAGER=zypper; OS_FAMILY=suse ;;
    arch|manjaro|endeavouros) PKG_MANAGER=pacman; OS_FAMILY=arch ;;
    *)
      if command -v dnf &>/dev/null; then PKG_MANAGER=dnf; OS_FAMILY=fedora
      elif command -v apt &>/dev/null; then PKG_MANAGER=apt; OS_FAMILY=debian
      elif command -v zypper &>/dev/null; then PKG_MANAGER=zypper; OS_FAMILY=suse
      elif command -v pacman &>/dev/null; then PKG_MANAGER=pacman; OS_FAMILY=arch
      else fail "No supported package manager"
      fi
      ;;
  esac
  info "Detected: $OS_NAME ($OS_FAMILY / $PKG_MANAGER)"
}

primary_ipv4() {
  local ip=""
  if command -v ip &>/dev/null; then
    ip=$(ip -4 route get 1.1.1.1 2>/dev/null | awk '{for (i=1;i<=NF;i++) if ($i=="src") { print $(i+1); exit }}')
    if [[ -z "$ip" || "$ip" == "127.0.0.1" ]]; then
      ip=$(ip -4 -o addr show scope global up 2>/dev/null \
        | awk '$2 !~ /^(lo|docker|virbr|veth|br-|cni|flannel|tailscale|wg)/ {split($4,a,"/"); print a[1]; exit}')
    fi
  fi
  [[ -z "$ip" ]] && ip=$(hostname -I 2>/dev/null | awk '{print $1}')
  if [[ -n "$ip" && "$ip" != "127.0.0.1" ]]; then echo "$ip"; else echo "127.0.0.1"; fi
}

install_binaries() {
  step "Platform binaries"
  local ctrl="$INSTALLER_ROOT/target/release/machina-controller"
  local agent="$INSTALLER_ROOT/target/release/machina-agent"
  [[ -x "$ctrl" ]] || fail "Missing $ctrl — run make release first"
  [[ -x "$agent" ]] || fail "Missing $agent — run make release first"
  # Keep the previous binaries so a bad deploy can be rolled back:
  #   cp /usr/local/bin/machina-controller.prev /usr/local/bin/machina-controller && \
  #     systemctl restart machina-controller   (same for machina-agent)
  [[ -x /usr/local/bin/machina-controller ]] && cp -f /usr/local/bin/machina-controller /usr/local/bin/machina-controller.prev
  [[ -x /usr/local/bin/machina-agent ]] && cp -f /usr/local/bin/machina-agent /usr/local/bin/machina-agent.prev
  install -Dm755 "$ctrl" /usr/local/bin/machina-controller
  install -Dm755 "$agent" /usr/local/bin/machina-agent
  ok "Installed machina-controller + machina-agent (previous kept as .prev for rollback)"
  install_pg_controller
}

# The PostgreSQL build of the controller (machina-controller-pg). Installed whenever it is available so switching database later
# is one command; built on the spot when a PostgreSQL database was asked for and no prebuilt binary exists.
install_pg_controller() {
  local pg="$INSTALLER_ROOT/target/release/machina-controller-pg"
  if [[ ! -x "$pg" && -n "$DB_MODE" && "$DB_MODE" != sqlite ]]; then
    step "Building the PostgreSQL build of the controller"
    ( cd "$INSTALLER_ROOT" && cargo build --release -p machina-controller --no-default-features --features postgres \
        --target-dir target/pg >>"$LOG_FILE" 2>&1 ) || fail "could not build the PostgreSQL controller — see $LOG_FILE"
    install -Dm755 "$INSTALLER_ROOT/target/pg/release/machina-controller" "$pg"
  fi
  if [[ -x "$pg" ]]; then
    install -Dm755 "$pg" /usr/local/bin/machina-controller-pg
    ok "Installed machina-controller-pg (PostgreSQL build)"
  fi
  if [[ -x "$INSTALLER_ROOT/target/release/machina-dbtool" ]]; then
    install -Dm755 "$INSTALLER_ROOT/target/release/machina-dbtool" /usr/local/bin/machina-dbtool
  fi
  if [[ -x "$INSTALLER_ROOT/scripts/db/machina-db.sh" ]]; then
    install -Dm755 "$INSTALLER_ROOT/scripts/db/machina-db.sh" /usr/local/bin/machina-db
  fi
}

# --database pod|package|external|sqlite: choose the controller's database (see scripts/db/machina-db.sh). SQLite is the default
# and needs nothing; the others set up PostgreSQL and point the controller at it before it first starts.
setup_database() {
  [[ -n "$DB_MODE" ]] || return 0
  step "Controller database: $DB_MODE"
  local args=("setup" "$DB_MODE")
  [[ "$DB_MODE" == external ]] && args+=("--url" "${DB_URL:?--database external needs --database-url postgres://...}")
  [[ "$DB_MIGRATE" == true && "$DB_MODE" != sqlite ]] && args+=("--migrate")
  /usr/local/bin/machina-db "${args[@]}" 2>&1 | tee -a "$LOG_FILE" || fail "database setup failed — see $LOG_FILE"
}

ensure_platform_env_var() {
  local key="$1" value="$2"
  local file="/etc/default/machina-platform"
  if grep -q "^${key}=" "$file" 2>/dev/null; then
    sed -i "s|^${key}=.*|${key}=${value}|" "$file"
  else
    echo "${key}=${value}" >>"$file"
  fi
}

# Native eBPF (machina-bpfd) replaced the external PacketWolf / Netra / Tetragon
# integrations; drop their now-unused keys so upgraded hosts don't carry dead config.
drop_legacy_fabric_env() {
  local file="/etc/default/machina-platform"
  [[ -f "$file" ]] || return 0
  if grep -qE '^(PACKETWOLF_|NETRA_|MACHINA_INGEST_KEY=)' "$file"; then
    sed -i -E '/^(PACKETWOLF_[A-Z_]*|NETRA_[A-Z_]*|MACHINA_INGEST_KEY)=/d' "$file"
    ok "Removed legacy PacketWolf/Netra/ingest keys from $file"
  fi
}

ensure_platform_secrets() {
  local file="/etc/default/machina-platform"
  # Generate strong secrets on first install and NEVER overwrite existing ones. Without these
  # the controller's boot-guard refuses to start on the dev-default JWT secret / 'admin'
  # password, so a fresh deploy would fail to come up.
  if ! grep -q '^MACHINA_JWT_SECRET=' "$file" 2>/dev/null; then
    echo "MACHINA_JWT_SECRET=$(openssl rand -hex 48)" >>"$file"
    ok "Generated MACHINA_JWT_SECRET"
  fi
  if ! grep -q '^MACHINA_AGENT_TOKEN=' "$file" 2>/dev/null; then
    echo "MACHINA_AGENT_TOKEN=$(openssl rand -hex 32)" >>"$file"
    ok "Generated MACHINA_AGENT_TOKEN (controller↔agent gRPC + console auth)"
  fi
  if ! grep -q '^MACHINA_ADMIN_PASSWORD=' "$file" 2>/dev/null; then
    local pw
    pw="$(openssl rand -base64 18 | tr -d '/+=')"
    echo "MACHINA_ADMIN_PASSWORD=${pw}" >>"$file"
    warn "Generated bootstrap MACHINA_ADMIN_PASSWORD: ${pw}  (shown once — save it)"
  fi
}

write_platform_env() {
  step "Platform configuration"
  local ip pub file="/etc/default/machina-platform"
  ip="$(primary_ipv4)"
  pub="${PUBLIC_URL:-http://${ip}:5093}"
  if [[ -f "$file" ]]; then
    # PRESERVE the existing env file — operator/generated secrets live here. Only append keys
    # the template introduces that are missing; never clobber existing values. (This function
    # previously ran `install -Dm644 <template>` unconditionally, WIPING secrets on every
    # re-deploy — the reason a --platform redeploy took the controller down.)
    local tline tkey
    while IFS= read -r tline; do
      [[ "$tline" =~ ^[[:space:]]*# || -z "${tline//[[:space:]]/}" ]] && continue
      tkey="${tline%%=*}"
      grep -q "^${tkey}=" "$file" || printf '%s\n' "$tline" >>"$file"
    done < "$INSTALLER_ROOT/contrib/machina-platform.env"
  else
    # Mode 600 from creation: ensure_platform_secrets (below) immediately appends
    # JWT/agent-token/admin-password secrets into this file, and the final
    # `chmod 600` at the end of this function previously left a window where a
    # freshly-created (mode 644, world-readable) file held those secrets.
    install -Dm600 "$INSTALLER_ROOT/contrib/machina-platform.env" "$file"
  fi
  ensure_platform_secrets
  grep -q '^MACHINA_CONTROLLER_ID=' /etc/default/machina-platform \
    || echo 'MACHINA_CONTROLLER_ID=ctrl-primary' >>/etc/default/machina-platform
  sed -i "s|^MACHINA_PUBLIC_URL=.*|MACHINA_PUBLIC_URL=${pub}|" /etc/default/machina-platform
  sed -i "s|^MACHINA_WEB_URL=.*|MACHINA_WEB_URL=http://${ip}:5092|" /etc/default/machina-platform
  if [[ "$SKIP_AUTH" == "0" ]]; then
    sed -i '/^MACHINA_SKIP_AUTH=/d' /etc/default/machina-platform
  else
    grep -q '^MACHINA_SKIP_AUTH=' /etc/default/machina-platform \
      || echo 'MACHINA_SKIP_AUTH=1' >>/etc/default/machina-platform
  fi
  drop_legacy_fabric_env
  ensure_daemon_platform_proxy_env
  chmod 600 /etc/default/machina-platform
  ok "Config -> /etc/default/machina-platform (public URL: $pub)"
}

ensure_daemon_platform_proxy_env() {
  local daemon_env="/etc/default/machina-daemon"
  local platform_env="/etc/default/machina-platform"
  local admin_pw="" prev_auth=""
  # Create with a restrictive mode up front (in case this runs before install.sh has ever
  # touched the file), and re-assert it below regardless of prior state: this function
  # writes MACHINA_PLATFORM_AUTH, a real basic-auth credential the daemon uses to
  # authenticate to the controller — it must never be left world-readable.
  [[ -f "$daemon_env" ]] || install -m600 /dev/null "$daemon_env"
  grep -q '^MACHINA_PLATFORM_CONTROLLER_URL=' "$daemon_env" 2>/dev/null \
    || echo 'MACHINA_PLATFORM_CONTROLLER_URL=http://127.0.0.1:5093' >>"$daemon_env"

  # machina-daemon only reads this file at process start (systemd EnvironmentFile), so if it's
  # already running by the time we change this value below, it keeps serving with the old/no
  # credential until something restarts it — the daemon→controller proxy then 401s until an
  # operator notices and restarts by hand. Capture the prior value so we can restart for them.
  prev_auth="$(grep '^MACHINA_PLATFORM_AUTH=' "$daemon_env" 2>/dev/null | head -1 || true)"

  # The controller bootstrap password lives in machina-platform. Quick/non-platform
  # redeploys historically left MACHINA_PLATFORM_AUTH=admin:admin forever, which
  # 401s the daemon→controller proxy once a real MACHINA_ADMIN_PASSWORD exists.
  if [[ -f "$platform_env" ]]; then
    admin_pw="$(grep '^MACHINA_ADMIN_PASSWORD=' "$platform_env" 2>/dev/null | head -1 | cut -d= -f2- || true)"
  fi
  if [[ -n "$admin_pw" ]]; then
    if grep -q '^MACHINA_PLATFORM_AUTH=' "$daemon_env" 2>/dev/null; then
      sed -i "s|^MACHINA_PLATFORM_AUTH=.*|MACHINA_PLATFORM_AUTH=admin:${admin_pw}|" "$daemon_env"
    else
      echo "MACHINA_PLATFORM_AUTH=admin:${admin_pw}" >>"$daemon_env"
    fi
    ok "Synced MACHINA_PLATFORM_AUTH → admin:<MACHINA_ADMIN_PASSWORD> in $daemon_env"
  elif ! grep -q '^MACHINA_PLATFORM_AUTH=' "$daemon_env" 2>/dev/null; then
    echo 'MACHINA_PLATFORM_AUTH=admin:admin' >>"$daemon_env"
    warn "MACHINA_PLATFORM_AUTH defaulted to admin:admin in $daemon_env — set MACHINA_ADMIN_PASSWORD" \
         "in $platform_env (and re-run install-platform / --platform) so the proxy matches."
  fi
  chmod 600 "$daemon_env"

  local new_auth
  new_auth="$(grep '^MACHINA_PLATFORM_AUTH=' "$daemon_env" 2>/dev/null | head -1 || true)"
  if [[ "$new_auth" != "$prev_auth" ]] && systemctl is-active --quiet machina-daemon 2>/dev/null; then
    # try-restart: only acts if the unit is already running, never starts it fresh here.
    systemctl try-restart machina-daemon >>"$LOG_FILE" 2>&1 \
      && ok "Restarted machina-daemon to pick up the new MACHINA_PLATFORM_AUTH" \
      || warn "machina-daemon restart failed after updating MACHINA_PLATFORM_AUTH — the proxy may 401" \
           "until you: sudo systemctl restart machina-daemon"
  fi
}

install_systemd_units() {
  step "Systemd units"
  install -Dm644 "$INSTALLER_ROOT/contrib/machina-agent.service" /usr/lib/systemd/system/machina-agent.service
  install -Dm644 "$INSTALLER_ROOT/contrib/machina-controller.service" /usr/lib/systemd/system/machina-controller.service
  if [[ "$BIND_HOST" != "127.0.0.1" ]]; then
    sed -i "s|--host 127.0.0.1|--host ${BIND_HOST}|" /usr/lib/systemd/system/machina-controller.service
  fi
  systemctl daemon-reload
  ok "Systemd units installed"
}

open_firewall_port() {
  [[ "$OPEN_FIREWALL" == true ]] || return 0
  step "Firewall (5093/tcp)"
  if command -v firewall-cmd &>/dev/null && systemctl is-active firewalld &>/dev/null; then
    firewall-cmd --add-port=5093/tcp --permanent >>"$LOG_FILE" 2>&1 || true
    firewall-cmd --reload >>"$LOG_FILE" 2>&1 || true
    ok "Opened 5093/tcp (firewalld)"
  elif command -v ufw &>/dev/null; then
    ufw allow 5093/tcp >>"$LOG_FILE" 2>&1 || true
    ok "Opened 5093/tcp (ufw)"
  elif command -v iptables &>/dev/null; then
    iptables -C INPUT -p tcp --dport 5093 -j ACCEPT 2>/dev/null \
      || iptables -I INPUT -p tcp --dport 5093 -j ACCEPT 2>/dev/null || true
    ok "Opened 5093/tcp (iptables)"
  else
    warn "No firewall tool detected"
  fi
}

start_services() {
  step "Start platform services"
  mkdir -p /var/lib/machina

  systemctl enable machina-agent machina-controller >>"$LOG_FILE" 2>&1
  # Clear any previous failure/burst-limit state so systemd starts fresh
  systemctl reset-failed machina-agent machina-controller >>"$LOG_FILE" 2>&1 || true
  systemctl restart machina-agent >>"$LOG_FILE" 2>&1 || fail "machina-agent failed — journalctl -u machina-agent"
  sleep 1
  systemctl reset-failed machina-controller >>"$LOG_FILE" 2>&1 || true
  systemctl restart machina-controller >>"$LOG_FILE" 2>&1 || fail "machina-controller failed — journalctl -u machina-controller"

  # Ensure bootstrap host points at local agent.
  if command -v sqlite3 &>/dev/null && [ -f /var/lib/machina/controller.db ]; then
    sqlite3 /var/lib/machina/controller.db \
      "UPDATE hosts SET agent_grpc_addr='127.0.0.1:50051', agent_console_addr='127.0.0.1:50052', state='online'
       WHERE agent_grpc_addr IS NOT NULL;" >>"$LOG_FILE" 2>&1 || true
  fi
  ok "Services started"
}

wait_for_health() {
  step "Controller health check"
  local i r
  for i in $(seq 1 45); do
    r="$(curl -sf http://127.0.0.1:5093/api/v1/health 2>/dev/null || true)"
    if echo "$r" | grep -q '"database":"ok"'; then
      ok "Controller healthy at http://127.0.0.1:5093"
      echo "  $r"
      return 0
    fi
    sleep 2
  done
  journalctl -u machina-controller --no-pager -n 30 2>/dev/null || true
  journalctl -u machina-agent --no-pager -n 20 2>/dev/null || true
  fail "Controller not healthy at http://127.0.0.1:5093/api/v1/health"
}

detect_os
install_binaries
write_platform_env
install_systemd_units
if $DISABLE_FIREWALL; then
  step "Disabling host firewall (firewalld/ufw)"
  if disable_firewalld "$LOG_FILE"; then
    ok "Host firewall stopped and disabled"
  else
    warn "No active firewalld/ufw — continuing"
  fi
else
  open_firewall_port
fi
setup_database
start_services
wait_for_health

ip="$(primary_ipv4)"
echo ""
ok "Platform control plane installed"
echo "  API:  http://${ip}:5093/api/v1/health"
echo "  Logs: journalctl -u machina-controller -f"
echo "  Agent: journalctl -u machina-agent -f"
