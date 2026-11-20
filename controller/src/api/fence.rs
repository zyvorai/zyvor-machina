// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::State;
use axum::Extension;
use axum::Json;
use serde::Serialize;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct FenceEventRow {
    pub id: Uuid,
    pub host_id: Uuid,
    pub action: String,
    pub success: bool,
    pub message: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn list_fence_events(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<FenceEventRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, FenceEventRow>(
        "SELECT id, host_id, action, success, message, created_at
         FROM fence_events ORDER BY created_at DESC LIMIT 100",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}
