#!/bin/bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# machina backup — backup VM configs and optionally disk images
# Supports local and NFS backup targets with retention policies.
#
# Usage:
#   ./scripts/backup.sh                         # Backup XML configs only
#   ./scripts/backup.sh --with-disks            # Backup configs + disk images
#   ./scripts/backup.sh --incremental           # Incremental disk backup (rsync hardlinks)
#   ./scripts/backup.sh --list                  # List what would be backed up
#   ./scripts/backup.sh --restore <dir>         # Restore configs from backup
#   ./scripts/backup.sh --nfs 192.168.1.10:/backups  # Backup to NFS share
#   ./scripts/backup.sh --retain 7              # Keep only last 7 backups
#   ./scripts/backup.sh --config /etc/machina/backup.conf  # Use config file
#   ./scripts/backup.sh --vm myvm                # Backup a single VM only
#   ./scripts/backup.sh --vm myvm --with-disks   # Single VM with disks
#   ./scripts/backup.sh --verify <dir>           # Verify backup checksums
set -eo pipefail

API="${MACHINA_API:-https://localhost:5092/api/v1}"
BACKUP_DIR="${MACHINA_BACKUP_DIR:-$HOME/machina-backups}"
DATE="${MACHINA_BACKUP_ID:-$(date +%Y%m%d-%H%M%S)}"
VM_FILTER=""
NFS_TARGET=""
NFS_MOUNT_POINT="/mnt/machina-backup"
NFS_OPTS="vers=4,soft,timeo=30"
UNMOUNT_AFTER=true
RETAIN=0
CONFIG_FILE=""
LOG_TAG="machina-backup"
INCREMENTAL=false
VERIFY_DIR=""

# Detect if running under systemd (no tty)
if [ -t 1 ]; then
    INTERACTIVE=true
else
    INTERACTIVE=false
fi

info()  {
    echo "ℹ️  $*"
    $INTERACTIVE || logger -t "$LOG_TAG" "INFO: $*"
}
ok()    {
    echo "✅ $*"
    $INTERACTIVE || logger -t "$LOG_TAG" "OK: $*"
}
warn()  {
    echo "⚠️  $*"
    $INTERACTIVE || logger -t "$LOG_TAG" "WARN: $*"
}
fail()  {
    echo "❌ $*"
    $INTERACTIVE || logger -t "$LOG_TAG" "FAIL: $*"
    # Write failed status before exiting
    if [ -n "${BACKUP_PATH:-}" ] && [ -d "$BACKUP_PATH" ]; then
        echo "failed" > "$BACKUP_PATH/backup.status"
        echo "$*" >> "$BACKUP_PATH/backup.status"
    fi
    exit 1
}

WITH_DISKS=false
LIST_ONLY=false
RESTORE_DIR=""

# ── Trap for cleanup on interrupt ─────────────────────────────────

cleanup_on_exit() {
    local exit_code=$?
    if [ $exit_code -ne 0 ] && [ -n "${BACKUP_PATH:-}" ] && [ -d "$BACKUP_PATH" ]; then
        write_status "failed" "Backup interrupted or failed (exit $exit_code)" ""
    fi
    # Always try to unmount NFS on exit
    if [ -n "${NFS_TARGET:-}" ] && mountpoint -q "${NFS_MOUNT_POINT:-/mnt/machina-backup}" 2>/dev/null; then
        sync 2>/dev/null || true
        umount "${NFS_MOUNT_POINT:-/mnt/machina-backup}" 2>/dev/null || true
    fi
    # Release lock
    if [ -n "${LOCK_FD:-}" ]; then
        exec 9>&- 2>/dev/null || true
    fi
}
trap cleanup_on_exit EXIT

# ── Status tracking ──────────────────────────────────────────────────

write_status() {
    local status="$1"
    local message="${2:-}"
    local progress="${3:-}"
    [ -z "${BACKUP_PATH:-}" ] && return 0
    [ -d "$BACKUP_PATH" ] || return 0
    # Atomic write: write to temp file then move
    {
        echo "status=$status"
        echo "message=$message"
        echo "progress=$progress"
        echo "updated=$(date +%Y%m%d-%H%M%S)"
    } > "$BACKUP_PATH/backup.status.tmp" && \
    mv "$BACKUP_PATH/backup.status.tmp" "$BACKUP_PATH/backup.status"
}

# ── Load config file ──────────────────────────────────────────────────

