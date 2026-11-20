// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, Query, State};
use axum::Extension;
use axum::Json;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::operations::{self, ExecuteRunbookRequest};
use crate::state::AppState;

#[derive(Debug, serde::Deserialize)]
pub struct ExecutionsQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    20
}

pub async fn overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<operations::OperationsOverview>, ApiError> {
    require_operator(&actor)?;
    operations::overview(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn list_runbooks(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<operations::RunbookCatalogRow>>, ApiError> {
    require_operator(&actor)?;
    operations::list_catalog(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn list_executions(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<ExecutionsQuery>,
) -> Result<Json<Vec<operations::RunbookExecutionRow>>, ApiError> {
    require_operator(&actor)?;
    operations::list_executions(&state.pool, q.limit)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn execute_runbook(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(incident): Path<String>,
    Json(body): Json<ExecuteRunbookRequest>,
) -> Result<Json<operations::RunbookExecuteResult>, ApiError> {
    crate::auth::require_operator(&actor)?;
    operations::execute_runbook(&state.pool, &incident, &actor.username, &body.context)
        .await
        .map(Json)
        .map_err(|e| ApiError::bad_request(e.to_string()))
}

pub async fn showback_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<operations::ShowbackOverview>, ApiError> {
    require_operator(&actor)?;
    operations::showback_overview(&state.pool, &state.config)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}
