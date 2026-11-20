#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Build ubuntu-24.04-desktop golden qcow2 on a remote Machina hypervisor.
#
# Usage:
#   ./scripts/build-ubuntu-desktop-golden-remote.sh sus 212.8.252.194
#   FORCE=1 ./scripts/build-ubuntu-desktop-golden-remote.sh sus 212.8.252.194
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
USER="${1:?usage: $0 USER HOST}"
HOST="${2:?usage: $0 USER HOST}"
SSH_OPTS=(-o BatchMode=yes -o StrictHostKeyChecking=no)
REMOTE="${USER}@${HOST}"

echo "== Upload build script and run on ${USER}@${HOST}"
ssh "${SSH_OPTS[@]}" "$REMOTE" "mkdir -p ~/machina/scripts"
scp -o StrictHostKeyChecking=no "${SCRIPT_DIR}/build-ubuntu-desktop-golden.sh" \
  "${USER}@${HOST}:~/machina/scripts/build-ubuntu-desktop-golden.sh"

# Pass the sudo password over ssh's own stdin channel rather than embedding it in the
# remote command string — avoids leaking it via `ps` on the remote host and avoids
# breaking/injecting shell syntax if the password contains a quote or metacharacter.
printf '%s\n' "${VSPASS:-max}" | ssh "${SSH_OPTS[@]}" "$REMOTE" \
  "sudo -S env FORCE=$(printf '%q' "${FORCE:-0}") FORCE_BASE=$(printf '%q' "${FORCE_BASE:-0}") \
  bash ~/machina/scripts/build-ubuntu-desktop-golden.sh"

echo "== Golden image ready on host"
