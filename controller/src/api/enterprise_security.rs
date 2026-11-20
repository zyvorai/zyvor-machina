// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::engine::enterprise_security::{
    self, CreateAirGapBundleRequest, UpsertTenantPolicyRequest,
};
use crate::state::AppState;

pub async fn overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<enterprise_security::EnterpriseSecurityOverview>, ApiError> {
    require_operator(&actor)?;
    enterprise_security::overview(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn list_air_gap_bundles(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<enterprise_security::AirGapBundleRow>>, ApiError> {
    require_operator(&actor)?;
    enterprise_security::list_air_gap_bundles(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn create_air_gap_bundle(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateAirGapBundleRequest>,
) -> Result<Json<enterprise_security::AirGapBundleRow>, ApiError> {
    require_admin(&actor)?;
    enterprise_security::create_air_gap_bundle(&state.pool, &body)
        .await
        .map(Json)
        .map_err(|e| ApiError::bad_request(e.to_string()))
}

pub async fn get_air_gap_bundle(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<enterprise_security::AirGapBundleRow>, ApiError> {
    require_operator(&actor)?;
    enterprise_security::get_air_gap_bundle(&state.pool, id)
        .await
        .map(Json)
        .map_err(|e| ApiError::not_found(e.to_string()))
}

pub async fn delete_air_gap_bundle(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    enterprise_security::delete_air_gap_bundle(&state.pool, id)
        .await
        .map_err(|e| {
            if e.to_string().contains("not found") {
                ApiError::not_found(e.to_string())
            } else {
                ApiError::internal(e.to_string())
            }
        })?;
    Ok(Json(serde_json::json!({ "deleted": true, "id": id })))
}

pub async fn fips_matrix(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<enterprise_security::FipsMatrix>, ApiError> {
    require_operator(&actor)?;
    enterprise_security::fips_matrix(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn tenant_isolation_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<enterprise_security::TenantIsolationOverview>, ApiError> {
    require_operator(&actor)?;
    enterprise_security::tenant_isolation_overview(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn upsert_tenant_policy(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(project): Path<String>,
    Json(body): Json<UpsertTenantPolicyRequest>,
) -> Result<Json<enterprise_security::TenantIsolationItem>, ApiError> {
    require_admin(&actor)?;
    enterprise_security::upsert_tenant_policy(&state.pool, &project, &body)
        .await
        .map(Json)
        .map_err(|e| ApiError::bad_request(e.to_string()))
}
