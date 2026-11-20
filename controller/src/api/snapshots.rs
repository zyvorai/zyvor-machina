// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::tasks::TaskResponse;
use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SnapshotRow {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub name: String,
    pub status: String,
    pub message: Option<String>,
    pub snapshot_path: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSnapshotBody {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub disk_only: bool,
    #[serde(default)]
    pub quiesce: bool,
    #[serde(default)]
    pub storage_mode: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct VmTimelineRow {
    pub kind: String,
    pub id: Uuid,
    pub label: String,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn list_vm_timeline(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(vm_id): Path<Uuid>,
) -> Result<Json<Vec<VmTimelineRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, VmTimelineRow>(
        r#"
        SELECT * FROM (
            SELECT 'backup' AS kind, b.id,
                   CASE WHEN b.status = 'completed' THEN 'Backup successful'
                        ELSE 'Backup ' || b.status END AS label,
                   b.status, strftime('%Y-%m-%dT%H:%M:%SZ', b.created_at) AS created_at
            FROM backup_records b WHERE b.vm_id = ?
            UNION ALL
            SELECT 'snapshot' AS kind, s.id,
                   'Snapshot: ' || s.name AS label,
                   s.status, strftime('%Y-%m-%dT%H:%M:%SZ', s.created_at) AS created_at
            FROM snapshot_records s WHERE s.vm_id = ?
            UNION ALL
            SELECT 'console' AS kind, c.id,
                   'Console ' || c.protocol || ' (' || c.backend || ')' AS label,
                   CASE WHEN c.ended_at IS NULL THEN 'active' ELSE 'ended' END AS status,
                   strftime('%Y-%m-%dT%H:%M:%SZ', c.started_at) AS created_at
            FROM console_sessions c WHERE c.vm_id = ?
            UNION ALL
            SELECT 'lifecycle' AS kind, v.id,
                   'VM ' || COALESCE(v.observed_state, v.desired_state, 'unknown') AS label,
                   COALESCE(v.observed_state, v.desired_state, 'unknown') AS status,
                   strftime('%Y-%m-%dT%H:%M:%SZ', COALESCE(v.updated_at, v.created_at)) AS created_at
            FROM vms v WHERE v.id = ?
        ) t
        ORDER BY created_at DESC
        LIMIT 50
        "#,
    )
    .bind(vm_id)
    .bind(vm_id)
    .bind(vm_id)
    .bind(vm_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn list_vm_snapshots(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(vm_id): Path<Uuid>,
) -> Result<Json<Vec<SnapshotRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, SnapshotRow>(
        "SELECT id, vm_id, name, status, message, COALESCE(snapshot_path, '') AS snapshot_path,
                strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
         FROM snapshot_records WHERE vm_id = ? ORDER BY created_at DESC",
    )
    .bind(vm_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn create_vm_snapshot(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(vm_id): Path<Uuid>,
    Json(body): Json<CreateSnapshotBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_one(&state.pool)
        .await?;

    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO snapshot_records (id, vm_id, name, status) VALUES (?, ?, ?, 'pending')",
    )
    .bind(id)
    .bind(vm_id)
    .bind(&body.name)
    .execute(&state.pool)
    .await?;

    let task_id = enqueue_task(
        &state,
        "vm.snapshot",
        serde_json::json!({
            "vm_id": vm_id.to_string(),
            "snapshot_id": id.to_string(),
            "name": body.name,
            "description": body.description,
            "disk_only": body.disk_only,
            "quiesce": body.quiesce,
            "storage_mode": body.storage_mode,
        }),
        Some("vm"),
        Some(vm_id),
        host_id,
    )
    .await
    .inspect_err(|_e| {
        let pool = state.pool.clone();
        tokio::spawn(async move {
            let _ = crate::db::query("DELETE FROM snapshot_records WHERE id = ?")
                .bind(id)
                .execute(&pool)
                .await;
        });
    })?;

    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: "vm.snapshot".into(),
    }))
}

pub async fn delete_vm_snapshot(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((vm_id, name)): Path<(Uuid, String)>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_one(&state.pool)
        .await?;

    let task_id = enqueue_task(
        &state,
        "vm.snapshot.delete",
        serde_json::json!({
            "vm_id": vm_id.to_string(),
            "snapshot_name": name,
        }),
        Some("vm"),
        Some(vm_id),
        host_id,
    )
    .await?;

    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: "vm.snapshot.delete".into(),
    }))
}

pub async fn revert_vm_snapshot(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((vm_id, name)): Path<(Uuid, String)>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_one(&state.pool)
        .await?;

    let task_id = enqueue_task(
        &state,
        "vm.snapshot.revert",
        serde_json::json!({
            "vm_id": vm_id.to_string(),
            "snapshot_name": name,
        }),
        Some("vm"),
        Some(vm_id),
        host_id,
    )
    .await?;

    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: "vm.snapshot.revert".into(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct CloneSnapshotBody {
    pub new_name: String,
    #[serde(default)]
    pub revert_source: bool,
    #[serde(default)]
    pub dest_host_id: Option<Uuid>,
    #[serde(default = "default_live")]
    pub live: bool,
}

fn default_live() -> bool {
    true
}

pub async fn clone_vm_snapshot(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((vm_id, name)): Path<(Uuid, String)>,
    Json(body): Json<CloneSnapshotBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    crate::auth::require_operator(&actor)?;
    machina_spec::validate_name(&body.new_name)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_one(&state.pool)
        .await?;

    let task_id = enqueue_task(
        &state,
        "vm.snapshot.clone",
        serde_json::json!({
            "vm_id": vm_id.to_string(),
            "snapshot_name": name,
            "new_name": body.new_name,
            "revert_source": body.revert_source,
            "dest_host_id": body.dest_host_id.map(|id| id.to_string()),
            "live": body.live,
        }),
        Some("vm"),
        Some(vm_id),
        host_id,
    )
    .await?;

    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: "vm.snapshot.clone".into(),
    }))
}
