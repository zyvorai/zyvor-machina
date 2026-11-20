// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::host_validate;
use crate::engine::vm_health;
use crate::state::AppState;

pub async fn vm_health_check(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<vm_health::VmHealthReport>, ApiError> {
    require_operator(&actor)?;
    let report = vm_health::run_vm_health_check(&state.pool, id)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(report))
}

pub async fn host_health_check(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<host_validate::HostValidationReport>, ApiError> {
    require_operator(&actor)?;
    let report = host_validate::validate_host(&state.pool, id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(report))
}