load_config() {
    local conf="$1"
    [ -f "$conf" ] || return 0

    while IFS='=' read -r key value; do
        key=$(echo "$key" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
        [ -z "$key" ] && continue
        [[ "$key" == \#* ]] && continue

        value=$(echo "$value" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//;s/^"//;s/"$//')

        case "$key" in
            api_url)        API="$value" ;;
            backup_dir)     BACKUP_DIR="$value" ;;
            nfs_target)     NFS_TARGET="$value" ;;
            nfs_mount_point) NFS_MOUNT_POINT="$value" ;;
            nfs_opts)       NFS_OPTS="$value" ;;
            unmount_after)  [ "$value" = "true" ] && UNMOUNT_AFTER=true || UNMOUNT_AFTER=false ;;
            with_disks)     [ "$value" = "true" ] && WITH_DISKS=true ;;
            retain)         RETAIN="$value" ;;
        esac
    done < "$conf"

    info "Loaded config from $conf"
}

# ── NFS mount/unmount ─────────────────────────────────────────────────

mount_nfs() {
    [ -z "$NFS_TARGET" ] && return 0

    info "Mounting NFS share: $NFS_TARGET -> $NFS_MOUNT_POINT"
    mkdir -p "$NFS_MOUNT_POINT" || fail "Cannot create NFS mount point: $NFS_MOUNT_POINT"

    if mountpoint -q "$NFS_MOUNT_POINT" 2>/dev/null; then
        local current_src
        current_src=$(findmnt -n -o SOURCE "$NFS_MOUNT_POINT" 2>/dev/null || true)
        if [ "$current_src" = "$NFS_TARGET" ]; then
            ok "NFS already mounted at $NFS_MOUNT_POINT"
        else
            fail "Mount point $NFS_MOUNT_POINT is in use by $current_src (expected $NFS_TARGET)"
        fi
    else
        mount -t nfs -o "$NFS_OPTS" "$NFS_TARGET" "$NFS_MOUNT_POINT" || fail "NFS mount failed: $NFS_TARGET"
        ok "NFS mounted"
    fi

    BACKUP_DIR="$NFS_MOUNT_POINT"
}

unmount_nfs() {
    [ -z "$NFS_TARGET" ] && return 0
    $UNMOUNT_AFTER || return 0

    if mountpoint -q "$NFS_MOUNT_POINT" 2>/dev/null; then
        info "Unmounting NFS share..."
        sync
        if umount "$NFS_MOUNT_POINT"; then
            ok "NFS unmounted"
        else
            warn "NFS unmount failed (may still be busy)"
        fi
    fi
}

# ── Retention (prune old backups) ─────────────────────────────────────

prune_old_backups() {
    if ! [[ "$RETAIN" =~ ^[0-9]+$ ]]; then
        warn "Invalid RETAIN value: $RETAIN (skipping prune)"
        return 0
    fi
    [ "$RETAIN" -le 0 ] && return 0

    info "Applying retention policy: keep last $RETAIN backups"

    local dirs=()
    while IFS= read -r d; do
        [ -d "$d" ] && dirs+=("$d")
    done < <(find "$BACKUP_DIR" -maxdepth 1 -mindepth 1 -type d -name '[0-9]*-[0-9]*' | sort)

    local backup_count=${#dirs[@]}
    if [ "$backup_count" -le "$RETAIN" ]; then
        info "  $backup_count backups exist, nothing to prune"
        return 0
    fi

    local to_remove=$((backup_count - RETAIN))
    info "  Pruning $to_remove old backup(s) (keeping $RETAIN of $backup_count)"

    for ((i = 0; i < to_remove; i++)); do
        local target="${dirs[$i]}"
        info "  Removing: $(basename "$target")"
        rm -rf "$target"
    done

    ok "Pruned $to_remove old backup(s)"
}

# ── Checksums ─────────────────────────────────────────────────────────

generate_checksums() {
    local dir="$1"
    info "Generating checksums..."
    (cd "$dir" && find . -type f ! -name 'checksums.sha256' ! -name 'backup.status' \
        -exec sha256sum {} \; > checksums.sha256)
    local count
    count=$(wc -l < "$dir/checksums.sha256")
    ok "Generated $count checksums"
}

verify_checksums() {
    local dir="$1"
    if [ ! -f "$dir/checksums.sha256" ]; then
        warn "No checksums.sha256 file found in $dir"
        return 1
    fi

    info "Verifying checksums in $dir..."
    local result
    if (cd "$dir" && sha256sum -c checksums.sha256 2>&1); then
        ok "All checksums verified"
        return 0
    else
        warn "Checksum verification failed"
        return 1
    fi
}

# ── Find previous backup for incremental ─────────────────────────────

find_previous_backup() {
    local latest=""
    while IFS= read -r d; do
        [ -d "$d/disks" ] && latest="$d"
    done < <(find "$BACKUP_DIR" -maxdepth 1 -mindepth 1 -type d -name '[0-9]*-[0-9]*' | sort)
    echo "$latest"
}

# ── Parse arguments ──────────────────────────────────────────────────

# First pass: find config file
for arg in "$@"; do
    case "$arg" in
        --config)  CONFIG_FILE="__next__" ;;
        *)
            if [ "$CONFIG_FILE" = "__next__" ]; then
                CONFIG_FILE="$arg"
            fi
            ;;
    esac
