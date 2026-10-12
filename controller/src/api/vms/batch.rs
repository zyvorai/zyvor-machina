// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::State;
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

use super::{delete_vm_inventory_row, power_action};

/// Cap on VMs per batch request — matches the `.take(64)` bound used by the
/// sibling read-only batch endpoints (`batch_vm_parity_summary`,
/// `batch_vm_guest_ips` in `vms/mod.rs`). Unlike those, these handlers enqueue
/// a task (or a destructive delete) per VM, so an unbounded `vm_ids` list is a
/// resource-exhaustion / mass-action vector; reject outright rather than
/// silently truncating so the caller knows to split the request.
const MAX_BATCH_VM_IDS: usize = 64;

fn check_batch_size(vm_ids: &[Uuid]) -> Result<(), ApiError> {
    if vm_ids.len() > MAX_BATCH_VM_IDS {
        return Err(ApiError::bad_request(format!(
            "batch request exceeds maximum of {MAX_BATCH_VM_IDS} VMs (got {})",
            vm_ids.len()
        )));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct BatchVmPowerBody {
    pub vm_ids: Vec<Uuid>,
    pub action: String,
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BatchVmPowerItem {
    pub vm_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BatchVmPowerResponse {
    pub results: Vec<BatchVmPowerItem>,
}

pub async fn batch_vm_power(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<BatchVmPowerBody>,
) -> Result<Json<BatchVmPowerResponse>, ApiError> {
    require_operator(&actor)?;
    check_batch_size(&body.vm_ids)?;
    let action = body.action.as_str();
    if !matches!(
        action,
        "start" | "stop" | "shutdown" | "reboot" | "pause" | "resume"
    ) {
        return Err(ApiError::bad_request("invalid batch power action"));
    }
    let operation = format!("vm.{action}");
    let mut results = Vec::with_capacity(body.vm_ids.len());
    for vm_id in body.vm_ids {
        match power_action(&state, vm_id, action, &operation, body.mode.clone()).await {
            Ok(Json(task)) => results.push(BatchVmPowerItem {
                vm_id: vm_id.to_string(),
                task_id: Some(task.task_id),
                error: None,
            }),
            Err(e) => results.push(BatchVmPowerItem {
                vm_id: vm_id.to_string(),
                task_id: None,
                error: Some(e.message),
            }),
        }
    }
    Ok(Json(BatchVmPowerResponse { results }))
}

#[derive(Debug, Deserialize)]
pub struct BatchVmSnapshotBody {
    pub vm_ids: Vec<Uuid>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_true")]
    pub disk_only: bool,
    #[serde(default)]
    pub quiesce: bool,
    #[serde(default)]
    pub storage_mode: String,
}

fn default_true() -> bool {
    true
}

pub async fn batch_vm_snapshot(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<BatchVmSnapshotBody>,
) -> Result<Json<BatchVmPowerResponse>, ApiError> {
    require_operator(&actor)?;
    check_batch_size(&body.vm_ids)?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let mut results = Vec::with_capacity(body.vm_ids.len());
    for (i, vm_id) in body.vm_ids.into_iter().enumerate() {
        let snap_name = if i == 0 {
            body.name.clone()
        } else {
            format!("{}-{}", body.name, i + 1)
        };
        let host_id: Option<Uuid> = match crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_optional(&state.pool)
            .await?
        {
            Some(h) => h,
            None => {
                results.push(BatchVmPowerItem {
                    vm_id: vm_id.to_string(),
                    task_id: None,
                    error: Some("vm not found".into()),
                });
                continue;
            }
        };
        let snapshot_id = Uuid::new_v4();
        if crate::db::query(
            "INSERT INTO snapshot_records (id, vm_id, name, status) VALUES (?, ?, ?, 'pending')",
        )
        .bind(snapshot_id)
        .bind(vm_id)
        .bind(&snap_name)
        .execute(&state.pool)
        .await
        .is_err()
        {
            results.push(BatchVmPowerItem {
                vm_id: vm_id.to_string(),
                task_id: None,
                error: Some("failed to record snapshot".into()),
            });
            continue;
        }
        match enqueue_task(
            &state,
            "vm.snapshot",
            serde_json::json!({
                "vm_id": vm_id.to_string(),
                "snapshot_id": snapshot_id.to_string(),
                "name": snap_name,
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
        {
            Ok(task_id) => results.push(BatchVmPowerItem {
                vm_id: vm_id.to_string(),
                task_id: Some(task_id.to_string()),
                error: None,
            }),
            Err(e) => results.push(BatchVmPowerItem {
                vm_id: vm_id.to_string(),
                task_id: None,
                error: Some(e.message),
            }),
        }
    }
    Ok(Json(BatchVmPowerResponse { results }))
}

#[derive(Debug, Deserialize)]
pub struct BatchVmDeleteBody {
    pub vm_ids: Vec<Uuid>,
    #[serde(default)]
    pub confirmed: bool,
}

pub async fn batch_vm_delete(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<BatchVmDeleteBody>,
) -> Result<Json<BatchVmPowerResponse>, ApiError> {
    require_operator(&actor)?;
    check_batch_size(&body.vm_ids)?;
    let require: bool = crate::db::query_scalar(
        "SELECT require_vm_delete_approval FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_one(&state.pool)
    .await
    .unwrap_or(false);
    if require && !body.confirmed {
        return Err(ApiError::bad_request(
            "VM deletion requires approval — resubmit with {\"confirmed\": true}",
        ));
    }
    let mut results = Vec::with_capacity(body.vm_ids.len());
    for vm_id in body.vm_ids {
        let row: Option<(Option<Uuid>, String)> =
            crate::db::query_as("SELECT host_id, observed_state FROM vms WHERE id = ?")
                .bind(vm_id)
                .fetch_optional(&state.pool)
                .await?;
        let Some((host_id, observed_state)) = row else {
            results.push(BatchVmPowerItem {
                vm_id: vm_id.to_string(),
                task_id: None,
                error: Some("vm not found".into()),
            });
            continue;
        };
        if observed_state == "missing" {
            match delete_vm_inventory_row(&state.pool, vm_id).await {
                Ok(name) => {
                    state.emit_event(
                        "vm.pruned",
                        format!("Pruned missing VM record {name} from inventory"),
                    );
                    results.push(BatchVmPowerItem {
                        vm_id: vm_id.to_string(),
                        task_id: None,
                        error: None,
                    });
                }
                Err(e) => results.push(BatchVmPowerItem {
                    vm_id: vm_id.to_string(),
                    task_id: None,
                    error: Some(e.message),
                }),
            }
            continue;
        }
        match enqueue_task(
            &state,
            "vm.delete",
            serde_json::json!({ "vm_id": vm_id.to_string() }),
            Some("vm"),
            Some(vm_id),
            host_id,
        )
        .await
        {
            Ok(task_id) => results.push(BatchVmPowerItem {
                vm_id: vm_id.to_string(),
                task_id: Some(task_id.to_string()),
                error: None,
            }),
            Err(e) => results.push(BatchVmPowerItem {
                vm_id: vm_id.to_string(),
                task_id: None,
                error: Some(e.message),
            }),
        }
    }
    Ok(Json(BatchVmPowerResponse { results }))
}
