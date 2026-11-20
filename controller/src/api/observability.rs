// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Query, State};
use axum::Extension;
use axum::Json;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::observability;
use crate::state::AppState;

#[derive(Debug, serde::Deserialize)]
pub struct TraceListQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    50
}

pub async fn overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<observability::ObservabilityOverview>, ApiError> {
    require_operator(&actor)?;
    observability::overview(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn list_traces(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<TraceListQuery>,
) -> Result<Json<Vec<observability::TraceSpanRow>>, ApiError> {
    require_operator(&actor)?;
    observability::list_traces(&state.pool, q.limit)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}
