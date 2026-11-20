#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Controller database durability and failover with Litestream in a Podman pod.
#
#   machina-ha.sh start  <replica-url>   install the config and start the replication pod
#   machina-ha.sh status                  replication generations / lag
#   machina-ha.sh restore <db-path>       restore the newest replica copy to <db-path> (default: a scratch file)
#   machina-ha.sh promote                 on the STANDBY: restore the replica and start the controller here
#   machina-ha.sh drill                   prove it end to end on a scratch database (safe; touches nothing real)
#
# Failover model: one active controller, one standby on another host. The standby promotes only when the primary
# is confirmed down (or --force). The leadership epoch (see docs/controller-ha.md) bumps on takeover, so agents
# refuse the old primary if it comes back. Each controller needs a unique MACHINA_CONTROLLER_ID.
set -euo pipefail

IMAGE="${LITESTREAM_IMAGE:-docker.io/litestream/litestream:0.3.13}"
DB="${MACHINA_DB:-/var/lib/machina/controller.db}"
CONF_DIR="${MACHINA_CONF_DIR:-/etc/machina}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTRIB="$HERE/../../contrib/ha"
die() { echo "error: $*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "$1 is required"; }
need podman

cmd="${1:-}"; shift || true
case "$cmd" in
  start)
    url="${1:-}"; [ -n "$url" ] || die "usage: $0 start <replica-url>   (e.g. s3://bucket/machina/controller)"
    sudo install -d "$CONF_DIR"
    sudo install -m 0644 "$CONTRIB/litestream.yml" "$CONF_DIR/litestream.yml"
    sed "s#REPLICA_URL_PLACEHOLDER#$url#" "$CONTRIB/machina-ha.kube.yaml" | sudo podman kube play --replace -
    echo "replication pod started: sudo podman pod ps"
    ;;
  status)
    sudo podman pod ps --filter name=machina-ha || true
    sudo podman run --rm -v "$CONF_DIR/litestream.yml:/etc/litestream.yml:ro" -e MACHINA_HA_REPLICA_URL="${MACHINA_HA_REPLICA_URL:-}" \
      "$IMAGE" generations -config /etc/litestream.yml "$DB"
    ;;
  restore)
    out="${1:-/tmp/machina-restored.db}"
    sudo podman run --rm -v "$(dirname "$out"):/out" -v "$CONF_DIR/litestream.yml:/etc/litestream.yml:ro" \
      -e MACHINA_HA_REPLICA_URL="${MACHINA_HA_REPLICA_URL:-}" "$IMAGE" \
      restore -config /etc/litestream.yml -o "/out/$(basename "$out")" "$DB"
    echo "restored to $out"
    ;;
  promote)
    force=0; [ "${1:-}" = "--force" ] && force=1
    primary="${MACHINA_PRIMARY_URL:-}"
    if [ -n "$primary" ] && [ "$force" = 0 ] && curl -sk --max-time 5 "$primary/api/v1/health" >/dev/null; then
      die "the primary at $primary still answers — stop it first, or pass --force (you risk two controllers)"
    fi
    [ -n "${MACHINA_CONTROLLER_ID:-}" ] || die "set a unique MACHINA_CONTROLLER_ID for this standby before promoting"
    sudo systemctl stop machina-controller 2>/dev/null || true
    sudo cp -a "$DB" "$DB.pre-promote.$(date +%s)" 2>/dev/null || true
    sudo rm -f "$DB" "$DB-wal" "$DB-shm"
    "$0" restore "$DB"
    sudo systemctl start machina-controller
    echo "promoted: this host now runs the controller. Agents will refuse the old primary (epoch fencing)."
    ;;
  drill)
    work="$(mktemp -d)"; trap 'sudo rm -rf "$work"' EXIT
    mkdir -p "$work/state" "$work/replica" "$work/out"
    python3 - "$work/state/controller.db" <<'PY'
import sqlite3, sys
c = sqlite3.connect(sys.argv[1]); c.execute("PRAGMA journal_mode=WAL")
c.execute("CREATE TABLE vms (id INTEGER PRIMARY KEY, name TEXT)")
c.executemany("INSERT INTO vms (name) VALUES (?)", [(f"vm-{i}",) for i in range(200)]); c.commit()
PY
    sudo podman rm -f machina-ha-drill >/dev/null 2>&1 || true
    sudo podman run -d --network none --name machina-ha-drill -v "$work/state:/var/lib/machina" -v "$work/replica:/replica" \
      -v "$CONTRIB/litestream-file.yml:/etc/litestream.yml:ro" "$IMAGE" replicate -config /etc/litestream.yml >/dev/null
    sleep 3
    python3 - "$work/state/controller.db" <<'PY'
import sqlite3, sys
c = sqlite3.connect(sys.argv[1]); c.execute("INSERT INTO vms (name) VALUES ('written-after-start')"); c.commit()
PY
    sleep 4
    sudo podman rm -f machina-ha-drill >/dev/null    # the "primary" dies
    sudo podman run --rm --network none -v "$work/out:/out" -v "$work/replica:/replica" -v "$CONTRIB/litestream-file.yml:/etc/litestream.yml:ro" \
      "$IMAGE" restore -config /etc/litestream.yml -o /out/restored.db /var/lib/machina/controller.db >/dev/null
    sudo chmod -R a+rX "$work/out"
    n="$(python3 -c "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); print(c.execute('select count(*) from vms').fetchone()[0], c.execute(\"select count(*) from vms where name='written-after-start'\").fetchone()[0])" "$work/out/restored.db")"
    [ "$n" = "201 1" ] && echo "drill OK: restored 201 rows including the write made after replication started" \
      || die "drill FAILED: expected '201 1', got '$n'"
    ;;
  *) sed -n '2,12p' "$0"; exit 2 ;;
esac
