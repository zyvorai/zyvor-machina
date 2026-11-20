#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# ============================================================================
# package-binary-remote.sh — Build Machina on a remote Linux host and tarball it
# ============================================================================
# Minimal rsync, `make release web` on the server (glibc + libvirt — must match
# target distro), tarball daemon + web/dist for client handoff.
#
# Usage:
#   ./scripts/package-binary-remote.sh <host> [user] [--fetch] [--reuse-build]
#   ./scripts/package-binary-remote.sh 212.8.252.194 sus --from-deploy --fetch
#
# Options:
#   --fetch        Copy tarball to ./dist/ on your laptop
#   --reuse-build  Skip make if target/release/machina-daemon exists
#   --from-deploy  Package ~/.deployment/machina (existing deploy tree; no full rebuild)
#
# Environment:
#   DEPLOY_HOST / DEPLOY_USER
#   MACHINA_PACKAGE_DIR         Remote output (default: ~/machina-dist)
#   MACHINA_PACKAGE_VERSION     Override version
#   DEPLOY_SSH_TIMEOUT
#   MACHINA_REMOTE_SKIP_SSH_CHECK=1
#
# Prerequisites on remote: Rust, Node/npm, libvirt dev headers (see install.sh --deps-only)
#
# See: docs/PACKAGE_BINARY_REMOTE.md
# ============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

FETCH=false
REUSE_BUILD=false
SKIP_DEPS=false
FROM_DEPLOY=false
POSITIONAL=()

for arg in "$@"; do
    case "$arg" in
        --fetch) FETCH=true ;;
        --reuse-build) REUSE_BUILD=true ;;
        --skip-deps) SKIP_DEPS=true ;;
        --from-deploy) FROM_DEPLOY=true; REUSE_BUILD=true; SKIP_DEPS=true ;;
        -h|--help)
            sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) POSITIONAL+=("$arg") ;;
    esac
done

HOST="${POSITIONAL[0]:-${DEPLOY_HOST:-}}"
USER="${POSITIONAL[1]:-${DEPLOY_USER:-sus}}"
SSH_TIMEOUT="${DEPLOY_SSH_TIMEOUT:-20}"

if [[ -z "${HOST}" ]]; then
    echo "Usage: $0 <host> [user] [--fetch] [--reuse-build]" >&2
    echo "  See: docs/PACKAGE_BINARY_REMOTE.md" >&2
    exit 1
fi

# HOST/USER end up spliced verbatim into heredoc text a remote `bash -s`
# re-parses (BUILD_DIR below, derived from them) — reject shell metacharacters.
[[ "$HOST" =~ ^[A-Za-z0-9_.:-]+$ ]] || { echo "invalid host: '${HOST}'" >&2; exit 1; }
[[ "$USER" =~ ^[A-Za-z0-9_.-]+$ ]] || { echo "invalid user: '${USER}'" >&2; exit 1; }

[ -f "${REPO_DIR}/Makefile" ] || { echo "Not in machina repo" >&2; exit 1; }

VERSION="${MACHINA_PACKAGE_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "${REPO_DIR}/daemon/Cargo.toml" | head -1)}"
VERSION="${VERSION:-0.1.0}"
ARCH="linux-amd64"
REMOTE="${USER}@${HOST}"
REMOTE_HOME=$(ssh -o BatchMode=yes -o ConnectTimeout="${SSH_TIMEOUT}" "${REMOTE}" 'echo "$HOME"')
if $FROM_DEPLOY; then
    BUILD_DIR="${REMOTE_HOME}/.deployment/machina"
else
    BUILD_DIR="${REMOTE_HOME}/.deployment/machina-package"
fi
OUT_DIR="${MACHINA_PACKAGE_DIR:-${REMOTE_HOME}/machina-dist}"
ARTIFACT="machina-${VERSION}-${ARCH}"
LOCAL_DIST="${REPO_DIR}/dist"

