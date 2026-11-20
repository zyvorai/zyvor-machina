# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# shellcheck shell=bash
# Machina deploy library (self-contained under scripts/lib/).

_DEPLOY_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

DEPLOY_UI_PROJECT="Machina"
DEPLOY_UI_ICON="🤖"
DEPLOY_UI_ICON_UNINSTALL="🗑️"
DEPLOY_UI_ICON_MAGIC="✨"
DEPLOY_UI_PORT="5092"
DEPLOY_UI_SCHEME="https"
DEPLOY_UI_DASH_PATH="/"
DEPLOY_UI_HEALTH_PATH="/api/v1/health"

# shellcheck source=deploy-ui.sh
source "$_DEPLOY_LIB_DIR/deploy-ui.sh"

machina_build_metadata() {
    local repo_dir="$1"
    MACHINA_VERSION=$(git -C "$repo_dir" describe --tags --always --dirty 2>/dev/null || echo 'dev')
    MACHINA_COMMIT=$(git -C "$repo_dir" rev-parse --short HEAD 2>/dev/null || echo 'unknown')
    export MACHINA_VERSION MACHINA_COMMIT
}

machina_parse_target() { deploy_ui_parse_target "$@"; }
machina_deploy_state_file() { deploy_ui_deploy_state_file "$1"; }
machina_save_deploy_last() {
    deploy_ui_save_deploy_last "$1" "$2" "$3" "$4" "${MACHINA_VERSION:-}" "${MACHINA_COMMIT:-}"
}
machina_load_deploy_last() { deploy_ui_load_deploy_last "$1"; }

machina_elapsed_fmt() {
    local s="${1:-0}"
    local m=$((s / 60)) r=$((s % 60))
    ((m > 0)) && printf '%dm ' "$m"
    printf '%ds' "$r"
}

machina_print_success() {
    local host="$1" elapsed="$2" user="$3" extra_flags="${4:-}"
    local cmd="./scripts/deploy remote ${user}@${host} --quick"
    [[ -n "$extra_flags" ]] && cmd+=" ${extra_flags}"
    deploy_ui_success "$host" "$elapsed" "$cmd"
}

machina_info()  { deploy_ui_info "$@"; }
machina_warn()  { deploy_ui_warn "$@"; }
machina_error() { deploy_ui_error "$@"; }
machina_note()  { deploy_ui_note "$@"; }
