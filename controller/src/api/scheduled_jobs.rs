// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//
// Day-2: CRUD for the general recurring-jobs framework (engine/scheduled_jobs_runner.rs).

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::scheduled_jobs_runner::{is_allowed_operation, ALLOWED_OPERATIONS};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ScheduledJobRow {
    pub id: Uuid,
    pub name: String,
    pub operation: String,
    pub payload: String,
    pub target_host_id: Option<Uuid>,
    pub interval_minutes: i64,
    pub enabled: bool,
    pub last_run_at: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateScheduledJobBody {
    pub name: String,
    pub operation: String,
    #[serde(default = "default_payload")]
    pub payload: String,
    #[serde(default)]
    pub target_host_id: Option<Uuid>,
    #[serde(default = "default_interval")]
    pub interval_minutes: i64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_payload() -> String {
    "{}".into()
}
fn default_interval() -> i64 {
    60
}
fn default_enabled() -> bool {
    true
}

pub async fn list_scheduled_jobs(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<ScheduledJobRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, ScheduledJobRow>(
        "SELECT id, name, operation, payload, target_host_id, interval_minutes, enabled,
                last_run_at, created_at
         FROM scheduled_jobs ORDER BY created_at DESC LIMIT 500",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn create_scheduled_job(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateScheduledJobBody>,
) -> Result<Json<ScheduledJobRow>, ApiError> {
    require_operator(&actor)?;
    if body.name.trim().is_empty() || body.name.len() > 128 {
        return Err(ApiError::bad_request("job name must be 1–128 characters"));
    }
    if !is_allowed_operation(&body.operation) {
        return Err(ApiError::bad_request(format!(
            "operation must be one of: {}",
            ALLOWED_OPERATIONS.join(", ")
        )));
    }
    if body.interval_minutes < 1 || body.interval_minutes > 60 * 24 * 30 {
        return Err(ApiError::bad_request("interval_minutes out of range"));
    }
    if serde_json::from_str::<serde_json::Value>(&body.payload).is_err() {
        return Err(ApiError::bad_request("payload must be valid JSON"));
    }
    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO scheduled_jobs
           (id, name, operation, payload, target_host_id, interval_minutes, enabled)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(body.name.trim())
    .bind(&body.operation)
    .bind(&body.payload)
    .bind(body.target_host_id)
    .bind(body.interval_minutes)
    .bind(body.enabled)
    .execute(&state.pool)
    .await?;
    let row = crate::db::query_as::<_, ScheduledJobRow>(
        "SELECT id, name, operation, payload, target_host_id, interval_minutes, enabled,
                last_run_at, created_at
         FROM scheduled_jobs WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct PatchScheduledJobBody {
    pub enabled: bool,
}

pub async fn patch_scheduled_job(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchScheduledJobBody>,
) -> Result<Json<ScheduledJobRow>, ApiError> {
    require_operator(&actor)?;
    let updated = crate::db::query("UPDATE scheduled_jobs SET enabled = ? WHERE id = ?")
        .bind(body.enabled)
        .bind(id)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if updated == 0 {
        return Err(ApiError::not_found("scheduled job not found"));
    }
    let row = crate::db::query_as::<_, ScheduledJobRow>(
        "SELECT id, name, operation, payload, target_host_id, interval_minutes, enabled,
                last_run_at, created_at
         FROM scheduled_jobs WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

pub async fn delete_scheduled_job(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let deleted = crate::db::query("DELETE FROM scheduled_jobs WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if deleted == 0 {
        return Err(ApiError::not_found("scheduled job not found"));
    }
    Ok(Json(serde_json::json!({ "deleted": true })))
}
