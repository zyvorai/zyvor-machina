#!/bin/bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Terraform `external` data source: prints {"command": "<one-time join command>"}.
# Reads the controller target from the query (JSON on stdin) and mints the token over SSH.
set -euo pipefail
query="$(cat)"
target="$(printf %s "$query" | python3 -c 'import sys,json; print(json.load(sys.stdin)["controller_ssh"])')"
key="$(printf %s "$query" | python3 -c 'import sys,json; print(json.load(sys.stdin)["key"])')"
here="$(cd "$(dirname "$0")" && pwd)"
cmd="$(ssh -i "${key/#\~/$HOME}" -o BatchMode=yes "$target" 'sudo bash -s' < "$here/../../ansible/roles/agent/files/mint-join-command.sh")"
[ "$cmd" != "NO_JOIN_COMMAND" ] || { echo "the controller has no join listener: set MACHINA_CONTROLLER_TLS_ADDR" >&2; exit 1; }
python3 -c 'import sys,json; print(json.dumps({"command": sys.argv[1]}))' "$cmd"
