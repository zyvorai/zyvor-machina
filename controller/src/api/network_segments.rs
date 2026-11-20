// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::network_overlay::{self, CreateSegmentRequest, IpamAllocateRequest, SegmentRow};
use crate::state::AppState;

pub async fn segments_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<network_overlay::SegmentsOverview>, ApiError> {
    require_operator(&actor)?;
    network_overlay::segments_overview(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn create_segment(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateSegmentRequest>,
) -> Result<Json<SegmentRow>, ApiError> {
    require_operator(&actor)?;
    network_overlay::create_segment(&state.pool, &body)
        .await
        .map(Json)
        .map_err(|e| ApiError::bad_request(e.to_string()))
}

pub async fn segment_connectivity(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<network_overlay::SegmentConnectivityResult>, ApiError> {
    require_operator(&actor)?;
    network_overlay::segment_connectivity(&state.pool, id)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn ipam_allocate(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<IpamAllocateRequest>,
) -> Result<Json<network_overlay::IpamAllocation>, ApiError> {
    require_operator(&actor)?;
    network_overlay::ipam_allocate(&state.pool, id, &body)
        .await
        .map(Json)
        .map_err(|e| ApiError::bad_request(e.to_string()))
}

pub async fn ipam_pools(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<network_overlay::IpamPoolRow>>, ApiError> {
    require_operator(&actor)?;
    network_overlay::list_ipam_pools(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn gitops_export(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<network_overlay::SegmentGitOpsExport>, ApiError> {
    require_operator(&actor)?;
    network_overlay::export_gitops(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn emergency_unlock(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<network_overlay::EmergencyUnlockResult>, ApiError> {
    require_operator(&actor)?;
    network_overlay::emergency_unlock(&state.pool, id)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn bind_network_to_segment(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((segment_id, network_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    network_overlay::bind_network(&state.pool, network_id, segment_id)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "bound": true, "segment_id": segment_id, "network_id": network_id }),
    ))
}
