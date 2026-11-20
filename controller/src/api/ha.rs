// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::template::{get_ha_policy, upsert_ha_policy, HaPolicyRow};
use crate::state::AppState;

#[derive(Debug, serde::Serialize)]
pub struct HaStatusResponse {
    pub status: crate::engine::ha::HaStatusRow,
    pub events: Vec<crate::engine::ha::HaEventRow>,
}

pub async fn get_ha_status(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<HaStatusResponse>, ApiError> {
    require_operator(&actor)?;
    let (status, events) = crate::engine::ha::ha_status(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(HaStatusResponse { status, events }))
}

#[derive(Debug, Deserialize)]
pub struct SetHaPolicyBody {
    pub enabled: bool,
    #[serde(default = "default_attempts")]
    pub restart_attempts: i32,
    #[serde(default = "default_priority")]
    pub restart_priority: String,
    #[serde(default)]
    pub fence_on_failure: bool,
    #[serde(default)]
    pub anti_affinity: bool,
}

fn default_attempts() -> i32 {
    3
}
fn default_priority() -> String {
    "medium".into()
}

pub async fn get_vm_ha_policy(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<HaPolicyRow>, ApiError> {
    require_operator(&actor)?;
    let policy = get_ha_policy(&state.pool, id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .unwrap_or(HaPolicyRow {
            enabled: false,
            restart_attempts: 3,
            restart_priority: "medium".into(),
            fence_on_failure: false,
            anti_affinity: false,
        });
    Ok(Json(policy))
}

pub async fn set_vm_ha_policy(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetHaPolicyBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let _exists: Uuid = crate::db::query_scalar("SELECT id FROM vms WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    upsert_ha_policy(
        &state.pool,
        id,
        body.enabled,
        body.restart_attempts,
        &body.restart_priority,
        body.fence_on_failure,
        body.anti_affinity,
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(serde_json::json!({
        "vm_id": id,
        "enabled": body.enabled,
        "fence_on_failure": body.fence_on_failure,
        "anti_affinity": body.anti_affinity,
    })))
}
