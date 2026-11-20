// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, Query, State};
use axum::Extension;
use axum::Json;
use serde::Serialize;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct NotificationRow {
    pub id: Uuid,
    pub kind: String,
    pub payload: serde_json::Value,
    pub delivered: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub delivered_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, serde::Deserialize)]
pub struct NotificationQuery {
    #[serde(default)]
    pub undelivered: bool,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    100
}

pub async fn list_notifications(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<NotificationQuery>,
) -> Result<Json<Vec<NotificationRow>>, ApiError> {
    require_operator(&actor)?;
    let limit = q.limit.clamp(1, 500);
    let rows = if q.undelivered {
        crate::db::query_as::<_, NotificationRow>(
            "SELECT id, kind, payload, delivered, created_at, delivered_at
             FROM notification_outbox WHERE delivered = FALSE ORDER BY created_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    } else {
        crate::db::query_as::<_, NotificationRow>(
            "SELECT id, kind, payload, delivered, created_at, delivered_at
             FROM notification_outbox ORDER BY created_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    };
    Ok(Json(rows))
}

pub async fn mark_notification_delivered(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    crate::db::query(
        "UPDATE notification_outbox SET delivered = TRUE, delivered_at = datetime('now') WHERE id = ?",
    )
    .bind(id)
    .execute(&state.pool)
    .await?;
    Ok(Json(serde_json::json!({ "delivered": true })))
}
