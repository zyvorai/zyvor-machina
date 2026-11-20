// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::State;
use axum::Extension;
use axum::Json;
use serde::Serialize;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct KubeVirtSyncResponse {
    pub synced: bool,
    pub cluster_id: String,
    pub message: String,
}

/// Trigger KubeVirt inventory sync (CRUD alignment with Machina DB).
pub async fn sync_inventory(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<KubeVirtSyncResponse>, ApiError> {
    require_operator(&actor)?;
    let cluster_id: Uuid =
        crate::db::query_scalar("SELECT id FROM clusters ORDER BY created_at LIMIT 1")
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::bad_request("no cluster configured"))?;
    let outcome = crate::engine::kubevirt_inventory::sync_cluster(&state, cluster_id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let message = outcome.reason.unwrap_or_else(|| {
        "KubeVirt VMs reconciled with platform inventory (VMware/Proxmox remain import-only)".into()
    });
    Ok(Json(KubeVirtSyncResponse {
        synced: outcome.synced,
        cluster_id: cluster_id.to_string(),
        message,
    }))
}
