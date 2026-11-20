// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, Query, State};
use axum::Extension;
use axum::Json;
use serde::Serialize;
use uuid::Uuid;

use crate::auth::{require_operator, AuthUser};

use crate::api::ApiError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct TaskResponse {
    pub task_id: String,
    pub status: String,
    pub operation: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TaskRow {
    pub id: Uuid,
    pub operation: String,
    pub status: String,
    pub progress: i16,
    pub message: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, serde::Deserialize)]
pub struct TaskQuery {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub operation: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    200
}

pub async fn list_tasks(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<TaskQuery>,
) -> Result<Json<Vec<TaskRow>>, ApiError> {
    require_operator(&actor)?;
    let limit = q.limit.clamp(1, 500);
    let rows = match (&q.status, &q.operation) {
        (Some(status), Some(op)) if !status.is_empty() && !op.is_empty() => {
            crate::db::query_as::<_, TaskRow>(
                "SELECT id, operation, status, progress, message,
                        strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
                 FROM tasks WHERE status = ? AND operation LIKE ? ORDER BY created_at DESC LIMIT ?",
            )
            .bind(status)
            .bind(format!("%{op}%"))
            .bind(limit)
            .fetch_all(&state.pool)
            .await?
        }
        (Some(status), _) if !status.is_empty() => {
            crate::db::query_as::<_, TaskRow>(
                "SELECT id, operation, status, progress, message,
                        strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
                 FROM tasks WHERE status = ? ORDER BY created_at DESC LIMIT ?",
            )
            .bind(status)
            .bind(limit)
            .fetch_all(&state.pool)
            .await?
        }
        (_, Some(op)) if !op.is_empty() => {
            crate::db::query_as::<_, TaskRow>(
                "SELECT id, operation, status, progress, message,
                        strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
                 FROM tasks WHERE operation LIKE ? ORDER BY created_at DESC LIMIT ?",
            )
            .bind(format!("%{op}%"))
            .bind(limit)
            .fetch_all(&state.pool)
            .await?
        }
        _ => {
            crate::db::query_as::<_, TaskRow>(
                "SELECT id, operation, status, progress, message,
                        strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
                 FROM tasks ORDER BY created_at DESC LIMIT ?",
            )
            .bind(limit)
            .fetch_all(&state.pool)
            .await?
        }
    };
    Ok(Json(rows))
}

pub async fn get_task(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskRow>, ApiError> {
    require_operator(&actor)?;
    let row = crate::db::query_as::<_, TaskRow>(
        "SELECT id, operation, status, progress, message,
                strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
         FROM tasks WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

pub async fn cancel_task(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskRow>, ApiError> {
    require_operator(&actor)?;
    // Fetch operation/payload before the cancel so an ha.recover task can be
    // compensated below: recover_vms (engine/ha.rs) writes the VM's host_id to
    // the recovery destination and bumps ha_recovery_count *before* the task
    // row is even created, so a still-'pending' ha.recover task has already
    // taken that side effect. A bare status write here — same class of bug as
    // the worker/enqueue/reap terminal-failure paths fixed elsewhere — would
    // leave the VM silently pointing at a host it was never created on.
    let row: Option<(String, serde_json::Value)> =
        crate::db::query_as("SELECT operation, payload FROM tasks WHERE id = ? AND status = 'pending'")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((operation, payload)) = row else {
        return Err(ApiError::bad_request("task not pending or not found"));
    };
    let updated = crate::db::query(
        "UPDATE tasks SET status = 'cancelled', message = 'cancelled by operator', updated_at = datetime('now')
         WHERE id = ? AND status = 'pending'",
    )
    .bind(id)
    .execute(&state.pool)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(ApiError::bad_request("task not pending or not found"));
    }
    if operation == "ha.recover" {
        let msg = crate::tasks::TaskMessage {
            task_id: id,
            operation,
            payload,
        };
        if let Err(e) = crate::tasks::worker::revert_failed_ha_recovery(&state.pool, &msg).await {
            tracing::error!(task_id = %id, "ha.recover: compensation after operator cancel failed: {e:#}");
        }
    }
    get_task(State(state), Extension(actor), Path(id)).await
}

pub async fn retry_task(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    let row: (
        String,
        serde_json::Value,
        Option<String>,
        Option<Uuid>,
        Option<Uuid>,
        String,
    ) = crate::db::query_as(
        "SELECT operation, payload, resource_type, resource_id, host_id, status FROM tasks WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;

    // Only a task that actually finished (failed/cancelled) is safe to retry —
    // retrying a still-pending/running task would race a duplicate execution
    // against the original, and retrying an already-completed one would
    // re-enqueue with stale payload data.
    if row.5 != "failed" && row.5 != "cancelled" {
        return Err(ApiError::bad_request(format!(
            "task is '{}', not failed/cancelled — nothing to retry",
            row.5
        )));
    }
    // ha.recover carries side effects (host_id write + ha_recovery_count bump)
    // that engine/ha.rs::recover_vms applies *before* creating the task row —
    // see cancel_task's compensation above. Blindly re-enqueuing this stale
    // payload here would skip that bookkeeping, and if the manual retry later
    // fails, finalize_terminal_task_failure's revert_failed_ha_recovery would
    // match on host_id and could revert a VM that a *later, unrelated*
    // successful recovery already moved on — stranding it on the original
    // failed host. The HA scanner already re-attempts recovery on its own
    // (recover_vms re-scans any VM whose host_id still points at an offline
    // host), so there is no safe manual re-entry point here; refuse it.
    if row.0 == "ha.recover" {
        return Err(ApiError::bad_request(
            "ha.recover tasks cannot be manually retried — the HA scanner automatically \
             re-attempts recovery for any VM still pointing at an offline host",
        ));
    }

    let new_id =
        crate::tasks::enqueue::enqueue_task(&state, &row.0, row.1, row.2.as_deref(), row.3, row.4)
            .await?;

    Ok(Json(TaskResponse {
        task_id: new_id.to_string(),
        status: "pending".into(),
        operation: row.0,
    }))
}
