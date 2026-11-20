#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0
#
# Build the tiny iPXE ISOs a BMC mounts as virtual media, so a machine reaches the Machina
# boot service without DHCP/TFTP on its network. The script baked into iPXE asks DHCP for an
# address and chains to the controller with the NIC's MAC, exactly as a PXE boot does:
#
#   sudo scripts/metal/build-ipxe-iso.sh http://CONTROLLER:5093 [OUT_DIR]
#
# Output: OUT_DIR/metal-ipxe.iso (BIOS) and OUT_DIR/metal-ipxe-uefi.iso (UEFI). Publish them
# with `machinactl dist publish-metal --dir OUT_DIR`. Build host only; needs git, make, gcc,
# liblzma-dev, mtools, xorriso/genisoimage, isolinux (apt: build-essential liblzma-dev mtools
# xorriso isolinux). IPXE_SRC=/path uses an existing iPXE checkout instead of cloning.
set -euo pipefail

CTL="${1:-}"
OUT="${2:-${MACHINA_METAL_OUT:-/var/tmp/machina-metal-out}}"
[[ "$CTL" =~ ^https?://[][A-Za-z0-9._:-]+$ ]] || { echo "usage: $0 http(s)://CONTROLLER[:port] [OUT_DIR]" >&2; exit 2; }
WORK="$(mktemp -d /var/tmp/machina-ipxe-XXXXXX)"
trap 'rm -rf "$WORK"' EXIT

SRC="${IPXE_SRC:-}"
if [[ -z $SRC ]]; then
    git clone --depth 1 https://github.com/ipxe/ipxe.git "$WORK/ipxe" >/dev/null 2>&1 || { echo "cannot clone iPXE (set IPXE_SRC for an offline checkout)" >&2; exit 1; }
    SRC="$WORK/ipxe"
fi

cat >"$WORK/embed.ipxe" <<IPXE
#!ipxe
dhcp
chain ${CTL%/}/api/v1/baremetal/boot/ipxe?mac=\${net0/mac:hexhyp}
IPXE
# The controller reads the MAC with ':' separators; hexhyp gives '-', which it also accepts.

mkdir -p "$OUT"
(
    cd "$SRC/src"
    make -j"$(nproc)" bin/ipxe.iso EMBED="$WORK/embed.ipxe" >/dev/null
    cp bin/ipxe.iso "$OUT/metal-ipxe.iso"
    make -j"$(nproc)" bin-x86_64-efi/ipxe.iso EMBED="$WORK/embed.ipxe" >/dev/null
    cp bin-x86_64-efi/ipxe.iso "$OUT/metal-ipxe-uefi.iso"
)
(cd "$OUT" && sha256sum metal-ipxe.iso metal-ipxe-uefi.iso)
echo "built into $OUT"
