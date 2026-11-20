// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::storage_tiers::{self, UpsertBackupSlaRequest};
use crate::state::AppState;

pub async fn tiers_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<storage_tiers::TiersOverview>, ApiError> {
    require_operator(&actor)?;
    storage_tiers::tiers_overview(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn bind_pool_tier(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((pool_id, tier_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    storage_tiers::bind_pool_tier(&state.pool, pool_id, tier_id)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "bound": true, "pool_id": pool_id, "tier_id": tier_id }),
    ))
}

pub async fn backup_sla_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<storage_tiers::BackupSlaOverview>, ApiError> {
    require_operator(&actor)?;
    storage_tiers::backup_sla_overview(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn upsert_backup_sla(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(pool_id): Path<Uuid>,
    Json(body): Json<UpsertBackupSlaRequest>,
) -> Result<Json<storage_tiers::BackupSlaRow>, ApiError> {
    require_operator(&actor)?;
    storage_tiers::upsert_backup_sla(&state.pool, pool_id, &body)
        .await
        .map(Json)
        .map_err(|e| ApiError::bad_request(e.to_string()))
}

pub async fn pool_snapshot_policy(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(pool_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    storage_tiers::snapshot_policy_for_pool(&state.pool, pool_id)
        .await
        .map(Json)
        .map_err(|e| ApiError::bad_request(e.to_string()))
}
