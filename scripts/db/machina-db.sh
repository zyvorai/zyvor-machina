#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

#
# machina-db: choose, set up, inspect and back up the controller's database.
#
#   machina-db setup pod [--port 5432] [--migrate]   managed PostgreSQL in a Podman container (systemd quadlet), generated password
#   machina-db setup package [--migrate]             PostgreSQL from the distribution's packages (apt or dnf)
#   machina-db setup external --url URL [--migrate]  use your own PostgreSQL server (RDS, Patroni, ...)
#   machina-db setup sqlite                     the embedded default (what a fresh install uses)
#   machina-db status                           backend, connection, size, controller view
#   machina-db url                              the configured DATABASE_URL, password hidden
#   machina-db backup [--out DIR] [--keep DAYS] dump (PostgreSQL) or online copy (SQLite)
#   machina-db restore FILE --yes               restore a PostgreSQL dump (stops the controller while it runs)
#
# `setup` writes DATABASE_URL into /etc/default/machina-platform and, for PostgreSQL, a systemd drop-in so the controller runs its
# PostgreSQL build (machina-controller-pg). A PostgreSQL controller starts empty unless you pass --migrate: that stops the controller,
# copies the current SQLite database into the new PostgreSQL one with machina-dbtool (row counts are compared), and starts the
# controller on it. The SQLite file is left untouched, so going back is `machina-db setup sqlite`.
#
# To try it without touching a real install, put --sandbox DIR first: every file, the unit/container name and the port live under
# DIR (or carry a zz- prefix) and the controller is never touched. Prefer it over the environment overrides below, which
# `sudo` usually drops, silently leaving the real paths in effect:
#   sudo machina-db --sandbox /var/tmp/zz-db setup pod --port 15440
# Environment overrides (for tests that run the script as the same user):
#   MACHINA_PLATFORM_ENV, MACHINA_DB_DIR, MACHINA_DB_UNIT, MACHINA_DB_CONTAINER, MACHINA_DB_QUADLET_DIR, MACHINA_DB_DROPIN_DIR,
#   MACHINA_DB_PW_FILE, MACHINA_DB_BACKUP_DIR, MACHINA_DB_NO_CONTROLLER=1 (do not touch the controller unit or restart it)
set -euo pipefail

SANDBOX=""
if [[ "${1:-}" == "--sandbox" ]]; then
  SANDBOX="${2:?--sandbox needs a directory}"
  shift 2
  mkdir -p "$SANDBOX"
  SANDBOX="$(cd "$SANDBOX" && pwd)"
  export MACHINA_PLATFORM_ENV="$SANDBOX/platform.env" MACHINA_DB_DIR="$SANDBOX/state" MACHINA_DB_PW_FILE="$SANDBOX/postgres.password"
  export MACHINA_DB_UNIT="zz-machina-postgres" MACHINA_DB_CONTAINER="zz-machina-postgres" MACHINA_DB_BACKUP_DIR="$SANDBOX/backups"
  export MACHINA_DB_DROPIN_DIR="$SANDBOX/dropin" MACHINA_DB_NO_CONTROLLER=1
  mkdir -p "$MACHINA_DB_DIR"
fi

ENV_FILE="${MACHINA_PLATFORM_ENV:-/etc/default/machina-platform}"
STATE_DIR="${MACHINA_DB_DIR:-/var/lib/machina}"
PG_DATA="$STATE_DIR/postgres"
PW_FILE="${MACHINA_DB_PW_FILE:-/etc/machina/postgres.password}"
UNIT="${MACHINA_DB_UNIT:-machina-postgres}"
CONTAINER="${MACHINA_DB_CONTAINER:-machina-postgres}"
QUADLET_DIR="${MACHINA_DB_QUADLET_DIR:-/etc/containers/systemd}"
DROPIN_DIR="${MACHINA_DB_DROPIN_DIR:-/etc/systemd/system/machina-controller.service.d}"
BACKUP_DIR="${MACHINA_DB_BACKUP_DIR:-/var/backups/machina/db}"
IMAGE="${MACHINA_DB_IMAGE:-docker.io/library/postgres:16}"
DB_NAME=machina
DB_USER=machina
SQLITE_URL="sqlite:///var/lib/machina/controller.db"

