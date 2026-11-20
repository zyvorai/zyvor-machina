// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Extension;
use axum::Json;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::developer;
use crate::state::AppState;

pub async fn overview() -> Json<developer::DeveloperOverview> {
    Json(developer::overview())
}

pub async fn terraform_schema() -> Json<Vec<developer::TerraformResourceSchema>> {
    Json(developer::terraform_schemas())
}

pub async fn export_vm_iac(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(vm_id): Path<Uuid>,
) -> Result<Json<developer::VmExportBundle>, ApiError> {
    require_operator(&actor)?;
    let row: (String, Option<Uuid>) = crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_one(&state.pool)
        .await?;
    let host_id = row
        .1
        .ok_or_else(|| ApiError::bad_request("VM has no host"))?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let bundle = developer::export_vm_bundle(&state.pool, &agent_addr, vm_id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(bundle))
}

pub async fn export_vm_iac_zip(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(vm_id): Path<Uuid>,
) -> Result<Response, ApiError> {
    require_operator(&actor)?;
    let row: (String, Option<Uuid>) = crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_one(&state.pool)
        .await?;
    let host_id = row
        .1
        .ok_or_else(|| ApiError::bad_request("VM has no host"))?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let (vm_name, bytes) = developer::export_vm_bundle_zip(&state.pool, &agent_addr, vm_id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let disposition = format!("attachment; filename=\"{vm_name}-iac.zip\"");
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/zip"),
            (header::CONTENT_DISPOSITION, disposition.as_str()),
        ],
        Body::from(bytes),
    )
        .into_response())
}
