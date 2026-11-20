// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Scale-to-zero: project sleep defaults and the fleet sleep summary.
//! Per-VM sleep/wake and policy live with the VM power routes.

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::vm_sleep::MIN_SLEEP_AFTER_MINUTES;
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ProjectSleepPolicy {
    pub project: String,
    pub sleep_after_minutes: i64,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct ProjectSleepBody {
    pub sleep_after_minutes: i64,
}

pub async fn list_project_policies(
    State(state): State<AppState>,
    Extension(_actor): Extension<AuthUser>,
) -> Result<Json<Vec<ProjectSleepPolicy>>, ApiError> {
    let rows = crate::db::query_as::<_, ProjectSleepPolicy>(
        "SELECT project, sleep_after_minutes, updated_at FROM vm_sleep_project_policies ORDER BY project",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn put_project_policy(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(project): Path<String>,
    Json(body): Json<ProjectSleepBody>,
) -> Result<Json<ProjectSleepPolicy>, ApiError> {
    require_operator(&actor)?;
    let project = project.trim().to_string();
    if project.is_empty() || project.len() > 128 {
        return Err(ApiError::bad_request("invalid project name"));
    }
    let m = body.sleep_after_minutes;
    if !(0..=10_080).contains(&m) || (1..MIN_SLEEP_AFTER_MINUTES).contains(&m) {
        return Err(ApiError::bad_request(format!(
            "sleep_after_minutes must be 0 or between {MIN_SLEEP_AFTER_MINUTES} and 10080"
        )));
    }
    let row = crate::db::query_as::<_, ProjectSleepPolicy>(
        "INSERT INTO vm_sleep_project_policies (project, sleep_after_minutes, updated_at)
         VALUES (?, ?, datetime('now'))
         ON CONFLICT (project) DO UPDATE SET
           sleep_after_minutes = EXCLUDED.sleep_after_minutes, updated_at = datetime('now')
         RETURNING project, sleep_after_minutes, updated_at",
    )
    .bind(&project)
    .bind(m)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

pub async fn delete_project_policy(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(project): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let res = crate::db::query("DELETE FROM vm_sleep_project_policies WHERE project = ?")
        .bind(project.trim())
        .execute(&state.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::not_found("no sleep policy for that project"));
    }
    Ok(Json(serde_json::json!({ "deleted": true })))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SleepingVm {
    pub id: Uuid,
    pub name: String,
    pub host_id: Option<Uuid>,
    pub project: Option<String>,
    pub memory_mib: i64,
    pub vcpus: i64,
    pub slept_at: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SleepEventRow {
    pub vm_id: Uuid,
    pub vm_name: String,
    pub kind: String,
    pub reason: String,
    pub at: String,
}

#[derive(Debug, Serialize)]
pub struct SleepSummary {
    pub sleeping: Vec<SleepingVm>,
    /// Guest RAM handed back to the hosts by sleeping VMs.
    pub memory_freed_mib: i64,
    pub vcpus_freed: i64,
    /// VMs with an auto-sleep policy in effect.
    pub auto_sleep_vms: i64,
    pub wakes_24h: i64,
    pub sleeps_24h: i64,
    pub recent: Vec<SleepEventRow>,
}

pub async fn summary(
    State(state): State<AppState>,
    Extension(_actor): Extension<AuthUser>,
) -> Result<Json<SleepSummary>, ApiError> {
    let sleeping = crate::db::query_as::<_, SleepingVm>(
        "SELECT id, name, host_id, project, COALESCE(memory_mib, 0) AS memory_mib,
                COALESCE(vcpus, 0) AS vcpus, slept_at
         FROM vms WHERE desired_state = 'sleeping' ORDER BY slept_at DESC",
    )
    .fetch_all(&state.pool)
    .await?;
    let auto_sleep_vms: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM vms v LEFT JOIN vm_sleep_project_policies p ON p.project = v.project
         WHERE v.sleep_after_minutes > 0 OR (v.sleep_after_minutes IS NULL AND p.sleep_after_minutes > 0)",
    )
    .fetch_one(&state.pool)
    .await?;
    let counts: (i64, i64) = crate::db::query_as(
        "SELECT COALESCE(SUM(CASE WHEN kind = 'wake' THEN 1 ELSE 0 END), 0), COALESCE(SUM(CASE WHEN kind = 'sleep' THEN 1 ELSE 0 END), 0)
         FROM vm_sleep_events WHERE at > datetime('now', '-1 day')",
    )
    .fetch_one(&state.pool)
    .await?;
    let recent = crate::db::query_as::<_, SleepEventRow>(
        "SELECT e.vm_id, v.name AS vm_name, e.kind, e.reason, e.at
         FROM vm_sleep_events e JOIN vms v ON v.id = e.vm_id
         ORDER BY e.id DESC LIMIT 30",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(SleepSummary {
        memory_freed_mib: sleeping.iter().map(|v| v.memory_mib).sum(),
        vcpus_freed: sleeping.iter().map(|v| v.vcpus).sum(),
        sleeping,
        auto_sleep_vms,
        wakes_24h: counts.0,
        sleeps_24h: counts.1,
        recent,
    }))
}