say() { printf '%s\n' "$*"; }
warn() { printf 'machina-db: %s\n' "$*" >&2; }
die() { warn "$*"; exit 1; }
need_root() { [[ "${MACHINA_DB_ALLOW_USER:-0}" == 1 || $EUID -eq 0 ]] || die "run as root (sudo)"; }
have() { command -v "$1" >/dev/null 2>&1; }

# ---- the platform env file -------------------------------------------------------------------------------------------------------
get_env() { # KEY -> value ('' if unset)
  [[ -f "$ENV_FILE" ]] || return 0
  sed -n "s/^$1=//p" "$ENV_FILE" | tail -1
}

set_env() { # KEY VALUE  (replace in place or append; keeps the file private)
  local key="$1" value="$2" tmp
  mkdir -p "$(dirname "$ENV_FILE")"
  [[ -f "$ENV_FILE" ]] || (umask 077 && : >"$ENV_FILE")
  tmp="$(mktemp "${ENV_FILE}.XXXXXX")"
  chmod 600 "$tmp"
  if grep -q "^${key}=" "$ENV_FILE"; then
    # awk, not sed: the value may contain characters sed treats specially
    awk -v k="$key" -v v="$value" 'BEGIN{FS=OFS="="} $1==k {print k "=" v; next} {print}' "$ENV_FILE" >"$tmp"
  else
    cat "$ENV_FILE" >"$tmp"
    printf '%s=%s\n' "$key" "$value" >>"$tmp"
  fi
  mv "$tmp" "$ENV_FILE"
  chmod 600 "$ENV_FILE"
}

redact_url() { # scheme://user:password@host/db -> password hidden
  sed -E 's#^([a-z]+://[^:/@]+):[^@]*@#\1:<hidden>@#' <<<"$1"
}