done

# Load default config if it exists
if [ -z "$CONFIG_FILE" ] && [ -f /etc/machina/backup.conf ]; then
    load_config /etc/machina/backup.conf
elif [ -n "$CONFIG_FILE" ] && [ "$CONFIG_FILE" != "__next__" ]; then
    load_config "$CONFIG_FILE"
fi

# API auth: the daemon's read APIs (/vms, /networks, /storage/pools, …) require a
# bearer token. Prefer MACHINA_API_TOKEN (env) or api_token (config); otherwise read
# the daemon-provisioned "machina-backup" service token. api-tokens.json only ever
# stores a SHA-256 hash + masked preview of each token (never the usable secret), so
# the raw value lives in a dedicated 0600 sidecar file instead — read that first.
# Without this every backup fails "VM not found" because the unauthenticated GET 401s.
API_TOKEN="${MACHINA_API_TOKEN:-${api_token:-}}"
if [ -z "$API_TOKEN" ] && [ -r /var/lib/machina/service-tokens/machina-backup.token ]; then
    API_TOKEN=$(cat /var/lib/machina/service-tokens/machina-backup.token 2>/dev/null) || API_TOKEN=""
fi
# Fallback for hosts whose daemon hasn't been restarted since upgrading past the
# token-hashing change yet: api-tokens.json may still hold the older plaintext format.
if [ -z "$API_TOKEN" ] && [ -r /var/lib/machina/api-tokens.json ]; then
    API_TOKEN=$(python3 - <<'PY' 2>/dev/null
import json
try:
    d = json.load(open("/var/lib/machina/api-tokens.json"))
    print(next((t.get("token","") for t in d.values() if t.get("name") == "machina-backup"), ""))
except Exception:
    pass
PY
) || API_TOKEN=""
fi
# curl wrapper that attaches the bearer header when a token is available.
# The header is fed to curl as a config read from its own stdin (`-K -`)
# rather than `-H ...` on the command line: argv (including -H values) is
# readable by any local user via `ps`/proc for the life of the process, a
# config piped in never touches argv or disk. (A `-K <(...)` process
# substitution stashed in an array for later expansion is NOT equivalent —
# the fd can already be closed by the time the deferred command runs; the
# pipe must be attached to the same curl invocation that consumes it.)
mcurl() {
    if [ -n "$API_TOKEN" ]; then
        printf 'header = "Authorization: Bearer %s"\n' "$API_TOKEN" | curl -sfk -K - "$@"
    else
        curl -sfk "$@"
    fi
}

# Second pass: CLI args override config
PREV_ARG=""
for arg in "$@"; do
    case "$PREV_ARG" in
        --nfs)      NFS_TARGET="$arg"; PREV_ARG=""; continue ;;
        --retain)   RETAIN="$arg"; PREV_ARG=""; continue ;;
        --restore)  RESTORE_DIR="$arg"; PREV_ARG=""; continue ;;
        --config)   PREV_ARG=""; continue ;;
        --mount-point) NFS_MOUNT_POINT="$arg"; PREV_ARG=""; continue ;;
        --nfs-opts) NFS_OPTS="$arg"; PREV_ARG=""; continue ;;
        --vm)       VM_FILTER="$arg"; PREV_ARG=""; continue ;;
        --verify)   VERIFY_DIR="$arg"; PREV_ARG=""; continue ;;
    esac

    case "$arg" in
        --with-disks)   WITH_DISKS=true ;;
        --incremental)  INCREMENTAL=true; WITH_DISKS=true ;;
        --list)         LIST_ONLY=true ;;
        --no-unmount)   UNMOUNT_AFTER=false ;;
        --unmount)      UNMOUNT_AFTER=true ;;
        --nfs|--retain|--restore|--config|--mount-point|--nfs-opts|--vm|--verify)
            PREV_ARG="$arg" ;;
        --help|-h)
            cat <<'HELPEOF'
