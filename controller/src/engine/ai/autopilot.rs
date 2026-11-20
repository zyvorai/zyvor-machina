// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::AuthUser;
use crate::engine::recommendations;
use crate::state::AppState;
use crate::tasks::enqueue::write_audit;
use crate::tasks::TaskMessage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposedAction {
    pub id: String,
    pub label: String,
    pub review: String,
    pub risk: String,
    pub action_type: String,
    pub object_ref: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct AutopilotProposal {
    pub mode: String,
    pub actions: Vec<ProposedAction>,
}

pub async fn propose(pool: &DbPool, vm_id: Option<Uuid>) -> anyhow::Result<AutopilotProposal> {
    let settings = super::settings::get_ai_settings(pool).await?;
    let mut actions = Vec::new();

    let recs = recommendations::generate_recommendations(pool).await?;
    for r in recs {
        if matches!(
            r.fix_action.as_str(),
            "bulk_backup" | "bulk_ha" | "open_hosts" | "open_vms"
        ) {
            actions.push(ProposedAction {
                id: r.id.clone(),
                label: r.title.clone(),
                review: format!("{} — {}", r.why, r.action),
                risk: r.risk.clone(),
                action_type: r.fix_action.clone(),
                object_ref: r.object_ref.unwrap_or(serde_json::json!({})),
            });
        }
    }

    if let Some(vid) = vm_id {
        if let Ok(health) = crate::engine::vm_health::run_vm_health_check(pool, vid).await {
            for issue in health.issues {
                if let (Some(fix), Some(label)) = (issue.fix_action, issue.fix_label) {
                    if matches!(
                        fix.as_str(),
                        "start_vm"
                            | "create_backup"
                            | "enable_ha"
                            | "install_guest_tools"
                            | "adopt_vm"
                    ) {
                        actions.push(ProposedAction {
                            id: format!("doctor-{}-{}", vid, issue.id),
                            label: label.clone(),
                            review: issue.message.clone(),
                            risk: if issue.severity == "critical" {
                                "Review required".into()
                            } else {
                                "Low".into()
                            },
                            action_type: fix,
                            object_ref: serde_json::json!({ "vm_id": vid.to_string() }),
                        });
                    }
                }
            }
        }
    }

    for vm in crate::engine::vm_inventory::stale_libvirt_vms(pool).await? {
        if vm.managed || vm_id.is_some_and(|v| v != vm.id) {
            continue;
        }
        actions.push(ProposedAction {
            id: format!("stale-vm-{}", vm.id),
            label: format!("Remove stale VM '{}'", vm.name),
            review: format!(
                "Its host is reporting inventory but has not listed this VM since {}; \
                 the domain no longer exists, so consoles, keys and power actions fail.",
                vm.last_seen
            ),
            risk: "Low".into(),
            action_type: "remove_stale_vm".into(),
            object_ref: serde_json::json!({ "vm_id": vm.id.to_string() }),
        });
    }

    Ok(AutopilotProposal {
        mode: settings.mode,
        actions,
    })
}

#[derive(Debug, Deserialize)]
pub struct ExecuteBody {
    pub action_type: String,
    #[serde(default)]
    pub object_ref: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct ExecuteResult {
    pub message: String,
    pub task_ids: Vec<String>,
}

pub async fn execute(
    state: &AppState,
    actor: &AuthUser,
    body: &ExecuteBody,
) -> Result<ExecuteResult, ApiError> {
    crate::auth::require_operator(actor)?;
    let settings = super::settings::get_ai_settings(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if !autopilot_mode_allowed(&settings.mode) {
        return Err(ApiError::bad_request(
            "Autopilot actions require advisor, autopilot_preview, or autopilot mode",
        ));
    }

    let mut task_ids = Vec::new();
    let message = match body.action_type.as_str() {
        "bulk_backup" => {
            let vm_ids: Vec<String> = body
                .object_ref
                .get("vm_ids")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            for id_str in vm_ids.iter().take(10) {
                let vm_id =
                    Uuid::parse_str(id_str).map_err(|_| ApiError::bad_request("invalid vm_id"))?;
                let host_id: Option<Uuid> =
                    crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
                        .bind(vm_id)
                        .fetch_optional(&state.pool)
                        .await?;
                let backup_id = Uuid::new_v4();
                let task_id = Uuid::new_v4();
                let payload = serde_json::json!({ "vm_id": vm_id.to_string(), "backup_id": backup_id.to_string() });
                {
                    let mut tx = state
                        .pool
                        .begin()
                        .await
                        .map_err(|e| ApiError::internal(e.to_string()))?;
                    crate::db::query(
                        "INSERT INTO backup_records (id, vm_id, backup_type, status) VALUES (?, ?, 'full', 'pending')",
                    )
                    .bind(backup_id)
                    .bind(vm_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| ApiError::internal(e.to_string()))?;
                    crate::db::query(
                        "INSERT INTO tasks (id, operation, status, resource_type, resource_id, host_id, payload)
                         VALUES (?, 'vm.backup', 'pending', 'vm', ?, ?, ?)",
                    )
                    .bind(task_id)
                    .bind(vm_id)
                    .bind(host_id)
                    .bind(&payload)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| ApiError::internal(e.to_string()))?;
                    tx.commit()
                        .await
                        .map_err(|e| ApiError::internal(e.to_string()))?;
                }
                let msg = TaskMessage {
                    task_id,
                    operation: "vm.backup".to_string(),
                    payload,
                };
                state
                    .task_bus
                    .publish("machina.tasks", &msg)
                    .await
                    .map_err(|e| ApiError::internal(e.to_string()))?;
                task_ids.push(task_id.to_string());
            }
            format!("Queued {} backup task(s)", task_ids.len())
        }
        "create_backup" => {
            let vm_id = parse_vm_id(&body.object_ref)?;
            let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
                .bind(vm_id)
                .fetch_optional(&state.pool)
                .await?;
            let backup_id = Uuid::new_v4();
            let task_id = Uuid::new_v4();
            let payload = serde_json::json!({ "vm_id": vm_id.to_string(), "backup_id": backup_id.to_string() });
            {
                let mut tx = state
                    .pool
                    .begin()
                    .await
                    .map_err(|e| ApiError::internal(e.to_string()))?;
                crate::db::query(
                    "INSERT INTO backup_records (id, vm_id, backup_type, status) VALUES (?, ?, 'full', 'pending')",
                )
                .bind(backup_id)
                .bind(vm_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
                crate::db::query(
                    "INSERT INTO tasks (id, operation, status, resource_type, resource_id, host_id, payload)
                     VALUES (?, 'vm.backup', 'pending', 'vm', ?, ?, ?)",
                )
                .bind(task_id)
                .bind(vm_id)
                .bind(host_id)
                .bind(&payload)
                .execute(&mut *tx)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
                tx.commit()
                    .await
                    .map_err(|e| ApiError::internal(e.to_string()))?;
            }
            let msg = TaskMessage {
                task_id,
                operation: "vm.backup".to_string(),
                payload,
            };
            state
                .task_bus
                .publish("machina.tasks", &msg)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
            task_ids.push(task_id.to_string());
            "Backup queued".into()
        }
        "enable_ha" | "bulk_ha" => {
            let vm_ids: Vec<String> = if body.action_type == "bulk_ha" {
                body.object_ref
                    .get("vm_ids")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default()
            } else {
                vec![parse_vm_id(&body.object_ref)?.to_string()]
            };
            for id_str in vm_ids.iter().take(10) {
                let vm_id =
                    Uuid::parse_str(id_str).map_err(|_| ApiError::bad_request("invalid vm_id"))?;
                crate::engine::template::upsert_ha_policy(
                    &state.pool,
                    vm_id,
                    true,
                    3,
                    "medium",
                    false,
                    false,
                )
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
            }
            format!("HA enabled on {} VM(s)", vm_ids.len().min(10))
        }
        "install_guest_tools" => {
            let vm_id = parse_vm_id(&body.object_ref)?;
            let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
                .bind(vm_id)
                .fetch_optional(&state.pool)
                .await?;
            let tid = crate::tasks::enqueue::enqueue_task(
                state,
                "vm.guest_tools.install",
                serde_json::json!({ "vm_id": vm_id.to_string() }),
                Some("vm"),
                Some(vm_id),
                host_id,
            )
            .await?;
            task_ids.push(tid.to_string());
            "Guest tools install queued".into()
        }
        "start_vm" => {
            let vm_id = parse_vm_id(&body.object_ref)?;
            let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
                .bind(vm_id)
                .fetch_optional(&state.pool)
                .await?;
            let tid = crate::tasks::enqueue::enqueue_task(
                state,
                "vm.start",
                serde_json::json!({ "vm_id": vm_id.to_string() }),
                Some("vm"),
                Some(vm_id),
                host_id,
            )
            .await?;
            task_ids.push(tid.to_string());
            "Start queued".into()
        }
        // A clean guest shutdown (never a power-off). Reversible: undo starts it again.
        "stop_vm" => {
            let vm_id = parse_vm_id(&body.object_ref)?;
            let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
                .bind(vm_id)
                .fetch_optional(&state.pool)
                .await?
                .flatten();
            let tid = crate::tasks::enqueue::enqueue_task(
                state,
                "vm.power",
                serde_json::json!({ "vm_id": vm_id.to_string(), "action": "shutdown" }),
                Some("vm"),
                Some(vm_id),
                host_id,
            )
            .await?;
            task_ids.push(tid.to_string());
            "Shutdown queued".into()
        }
        "sync_hosts" => {
            let hosts: Vec<Uuid> =
                crate::db::query_scalar("SELECT id FROM hosts WHERE state = 'online'")
                    .fetch_all(&state.pool)
                    .await?;
            for hid in hosts {
                let tid = crate::tasks::enqueue::enqueue_task(
                    state,
                    "host.sync",
                    serde_json::json!({ "host_id": hid.to_string() }),
                    Some("host"),
                    Some(hid),
                    Some(hid),
                )
                .await?;
                task_ids.push(tid.to_string());
            }
            format!("Queued sync for {} host(s)", task_ids.len())
        }
        "remove_stale_vm" => {
            let vm_id = parse_vm_id(&body.object_ref)?;
            let vm = crate::engine::vm_inventory::stale_libvirt_vm(&state.pool, vm_id)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
                .ok_or_else(|| {
                    ApiError::conflict(
                        "VM is not stale",
                        "its host listed it recently or is not reporting; nothing removed",
                    )
                })?;
            if vm.managed {
                return Err(ApiError::bad_request(
                    "managed VM: delete it from the VM page instead",
                ));
            }
            crate::db::query("DELETE FROM vms WHERE id = ?")
                .bind(vm_id)
                .execute(&state.pool)
                .await?;
            state.emit_event(
                "vm.removed",
                format!("Stale VM '{}' removed — absent from hypervisor", vm.name),
            );
            format!("Removed stale VM '{}'", vm.name)
        }
        other => {
            return Err(ApiError::bad_request(format!(
                "unsupported autopilot action: {other}"
            )))
        }
    };

    write_audit(
        state,
        &actor.username,
        "ai.autopilot.execute",
        "ai",
        None,
        serde_json::json!({
            "action_type": body.action_type,
            "object_ref": body.object_ref,
            "task_ids": task_ids,
        }),
    )
    .await?;

    Ok(ExecuteResult { message, task_ids })
}

fn parse_vm_id(object_ref: &serde_json::Value) -> Result<Uuid, ApiError> {
    let id = object_ref
        .get("vm_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::bad_request("vm_id required"))?;
    Uuid::parse_str(id).map_err(|_| ApiError::bad_request("invalid vm_id"))
}

fn autopilot_mode_allowed(mode: &str) -> bool {
    matches!(mode, "advisor" | "autopilot_preview" | "autopilot")
}

fn is_auto_safe(action: &ProposedAction) -> bool {
    let low = action.risk.to_lowercase().contains("low");
    let ty = action.action_type.as_str();
    low && matches!(
        ty,
        "bulk_backup"
            | "create_backup"
            | "bulk_ha"
            | "enable_ha"
            | "install_guest_tools"
            | "start_vm"
    )
}

#[derive(Debug, Serialize)]
pub struct AutopilotRunResult {
    pub executed_count: usize,
    pub skipped_count: usize,
    pub results: Vec<ExecuteResult>,
}

pub async fn run_safe_batch(
    state: &AppState,
    actor: &AuthUser,
    vm_id: Option<Uuid>,
    max_actions: usize,
) -> Result<AutopilotRunResult, ApiError> {
    crate::auth::require_operator(actor)?;
    let settings = super::settings::get_ai_settings(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if settings.mode != "autopilot" {
        return Err(ApiError::bad_request(
            "Autopilot run requires ai_mode=autopilot (full mode with guardrails)",
        ));
    }

    let proposal = propose(&state.pool, vm_id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let cap = max_actions.clamp(1, 10);
    let all = proposal.actions;
    let skipped_count = all.iter().filter(|a| !is_auto_safe(a)).count();
    let safe: Vec<ProposedAction> = all.into_iter().filter(is_auto_safe).take(cap).collect();

    let mut results = Vec::new();
    for action in safe {
        let body = ExecuteBody {
            action_type: action.action_type.clone(),
            object_ref: action.object_ref.clone(),
        };
        let result = execute(state, actor, &body).await?;
        results.push(result);
    }

    write_audit(
        state,
        &actor.username,
        "ai.autopilot.run",
        "ai",
        None,
        serde_json::json!({
            "executed_count": results.len(),
            "skipped_count": skipped_count,
        }),
    )
    .await?;

    Ok(AutopilotRunResult {
        executed_count: results.len(),
        skipped_count,
        results,
    })
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AutopilotHistoryEntry {
    pub id: Uuid,
    pub actor: String,
    pub action: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub detail: serde_json::Value,
}

pub async fn list_history(
    pool: &DbPool,
    limit: i64,
) -> anyhow::Result<Vec<AutopilotHistoryEntry>> {
    let cap = limit.clamp(1, 100);
    let rows = crate::db::query_as::<_, AutopilotHistoryEntry>(
        "SELECT id, actor, action, created_at, COALESCE(detail, '{}') AS detail
         FROM audit_logs
         WHERE action LIKE 'ai.autopilot%'
         ORDER BY created_at DESC
         LIMIT ?",
    )
    .bind(cap)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
