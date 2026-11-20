#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Reject customer bundles whose root install scripts resolve paths outside the extract dir.
#
# Bundle-root scripts (install-cluster.sh, install.sh, …) must use:
#   ROOT="$(cd "$(dirname "$0")" && pwd)"
# Scripts under bin/ may use (dirname "$0")/.. to reach the bundle root.
#
# Usage (during pack):
#   ./scripts/lib/verify-bundle-script-paths.sh /path/to/stage-dir
# Or source and call verify_bundle_script_paths STAGE
set -euo pipefail

verify_bundle_script_paths() {
  local stage="${1:?bundle stage directory}"
  local bad=0 f

  if [[ ! -d "${stage}" ]]; then
    echo "ERROR: bundle stage not found: ${stage}" >&2
    return 1
  fi

  # Absolute path to the stage dir, used below so the runtime checks can be
  # run with CWD *outside* the stage dir (see comment at the checks).
  local stage_abs
  stage_abs="$(cd "${stage}" && pwd)"

  local -a root_scripts=(
    install-cluster.sh apply-cluster-network.sh test-cluster.sh test-host.sh
    install.sh install-everything.sh install-client-deps.sh test-package.sh uninstall.sh
  )

  for f in "${root_scripts[@]}"; do
    [[ -f "${stage}/${f}" ]] || continue
    if grep -q 'dirname "\$0")/\.\.' "${stage}/${f}"; then
      echo "ERROR: ${f} uses (dirname \$0)/.. — must stay inside the extracted tarball" >&2
      bad=1
    fi
  done

  # Runtime checks below MUST invoke the script via its absolute path from a
  # CWD that is *not* the stage dir. If we instead `cd "${stage}"` and ran
  # `./foo.sh`, a script that (incorrectly) resolves ROOT="$(pwd)" instead of
  # ROOT via "$(dirname "$0")" would still pass, because CWD would happen to
  # equal the extract dir in this one test — exactly the bug this check
  # exists to catch. Running from elsewhere makes the two resolution
  # strategies actually diverge.
  local verify_tmpdir
  verify_tmpdir="$(mktemp -d)"
  trap 'rm -rf "${verify_tmpdir}"' RETURN

  if [[ -x "${stage}/install-cluster.sh" ]]; then
    if [[ ! -f "${stage}/cluster/install-cluster-prereqs.sh" ]]; then
      echo "ERROR: install-cluster.sh present but cluster/install-cluster-prereqs.sh missing" >&2
      bad=1
    fi
    if ! (cd "${verify_tmpdir}" && "${stage_abs}/install-cluster.sh" --help >/dev/null 2>&1); then
      echo "ERROR: install-cluster.sh --help failed when run from outside the extract dir (ROOT must resolve via \$0, not \$PWD)" >&2
      bad=1
    fi
  fi

  if [[ -x "${stage}/test-host.sh" ]] && grep -q 'pkg_script_help' "${stage}/test-host.sh" 2>/dev/null; then
    if ! (cd "${verify_tmpdir}" && "${stage_abs}/test-host.sh" --help >/dev/null 2>&1); then
      echo "ERROR: test-host.sh --help failed when run from outside the extract dir (ROOT must resolve via \$0, not \$PWD)" >&2
      bad=1
    fi
  fi

  return "${bad}"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  verify_bundle_script_paths "$1"
fi
