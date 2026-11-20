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
use crate::tasks::enqueue::enqueue_task;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct BlueprintRow {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub actions: serde_json::Value,
    pub vm_ids: crate::db::Json<Vec<Uuid>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateBlueprintBody {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub vm_ids: Vec<Uuid>,
}

pub async fn get_blueprint(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<BlueprintRow>, ApiError> {
    require_operator(&actor)?;
    let row = crate::db::query_as::<_, BlueprintRow>(
        "SELECT id, name, description, actions, vm_ids, strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at FROM blueprints WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("blueprint not found"))?;
    Ok(Json(row))
}

pub async fn list_blueprints(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<BlueprintRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, BlueprintRow>(
        "SELECT id, name, description, actions, vm_ids, strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at FROM blueprints ORDER BY name",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn create_blueprint(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateBlueprintBody>,
) -> Result<Json<BlueprintRow>, ApiError> {
    require_operator(&actor)?;
    let name = body.name.trim();
    machina_spec::validate_label(name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    if body.actions.is_empty() {
        return Err(ApiError::bad_request(
            "actions required (start, stop, backup)",
        ));
    }
    let cluster_id: Uuid = crate::db::query_scalar("SELECT id FROM clusters LIMIT 1")
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::bad_request("no cluster configured"))?;
    let id = Uuid::new_v4();
    let actions = serde_json::to_value(&body.actions).unwrap_or(serde_json::json!([]));
    crate::db::query(
        "INSERT INTO blueprints (id, cluster_id, name, description, actions, vm_ids)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(cluster_id)
    .bind(name)
    .bind(body.description.trim())
    .bind(actions)
    .bind(serde_json::to_string(&body.vm_ids).unwrap_or_else(|_| "[]".into()))
    .execute(&state.pool)
    .await?;
    let row = crate::db::query_as::<_, BlueprintRow>(
        "SELECT id, name, description, actions, vm_ids, strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at FROM blueprints WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

pub async fn run_blueprint(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    let row: (serde_json::Value, crate::db::Json<Vec<Uuid>>) =
        crate::db::query_as("SELECT actions, vm_ids FROM blueprints WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::not_found("blueprint not found"))?;

    let actions: Vec<String> = serde_json::from_value(row.0).unwrap_or_default();
    let mut task_ids = Vec::new();
    for vm_id in row.1.iter() {
        let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_optional(&state.pool)
            .await?
            .flatten();
        for action in &actions {
            let (op, payload) = match action.as_str() {
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
                other => {
                    return Err(ApiError::bad_request(format!(
                        "unknown blueprint action: {other}"
                    )))
                }
            };
            let task_id =
                enqueue_task(&state, op, payload, Some("vm"), Some(*vm_id), host_id).await?;
            task_ids.push(task_id.to_string());
        }
    }
    Ok(Json(serde_json::json!({ "task_ids": task_ids })))
}

pub async fn delete_blueprint(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    let deleted = crate::db::query("DELETE FROM blueprints WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("blueprint not found"));
    }
    Ok(Json(serde_json::json!({ "deleted": true })))
}