Usage: backup.sh [OPTIONS]

Backup modes:
  (default)              Backup XML configs (VMs, networks, pools)
  --vm NAME              Backup a single VM only (config + optionally disk)
  --with-disks           Also copy disk images (can be very large)
  --incremental          Incremental disk backup (hardlinks unchanged files)
  --list                 Show what would be backed up without doing it
  --restore DIR          Restore VM/network/pool configs from a backup directory
  --verify DIR           Verify backup integrity (checksums)

NFS options:
  --nfs SERVER:/PATH     Mount NFS share and backup there
  --mount-point PATH     NFS mount point (default: /mnt/machina-backup)
  --nfs-opts OPTS        NFS mount options (default: vers=4,soft,timeo=30)
  --no-unmount           Leave NFS mounted after backup
  --unmount              Unmount NFS after backup (default)

Retention:
  --retain N             Keep only the last N backups, prune older ones

Config:
  --config FILE          Load config from file (default: /etc/machina/backup.conf)

Environment:
  MACHINA_API          API URL (default: https://localhost:5092/api/v1)
  MACHINA_BACKUP_DIR   Backup root (default: ~/machina-backups)

Timer setup:
  sudo systemctl enable --now machina-backup.timer    # daily backups
  sudo systemctl list-timers machina-backup           # check schedule
  journalctl -u machina-backup.service                # check logs
HELPEOF
            exit 0
            ;;
    esac
done

# --vm is spliced directly into filesystem paths below (vms/$name.xml,
# disks/$vm_name_disk...) — reject anything but a plain VM-name-shaped
# string so a value like "../../etc/cron.d/x" can't escape BACKUP_PATH.
if [ -n "$VM_FILTER" ]; then
    case "$VM_FILTER" in
        */*|*..*) fail "Invalid --vm name: '$VM_FILTER' (must not contain '/' or '..')" ;;
    esac
    [[ "$VM_FILTER" =~ ^[A-Za-z0-9_.-]+$ ]] || fail "Invalid --vm name: '$VM_FILTER' (only letters, digits, '.', '_', '-' allowed)"
fi

# ── Handle verify ────────────────────────────────────────────────────

if [ -n "$VERIFY_DIR" ]; then
    mount_nfs
    verify_result=0
    verify_checksums "$VERIFY_DIR" || verify_result=1
    unmount_nfs
    exit $verify_result
fi

# ── Handle restore ───────────────────────────────────────────────────

if [ -n "$RESTORE_DIR" ]; then
    mount_nfs

    echo "Restoring from $RESTORE_DIR"
    if [ ! -d "$RESTORE_DIR/vms" ]; then
        fail "No vms/ directory in $RESTORE_DIR"
    fi

    # Restore VMs
    info "Restoring VM definitions..."
    for xml in "$RESTORE_DIR"/vms/*.xml; do
        [ -f "$xml" ] || continue
        name=$(basename "$xml" .xml)
        info "Defining VM: $name"
        if virsh define "$xml" 2>&1; then
            ok "  $name defined"
        else
            warn "  $name failed"
        fi
    done

    # Restore networks
    if [ -d "$RESTORE_DIR/networks" ]; then
        info "Restoring network definitions..."
        for xml in "$RESTORE_DIR"/networks/*.xml; do
            [ -f "$xml" ] || continue
            name=$(basename "$xml" .xml)
            info "Defining network: $name"
            if virsh net-define "$xml" 2>&1; then
                ok "  $name defined"
            else
                warn "  $name failed"
            fi
        done
    fi

    # Restore storage pools
    if [ -d "$RESTORE_DIR/pools" ]; then
        info "Restoring storage pool definitions..."
        for xml in "$RESTORE_DIR"/pools/*.xml; do
            [ -f "$xml" ] || continue
            name=$(basename "$xml" .xml)
            info "Defining pool: $name"
            if virsh pool-define "$xml" 2>&1; then
                ok "  $name defined"
            else
                warn "  $name failed"
            fi
        done
    fi

    # Restore disk images if present
    if [ -d "$RESTORE_DIR/disks" ]; then
        info "Restoring disk images..."
        # Build a map of disk basenames to original paths from VM XMLs
        declare -A DISK_DEST_MAP
        # Collect mappings into a variable first, then parse (avoids subshell scoping)
        # NOTE: this whole restore block runs at script top level (not inside a
        # function), so `local` here is a fatal "local: can only be used in a
        # function" error — it previously crashed every `--restore` run that hit
        # a backup with a disks/ directory. Plain assignments only below.
        disk_mappings=""
        for xml in "$RESTORE_DIR"/vms/*.xml; do
            [ -f "$xml" ] || continue
            # Extract disk source paths from XML (grep -oE, either quote style —
            # libvirt's own dumpxml always emits single-quoted attributes, but
            # this accepts double too rather than assume one).
            src_paths=$(grep -oE "source (file|dev)=['\"][^'\"]*['\"]" "$xml" 2>/dev/null \
                | sed -E "s/^source (file|dev)=//; s/^['\"]//; s/['\"]\$//" || true)
            while IFS= read -r src_path; do
                [ -z "$src_path" ] && continue
                bn=$(basename "$src_path")
                vm_bn="$(basename "$xml" .xml)_${bn}"
                disk_mappings="${disk_mappings}${bn}|${src_path}"$'\n'
                disk_mappings="${disk_mappings}${vm_bn}|${src_path}"$'\n'
            done <<< "$src_paths"
            # Network-backed (e.g. Ceph/RBD): `<source protocol='rbd' name='pool/image'>`
            # has neither file= nor dev=, so it was invisible here entirely —
            # every such disk fell through to the wrong "guess a local path"
            # fallback below instead of restoring into its actual RBD image.
            rbd_srcs=$(grep -E "source protocol=['\"]rbd['\"]" "$xml" 2>/dev/null \
                | grep -oE "name=['\"][^'\"]*['\"]" \
                | sed -E "s/^name=//; s/^['\"]//; s/['\"]\$//" || true)
            while IFS= read -r rbd_name; do
                [ -z "$rbd_name" ] && continue
                rbd_bn="${rbd_name//\//_}.qcow2"
                vm_rbd_bn="$(basename "$xml" .xml)_${rbd_bn}"
                disk_mappings="${disk_mappings}${rbd_bn}|rbd:${rbd_name}"$'\n'
                disk_mappings="${disk_mappings}${vm_rbd_bn}|rbd:${rbd_name}"$'\n'
            done <<< "$rbd_srcs"
        done
        while IFS='|' read -r key val; do
            [ -z "$key" ] && continue
            DISK_DEST_MAP["$key"]="$val"
        done <<< "$disk_mappings"

        for disk_file in "$RESTORE_DIR"/disks/*; do
            [ -f "$disk_file" ] || continue
            disk_name=$(basename "$disk_file")
            # Look up original path, fall back to /var/lib/libvirt/images/
            dest="${DISK_DEST_MAP[$disk_name]:-}"
            if [ -z "$dest" ]; then
                # Strip VM name prefix if present (format: vmname_diskfile)
                stripped="${disk_name#*_}"
                dest="${DISK_DEST_MAP[$stripped]:-/var/lib/libvirt/images/$stripped}"
            fi
            case "$dest" in
                rbd:*)
                    # The RBD image is the VM's current live disk — it already
                    # exists, so `-n` skips create (qemu-img would otherwise
                    # try to create a new image and fail: "File exists").
                    info "  Restoring $disk_name -> $dest"
                    if qemu-img convert -n -O raw "$disk_file" "$dest"; then
                        ok "  $disk_name restored"
                    else
                        warn "  $disk_name restore failed"
                    fi
                    ;;
                *)
                    if [ -f "$dest" ]; then
                        warn "  $disk_name already exists at $dest, skipping"
                    else
                        dest_dir=$(dirname "$dest")
                        [ -d "$dest_dir" ] || mkdir -p "$dest_dir"
                        info "  Copying $disk_name -> $dest"
                        if cp "$disk_file" "$dest"; then
                            ok "  $disk_name restored"
                        else
                            warn "  $disk_name copy failed"
                        fi
                    fi
                    ;;
            esac
        done
    fi

    ok "Restore complete"

    unmount_nfs
    exit 0
fi

# ── Mount NFS if configured ──────────────────────────────────────────

mount_nfs

BACKUP_PATH="$BACKUP_DIR/$DATE"

# ── Acquire lock (prevent concurrent backups) ────────────────────────

LOCK_FILE="$BACKUP_DIR/.backup.lock"
mkdir -p "$BACKUP_DIR" || fail "Cannot create backup directory: $BACKUP_DIR"
# .backup.lock is a fixed, predictable name; if BACKUP_DIR is ever
# group/world-writable (e.g. a shared NFS target) a local attacker could
# pre-plant it as a symlink — plain `exec 9>` follows symlinks and would
# truncate/open whatever it points to (this backup usually runs as root).
[ -L "$LOCK_FILE" ] && fail "Refusing to use lock file (symlink detected): $LOCK_FILE"
# Same predictable-path concern for the timestamped backup directory itself.
[ -e "$BACKUP_PATH" ] && [ ! -d "$BACKUP_PATH" ] && fail "Refusing to use backup path (not a plain directory): $BACKUP_PATH"
LOCK_FD=9
exec 9>"$LOCK_FILE" || fail "Cannot create lock file: $LOCK_FILE"
if ! flock -n 9; then
    fail "Another backup is already running (lock: $LOCK_FILE)"
fi

# ── Check daemon ─────────────────────────────────────────────────────

mcurl "$API/health" > /dev/null 2>&1 || fail "Daemon not reachable at $API"

# ── Gather data ──────────────────────────────────────────────────────

if [ -n "$VM_FILTER" ]; then
    VM_DETAIL=$(mcurl "$API/vms/$VM_FILTER" 2>/dev/null) || fail "VM '$VM_FILTER' not found"
    VM_NAMES="$VM_FILTER"
    VM_COUNT=1
    NET_NAMES=""
    NET_COUNT=0
    POOLS=""
else
    VMS=$(mcurl "$API/vms" 2>/dev/null) || fail "Failed to fetch VM list from API"
    NETS=$(mcurl "$API/networks" 2>/dev/null) || fail "Failed to fetch network list from API"
    POOLS=$(mcurl "$API/storage/pools" 2>/dev/null) || fail "Failed to fetch storage pool list from API"

    # Validate JSON responses before parsing
    echo "$VMS" | python3 -c "import json,sys; json.load(sys.stdin)" 2>/dev/null || fail "API returned invalid JSON for VMs"

    VM_NAMES=$(echo "$VMS" | python3 -c "import json,sys; [print(v['name']) for v in json.load(sys.stdin)]" 2>/dev/null)
    NET_NAMES=$(echo "$NETS" | python3 -c "import json,sys; [print(n['name']) for n in json.load(sys.stdin)]" 2>/dev/null)

    VM_COUNT=$(echo "$VM_NAMES" | grep -c . || true)
    NET_COUNT=$(echo "$NET_NAMES" | grep -c . || true)
fi

# Total steps for progress tracking
TOTAL_STEPS=$((VM_COUNT + NET_COUNT + 2)) # +2 for pool+checksums
$WITH_DISKS && TOTAL_STEPS=$((TOTAL_STEPS + VM_COUNT))
CURRENT_STEP=0

update_progress() {
    CURRENT_STEP=$((CURRENT_STEP + 1))
    local pct=$((CURRENT_STEP * 100 / TOTAL_STEPS))
    [ $pct -gt 100 ] && pct=100
    write_status "running" "$1" "$pct"
}

# ── List mode ────────────────────────────────────────────────────────

if $LIST_ONLY; then
    echo "Backup plan"
    echo "  VMs ($VM_COUNT):"
    echo "$VM_NAMES" | while read -r name; do
        [ -z "$name" ] && continue
        echo "    $name"
        if $WITH_DISKS; then
            DETAILS=$(mcurl "$API/vms/$name" 2>/dev/null) || DETAILS=""
            echo "$DETAILS" | python3 -c "
import json,sys
d=json.load(sys.stdin)
for disk in d.get('disks',[]):
    print(f'      disk: {disk[\"source\"]}')
" 2>/dev/null
        fi
    done
    echo "  Networks ($NET_COUNT):"
    echo "$NET_NAMES" | while read -r name; do
        [ -z "$name" ] && continue
        echo "    $name"
    done
    echo ""
    echo "  Backup to: $BACKUP_PATH"
    [ -n "$VM_FILTER" ] && echo "  Mode: single VM ($VM_FILTER)"
    [ -n "$NFS_TARGET" ] && echo "  NFS target: $NFS_TARGET"
    [ "$RETAIN" -gt 0 ] 2>/dev/null && echo "  Retention: keep last $RETAIN"
    $WITH_DISKS && echo "  Disk images will be copied (may be large)"
    $INCREMENTAL && echo "  Incremental mode: hardlink unchanged files"

    unmount_nfs
    exit 0
fi

# ── Create backup ────────────────────────────────────────────────────

echo "machina backup"
[ -n "$VM_FILTER" ] && echo "  VM: $VM_FILTER"
echo "  Destination: $BACKUP_PATH"
[ -n "$NFS_TARGET" ] && echo "  NFS target: $NFS_TARGET"
$INCREMENTAL && echo "  Mode: incremental"
echo ""

mkdir -p "$BACKUP_PATH/vms"
[ -z "$VM_FILTER" ] && mkdir -p "$BACKUP_PATH/networks" "$BACKUP_PATH/pools"

# Write initial status
write_status "running" "Starting backup" "0"

# Backup VM XML configs
info "Backing up $VM_COUNT VM configs..."
while IFS= read -r name; do
    [ -z "$name" ] && continue
    XML=$(mcurl "$API/vms/$name/xml" 2>/dev/null) || XML=""
    if [ -n "$XML" ]; then
        printf '%s\n' "$XML" > "$BACKUP_PATH/vms/$name.xml"
        echo "  $name"
    else
        warn "  Failed to fetch XML for '$name'"
    fi
done <<< "$VM_NAMES"
update_progress "VM configs saved"
ok "VM configs saved"

# Backup network and pool configs (skip for per-VM backup)
if [ -z "$VM_FILTER" ]; then
    info "Backing up $NET_COUNT network configs..."
    while IFS= read -r name; do
        [ -z "$name" ] && continue
        XML=$(mcurl "$API/networks/$name/xml" 2>/dev/null) || XML=""
        if [ -n "$XML" ]; then
            printf '%s\n' "$XML" > "$BACKUP_PATH/networks/$name.xml"
            echo "  $name"
        fi
    done <<< "$NET_NAMES"
    update_progress "Network configs saved"
    ok "Network configs saved"

    POOL_NAMES=$(echo "$POOLS" | python3 -c "import json,sys; [print(p['name']) for p in json.load(sys.stdin)]" 2>/dev/null)
    info "Backing up storage pool configs..."
    while IFS= read -r name; do
        [ -z "$name" ] && continue
        XML=$(mcurl "$API/storage/pools/$name/xml" 2>/dev/null) || XML=""
        if [ -n "$XML" ]; then
            printf '%s\n' "$XML" > "$BACKUP_PATH/pools/$name.xml"
            echo "  $name"
        fi
    done <<< "$POOL_NAMES"
    update_progress "Pool configs saved"
    ok "Pool configs saved"

    # Save JSON snapshots
    echo "$VMS" | python3 -m json.tool > "$BACKUP_PATH/vms.json" 2>/dev/null
    echo "$NETS" | python3 -m json.tool > "$BACKUP_PATH/networks.json" 2>/dev/null
    echo "$POOLS" | python3 -m json.tool > "$BACKUP_PATH/pools.json" 2>/dev/null

    # Save node info
    mcurl "$API/node" 2>/dev/null | python3 -m json.tool > "$BACKUP_PATH/node.json" 2>/dev/null || true
else
    echo "$VM_DETAIL" | python3 -m json.tool > "$BACKUP_PATH/vm-detail.json" 2>/dev/null
fi

# Optionally backup disk images
if $WITH_DISKS; then
    mkdir -p "$BACKUP_PATH/disks"

    # Find previous backup for incremental
    LINK_DEST=""
    if $INCREMENTAL; then
        PREV_BACKUP=$(find_previous_backup)
        if [ -n "$PREV_BACKUP" ] && [ -d "$PREV_BACKUP/disks" ]; then
            LINK_DEST="$PREV_BACKUP/disks"
            info "Incremental: linking against $(basename "$PREV_BACKUP")"
        else
            info "No previous backup with disks found, doing full copy"
        fi
    fi

    info "Backing up disk images (this may take a while)..."

    # Collect all disk paths with VM name prefix to avoid filename collisions
    DISK_LIST=""
    while IFS= read -r name; do
        [ -z "$name" ] && continue
        DETAILS=$(mcurl "$API/vms/$name" 2>/dev/null) || continue
        PATHS=$(echo "$DETAILS" | python3 -c "
import json,sys
d=json.load(sys.stdin)
for disk in d.get('disks',[]):
    print(disk['source'])
" 2>/dev/null)
        while IFS= read -r disk_path; do
            [ -z "$disk_path" ] && continue
            # A network-backed (e.g. Ceph/RBD) disk's source is `rbd:pool/image`,
            # not a local path — `[ -f ]` on it is always false, which used to
            # make this silently skip the disk entirely (one "Disk not found"
            # warning, easy to miss in a multi-VM run) instead of backing it up.
            case "$disk_path" in
                rbd:*) ;;
                *) [ -f "$disk_path" ] || { warn "  Disk not found: $disk_path"; continue; } ;;
            esac
            DISK_LIST="${DISK_LIST}${name}|${disk_path}"$'\n'
        done <<< "$PATHS"
    done <<< "$VM_NAMES"

    while IFS='|' read -r vm_name disk_path; do
        [ -z "$disk_path" ] && continue
        case "$disk_path" in
            rbd:*)
                # No local file to cp/rsync — qemu-img reads an rbd: source
                # directly (relies on the hypervisor's /etc/ceph/ceph.conf +
                # keyring for monitor/auth, same as everywhere else in this
                # codebase that touches RBD). Always a full copy: qcow2-file
                # rsync --link-dest hardlinking has no equivalent against a
                # raw Ceph image.
                disk_basename="${disk_path#rbd:}"
                disk_basename="${disk_basename//\//_}"
                disk_name="${vm_name}_${disk_basename}.qcow2"
                write_status "running" "Copying disk: $disk_basename ($vm_name, rbd)" ""
                echo "  Converting: $disk_path -> $BACKUP_PATH/disks/$disk_name"
                qemu-img convert -O qcow2 "$disk_path" "$BACKUP_PATH/disks/$disk_name"
                ;;
            *)
                disk_basename=$(basename "$disk_path")
                # Prefix with VM name to avoid collisions between VMs
                disk_name="${vm_name}_${disk_basename}"
                write_status "running" "Copying disk: $disk_basename ($vm_name)" ""

                if [ -n "$LINK_DEST" ]; then
                    echo "  Syncing: $disk_path (incremental)"
                    rsync -a --link-dest="$LINK_DEST" "$disk_path" "$BACKUP_PATH/disks/$disk_name"
                else
                    echo "  Copying: $disk_path"
                    cp "$disk_path" "$BACKUP_PATH/disks/$disk_name"
                fi
                ;;
        esac
    done <<< "$DISK_LIST"
    update_progress "Disk images saved"
    ok "Disk images saved"
fi

# Save backup metadata
POOL_COUNT=0
if [ -z "$VM_FILTER" ] && [ -n "$POOL_NAMES" ]; then
    POOL_COUNT=$(echo "$POOL_NAMES" | grep -c . || true)
fi

cat > "$BACKUP_PATH/backup.meta" <<META
timestamp=$DATE
hostname=$(hostname)
api_url=$API
with_disks=$WITH_DISKS
incremental=$INCREMENTAL
nfs_target=${NFS_TARGET:-local}
vm_filter=${VM_FILTER:-all}
vm_count=$VM_COUNT
net_count=$NET_COUNT
pool_count=$POOL_COUNT
META

# Generate checksums
generate_checksums "$BACKUP_PATH"

# ── Summary ──────────────────────────────────────────────────────────

TOTAL_SIZE=$(du -sh "$BACKUP_PATH" 2>/dev/null | cut -f1)
FILE_COUNT=$(find "$BACKUP_PATH" -type f | wc -l)

echo ""
echo "Backup complete"
echo "  Location: $BACKUP_PATH"
echo "  Files:    $FILE_COUNT"
echo "  Size:     $TOTAL_SIZE"
$INCREMENTAL && echo "  Mode:     incremental"
echo ""
echo "  Restore:  $0 --restore $BACKUP_PATH"
echo "  Verify:   $0 --verify $BACKUP_PATH"

# Write completed status
write_status "completed" "Backup complete: $FILE_COUNT files, $TOTAL_SIZE" "100"

# ── Prune old backups ────────────────────────────────────────────────

prune_old_backups

# ── Cleanup NFS ──────────────────────────────────────────────────────

unmount_nfs