RSYNC_EXCLUDES=(
    --exclude='target/'
    --exclude='.git/'
    --exclude='web/node_modules/'
    --exclude='web/dist/'
)

# shellcheck source=lib/package-remote-ui.sh
source "${SCRIPT_DIR}/lib/package-remote-ui.sh"

pkg_remote_banner "Machina" "${VERSION}" "${REMOTE}" "${ARCH}"

if [[ "${MACHINA_REMOTE_SKIP_SSH_CHECK:-}" != "1" ]]; then
    pkg_remote_phase "Preflight"
    ssh -o BatchMode=yes -o ConnectTimeout="${SSH_TIMEOUT}" -o StrictHostKeyChecking=accept-new \
        "${REMOTE}" "true"
    pkg_ok "SSH ${REMOTE}"
fi

if $FROM_DEPLOY; then
    pkg_remote_phase "Use existing deploy tree"
    pkg_remote_kv "Build dir" "${BUILD_DIR}"
    ssh "${REMOTE}" "test -x '${BUILD_DIR}/target/release/machina-daemon'" \
        || { echo "No release build in ${BUILD_DIR} — deploy or run without --from-deploy" >&2; exit 1; }
    pkg_ok "Found machina-daemon in deploy tree"
    pkg_remote_phase "Sync packaging scripts only"
    ssh "${REMOTE}" "mkdir -p '${BUILD_DIR}/scripts/lib' '${BUILD_DIR}/contrib'"
    rsync -az \
        -e "ssh -o StrictHostKeyChecking=no -o ServerAliveInterval=15 -o ServerAliveCountMax=120" \
        "${REPO_DIR}/scripts/lib/" "${REMOTE}:${BUILD_DIR}/scripts/lib/"
    rsync -az \
        "${REPO_DIR}/scripts/zyvor-branding/" "${REMOTE}:${BUILD_DIR}/scripts/zyvor-branding/" 2>/dev/null || true
    rsync -az "${REPO_DIR}/contrib/machina.toml" "${REPO_DIR}/contrib/machina-daemon.service" \
        "${REMOTE}:${BUILD_DIR}/contrib/"
    rsync -az "${REPO_DIR}/install.sh" "${REMOTE}:${BUILD_DIR}/"
    rsync -az \
        "${REPO_DIR}/contrib/mkosi-defs/" "${REMOTE}:${BUILD_DIR}/contrib/mkosi-defs/" 2>/dev/null || true
else
    pkg_remote_phase "Sync source"
    pkg_remote_kv "Build dir" "${BUILD_DIR}"
    ssh "${REMOTE}" "mkdir -p '${BUILD_DIR}'"
    rsync -az --delete "${RSYNC_EXCLUDES[@]}" \
        -e "ssh -o StrictHostKeyChecking=no -o ServerAliveInterval=15 -o ServerAliveCountMax=120" \
        "${REPO_DIR}/" "${REMOTE}:${BUILD_DIR}/"
fi

if ! $SKIP_DEPS; then
    pkg_remote_phase "Build dependencies"
    ssh "${REMOTE}" bash -s <<REMOTE_DEPS
set -euo pipefail
cd '${BUILD_DIR}'
if [ -f install.sh ]; then
  sudo ./install.sh --deps-only 2>&1 | tail -20
else
  echo "install.sh missing" >&2
  exit 1
fi
echo "build deps OK"
REMOTE_DEPS
fi

pkg_remote_phase "Compile (make release web)"
BUILD_CMD="cd '${BUILD_DIR}' && make release web"
if $REUSE_BUILD; then
    if ssh "${REMOTE}" "test -x '${BUILD_DIR}/target/release/machina-daemon'"; then
        pkg_ok "Reusing target/release (--reuse-build)"
        BUILD_CMD="true"
    fi
fi

if [[ "${BUILD_CMD}" != "true" ]]; then
    pkg_info "First run often 10–20 min (Rust + Node + libvirt dev)…"
    ssh "${REMOTE}" "${BUILD_CMD}" 2>&1 | sed 's/^/  [make] /'
    pkg_ok "Build finished"
