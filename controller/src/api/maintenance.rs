// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct MaintenanceScheduleRow {
    pub id: Uuid,
    pub host_id: Uuid,
    pub action: String,
    pub evacuate: bool,
    pub run_at: chrono::DateTime<chrono::Utc>,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateScheduleBody {
    pub host_id: Uuid,
    #[serde(default = "default_action")]
    pub action: String,
    #[serde(default = "default_true")]
    pub evacuate: bool,
    pub run_at: chrono::DateTime<chrono::Utc>,
}

fn default_action() -> String {
    "enter".into()
}
fn default_true() -> bool {
    true
}

pub async fn get_schedule(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<MaintenanceScheduleRow>, ApiError> {
    require_operator(&actor)?;
    let row = crate::db::query_as::<_, MaintenanceScheduleRow>(
        "SELECT id, host_id, action, evacuate, run_at, status FROM maintenance_schedules WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("schedule not found"))?;
    Ok(Json(row))
}

pub async fn list_schedules(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<MaintenanceScheduleRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, MaintenanceScheduleRow>(
        "SELECT id, host_id, action, evacuate, run_at, status FROM maintenance_schedules
         ORDER BY run_at DESC LIMIT 100",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn create_schedule(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateScheduleBody>,
) -> Result<Json<MaintenanceScheduleRow>, ApiError> {
    require_admin(&actor)?;
    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO maintenance_schedules (id, host_id, action, evacuate, run_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(body.host_id)
    .bind(&body.action)
    .bind(body.evacuate)
    .bind(body.run_at)
    .execute(&state.pool)
    .await?;
    let row = crate::db::query_as::<_, MaintenanceScheduleRow>(
        "SELECT id, host_id, action, evacuate, run_at, status FROM maintenance_schedules WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

pub async fn delete_schedule(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    let deleted =
        crate::db::query("DELETE FROM maintenance_schedules WHERE id = ? AND status = 'pending'")
            .bind(id)
            .execute(&state.pool)
            .await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found(
            "schedule not found or already running/completed",
        ));
    }
    Ok(Json(serde_json::json!({ "deleted": true })))
}

pub async fn fence_host_manual(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(host_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    let ok = crate::engine::drs::fence_host(&state, host_id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(serde_json::json!({ "fenced": ok })))
}