backend_of() { case "$1" in postgres://*|postgresql://*) echo postgres ;; sqlite:*|"") echo sqlite ;; *) echo unknown ;; esac; }
current_url() { local u; u="$(get_env DATABASE_URL)"; echo "${u:-$SQLITE_URL}"; }

rand_pw() { head -c 48 /dev/urandom | od -An -tx1 | tr -d ' \n' | head -c 32; }

ensure_pw() { # generate once, keep forever (0600); prints it
  if [[ ! -s "$PW_FILE" ]]; then
    mkdir -p "$(dirname "$PW_FILE")"
    (umask 077 && rand_pw >"$PW_FILE")
  fi
  cat "$PW_FILE"
}

# ---- the controller unit ---------------------------------------------------------------------------------------------------------
controller_bin_pg() { command -v machina-controller-pg 2>/dev/null || true; }

write_dropin() { # [after-unit]
  [[ "${MACHINA_DB_NO_CONTROLLER:-0}" == 1 ]] && return 0
  local bin after="${1:-}"
  bin="$(controller_bin_pg)"
  [[ -n "$bin" ]] || die "machina-controller-pg is not installed (the PostgreSQL build of the controller); install it next to machina-controller"
  mkdir -p "$DROPIN_DIR"
  {
    echo "# Written by machina-db: run the controller's PostgreSQL build. Remove this file (or: machina-db setup sqlite) to go back."
    echo "[Unit]"
    if [[ -n "$after" ]]; then echo "After=$after"; echo "Wants=$after"; fi
    echo "[Service]"
    echo "ExecStart="
    echo "ExecStart=$bin --host 127.0.0.1 --port 5093"
  } >"$DROPIN_DIR/10-database.conf"
}

remove_dropin() { [[ "${MACHINA_DB_NO_CONTROLLER:-0}" == 1 ]] || rm -f "$DROPIN_DIR/10-database.conf"; }

restart_controller() {
  [[ "${MACHINA_DB_NO_CONTROLLER:-0}" == 1 ]] && return 0
  have systemctl || return 0
  systemctl daemon-reload || true
  if systemctl is-enabled machina-controller.service >/dev/null 2>&1; then
    say "restarting machina-controller ..."
    systemctl restart machina-controller.service
  fi
}

# ---- waiting for PostgreSQL ------------------------------------------------------------------------------------------------------
wait_pg_pod() { # up to ~2 minutes
  for _ in $(seq 1 60); do
    if podman exec "$CONTAINER" pg_isready -U "$DB_USER" -d "$DB_NAME" >/dev/null 2>&1; then return 0; fi
    sleep 2
  done
  return 1
}

tcp_open() { # host port
  (exec 3<>"/dev/tcp/$1/$2") >/dev/null 2>&1
}

url_host() { sed -E 's#^[a-z]+://([^@]*@)?([^:/?]+).*#\2#' <<<"$1"; }
url_port() { local p; p="$(sed -nE 's#^[a-z]+://([^@]*@)?[^:/?]+:([0-9]+).*#\2#p' <<<"$1")"; echo "${p:-5432}"; }

# ---- setup -----------------------------------------------------------------------------------------------------------------------
announce() { say "machina-db: ${SANDBOX:+SANDBOX $SANDBOX: }env file $ENV_FILE, data under $STATE_DIR"; }

# --migrate: copy the SQLite database the controller has been using into the (new, still empty) PostgreSQL database `$1`
migrate_sqlite_into() {
  local to="$1" from="$OLD_URL" tool
  [[ "$(backend_of "$from")" == sqlite ]] || die "--migrate copies from SQLite, but the current database is $(redact_url "$from")"
  local file="${from#sqlite://}"; file="${file%%\?*}"
  [[ -f "$file" ]] || die "--migrate: no SQLite database at $file"
  tool="$(command -v machina-dbtool 2>/dev/null || true)"
  [[ -n "$tool" ]] || tool="$(dirname "$0")/../../target/release/machina-dbtool"
  [[ -x "$tool" ]] || die "machina-dbtool not found (it ships with the controller package; from source: cargo build --release -p machina-dbtool)"
  if [[ "${MACHINA_DB_NO_CONTROLLER:-0}" != 1 ]]; then systemctl stop machina-controller.service 2>/dev/null || true; fi
  say "copying $file into PostgreSQL ..."
  "$tool" copy --from "$from" --to "$to" --yes || die "the copy failed; the controller was NOT switched (still on SQLite, now stopped: systemctl start machina-controller)"
}

setup_sqlite() {
  need_root
  announce
  set_env DATABASE_URL "$SQLITE_URL"
  remove_dropin
  restart_controller
  say "database: embedded SQLite ($SQLITE_URL)"
}

setup_pod() {
  need_root
  announce
  local port=5432 migrate=0
  while [[ $# -gt 0 ]]; do
    case "$1" in --port) port="${2:?}"; shift 2 ;; --migrate) migrate=1; shift ;; *) die "unknown option for setup pod: $1" ;; esac
  done
  have podman || die "podman is required for the managed pod (apt install podman / dnf install podman), or use: setup package | setup external"
  local pw url
  pw="$(ensure_pw)"
  mkdir -p "$PG_DATA" "$QUADLET_DIR"
  chmod 700 "$PG_DATA"
  # the container reads its superuser password from a root-only env file, never from the unit text
  (umask 077 && printf 'POSTGRES_PASSWORD=%s\n' "$pw" >"$STATE_DIR/postgres.env")
  cat >"$QUADLET_DIR/$UNIT.container" <<EOF
# Written by machina-db: the controller's PostgreSQL. Data: $PG_DATA
[Unit]
Description=Machina PostgreSQL (managed by machina-db)
After=network-online.target
Wants=network-online.target

[Container]
Image=$IMAGE
ContainerName=$CONTAINER
Environment=POSTGRES_USER=$DB_USER
Environment=POSTGRES_DB=$DB_NAME
EnvironmentFile=$STATE_DIR/postgres.env
PublishPort=127.0.0.1:$port:5432
Volume=$PG_DATA:/var/lib/postgresql/data:Z
HealthCmd=pg_isready -U $DB_USER -d $DB_NAME
HealthInterval=10s
HealthRetries=6

[Service]
Restart=always
TimeoutStartSec=300

[Install]
WantedBy=multi-user.target
EOF
  systemctl daemon-reload
  say "starting PostgreSQL (pulling $IMAGE the first time) ..."
  # `systemctl start` can report failure while the first image pull and database initialisation are still running; what counts is
  # whether PostgreSQL answers
  systemctl start "$UNIT.service" || warn "systemctl start reported a problem; waiting to see whether PostgreSQL comes up anyway"
  wait_pg_pod || { journalctl -u "$UNIT.service" -n 20 --no-pager >&2 || true; die "PostgreSQL did not become ready; see: journalctl -u $UNIT"; }
  url="postgres://$DB_USER:$pw@127.0.0.1:$port/$DB_NAME"
  if [[ $migrate == 1 ]]; then migrate_sqlite_into "$url"; fi
  set_env DATABASE_URL "$url"
  write_dropin "$UNIT.service"
  install_backup_timer
  restart_controller
  say "database: managed PostgreSQL pod on 127.0.0.1:$port ($(redact_url "$url")); daily backups to $BACKUP_DIR"
}

