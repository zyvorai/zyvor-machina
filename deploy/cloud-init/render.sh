#!/bin/bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Usage: ./render.sh 'JOIN COMMAND' > user-data
#        ./render.sh --from-controller user@controller > user-data
# The join command comes from the web wizard (Platform > Hosts > Add host), from POST /api/v1/enrollment/tokens
# ("join_command"), or with --from-controller from a controller you can ssh to (needs sudo there). It carries a
# single-use token that expires after one hour: render right before you launch the node.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
if [ "${1:-}" = "--from-controller" ]; then
  cmd="$(ssh -o BatchMode=yes "${2:?user@controller}" 'sudo bash -s' < "$here/../ansible/roles/agent/files/mint-join-command.sh")"
else
  cmd="${1:?usage: render.sh 'JOIN COMMAND' | --from-controller user@host}"
fi
[ "$cmd" != "NO_JOIN_COMMAND" ] || { echo "the controller has no join listener (MACHINA_CONTROLLER_TLS_ADDR)" >&2; exit 1; }
case "$cmd" in *$'\n'*) echo "the join command must be one line" >&2; exit 1 ;; esac
python3 - "$here/node.user-data.tpl" "$cmd" <<'PY'
import sys
tpl, cmd = open(sys.argv[1]).read(), sys.argv[2]
sys.stdout.write(tpl.replace("@JOIN_COMMAND@", cmd))
PY
