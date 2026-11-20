// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Query, State};
use axum::Extension;
use axum::Json;
use serde::Serialize;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct EventRow {
    pub id: Uuid,
    pub kind: String,
    pub message: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, serde::Deserialize)]
pub struct EventQuery {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    200
}

pub async fn list_events(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<EventQuery>,
) -> Result<Json<Vec<EventRow>>, ApiError> {
    require_operator(&actor)?;
    let limit = q.limit.clamp(1, 500);
    let rows = if let Some(kind) = q.kind.filter(|k| !k.is_empty()) {
        crate::db::query_as::<_, EventRow>(
            "SELECT id, kind, message,
                    strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
             FROM events WHERE kind LIKE ? ORDER BY created_at DESC LIMIT ?",
        )
        .bind(format!("%{kind}%"))
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    } else {
        crate::db::query_as::<_, EventRow>(
            "SELECT id, kind, message,
                    strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
             FROM events ORDER BY created_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    };
    Ok(Json(rows))
}
