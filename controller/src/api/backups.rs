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
pub struct BackupRow {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub backup_type: String,
    pub status: String,
    pub message: Option<String>,
    pub backup_path: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// "" (never checked), "ok" or "failed" — see engine/backup_verifier.rs.
    pub verify_status: String,
    pub verified_at: Option<String>,
    pub verify_message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateBackupBody {
    #[serde(default = "default_type")]
    pub backup_type: String,
    #[serde(default)]
    pub target_id: Option<Uuid>,
}

fn default_type() -> String {
    "full".into()
}

pub async fn list_vm_backups(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(vm_id): Path<Uuid>,
) -> Result<Json<Vec<BackupRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, BackupRow>(
        "SELECT id, vm_id, backup_type, status, message, COALESCE(backup_path, '') AS backup_path,
                strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at,
                verify_status,
                strftime('%Y-%m-%dT%H:%M:%SZ', verified_at) AS verified_at, verify_message
         FROM backup_records WHERE vm_id = ? ORDER BY created_at DESC LIMIT 200",
    )
    .bind(vm_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn create_vm_backup(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(vm_id): Path<Uuid>,
    Json(body): Json<CreateBackupBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_one(&state.pool)
        .await?;

    let id = Uuid::new_v4();
    // Insert the record BEFORE enqueuing: the worker looks up backup_records by
    // this id and hard-fails "backup record not found" if absent, so a worker
    // (possibly another node) must never dequeue vm.backup before the row commits.
    // Compensate with a delete if the enqueue fails. Mirrors create_vm_snapshot.
    crate::db::query(
        "INSERT INTO backup_records (id, vm_id, backup_type, status) VALUES (?, ?, ?, 'pending')",
    )
    .bind(id)
    .bind(vm_id)
    .bind(&body.backup_type)
    .execute(&state.pool)
    .await?;

    let task_id = enqueue_task(
        &state,
        "vm.backup",
        serde_json::json!({
            "vm_id": vm_id.to_string(),
            "backup_id": id.to_string(),
            "target_id": body.target_id.map(|t| t.to_string()),
        }),
        Some("vm"),
        Some(vm_id),
        host_id,
    )
    .await
    .inspect_err(|_e| {
        let pool = state.pool.clone();
        tokio::spawn(async move {
            let _ = crate::db::query("DELETE FROM backup_records WHERE id = ?")
                .bind(id)
                .execute(&pool)
                .await;
        });
    })?;

    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: "vm.backup".into(),
    }))
}

