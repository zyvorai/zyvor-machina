#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Run the sprite test suite (spec + core + daemon, filtered to `sprite`) on a
# deployed host, as root — the live-boot integration tests in
# core/src/cloud_hypervisor/sprite.rs (boot_and_teardown_sprite_chv_*) need
# real cloud-hypervisor + write access to /var/lib/machina, so they skip
# cleanly under a non-root `cargo test` and only actually run this way.
#
# Codifies a sequence that was otherwise being hand-typed over SSH each time
# during development (see docs/customer or git log for context): fix up
# target/ ownership left root-owned by install.sh's `sudo` build step, then
# run the tests with root's env pointed at the deploying user's rustup.
#
# Usage: scripts/sprite-remote-test.sh USER HOST [REMOTE_DIR]
set -euo pipefail

USER_NAME="${1:?usage: sprite-remote-test.sh USER HOST [REMOTE_DIR]}"
HOST="${2:?usage: sprite-remote-test.sh USER HOST [REMOTE_DIR]}"
REMOTE_DIR="${3:-/home/${USER_NAME}/.deployment/machina}"

SSH_OPTS=(-o StrictHostKeyChecking=no -o ConnectTimeout=30 -o ServerAliveInterval=15)

echo "== sprite-remote-test: ${USER_NAME}@${HOST}:${REMOTE_DIR} =="

echo "-- fixing target/ ownership (root-owned after install.sh's sudo build) --"
ssh "${SSH_OPTS[@]}" "${USER_NAME}@${HOST}" "sudo chown -R ${USER_NAME}:${USER_NAME} '${REMOTE_DIR}'"

echo "-- running sprite tests as root (needed for the live cloud-hypervisor boot tests) --"
ssh "${SSH_OPTS[@]}" "${USER_NAME}@${HOST}" \
  "sudo env HOME=/home/${USER_NAME} PATH=/home/${USER_NAME}/.cargo/bin:\$PATH bash -c '\
    cd \"${REMOTE_DIR}\" && \
    CARGO_BUILD_JOBS=1 cargo test -p machina-spec -p machina-core -p machina-daemon -- sprite --test-threads=1 --nocapture\
  '"

echo "== sprite-remote-test: PASS =="
