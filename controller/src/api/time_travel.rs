// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Restore points, rewind and forks (`engine::time_travel`).

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::api::tasks::TaskResponse;
use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::time_travel::{self, TimeTravelView, MAX_KEEP, MIN_INTERVAL_MINUTES};
use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

async fn vm_row(state: &AppState, id: Uuid) -> Result<(String, Option<Uuid>, String), ApiError> {
    crate::db::query_as("SELECT name, host_id, observed_state FROM vms WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("vm not found"))
}

async fn task(
    state: &AppState,
    op: &str,
    id: Uuid,
    host: Option<Uuid>,
    payload: serde_json::Value,
) -> Result<Json<TaskResponse>, ApiError> {
    let task_id = enqueue_task(state, op, payload, Some("vm"), Some(id), host).await?;
    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: op.into(),
    }))
}

pub async fn get_time_travel(
    State(state): State<AppState>,
    Extension(_actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<TimeTravelView>, ApiError> {
    time_travel::view(&state.pool, id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map(Json)
        .ok_or_else(|| ApiError::not_found("vm not found"))
}

#[derive(Debug, Default, Deserialize)]
pub struct CreatePointBody {
    #[serde(default)]
    pub note: Option<String>,
}

pub async fn create_restore_point(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    body: Option<Json<CreatePointBody>>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    let (_, host, _) = vm_row(&state, id).await?;
    let note = body
        .and_then(|b| b.0.note)
        .map(|n| n.chars().take(200).collect::<String>())
        .filter(|n| !n.trim().is_empty());
    task(
        &state,
        "vm.restore_point",
        id,
        host,
        serde_json::json!({ "vm_id": id.to_string(), "kind": "manual", "note": note }),
    )
    .await
}

#[derive(Debug, Deserialize)]
pub struct PolicyBody {
    /// 0 turns scheduled restore points off.
    pub every_minutes: i64,
    #[serde(default)]
    pub keep: Option<i64>,
}

pub async fn set_policy(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<PolicyBody>,
) -> Result<Json<TimeTravelView>, ApiError> {
    require_operator(&actor)?;
    if body.every_minutes != 0 && !(MIN_INTERVAL_MINUTES..=10_080).contains(&body.every_minutes) {
        return Err(ApiError::bad_request(format!(
            "every_minutes must be 0 (off) or {MIN_INTERVAL_MINUTES}-10080"
        )));
    }
    if let Some(k) = body.keep {
        if !(1..=MAX_KEEP).contains(&k) {
            return Err(ApiError::bad_request(format!("keep must be 1-{MAX_KEEP}")));
        }
    }
    vm_row(&state, id).await?;
    crate::db::query(
        "UPDATE vms SET restore_point_minutes = ?, restore_point_keep = COALESCE(?, restore_point_keep) WHERE id = ?",
    )
    .bind((body.every_minutes > 0).then_some(body.every_minutes))
    .bind(body.keep)
    .bind(id)
    .execute(&state.pool)
    .await?;
    get_time_travel(State(state), Extension(actor), Path(id)).await
}

pub async fn rewind(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((id, point)): Path<(Uuid, Uuid)>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    let (_, host, _) = vm_row(&state, id).await?;
    let exists: Option<i64> =
        crate::db::query_scalar("SELECT 1 FROM vm_restore_points WHERE id = ? AND vm_id = ?")
            .bind(point)
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    if exists.is_none() {
        return Err(ApiError::not_found("restore point not found"));
    }
    let blocking = time_travel::forks_after(&state.pool, id, point)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if !blocking.is_empty() {
        return Err(ApiError::conflict(
            format!(
                "forks {} depend on later restore points",
                blocking.join(", ")
            ),
            "detach or delete those forks first",
        )
        .with_code("fork_pins_later_point"));
    }
    task(
        &state,
        "vm.rewind",
        id,
        host,
        serde_json::json!({ "vm_id": id.to_string(), "restore_point_id": point.to_string() }),
    )
    .await
}

#[derive(Debug, Deserialize)]
pub struct ForkBody {
    pub name: String,
    /// Fork from this point; omit to fork the VM as it is right now.
    #[serde(default)]
    pub restore_point_id: Option<Uuid>,
    /// Carry RAM across too (live source only; always isolated).
    #[serde(default)]
    pub memory: bool,
    #[serde(default)]
    pub isolate: bool,
    #[serde(default = "yes")]
    pub reseed: bool,
    #[serde(default = "yes")]
    pub start: bool,
}

fn yes() -> bool {
    true
}

pub async fn fork(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<ForkBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let (_, host, observed) = vm_row(&state, id).await?;
    let taken: Option<i64> = crate::db::query_scalar("SELECT 1 FROM vms WHERE name = ?")
        .bind(&body.name)
        .fetch_optional(&state.pool)
        .await?;
    if taken.is_some() {
        return Err(ApiError::conflict(
            format!("a VM named '{}' already exists", body.name),
            "pick another name",
        ));
    }
    if body.memory {
        if body.restore_point_id.is_some() {
            return Err(ApiError::bad_request(
                "a memory fork copies the running VM now; restore points hold disks only",
            ));
        }
        if observed != "running" {
            return Err(ApiError::bad_request("a memory fork needs a running VM")
                .with_code("vm_not_running"));
        }
    }
    if let Some(p) = body.restore_point_id {
        let exists: Option<i64> =
            crate::db::query_scalar("SELECT 1 FROM vm_restore_points WHERE id = ? AND vm_id = ?")
                .bind(p)
                .bind(id)
                .fetch_optional(&state.pool)
                .await?;
        if exists.is_none() {
            return Err(ApiError::not_found("restore point not found"));
        }
    }
    task(
        &state,
        "vm.fork",
        id,
        host,
        serde_json::json!({
            "vm_id": id.to_string(),
            "new_name": body.name,
            "restore_point_id": body.restore_point_id.map(|p| p.to_string()),
            "memory": body.memory,
            "isolate": body.isolate || body.memory,
            "reseed": body.reseed,
            "start": body.start || body.memory,
        }),
    )
    .await
}

pub async fn detach(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    let (_, host, _) = vm_row(&state, id).await?;
    let is_fork: Option<i64> = crate::db::query_scalar("SELECT 1 FROM vm_forks WHERE fork_vm_id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    if is_fork.is_none() {
        return Err(ApiError::bad_request("this VM is not a fork"));
    }
    task(
        &state,
        "vm.fork.detach",
        id,
        host,
        serde_json::json!({ "vm_id": id.to_string() }),
    )
    .await
}