// ---- Scheduled backups (day-2) --------------------------------------------------

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct BackupScheduleRow {
    pub id: Uuid,
    pub name: String,
    pub project: String,
    pub tag_filter: String,
    pub backup_type: String,
    pub target_id: Option<Uuid>,
    pub interval_hours: i64,
    pub retain_count: i64,
    pub enabled: bool,
    pub last_run_at: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateBackupScheduleBody {
    pub name: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub tag_filter: String,
    #[serde(default = "default_type")]
    pub backup_type: String,
    #[serde(default)]
    pub target_id: Option<Uuid>,
    #[serde(default = "default_interval_hours")]
    pub interval_hours: i64,
    #[serde(default = "default_retain")]
    pub retain_count: i64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_interval_hours() -> i64 {
    24
}
fn default_retain() -> i64 {
    7
}
fn default_enabled() -> bool {
    true
}

/// Re-check one stored backup now (the same check the scheduler runs).
pub async fn verify_vm_backup(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((vm_id, backup_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let owns: Option<Uuid> = crate::db::query_scalar("SELECT vm_id FROM backup_records WHERE id = ?")
        .bind(backup_id)
        .fetch_optional(&state.pool)
        .await?;
    if owns != Some(vm_id) {
        return Err(ApiError::not_found("backup not found for this machine"));
    }
    let (ok, message) = crate::engine::backup_verifier::verify_one(&state, backup_id, &actor.username)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": ok, "message": message })))
}

pub async fn list_backup_schedules(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<BackupScheduleRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, BackupScheduleRow>(
        "SELECT id, name, project, tag_filter, backup_type, target_id,
                interval_hours, retain_count, enabled,
                last_run_at, created_at
         FROM backup_schedules ORDER BY created_at DESC LIMIT 500",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn create_backup_schedule(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateBackupScheduleBody>,
) -> Result<Json<BackupScheduleRow>, ApiError> {
    require_operator(&actor)?;
    if body.name.trim().is_empty() || body.name.len() > 128 {
        return Err(ApiError::bad_request(
            "schedule name must be 1–128 characters",
        ));
    }
    if body.interval_hours < 1 || body.interval_hours > 24 * 30 {
        return Err(ApiError::bad_request("interval_hours must be 1–720"));
    }
    if body.retain_count < 0 || body.retain_count > 1000 {
        return Err(ApiError::bad_request("retain_count must be 0–1000"));
    }
    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO backup_schedules
           (id, name, project, tag_filter, backup_type, target_id, interval_hours, retain_count, enabled)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(body.name.trim())
    .bind(&body.project)
    .bind(&body.tag_filter)
    .bind(&body.backup_type)
    .bind(body.target_id)
    .bind(body.interval_hours)
    .bind(body.retain_count)
    .bind(body.enabled)
    .execute(&state.pool)
    .await?;
    let row = crate::db::query_as::<_, BackupScheduleRow>(
        "SELECT id, name, project, tag_filter, backup_type, target_id,
                interval_hours, retain_count, enabled, last_run_at, created_at
         FROM backup_schedules WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

pub async fn delete_backup_schedule(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    crate::db::query("DELETE FROM backup_schedules WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

pub async fn restore_vm_backup(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((vm_id, backup_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    // Reject up-front (clean 404) if the backup doesn't belong to this VM or
    // isn't completed — the worker enforces the same guard, but this gives the
    // caller a proper error instead of a task that fails asynchronously, and
    // prevents restoring one VM's image onto another (data loss + cross-tenant).
    let owns_backup: Option<i64> = crate::db::query_scalar(
        "SELECT 1 FROM backup_records
         WHERE id = ? AND vm_id = ? AND status IN ('completed', 'succeeded')",
    )
    .bind(backup_id)
    .bind(vm_id)
    .fetch_optional(&state.pool)
    .await?;
    if owns_backup.is_none() {
        return Err(ApiError::not_found(format!(
            "completed backup {backup_id} not found for VM {vm_id}"
        )));
    }
    let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_one(&state.pool)
        .await?;

    let task_id = enqueue_task(
        &state,
        "vm.backup.restore",
        serde_json::json!({
            "vm_id": vm_id.to_string(),
            "backup_id": backup_id.to_string(),
        }),
        Some("vm"),
        Some(vm_id),
        host_id,
    )
    .await?;

    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: "vm.backup.restore".into(),
    }))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct BackupTimelineRow {
    pub kind: String,
    pub id: Uuid,
    pub vm_id: Uuid,
    pub vm_name: String,
    pub label: String,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn list_backup_timeline(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<BackupTimelineRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, BackupTimelineRow>(
        r#"
        SELECT * FROM (
            SELECT 'backup' AS kind, b.id, b.vm_id, v.name AS vm_name,
                   CASE WHEN b.status = 'completed' THEN 'Backup successful'
                        ELSE 'Backup ' || b.status END AS label,
                   b.status, b.created_at
            FROM backup_records b
            JOIN vms v ON v.id = b.vm_id
            UNION ALL
            SELECT 'snapshot' AS kind, s.id, s.vm_id, v.name AS vm_name,
                   'Snapshot: ' || s.name AS label,
                   s.status, s.created_at
            FROM snapshot_records s
            JOIN vms v ON v.id = s.vm_id
        ) t
        ORDER BY created_at DESC
        LIMIT 100
        "#,
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    // The restore path must never hand a backup to a VM it doesn't belong to,
    // and must never restore from a non-completed record. This guards the exact
    // SQL predicate used by both the API handler (restore_vm_backup) and the
    // worker (vm_backup_restore) — a cross-VM restore is data loss + cross-tenant
    // exposure, so a regression here is catastrophic.
    async fn owns_completed(pool: &crate::db::DbPool, backup: Uuid, vm: Uuid) -> bool {
        let hit: Option<i64> = crate::db::query_scalar(
            "SELECT 1 FROM backup_records
             WHERE id = ? AND vm_id = ? AND status IN ('completed', 'succeeded')",
        )
        .bind(backup)
        .bind(vm)
        .fetch_optional(pool)
        .await
        .unwrap();
        hit.is_some()
    }

    #[tokio::test]
    async fn restore_guard_rejects_cross_vm_and_incomplete_backups() {
        let pool = crate::db::testing::pool().await;

        let vm_a = Uuid::new_v4();
        let vm_b = Uuid::new_v4();
        for (id, name) in [(vm_a, "vm-a"), (vm_b, "vm-b")] {
            crate::db::query("INSERT INTO vms (id, name) VALUES (?, ?)")
                .bind(id)
                .bind(name)
                .execute(&pool)
                .await
                .unwrap();
        }

        let good = Uuid::new_v4(); // completed backup of vm_a
        let pending = Uuid::new_v4(); // pending backup of vm_a
        crate::db::query("INSERT INTO backup_records (id, vm_id, status) VALUES (?, ?, 'completed')")
            .bind(good)
            .bind(vm_a)
            .execute(&pool)
            .await
            .unwrap();
        crate::db::query("INSERT INTO backup_records (id, vm_id, status) VALUES (?, ?, 'pending')")
            .bind(pending)
            .bind(vm_a)
            .execute(&pool)
            .await
            .unwrap();

        // Owner + completed => allowed.
        assert!(owns_completed(&pool, good, vm_a).await);
        // Same backup, DIFFERENT vm => rejected (the cross-VM data-loss case).
        assert!(!owns_completed(&pool, good, vm_b).await);
        // Owner but not completed => rejected (truncated/partial image).
        assert!(!owns_completed(&pool, pending, vm_a).await);
        // Unknown backup id => rejected.
        assert!(!owns_completed(&pool, Uuid::new_v4(), vm_a).await);
    }
}
