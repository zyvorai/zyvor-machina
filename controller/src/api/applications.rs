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
use crate::tasks::enqueue::enqueue_task;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ApplicationGroupRow {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct ApplicationGroupDetail {
    #[serde(flatten)]
    pub group: ApplicationGroupRow,
    pub vm_ids: Vec<Uuid>,
    pub vm_names: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateApplicationBody {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub vm_ids: Vec<Uuid>,
}

pub async fn list_applications(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<ApplicationGroupRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, ApplicationGroupRow>(
        "SELECT id, name, description,
                strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
         FROM application_groups ORDER BY name LIMIT 500",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn get_application(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApplicationGroupDetail>, ApiError> {
    require_operator(&actor)?;
    let group = crate::db::query_as::<_, ApplicationGroupRow>(
        "SELECT id, name, description,
                strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
         FROM application_groups WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("application not found"))?;

    let vms: Vec<(Uuid, String)> = crate::db::query_as(
        "SELECT v.id, v.name FROM application_group_vms agv
         JOIN vms v ON v.id = agv.vm_id WHERE agv.group_id = ? ORDER BY v.name",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(ApplicationGroupDetail {
        vm_ids: vms.iter().map(|(id, _)| *id).collect(),
        vm_names: vms.iter().map(|(_, n)| n.clone()).collect(),
        group,
    }))
}

pub async fn create_application(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateApplicationBody>,
) -> Result<Json<ApplicationGroupDetail>, ApiError> {
    require_operator(&actor)?;
    let name = body.name.trim();
    machina_spec::validate_label(name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let cluster_id: Uuid = crate::db::query_scalar("SELECT id FROM clusters LIMIT 1")
        .fetch_one(&state.pool)
        .await?;
    let id = Uuid::new_v4();
    let mut tx = state.pool.begin().await?;
    crate::db::query(
        "INSERT INTO application_groups (id, cluster_id, name, description) VALUES (?, ?, ?, ?)",
    )
    .bind(id)
    .bind(cluster_id)
    .bind(name)
    .bind(body.description.trim())
    .execute(&mut *tx)
    .await?;
    for vm_id in &body.vm_ids {
        crate::db::query("INSERT INTO application_group_vms (group_id, vm_id) VALUES (?, ?) ON CONFLICT DO NOTHING")
            .bind(id)
            .bind(vm_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    get_application(State(state), Extension(actor), Path(id)).await
}

pub async fn delete_application(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let mut tx = state.pool.begin().await?;
    let existed: Option<String> =
        crate::db::query_scalar("SELECT name FROM application_groups WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(name) = existed else {
        return Err(ApiError::not_found("application not found"));
    };
    crate::db::query("DELETE FROM application_group_vms WHERE group_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    crate::db::query("DELETE FROM application_groups WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    state.emit_event(
        "application.delete",
        format!("Deleted application group {name}"),
    );
    Ok(Json(serde_json::json!({ "deleted": true, "id": id })))
}

#[derive(Debug, Deserialize)]
pub struct ApplicationActionBody {
    pub action: String,
}

pub async fn run_application_action(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<ApplicationActionBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let vms: Vec<(Uuid, Option<Uuid>)> = crate::db::query_as(
        "SELECT agv.vm_id, v.host_id FROM application_group_vms agv
         JOIN vms v ON v.id = agv.vm_id WHERE agv.group_id = ?",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    if vms.is_empty() {
        return Err(ApiError::bad_request("application group has no VMs"));
    }
    let mut task_ids = Vec::new();
    for (vm_id, host_id) in vms {
        let (op, payload) = match body.action.as_str() {
            "start" => (
                "vm.power",
                serde_json::json!({ "vm_id": vm_id.to_string(), "action": "start" }),
            ),
            "stop" => (
                "vm.power",
                serde_json::json!({ "vm_id": vm_id.to_string(), "action": "stop" }),
            ),
            "backup" => (
                "vm.backup",
                serde_json::json!({ "vm_id": vm_id.to_string(), "backup_type": "full" }),
            ),
            other => return Err(ApiError::bad_request(format!("unknown action: {other}"))),
        };
        let task_id = enqueue_task(&state, op, payload, Some("vm"), Some(vm_id), host_id).await?;
        task_ids.push(task_id.to_string());
    }
    Ok(Json(serde_json::json!({ "task_ids": task_ids })))
}
