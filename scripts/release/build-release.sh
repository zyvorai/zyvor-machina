#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

#
# Assemble a Machina release from already-built binaries: three deb + three rpm packages (machina, machina-controller,
# machina-agent), an offline bundle tarball with its own installer, SHA256SUMS and a CycloneDX SBOM.
#
#   scripts/release/build-release.sh --version 1.2.0 --bin-dir target/release --web-dir web/dist [--out dist/release]
#
# Needs nfpm (go install github.com/goreleaser/nfpm/v2/cmd/nfpm@latest). Runs anywhere (macOS or Linux): it packages
# prebuilt binaries and never compiles. The binaries' glibc floor is the build machine's: build on the oldest distro you
# support (the release workflow uses ubuntu-22.04).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
VERSION="" BIN_DIR="" WEB_DIR="" OUT="$REPO/dist/release" ARCH=""
while [ $# -gt 0 ]; do
    case "$1" in
        --version) VERSION="$2"; shift 2 ;;
        --bin-dir) BIN_DIR="$2"; shift 2 ;;
        --web-dir) WEB_DIR="$2"; shift 2 ;;
        --out) OUT="$2"; shift 2 ;;
        --arch) ARCH="$2"; shift 2 ;;
        -h|--help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done
[ -n "$VERSION" ] && [ -n "$BIN_DIR" ] && [ -n "$WEB_DIR" ] || { echo "need --version, --bin-dir and --web-dir" >&2; exit 2; }
if [ -z "$ARCH" ]; then case "$(uname -m)" in x86_64) ARCH=amd64 ;; aarch64|arm64) ARCH=arm64 ;; *) echo "unsupported package architecture: $(uname -m)" >&2; exit 2 ;; esac; fi
case "$ARCH" in amd64|arm64) ;; *) echo "--arch must be amd64 or arm64" >&2; exit 2 ;; esac
# deb/rpm versions: a leading v is dropped and pre-release dashes become tildes so 1.0.0-rc1 sorts before 1.0.0.
VERSION="${VERSION#v}"
REL_VERSION="$VERSION"   # tarball/SBOM names keep the plain version
PKG_VERSION="${VERSION/-/\~}"
[[ "$PKG_VERSION" =~ ^[0-9][0-9A-Za-z.+~]*$ ]] || { echo "unusable version: $VERSION" >&2; exit 2; }
NFPM="${NFPM:-$(command -v nfpm || echo "$HOME/go/bin/nfpm")}"
[ -x "$NFPM" ] || { echo "nfpm not found (go install github.com/goreleaser/nfpm/v2/cmd/nfpm@latest)" >&2; exit 1; }
for b in machina-daemon machina-controller machina-agent machina-bpfd; do
    [ -x "$BIN_DIR/$b" ] || { echo "missing binary: $BIN_DIR/$b" >&2; exit 1; }
done
[ -f "$WEB_DIR/index.html" ] || { echo "$WEB_DIR is not a built web UI (no index.html)" >&2; exit 1; }

OUT="$(mkdir -p "$OUT" && cd "$OUT" && pwd)"
STAGE="$OUT/.stage"
rm -rf "$STAGE" && mkdir -p "$STAGE"/{bin,units,etc,doc,scripts,web,bundle}

# ── stage ──────────────────────────────────────────────────────────────────────
for b in machina-daemon machina-controller machina-agent machina-bpfd; do install -m 0755 "$BIN_DIR/$b" "$STAGE/bin/$b"; done
install -m 0755 "$REPO/machinactl" "$STAGE/bin/machinactl"
install -m 0755 "$REPO/scripts/preflight.sh" "$STAGE/bin/machina-preflight"
install -m 0755 "$REPO/scripts/production-preflight.sh" "$STAGE/bin/production-preflight"
install -m 0755 "$REPO/scripts/db/machina-db.sh" "$STAGE/bin/machina-db"
if [ -x "$BIN_DIR/machina-dbtool" ]; then install -m 0755 "$BIN_DIR/machina-dbtool" "$STAGE/bin/machina-dbtool"; HAVE_DBTOOL=1; else HAVE_DBTOOL=0; fi
# the controller's PostgreSQL build ships next to the default one when it was built (make release-pg)
HAVE_PG=0
if [ -x "$BIN_DIR/machina-controller-pg" ]; then install -m 0755 "$BIN_DIR/machina-controller-pg" "$STAGE/bin/machina-controller-pg"; HAVE_PG=1; fi
cp -R "$WEB_DIR/." "$STAGE/web/"
cp "$REPO/contrib/machina.toml" "$STAGE/etc/machina.toml"
cp "$REPO/contrib/machina-daemon.default" "$STAGE/etc/machina-daemon.default"
cp "$REPO/LICENSE" "$STAGE/doc/LICENSE"
cp "$REPO/docs/INSTALL.md" "$STAGE/doc/INSTALL.md"
# Package units point at /usr/bin; the offline bundle keeps /usr/local/bin like the source installer.
for u in machina-daemon machina-controller machina-agent machina-bpfd machina-backup; do
    sed 's#/usr/local/bin/#/usr/bin/#g' "$REPO/contrib/$u.service" >"$STAGE/units/$u.service"
