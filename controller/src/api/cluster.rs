// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::State;
use axum::Extension;
use axum::Json;
use serde::Serialize;
use crate::db::DbPool;

use crate::auth::{require_admin, require_operator, AuthUser};
use crate::engine::drs::{self, ClusterSettings, ClusterSettingsPatch};
use crate::state::AppState;

use super::ApiError;

#[derive(Debug, Serialize)]
pub struct ClusterSummary {
    pub id: String,
    pub name: String,
    pub host_count: i64,
    pub vm_count: i64,
    pub running_vms: i64,
    pub offline_hosts: i64,
    /// Only populated for operator/admin callers — this mirrors the same
    /// `ClusterSettings` payload that `get_settings` gates behind
    /// `require_operator`, so a viewer hitting the dashboard summary must not
    /// receive it either. Basic fleet counts above stay visible to all
    /// authenticated roles since the platform welcome/dashboard views rely on
    /// them for every role.
    pub settings: Option<ClusterSettings>,
}

pub async fn get_cluster(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ClusterSummary>, ApiError> {
    let include_settings = require_operator(&actor).is_ok();
    Ok(Json(
        build_cluster_summary(&state.pool, include_settings).await?,
    ))
}

#[derive(Debug, serde::Serialize)]
pub struct LeadershipStatus {
    pub controller_id: String,
    pub is_leader: bool,
    pub holder_id: String,
    pub lease_until: chrono::DateTime<chrono::Utc>,
}

pub async fn get_leadership(
    State(state): State<AppState>,
) -> Result<Json<LeadershipStatus>, ApiError> {
    let row: (String, chrono::DateTime<chrono::Utc>) =
        crate::db::query_as("SELECT holder_id, lease_until FROM controller_leadership WHERE id = 1")
            .fetch_one(&state.pool)
            .await?;
    Ok(Json(LeadershipStatus {
        controller_id: state.config.controller_id.clone(),
        is_leader: state.leader.is_leader(),
        holder_id: row.0,
        lease_until: row.1,
    }))
}

pub async fn get_settings(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ClusterSettings>, ApiError> {
    require_operator(&actor)?;
    let settings = drs::get_cluster_settings(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(settings))
}

pub async fn patch_settings(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ClusterSettingsPatch>,
) -> Result<Json<ClusterSettings>, ApiError> {
    require_admin(&actor)?;
    drs::update_cluster_settings(&state.pool, &body)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let settings = drs::get_cluster_settings(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(settings))
}

async fn build_cluster_summary(
    pool: &DbPool,
    include_settings: bool,
) -> Result<ClusterSummary, ApiError> {
    let row: (uuid::Uuid, String) =
        crate::db::query_as("SELECT id, name FROM clusters ORDER BY created_at LIMIT 1")
            .fetch_one(pool)
            .await?;
    let host_count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM hosts")
        .fetch_one(pool)
        .await?;
    let vm_count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM vms")
        .fetch_one(pool)
        .await?;
    let running_vms: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM vms WHERE observed_state = 'running'")
            .fetch_one(pool)
            .await?;
    let offline_hosts: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM hosts WHERE state = 'offline'")
            .fetch_one(pool)
            .await?;
    let settings = if include_settings {
        Some(
            drs::get_cluster_settings(pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?,
        )
    } else {
        None
    };
    Ok(ClusterSummary {
        id: row.0.to_string(),
        name: row.1,
        host_count,
        vm_count,
        running_vms,
        offline_hosts,
        settings,
    })
}

#[derive(Debug, serde::Deserialize)]
pub struct PatchClusterBody {
    pub name: Option<String>,
}

pub async fn patch_cluster(
    State(state): State<AppState>,
    Extension(actor): Extension<crate::auth::AuthUser>,
    Json(body): Json<PatchClusterBody>,
) -> Result<Json<ClusterSummary>, ApiError> {
    crate::auth::require_admin(&actor)?;
    if let Some(name) = &body.name {
        crate::db::query("UPDATE clusters SET name = ?")
            .bind(name)
            .execute(&state.pool)
            .await?;
    }
    get_cluster(State(state), Extension(actor)).await
}
