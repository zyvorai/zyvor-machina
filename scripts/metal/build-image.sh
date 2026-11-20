#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0
#
# Build the Machina bare-metal ramdisk (kernel + initrd + squashfs rootfs) with mkosi.
# Run on a Linux build host as root (mkosi needs it for the package manager); never on a
# laptop. Output: OUT/metal-vmlinuz, metal-initrd, metal-rootfs.squashfs, SHA256SUMS.metal
# Publish with:  sudo machinactl dist publish-metal --dir OUT
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="${1:-${MACHINA_METAL_OUT:-/var/tmp/machina-metal-out}}"
WORK="${MACHINA_METAL_WORK:-/var/tmp/machina-metal-build}"

command -v mkosi >/dev/null || { echo "mkosi not found (install.sh installs it)" >&2; exit 1; }
command -v mksquashfs >/dev/null || { echo "mksquashfs not found (apt install squashfs-tools)" >&2; exit 1; }
[[ $EUID -eq 0 ]] || { echo "run as root (sudo)" >&2; exit 1; }

rm -rf "$WORK"
mkdir -p "$WORK" "$OUT"
cp -a "$SCRIPT_DIR/contrib/mkosi-defs/metal/mkosi.conf" "$WORK/mkosi.conf"

EX="$WORK/mkosi.extra"
install -Dm0755 "$SCRIPT_DIR/contrib/metal/machina-metal-run" "$EX/usr/local/sbin/machina-metal-run"
install -Dm0644 "$SCRIPT_DIR/contrib/metal/machina-metal.service" "$EX/etc/systemd/system/machina-metal.service"
install -Dm0644 "$SCRIPT_DIR/contrib/metal/10-dhcp.network" "$EX/etc/systemd/network/10-dhcp.network"
mkdir -p "$EX/etc/systemd/system/multi-user.target.wants"
ln -sf /etc/systemd/system/machina-metal.service "$EX/etc/systemd/system/multi-user.target.wants/machina-metal.service"
ln -sf /usr/lib/systemd/system/systemd-networkd.service "$EX/etc/systemd/system/multi-user.target.wants/systemd-networkd.service" || true

echo "==> mkosi build (this downloads packages)"
(cd "$WORK" && mkosi --force --output-directory="$WORK/out" --image-id=machina-metal build)

ROOT="$WORK/out/machina-metal"
[[ -d $ROOT ]] || { echo "mkosi did not produce $ROOT" >&2; ls -la "$WORK/out" >&2; exit 1; }

KERNEL="$(ls -1 "$ROOT"/boot/vmlinuz-* 2>/dev/null | sort -V | tail -1)"
INITRD="$(ls -1 "$ROOT"/boot/initrd.img-* 2>/dev/null | sort -V | tail -1)"
[[ -n $KERNEL && -n $INITRD ]] || { echo "no kernel/initrd under $ROOT/boot" >&2; exit 1; }

install -m0644 "$KERNEL" "$OUT/metal-vmlinuz"
install -m0644 "$INITRD" "$OUT/metal-initrd"
echo "==> squashfs"
rm -f "$OUT/metal-rootfs.squashfs"
mksquashfs "$ROOT" "$OUT/metal-rootfs.squashfs" -comp zstd -e boot -noappend -quiet
(cd "$OUT" && sha256sum metal-vmlinuz metal-initrd metal-rootfs.squashfs > SHA256SUMS.metal)
rm -rf "$WORK"
echo "built into $OUT"
cat "$OUT/SHA256SUMS.metal"
