// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::State;
use axum::Extension;
use axum::Json;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::placement::{
    compute_recommendations, persist_recommendations, PlacementRecommendationRow,
};
use crate::state::AppState;

pub async fn list_recommendations(
    State(state): State<AppState>,
) -> Result<Json<Vec<PlacementRecommendationRow>>, ApiError> {
    let rows = compute_recommendations(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    // Read-only: GET must not supersede/insert rows. DRS and POST /refresh persist.
    Ok(Json(rows))
}

pub async fn refresh_recommendations(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<PlacementRecommendationRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = compute_recommendations(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if let Err(e) = persist_recommendations(&state.pool, &rows).await {
        tracing::warn!("persist placement recommendations: {e:#}");
    }
    Ok(Json(rows))
}

/// Rack-loss readiness: per failure domain (site/rack), whether HA could restart what runs in
/// it on the hosts that remain. Read-only.
pub async fn failure_domains(
    State(state): State<AppState>,
) -> Result<Json<crate::engine::failure_domains::FailureDomainsReport>, ApiError> {
    crate::engine::failure_domains::load_report(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}