fi

pkg_remote_phase "Assemble customer bundle"
pkg_remote_kv "Output" "${OUT_DIR}/${ARTIFACT}"
ssh "${REMOTE}" bash -s <<REMOTE_PACK
set -euo pipefail
OUT_DIR='${OUT_DIR}'
BUILD_DIR='${BUILD_DIR}'
ARTIFACT='${ARTIFACT}'
VERSION='${VERSION}'

STAGE="\${OUT_DIR}/\${ARTIFACT}"
rm -rf "\${STAGE}"
mkdir -p "\${STAGE}/web/dist"
cp "\${BUILD_DIR}/target/release/machina-daemon" "\${STAGE}/"
chmod +x "\${STAGE}/machina-daemon" 2>/dev/null || true
cp -a "\${BUILD_DIR}/web/dist/." "\${STAGE}/web/dist/"
cp "\${BUILD_DIR}/contrib/machina.toml" "\${STAGE}/machina.toml.example"
cp "\${BUILD_DIR}/contrib/machina-daemon.service" "\${STAGE}/" 2>/dev/null || true
mkdir -p "\${STAGE}/contrib"
cp -a "\${BUILD_DIR}/contrib/mkosi-defs" "\${STAGE}/contrib/" 2>/dev/null || true

LIB="\${BUILD_DIR}/scripts/lib"
for f in package-install.sh package-client-install.sh package-client-test.sh; do
  test -f "\${LIB}/\${f}" || { echo "missing \${LIB}/\${f}" >&2; exit 1; }
done
cp "\${LIB}/package-install.sh" "\${STAGE}/install.sh"
cp "\${LIB}/package-client-install.sh" "\${STAGE}/install-client-deps.sh"
cp "\${LIB}/package-client-test.sh" "\${STAGE}/test-package.sh"
mkdir -p "\${STAGE}/.package-lib"
cp "\${LIB}/package-ui.sh" "\${STAGE}/.package-lib/"
cp "\${LIB}/package-auth-bootstrap.sh" "\${STAGE}/.package-lib/"
cp "\${LIB}/install-everything.sh" "\${STAGE}/"
cp "\${LIB}/package-uninstall-lib.sh" "\${STAGE}/.package-lib/"
cp "\${LIB}/package-uninstall.sh" "\${STAGE}/uninstall.sh"
chmod +x "\${STAGE}/install.sh" "\${STAGE}/install-client-deps.sh" "\${STAGE}/test-package.sh" \
  "\${STAGE}/install-everything.sh" "\${STAGE}/uninstall.sh"
chmod +x "\${LIB}/write-customer-help.sh"
"\${LIB}/write-customer-help.sh" "\${STAGE}" "Machina" host
cp "\${LIB}/START_HERE.txt" "\${STAGE}/"
cat > "\${STAGE}/.package-lib/product.meta" <<'META'
PRODUCT_NAME=Machina
ACCESS_SCHEME=https
ACCESS_PORT=5092
ACCESS_PATH=
AUTO_FULL_INSTALL=1
FINISH_EXTRA_1='Service: sudo systemctl status machina-daemon'
FINISH_EXTRA_2='Logs: sudo journalctl -u machina-daemon -f'
FINISH_EXTRA_3='Help: cat HELP.txt'
META
cp "\${BUILD_DIR}/install.sh" "\${STAGE}/install-full.sh" 2>/dev/null || true
chmod +x "\${STAGE}/install-full.sh" 2>/dev/null || true
cp "\${LIB}/HOST_SETUP.txt" "\${LIB}/PREREQUISITES.txt" "\${STAGE}/"
cp "\${LIB}/package-host-test.sh" "\${STAGE}/test-host.sh"
chmod +x "\${STAGE}/test-host.sh"

cat > "\${STAGE}/QUICKSTART.txt" <<'QEOF'
Machina — install guide (libvirt host — NOT Kubernetes)
=======================================================