done
cp "$REPO/contrib/machina-backup.timer" "$STAGE/units/machina-backup.timer"

# Maintainer scripts = shared helpers + a per-package body, so no helper file is owned by two packages.
asm() { # name  uses_bootstrap(0|1)  kind(postinst|prerm)
    local name="$1" boot="$2" kind="$3" out="$STAGE/scripts/$1.$3"
    { echo '#!/bin/sh'; echo 'set -e'
      cat "$REPO/packaging/scripts/common.sh"
      [ "$boot" = 1 ] && grep -v '^#!' "$REPO/packaging/lib/bootstrap-secrets.sh" || true
      cat "$REPO/packaging/body/$name.$kind.body"; } >"$out"
    chmod 0755 "$out"
    sh -n "$out" || { echo "generated script has a syntax error: $out" >&2; exit 1; }
}
for n in machina machina-controller machina-agent; do
    boot=0; [ "$n" = machina-controller ] && boot=1
    asm "$n" "$boot" postinst
    asm "$n" 0 prerm
    [ "$n" = machina-controller ] && asm "$n" 0 postrm
done

# ── packages ───────────────────────────────────────────────────────────────────
export STAGE
for n in machina machina-controller machina-agent; do
    # nfpm does not expand variables inside file paths, so render ${STAGE} ourselves (VERSION it expands itself).
    sed -e "s#\${STAGE}#$STAGE#g" -e "s#\${ARCH}#$ARCH#g" "$REPO/packaging/nfpm/$n.yaml" >"$STAGE/nfpm-$n.yaml"
    if [ "$ARCH" = arm64 ]; then sed -i.bak 's/qemu-system-x86/qemu-system-arm/g' "$STAGE/nfpm-$n.yaml" && rm -f "$STAGE/nfpm-$n.yaml.bak"; fi
    if [ "$n" = machina-controller ] && [ "$HAVE_DBTOOL" = 1 ]; then
        sed -i.bak "/dst: \/usr\/bin\/machina-db\$/{n;a\\
  - src: $STAGE/bin/machina-dbtool\\
    dst: /usr/bin/machina-dbtool\\
    file_info: { mode: 0755 }
}" "$STAGE/nfpm-$n.yaml" && rm -f "$STAGE/nfpm-$n.yaml.bak"
    fi
    if [ "$n" = machina-controller ] && [ "$HAVE_PG" = 1 ]; then
        # add the PostgreSQL build to the controller package's file list, right after the default binary
        sed -i.bak "/dst: \/usr\/bin\/machina-controller\$/{n;a\\
  - src: $STAGE/bin/machina-controller-pg\\
    dst: /usr/bin/machina-controller-pg\\
    file_info: { mode: 0755 }
}" "$STAGE/nfpm-$n.yaml" && rm -f "$STAGE/nfpm-$n.yaml.bak"
    fi
    for fmt in deb rpm; do
        VERSION="$PKG_VERSION" "$NFPM" package --config "$STAGE/nfpm-$n.yaml" --packager "$fmt" --target "$OUT/" >/dev/null
    done
done

# ── offline bundle ─────────────────────────────────────────────────────────────
B="$STAGE/bundle/machina-$REL_VERSION-linux-$ARCH"
mkdir -p "$B"/{bin,units,etc,web,lib}
cp "$STAGE"/bin/* "$B/bin/"
cp "$STAGE"/units/* "$B/units/"
sed 's#/usr/bin/#/usr/local/bin/#g' "$STAGE/units/machina-daemon.service" >"$B/units/machina-daemon.service"
for u in machina-controller machina-agent machina-bpfd machina-backup; do sed 's#/usr/bin/#/usr/local/bin/#g' "$STAGE/units/$u.service" >"$B/units/$u.service"; done
cp "$STAGE/etc/"* "$B/etc/"
cp -R "$STAGE/web/." "$B/web/"
cp "$REPO/packaging/lib/bootstrap-secrets.sh" "$B/lib/"
cp "$REPO/packaging/bundle/install.sh" "$B/install.sh"
cp "$STAGE/doc/LICENSE" "$STAGE/doc/INSTALL.md" "$B/"
echo "$REL_VERSION" >"$B/VERSION"
( cd "$B" && find . -type f ! -name MANIFEST.sha256 -print0 | sort -z | xargs -0 shasum -a 256 >MANIFEST.sha256 )
COPYFILE_DISABLE=1 tar -C "$STAGE/bundle" -czf "$OUT/machina-$REL_VERSION-linux-$ARCH.tar.gz" "machina-$REL_VERSION-linux-$ARCH"

# ── SBOM and checksums ─────────────────────────────────────────────────────────
python3 "$HERE/gen-sbom.py" "$REL_VERSION" "$OUT/machina-$REL_VERSION-$ARCH.sbom.cdx.json"
rm -rf "$STAGE"
( cd "$OUT" && shasum -a 256 $(ls | grep -E '\.(deb|rpm|tar\.gz|json)$' | sort) >"SHA256SUMS-$ARCH" )
echo
echo "Release $REL_VERSION written to $OUT:"
( cd "$OUT" && ls -lh | awk 'NR>1 {print "  " $5 "  " $9}' )
