// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::State;
use axum::Extension;
use axum::Json;
use serde::Serialize;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ProxmoxSyncResponse {
    pub synced: bool,
    pub imported: usize,
    pub message: String,
    pub scope: String,
}

/// Honest Proxmox scope: inventory import stub — full API adapter deferred.
pub async fn sync_inventory(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ProxmoxSyncResponse>, ApiError> {
    require_operator(&actor)?;
    let count: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM vms WHERE inventory_source = 'proxmox'")
            .fetch_one(&state.pool)
            .await?;
    Ok(Json(ProxmoxSyncResponse {
        synced: false,
        imported: count as usize,
        message: "Proxmox adapter is import-only in v1 — use VM export + Machina import or manual inventory tagging. No live CRUD sync yet.".into(),
        scope: "import_only".into(),
    }))
}