setup_package() {
  need_root
  announce
  local migrate=0
  for a in "$@"; do [[ "$a" == --migrate ]] && migrate=1; done
  [[ -z "$SANDBOX" ]] || die "setup package installs system packages and cannot run in a sandbox"
  local pw url
  pw="$(ensure_pw)"
  if have apt-get; then
    DEBIAN_FRONTEND=noninteractive apt-get install -y postgresql
  elif have dnf; then
    dnf install -y postgresql-server
    [[ -d /var/lib/pgsql/data/base ]] || postgresql-setup --initdb
  else
    die "no apt-get or dnf found: install PostgreSQL yourself and use: setup external --url ..."
  fi
  systemctl enable --now postgresql
  # CREATE ROLE/DATABASE are idempotent here: the password is reset to the stored one, an existing database is kept
  runuser -u postgres -- psql -v ON_ERROR_STOP=1 -qc "DO \$\$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='$DB_USER') THEN CREATE ROLE $DB_USER LOGIN; END IF; END \$\$;" \
    -c "ALTER ROLE $DB_USER PASSWORD '$pw'"
  runuser -u postgres -- psql -tAc "SELECT 1 FROM pg_database WHERE datname='$DB_NAME'" | grep -q 1 \
    || runuser -u postgres -- psql -qc "CREATE DATABASE $DB_NAME OWNER $DB_USER"
  # Red Hat family defaults to ident for 127.0.0.1; allow password logins from this host for the machina role
  local hba
  hba="$(runuser -u postgres -- psql -tAc 'SHOW hba_file')"
  grep -q "machina-db" "$hba" || { printf '# machina-db\nhost %s %s 127.0.0.1/32 scram-sha-256\n' "$DB_NAME" "$DB_USER" | cat - "$hba" >"$hba.new" && cat "$hba.new" >"$hba" && rm -f "$hba.new"; }
  systemctl reload postgresql
  url="postgres://$DB_USER:$pw@127.0.0.1:5432/$DB_NAME"
  if [[ $migrate == 1 ]]; then migrate_sqlite_into "$url"; fi
  set_env DATABASE_URL "$url"
  write_dropin "postgresql.service"
  install_backup_timer
  restart_controller
  say "database: PostgreSQL from distribution packages ($(redact_url "$url")); daily backups to $BACKUP_DIR"
}

