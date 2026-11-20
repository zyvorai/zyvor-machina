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
pub struct VmwareSyncResponse {
    pub synced: bool,
    pub imported: usize,
    pub message: String,
    pub scope: String,
    pub migration_advisor: String,
}

/// Honest VMware/vSphere scope: migration advisor + import guidance, no live CRUD sync in v1.
pub async fn sync_inventory(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<VmwareSyncResponse>, ApiError> {
    require_operator(&actor)?;
    let count: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM vms WHERE inventory_source IN ('vmware', 'vsphere', 'discovered')",
    )
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(VmwareSyncResponse {
        synced: false,
        imported: count as usize,
        message: "VMware/vSphere remains import + migration-assistant only. Use Platform → Migration for pre-checks, or export OVA/qcow2 and Machina import.".into(),
        scope: "import_and_migrate_advisor".into(),
        migration_advisor: "GET /api/v1/migrations/advisor?provider=vmware".into(),
    }))
}
