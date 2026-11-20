// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct FleetSnapshotScheduleRow {
    pub id: Uuid,
    pub name: String,
    pub cron_expr: String,
    pub project: String,
    pub tag_filter: String,
    pub disk_only: bool,
    pub quiesce: bool,
    pub retain_count: i32,
    pub enabled: bool,
    pub last_run_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct CreateFleetSnapshotScheduleBody {
    pub name: String,
    #[serde(default = "default_cron")]
    pub cron_expr: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub tag_filter: String,
    #[serde(default = "default_true")]
    pub disk_only: bool,
    #[serde(default)]
    pub quiesce: bool,
    #[serde(default = "default_retain")]
    pub retain_count: i32,
}

fn default_cron() -> String {
    "0 2 * * *".into()
}
fn default_true() -> bool {
    true
}
fn default_retain() -> i32 {
    5
}

pub async fn list_fleet_snapshot_schedules(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<FleetSnapshotScheduleRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, FleetSnapshotScheduleRow>(
        "SELECT id, name, cron_expr, project, tag_filter, disk_only, quiesce, retain_count, enabled,
                strftime('%Y-%m-%dT%H:%M:%SZ', last_run_at) AS last_run_at
         FROM fleet_snapshot_schedules ORDER BY name",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn create_fleet_snapshot_schedule(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateFleetSnapshotScheduleBody>,
) -> Result<Json<FleetSnapshotScheduleRow>, ApiError> {
    require_operator(&actor)?;
    if body.name.trim().is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO fleet_snapshot_schedules (id, name, cron_expr, project, tag_filter, disk_only, quiesce, retain_count)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(&body.name)
    .bind(&body.cron_expr)
    .bind(&body.project)
    .bind(&body.tag_filter)
    .bind(body.disk_only)
    .bind(body.quiesce)
    .bind(body.retain_count)
    .execute(&state.pool)
    .await?;
    let row = crate::db::query_as::<_, FleetSnapshotScheduleRow>(
        "SELECT id, name, cron_expr, project, tag_filter, disk_only, quiesce, retain_count, enabled,
                strftime('%Y-%m-%dT%H:%M:%SZ', last_run_at) AS last_run_at
         FROM fleet_snapshot_schedules WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

pub async fn delete_fleet_snapshot_schedule(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let r = crate::db::query("DELETE FROM fleet_snapshot_schedules WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found("schedule not found"));
    }
    Ok(Json(serde_json::json!({ "deleted": true })))
}