setup_external() {
  need_root
  announce
  local url="" migrate=0
  while [[ $# -gt 0 ]]; do
    case "$1" in --url) url="${2:?}"; shift 2 ;; --migrate) migrate=1; shift ;; *) die "unknown option for setup external: $1" ;; esac
  done
  [[ "$(backend_of "$url")" == postgres ]] || die "--url must be a postgres:// URL (postgres://user:password@host:5432/database)"
  local host port
  host="$(url_host "$url")"; port="$(url_port "$url")"
  if have psql; then
    psql "$url" -qAtc 'select 1' >/dev/null 2>&1 || die "cannot log in with that URL (checked with psql)"
  else
    tcp_open "$host" "$port" || die "cannot reach $host:$port"
    warn "psql is not installed, so only the connection to $host:$port was checked; the controller verifies the login when it starts"
  fi
  if [[ $migrate == 1 ]]; then migrate_sqlite_into "$url"; fi
  set_env DATABASE_URL "$url"
  write_dropin ""
  restart_controller
  say "database: external PostgreSQL ($(redact_url "$url")). Back it up with your own tooling; machina-db backup also works if pg_dump is installed."
}

install_backup_timer() {
  [[ "${MACHINA_DB_NO_CONTROLLER:-0}" == 1 ]] && return 0
  local bin; bin="$(command -v machina-db 2>/dev/null || echo "$0")"
  cat >/etc/systemd/system/machina-db-backup.service <<EOF
[Unit]
Description=Machina database backup

[Service]
Type=oneshot
ExecStart=$bin backup
EOF
  cat >/etc/systemd/system/machina-db-backup.timer <<EOF
[Unit]
Description=Daily Machina database backup

[Timer]
OnCalendar=daily
RandomizedDelaySec=1h
Persistent=true

[Install]
WantedBy=timers.target
EOF
  systemctl daemon-reload
  systemctl enable --now machina-db-backup.timer >/dev/null 2>&1 || true
}

# ---- status / url / backup / restore ---------------------------------------------------------------------------------------------
cmd_url() { redact_url "$(current_url)"; }

cmd_status() {
  local url backend host port
  url="$(current_url)"; backend="$(backend_of "$url")"
  say "backend:      $backend"
  say "DATABASE_URL: $(redact_url "$url")"
  case "$backend" in
    sqlite)
      local f="${url#sqlite://}"; f="${f%%\?*}"
      if [[ -f "$f" ]]; then say "file:         $f ($(du -h "$f" | cut -f1))"; else say "file:         $f (not created yet: it appears when the controller first starts)"; fi
      ;;
    postgres)
      host="$(url_host "$url")"; port="$(url_port "$url")"
      if tcp_open "$host" "$port"; then say "server:       reachable at $host:$port"; else say "server:       NOT reachable at $host:$port"; fi
      if have psql && psql "$url" -qAtc 'select 1' >/dev/null 2>&1; then
        say "login:        ok ($(psql "$url" -qAtc 'show server_version'), database $(psql "$url" -qAtc "select pg_size_pretty(pg_database_size(current_database()))"))"
        say "migrations:   $(psql "$url" -qAtc 'select count(*) from _sqlx_migrations' 2>/dev/null || echo 'none yet') applied"
      elif have podman && podman exec "$CONTAINER" pg_isready -U "$DB_USER" -d "$DB_NAME" >/dev/null 2>&1; then
        say "pod:          $(podman exec "$CONTAINER" psql -U "$DB_USER" -d "$DB_NAME" -qAtc "select 'ok, ' || pg_size_pretty(pg_database_size(current_database()))")"
      fi
      if have systemctl && systemctl list-unit-files "$UNIT.service" >/dev/null 2>&1 && systemctl cat "$UNIT.service" >/dev/null 2>&1; then
        say "pod unit:     $(systemctl is-active "$UNIT.service" || true)"
      fi
      ;;
  esac
  if [[ -f "$DROPIN_DIR/10-database.conf" ]]; then say "controller:   runs machina-controller-pg (drop-in $DROPIN_DIR/10-database.conf)"; else say "controller:   runs the default build"; fi
  if have curl; then
    local h; h="$(curl -fsS -m 3 http://127.0.0.1:5093/api/v1/health 2>/dev/null || true)"
    if [[ -n "$h" ]]; then say "health:       $h"; fi
  fi
}

