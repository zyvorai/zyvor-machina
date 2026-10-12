#!/bin/sh
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0
#
# Machina bare-metal image deploy. Runs inside the Machina ramdisk (machina-metal-run
# fetches it from the controller). Reads the deploy plan from the URL in $MACHINA_PLAN,
# downloads the OS image, verifies its checksum, writes it to the chosen disk, seeds
# cloud-init (NoCloud) with the first-boot join, tells the controller, and reboots.
#
#   deploy.sh --print-plan FILE   parse a plan file and print what would happen (no writes)
#
# Environment: MACHINA_PLAN, MACHINA_TOKEN, MACHINA_CACERT (optional, set by the runner).
set -eu

CURL="curl -fsSL --retry 5 --retry-delay 3"
[ -n "${MACHINA_CACERT:-}" ] && CURL="$CURL --cacert $MACHINA_CACERT"

say() { echo "machina-deploy: $*"; }

# plan_get KEY -> value of a top-level string/number key in the plan JSON
plan_get() {
    python3 -c 'import json,sys
d=json.load(open(sys.argv[1])); v=d.get(sys.argv[2],"")
print(v if not isinstance(v,(dict,list)) else json.dumps(v))' "$PLAN_FILE" "$1"
}

report() {
    # report STATUS MESSAGE  (best effort; the controller decides what to do)
    [ -n "${COMPLETE_URL:-}" ] || return 0
    python3 -c 'import json,sys;print(json.dumps({"status":sys.argv[1],"message":sys.argv[2]}))' "$1" "$2" |
        $CURL -X POST -H "Authorization: Bearer ${MACHINA_TOKEN:-}" -H 'Content-Type: application/json' \
            --data-binary @- "$COMPLETE_URL" >/dev/null 2>&1 || true
}

die() {
    say "FAILED: $*"
    report failed "$*"
    sleep "${MACHINA_METAL_FAIL_PAUSE:-300}"
    exit 1
}

if [ "${1:-}" = "--print-plan" ]; then
    PLAN_FILE=$2
    for k in image_url format size_bytes checksum checksum_algo target_disk hostname; do
        printf '%s=%s\n' "$k" "$(plan_get $k)"
    done
    exit 0
fi

[ -n "${MACHINA_PLAN:-}" ] || die "no machina.plan on the kernel command line"
WORK=$(mktemp -d)
PLAN_FILE="$WORK/plan.json"
$CURL "$MACHINA_PLAN" -o "$PLAN_FILE" || die "cannot fetch the deploy plan"

IMAGE_URL=$(plan_get image_url)
FORMAT=$(plan_get format)
SIZE=$(plan_get size_bytes)
SUM=$(plan_get checksum)
ALGO=$(plan_get checksum_algo)
DISK=$(plan_get target_disk)
COMPLETE_URL=$(plan_get complete_url)
export COMPLETE_URL
[ -n "$IMAGE_URL" ] && [ -n "$SUM" ] && [ -n "$DISK" ] || die "plan is missing image, checksum or target disk"
case "$DISK" in /dev/*) ;; *) DISK="/dev/$DISK" ;; esac
[ -b "$DISK" ] || die "target disk $DISK does not exist on this machine"
case "$ALGO" in sha256) SUMCMD=sha256sum ;; sha512) SUMCMD=sha512sum ;; *) die "unsupported checksum $ALGO" ;; esac

# The image is staged in RAM (tmpfs) so its checksum is verified before a byte reaches the disk.
need_kb=$(( (SIZE / 1024) + 262144 ))
avail_kb=$(awk '/MemAvailable/ {print $2}' /proc/meminfo)
[ "$avail_kb" -gt "$((need_kb + 524288))" ] || die "not enough RAM to stage the image (need ~$((need_kb / 1024)) MiB, free $((avail_kb / 1024)) MiB)"
mkdir -p "$WORK/stage"
mount -t tmpfs -o "size=${need_kb}k" tmpfs "$WORK/stage"
IMG="$WORK/stage/image"

say "downloading $IMAGE_URL"
report progress "downloading image"
$CURL -C - "$IMAGE_URL" -o "$IMG" || die "image download failed"
say "verifying $ALGO"
got=$($SUMCMD "$IMG" | cut -d' ' -f1)
[ "$got" = "$SUM" ] || die "checksum mismatch: expected $SUM, got $got"

case "$FORMAT" in
    qcow2) SRC="-f qcow2" ;;
    raw) SRC="-f raw" ;;
    *) die "unsupported image format $FORMAT" ;;
esac
virt=$(qemu-img info --output=json $SRC "$IMG" | python3 -c 'import json,sys;print(json.load(sys.stdin)["virtual-size"])')
disk_bytes=$(blockdev --getsize64 "$DISK")
[ "$virt" -le "$disk_bytes" ] || die "image needs $virt bytes, $DISK has $disk_bytes"

say "writing to $DISK"
report progress "writing image to $DISK"
wipefs -a "$DISK" >/dev/null 2>&1 || true
qemu-img convert -p -O raw $SRC -t none "$IMG" "$DISK" || die "write failed"
sync
partprobe "$DISK" 2>/dev/null || true
udevadm settle 2>/dev/null || true
rm -f "$IMG"

# Seed cloud-init (NoCloud) on the image's root filesystem: the largest ext4/xfs partition.
ROOT=""
best=0
for p in $(lsblk -lnpo NAME,TYPE "$DISK" | awk '$2=="part" {print $1}'); do
    fs=$(blkid -o value -s TYPE "$p" 2>/dev/null || true)
    case "$fs" in ext4|xfs|btrfs) ;; *) continue ;; esac
    sz=$(blockdev --getsize64 "$p")
    if [ "$sz" -gt "$best" ]; then best=$sz; ROOT=$p; fi
done
[ -n "$ROOT" ] || die "no root filesystem found on $DISK after writing the image"
mkdir -p "$WORK/root"
mount "$ROOT" "$WORK/root" || die "cannot mount $ROOT"
SEED="$WORK/root/var/lib/cloud/seed/nocloud"
mkdir -p "$SEED"
python3 - "$PLAN_FILE" "$SEED" <<'PY'
import json, sys
plan = json.load(open(sys.argv[1]))
open(sys.argv[2] + "/user-data", "w").write(plan["user_data"])
open(sys.argv[2] + "/meta-data", "w").write(plan["meta_data"])
PY
chmod 0600 "$SEED/user-data"
sync
umount "$WORK/root"

say "image written; rebooting into it"
report written "image written to $DISK"
sleep 2
reboot -f