HOST FIRST (one command — bundled binaries, no compile)
  1. tar xzf machina-*-linux-amd64.tar.gz && cd machina-*-linux-amd64
  2. ./install-everything.sh   # deps + systemd + firewall + tests
  3. Open the https URL printed at the end (this server's LAN IP)

Manual steps (optional)
  ./install.sh && ./test-host.sh && sudo ./install-full.sh --open-firewall && ./test-package.sh

Checklist: PREREQUISITES.txt  |  Details: HOST_SETUP.txt

Packaged by Zyvor — zyvor.dev · © 2026
QEOF

cp "\${BUILD_DIR}/scripts/zyvor-branding/ZYVOR_INSTALL.txt" "\${STAGE}/ZYVOR_INSTALL.txt" 2>/dev/null || true

cat > "\${STAGE}/README.txt" <<README_EOF
Machina ${VERSION} — Linux amd64 client bundle
==============================================

START: cat START_HERE.txt  |  full help: cat HELP.txt

NOT KUBERNETES — runs on a libvirt/KVM hypervisor host.

WHAT IS IN THIS ARCHIVE
  machina-daemon, web/dist/
  install.sh, test-host.sh, test-package.sh, uninstall.sh
  HOST_SETUP.txt, PREREQUISITES.txt
  install-full.sh       Full host install (deps + systemd + /usr/local; uses bundled binaries)

REQUIREMENTS — see PREREQUISITES.txt
  Linux x86_64, KVM, libvirtd, qemu-kvm

ORDER: ./install.sh → ./test-host.sh → sudo ./install-full.sh [--bind 0.0.0.0] → Web UI :5092

FLAGS (install-full.sh): --deps-only --bind 0.0.0.0 --open-firewall (no source tree needed)

UNINSTALL: ./uninstall.sh --yes [--remove-dir]
README_EOF

chmod +x "\${LIB}/finalize-customer-bundle.sh"
"\${LIB}/finalize-customer-bundle.sh" "\${STAGE}" "\${BUILD_DIR}" "Machina" "\${VERSION}"
for req in LICENSE LEGAL-INDEX.txt HELP.txt START_HERE.txt install.sh uninstall.sh README.txt QUICKSTART.txt HOST_SETUP.txt PREREQUISITES.txt \
  test-host.sh test-package.sh install-client-deps.sh machina-daemon machina.toml.example; do
  test -e "\${STAGE}/\${req}" || { echo "bundle missing \${req}" >&2; exit 1; }
done
echo "Customer bundle OK"

cd "\${OUT_DIR}"
tar czf "\${ARTIFACT}.tar.gz" "\${ARTIFACT}"
sha256sum "\${ARTIFACT}.tar.gz" | tee "\${ARTIFACT}.tar.gz.sha256"
ls -lh "\${ARTIFACT}.tar.gz"
file "\${STAGE}/machina-daemon"
"\${STAGE}/machina-daemon" --help 2>&1 | head -5 || true
REMOTE_PACK

TARBALL="${ARTIFACT}.tar.gz"
REMOTE_TARBALL="${OUT_DIR}/${TARBALL}"

if $FETCH; then
    pkg_remote_phase "Fetch to laptop"
    mkdir -p "${LOCAL_DIST}"
    scp -o StrictHostKeyChecking=no \
        "${REMOTE}:${REMOTE_TARBALL}" \
        "${REMOTE}:${OUT_DIR}/${TARBALL}.sha256" \
        "${LOCAL_DIST}/"
    (cd "${LOCAL_DIST}" && shasum -a 256 -c "${TARBALL}.sha256" 2>/dev/null || sha256sum -c "${TARBALL}.sha256") && pkg_ok "Checksum verified"
fi

pkg_remote_done "Machina" "${REMOTE}:${REMOTE_TARBALL}" "${REMOTE}:${OUT_DIR}/${TARBALL}.sha256"
