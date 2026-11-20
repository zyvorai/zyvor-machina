// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//
// Day-2: per-VM autonomous health-watchdog policy (engine/health_watchdog.rs).

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct WatchdogPolicy {
    pub vm_id: Uuid,
    pub enabled: bool,
    pub failure_threshold_secs: i64,
    pub cooldown_secs: i64,
    pub max_restarts_per_hour: i64,
    pub unhealthy_since: Option<String>,
    pub last_restart_at: Option<String>,
    pub restarts_this_hour: i64,
}

#[derive(Debug, Deserialize)]
pub struct SetWatchdogBody {
    pub enabled: bool,
    #[serde(default)]
    pub failure_threshold_secs: Option<i64>,
    #[serde(default)]
    pub cooldown_secs: Option<i64>,
    #[serde(default)]
    pub max_restarts_per_hour: Option<i64>,
}

pub async fn get_vm_watchdog(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(vm_id): Path<Uuid>,
) -> Result<Json<WatchdogPolicy>, ApiError> {
    require_operator(&actor)?;
    let row = crate::db::query_as::<_, WatchdogPolicy>(
        "SELECT vm_id, enabled, failure_threshold_secs, cooldown_secs, max_restarts_per_hour,
                unhealthy_since, last_restart_at, restarts_this_hour
         FROM vm_watchdog WHERE vm_id = ?",
    )
    .bind(vm_id)
    .fetch_optional(&state.pool)
    .await?;
    // Default (disabled) view when no policy row exists yet.
    Ok(Json(row.unwrap_or(WatchdogPolicy {
        vm_id,
        enabled: false,
        failure_threshold_secs: 120,
        cooldown_secs: 600,
        max_restarts_per_hour: 3,
        unhealthy_since: None,
        last_restart_at: None,
        restarts_this_hour: 0,
    })))
}

pub async fn set_vm_watchdog(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(vm_id): Path<Uuid>,
    Json(body): Json<SetWatchdogBody>,
) -> Result<Json<WatchdogPolicy>, ApiError> {
    require_operator(&actor)?;
    // Validate the VM exists so we don't create a dangling policy row.
    let exists: Option<i64> = crate::db::query_scalar("SELECT 1 FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_optional(&state.pool)
        .await?;
    if exists.is_none() {
        return Err(ApiError::not_found("vm not found"));
    }
    let threshold = body.failure_threshold_secs.unwrap_or(120).clamp(30, 3600);
    let cooldown = body.cooldown_secs.unwrap_or(600).clamp(60, 86400);
    let max_per_hour = body.max_restarts_per_hour.unwrap_or(3).clamp(1, 60);

    crate::db::query(
        "INSERT INTO vm_watchdog (vm_id, enabled, failure_threshold_secs, cooldown_secs, max_restarts_per_hour)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(vm_id) DO UPDATE SET
             enabled = excluded.enabled,
             failure_threshold_secs = excluded.failure_threshold_secs,
             cooldown_secs = excluded.cooldown_secs,
             max_restarts_per_hour = excluded.max_restarts_per_hour",
    )
    .bind(vm_id)
    .bind(body.enabled)
    .bind(threshold)
    .bind(cooldown)
    .bind(max_per_hour)
    .execute(&state.pool)
    .await?;

    let row = crate::db::query_as::<_, WatchdogPolicy>(
        "SELECT vm_id, enabled, failure_threshold_secs, cooldown_secs, max_restarts_per_hour,
                unhealthy_since, last_restart_at, restarts_this_hour
         FROM vm_watchdog WHERE vm_id = ?",
    )
    .bind(vm_id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}