cmd_backup() {
  local out="$BACKUP_DIR" keep=14
  while [[ $# -gt 0 ]]; do
    case "$1" in --out) out="${2:?}"; shift 2 ;; --keep) keep="${2:?}"; shift 2 ;; *) die "unknown option for backup: $1" ;; esac
  done
  local url backend stamp file
  url="$(current_url)"; backend="$(backend_of "$url")"; stamp="$(date -u +%Y%m%dT%H%M%SZ)"
  mkdir -p "$out"; chmod 700 "$out" 2>/dev/null || true
  case "$backend" in
    sqlite)
      local f="${url#sqlite://}"; f="${f%%\?*}"
      [[ -f "$f" ]] || die "no database file at $f"
      file="$out/controller-$stamp.sqlite"
      if have sqlite3; then sqlite3 "$f" ".backup '$file'"; else die "sqlite3 is required for an online SQLite backup (apt install sqlite3)"; fi
      ;;
    postgres)
      file="$out/controller-$stamp.dump"
      if have pg_dump; then
        pg_dump -Fc "$url" >"$file"
      elif have podman && podman exec "$CONTAINER" true >/dev/null 2>&1; then
        podman exec "$CONTAINER" pg_dump -U "$DB_USER" -Fc "$DB_NAME" >"$file"
      else
        die "pg_dump not found (and no managed pod): install postgresql-client"
      fi
      ;;
    *) die "unknown database backend for $(redact_url "$url")" ;;
  esac
  chmod 600 "$file"
  find "$out" -maxdepth 1 -type f \( -name 'controller-*.dump' -o -name 'controller-*.sqlite' \) -mtime +"$keep" -delete 2>/dev/null || true
  say "backup: $file ($(du -h "$file" | cut -f1)), keeping $keep days"
}

cmd_restore() {
  local file="" yes=0
  while [[ $# -gt 0 ]]; do
    case "$1" in --yes) yes=1; shift ;; -*) die "unknown option for restore: $1" ;; *) file="$1"; shift ;; esac
  done
  [[ -n "$file" && -f "$file" ]] || die "usage: machina-db restore FILE --yes"
  [[ $yes == 1 ]] || die "restore replaces the current database contents; pass --yes to confirm"
  need_root
  local url; url="$(current_url)"
  [[ "$(backend_of "$url")" == postgres ]] || die "restore is for the PostgreSQL backend; for SQLite stop the controller and copy the file back"
  [[ "${MACHINA_DB_NO_CONTROLLER:-0}" == 1 ]] || systemctl stop machina-controller.service 2>/dev/null || true
  if have pg_restore; then
    pg_restore --clean --if-exists --no-owner -d "$url" "$file"
  elif have podman && podman exec "$CONTAINER" true >/dev/null 2>&1; then
    podman exec -i "$CONTAINER" pg_restore -U "$DB_USER" --clean --if-exists --no-owner -d "$DB_NAME" <"$file"
  else
    die "pg_restore not found"
  fi
  restart_controller
  say "restored from $file"
}

usage() { sed -n '4,17p' "$0" | sed 's/^# \{0,1\}//'; }

main() {
  local cmd="${1:-help}"; shift || true
  case "$cmd" in
    setup)
      local mode="${1:-}"; shift || true
      case "$mode" in
        pod) setup_pod "$@" ;;
        package) setup_package "$@" ;;
        external) setup_external "$@" ;;
        sqlite) setup_sqlite "$@" ;;
        *) die "usage: machina-db setup pod|package|external|sqlite" ;;
      esac ;;
    status) cmd_status ;;
    url) cmd_url ;;
    backup) cmd_backup "$@" ;;
    restore) cmd_restore "$@" ;;
    help|-h|--help) usage ;;
    *) die "unknown command: $cmd (try: machina-db help)" ;;
  esac
}

OLD_URL="$(current_url)"   # the database in use before a setup changes it (the source for --migrate)
main "$@"
