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
pub struct VmScheduleRow {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub action: String,
    pub interval_minutes: i64,
    pub retention: Option<i64>,
    pub label: String,
    pub enabled: bool,
    pub next_run_at: String,
    pub last_run_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateVmScheduleBody {
    pub action: String,
    #[serde(default = "default_interval")]
    pub interval_minutes: i64,
    pub retention: Option<i64>,
    #[serde(default)]
    pub label: String,
}

fn default_interval() -> i64 {
    1440
}

const VALID_ACTIONS: &[&str] = &["start", "shutdown", "stop", "snapshot"];

pub async fn list_vm_schedules(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(vm_id): Path<Uuid>,
) -> Result<Json<Vec<VmScheduleRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, VmScheduleRow>(
        "SELECT id, vm_id, action, interval_minutes, retention, label, enabled,
                next_run_at, last_run_at, created_at
         FROM vm_schedules WHERE vm_id = ? ORDER BY created_at",
    )
    .bind(vm_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn create_vm_schedule(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(vm_id): Path<Uuid>,
    Json(body): Json<CreateVmScheduleBody>,
) -> Result<Json<VmScheduleRow>, ApiError> {
    require_operator(&actor)?;

    if !VALID_ACTIONS.contains(&body.action.as_str()) {
        return Err(ApiError::bad_request(
            "action must be start, shutdown, stop, or snapshot",
        ));
    }
    if body.interval_minutes < 1 {
        return Err(ApiError::bad_request("interval_minutes must be >= 1"));
    }

    // Verify VM exists
    let exists: Option<Uuid> = crate::db::query_scalar("SELECT id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_optional(&state.pool)
        .await?;
    if exists.is_none() {
        return Err(ApiError::not_found("vm not found"));
    }

    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO vm_schedules (id, vm_id, action, interval_minutes, retention, label, next_run_at)
         VALUES (?, ?, ?, ?, ?, ?, datetime('now', '+' || ? || ' minutes'))",
    )
    .bind(id)
    .bind(vm_id)
    .bind(&body.action)
    .bind(body.interval_minutes)
    .bind(body.retention)
    .bind(&body.label)
    .bind(body.interval_minutes)
    .execute(&state.pool)
    .await?;

    let row = crate::db::query_as::<_, VmScheduleRow>(
        "SELECT id, vm_id, action, interval_minutes, retention, label, enabled,
                next_run_at, last_run_at, created_at
         FROM vm_schedules WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(row))
}

pub async fn delete_vm_schedule(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((vm_id, schedule_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;

    let affected = crate::db::query("DELETE FROM vm_schedules WHERE id = ? AND vm_id = ?")
        .bind(schedule_id)
        .bind(vm_id)
        .execute(&state.pool)
        .await?
        .rows_affected();

    if affected == 0 {
        return Err(ApiError::not_found("schedule not found"));
    }

    Ok(Json(serde_json::json!({"ok": true})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_actions_contains_all_expected() {
        for action in &["start", "shutdown", "stop", "snapshot"] {
            assert!(
                VALID_ACTIONS.contains(action),
                "expected '{action}' to be a valid action"
            );
        }
    }

    #[test]
    fn invalid_actions_rejected() {
        for action in &["restart", "reboot", "delete", "", "STOP"] {
            assert!(
                !VALID_ACTIONS.contains(action),
                "expected '{action}' to be invalid"
            );
        }
    }

    #[test]
    fn default_interval_is_1440() {
        assert_eq!(default_interval(), 1440);
    }

    #[test]
    fn create_body_deserializes_with_defaults() {
        let json = r#"{"action":"snapshot"}"#;
        let body: CreateVmScheduleBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.action, "snapshot");
        assert_eq!(body.interval_minutes, 1440);
        assert!(body.retention.is_none());
        assert_eq!(body.label, "");
    }

    #[test]
    fn create_body_accepts_explicit_interval() {
        let json = r#"{"action":"start","interval_minutes":60,"retention":5,"label":"morning"}"#;
        let body: CreateVmScheduleBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.interval_minutes, 60);
        assert_eq!(body.retention, Some(5));
        assert_eq!(body.label, "morning");
    }
}
