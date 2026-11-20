// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::State;
use axum::Extension;
use axum::Json;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::recommendations;
use crate::state::AppState;

pub async fn list_recommendations(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<recommendations::Recommendation>>, ApiError> {
    require_operator(&actor)?;
    let rows = recommendations::generate_recommendations(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(rows))
}
