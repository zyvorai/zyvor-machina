// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! After-the-fact safety for AI actions: remember what a machine looked like before an action ran,
//! check afterwards that the action did what it claimed, and undo the ones that can be undone.

use serde::Serialize;
use uuid::Uuid;

use super::actions::{get_action, ZyraActionRow};
use crate::state::AppState;

/// One executed (or failed) action with its verification and undo state, for the history view.
#[derive(Debug, Clone, Serialize)]
pub struct ActionHistoryRow {
    pub id: Uuid,
    pub action_type: String,
    pub label: String,
    pub status: String,
    pub requested_by: String,
    pub approved_by: Option<String>,
    pub executed_at: Option<String>,
    pub vm_id: Option<String>,
    pub before_state: serde_json::Value,
    pub verify: serde_json::Value,
    pub undoable: bool,
    pub undone_at: Option<String>,
}

fn vm_id_of(object_ref: &serde_json::Value) -> Option<Uuid> {
    object_ref
        .get("vm_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
}

fn is_off(state: &str) -> bool {
    matches!(state, "shutoff" | "stopped" | "shut off")
}

/// What undoing this action would do, if anything. Only actions whose reverse is safe and exact qualify.
pub fn undo_kind(action_type: &str, before: &serde_json::Value) -> Option<&'static str> {
    match action_type {
        // Only undo "enable HA" when HA was not already on before the action.
        "enable_ha" if before.get("ha_enabled").and_then(|v| v.as_bool()) != Some(true) => {
            Some("disable_ha")
        }
        // Only undo "start" when the machine was off before the action.
        "start_vm"
            if before
                .get("observed_state")
                .and_then(|v| v.as_str())
                .is_some_and(is_off) =>
        {
            Some("shutdown_vm")
        }
        // Only undo "stop" when the machine was running before the action.
        "stop_vm"
            if before
                .get("observed_state")
                .and_then(|v| v.as_str())
                .is_some_and(|st| st == "running") =>
        {
            Some("start_vm")
        }
        crate::engine::rightsizing::RESIZE_ACTION
            if before
                .get("vcpus")
                .and_then(|v| v.as_i64())
                .is_some_and(|v| v > 0) =>
        {
            Some("resize_back")
        }
        crate::api::stacks::DEPLOY_ACTION
            if before.get("stack_id").is_some_and(|v| v.is_string()) =>
        {
            if before.get("existed").and_then(|v| v.as_bool()) == Some(true) {
                Some("revert_stack")
            } else {
                Some("delete_stack")
            }
        }
        _ => None,
    }
}

/// Snapshot the machine just before an action runs. Best effort: never blocks the action.
pub async fn record_before(state: &AppState, action: &ZyraActionRow) {
    if action.action_type == crate::engine::rightsizing::RESIZE_ACTION {
        let snapshot = crate::engine::rightsizing::before(state, &action.object_ref).await;
        let _ = crate::db::query("UPDATE ai_actions SET before_state = ? WHERE id = ?")
            .bind(&snapshot)
            .bind(action.id)
            .execute(&state.pool)
            .await;
        return;
    }
    if action.action_type == crate::api::stacks::DEPLOY_ACTION {
        let snapshot = crate::api::stacks::deploy_before(state, &action.object_ref).await;
        let _ = crate::db::query("UPDATE ai_actions SET before_state = ? WHERE id = ?")
            .bind(&snapshot)
            .bind(action.id)
            .execute(&state.pool)
            .await;
        return;
    }
    let Some(vm_id) = vm_id_of(&action.object_ref) else {
        return;
    };
    let vm: Option<(String, Option<String>)> =
        crate::db::query_as("SELECT observed_state, guest_tools_status FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
    let ha_enabled: Option<bool> =
        crate::db::query_scalar("SELECT enabled FROM ha_policies WHERE vm_id = ?")
            .bind(vm_id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
    let snapshot = serde_json::json!({
        "vm_id": vm_id.to_string(),
        "observed_state": vm.as_ref().map(|v| v.0.clone()),
        "guest_tools_status": vm.and_then(|v| v.1),
        "ha_enabled": ha_enabled.unwrap_or(false),
    });
    let _ = crate::db::query("UPDATE ai_actions SET before_state = ? WHERE id = ?")
        .bind(&snapshot)
        .bind(action.id)
        .execute(&state.pool)
        .await;
}

/// Recent executed/failed actions, newest first.
pub async fn history(state: &AppState, limit: i64) -> anyhow::Result<Vec<ActionHistoryRow>> {
    let rows: Vec<(
        Uuid,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        serde_json::Value,
        serde_json::Value,
        serde_json::Value,
        Option<String>,
    )> = crate::db::query_as(
        "SELECT id, action_type, label, status, requested_by, approved_by, executed_at,
                object_ref, before_state, verify_result, undone_at
         FROM ai_actions
         WHERE status IN ('executed', 'failed')
         ORDER BY datetime(COALESCE(executed_at, created_at)) DESC
         LIMIT ?",
    )
    .bind(limit.clamp(1, 200))
    .fetch_all(&state.pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                action_type,
                label,
                status,
                requested_by,
                approved_by,
                executed_at,
                object_ref,
                before_state,
                verify,
                undone_at,
            )| {
                let undoable = status == "executed"
                    && undone_at.is_none()
                    && undo_kind(&action_type, &before_state).is_some();
                ActionHistoryRow {
                    id,
                    action_type,
                    label,
                    status,
                    requested_by,
                    approved_by,
                    executed_at,
                    vm_id: vm_id_of(&object_ref).map(|v| v.to_string()),
                    before_state,
                    verify,
                    undoable,
                    undone_at,
                }
            },
        )
        .collect())
}

/// What the world looks like now compared with what the action promised: (status, detail).
async fn check(state: &AppState, action: &ZyraActionRow) -> (&'static str, String) {
    if action.action_type == crate::api::stacks::DEPLOY_ACTION {
        return crate::api::stacks::deploy_check(state, &action.object_ref).await;
    }
    if action.action_type == crate::engine::rightsizing::RESIZE_ACTION {
        return crate::engine::rightsizing::check(state, &action.object_ref).await;
    }
    if action.action_type == crate::engine::consolidation::CONSOLIDATE_ACTION {
        return crate::engine::consolidation::check(state, &action.object_ref).await;
    }
    let Some(vm_id) = vm_id_of(&action.object_ref) else {
        return (
            "unknown",
            "No single machine to check for this action.".into(),
        );
    };
    match action.action_type.as_str() {
        "start_vm" => {
            let s: Option<String> =
                crate::db::query_scalar("SELECT observed_state FROM vms WHERE id = ?")
                    .bind(vm_id)
                    .fetch_optional(&state.pool)
                    .await
                    .ok()
                    .flatten();
            match s.as_deref() {
                Some("running") => ("ok", "The machine is running.".into()),
                Some(other) => (
                    "pending",
                    format!("The machine is {other}; it may still be starting."),
                ),
                None => ("unknown", "The machine was not found.".into()),
            }
        }
        "stop_vm" => {
            let s: Option<String> =
                crate::db::query_scalar("SELECT observed_state FROM vms WHERE id = ?")
                    .bind(vm_id)
                    .fetch_optional(&state.pool)
                    .await
                    .ok()
                    .flatten();
            match s.as_deref() {
                Some(st) if is_off(st) => ("ok", "The machine is stopped.".into()),
                Some(other) => (
                    "pending",
                    format!("The machine is {other}; it may still be shutting down."),
                ),
                None => ("unknown", "The machine was not found.".into()),
            }
        }
        "enable_ha" => {
            let on: Option<bool> =
                crate::db::query_scalar("SELECT enabled FROM ha_policies WHERE vm_id = ?")
                    .bind(vm_id)
                    .fetch_optional(&state.pool)
                    .await
                    .ok()
                    .flatten();
            if on == Some(true) {
                ("ok", "High availability is on.".into())
            } else {
                ("failed", "High availability is not enabled.".into())
            }
        }
        "create_backup" => {
            let n: i64 = crate::db::query_scalar(
                "SELECT COUNT(*) FROM backup_records
                 WHERE vm_id = ? AND status = 'completed' AND datetime(created_at) >= datetime('now', '-1 day')",
            )
            .bind(vm_id)
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);
            if n > 0 {
                ("ok", "A completed backup exists from the last day.".into())
            } else {
                (
                    "pending",
                    "No completed backup yet — it may still be running.".into(),
                )
            }
        }
        "install_guest_tools" => {
            let s: Option<String> = crate::db::query_scalar::<_, Option<String>>(
                "SELECT guest_tools_status FROM vms WHERE id = ?",
            )
            .bind(vm_id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten()
            .flatten();
            let s = s.unwrap_or_default().to_lowercase();
            if s.starts_with("install") || s.starts_with("run") || s.starts_with("ok") {
                ("ok", format!("Guest tools status: {s}."))
            } else {
                (
                    "pending",
                    format!(
                        "Guest tools status: {}.",
                        if s.is_empty() { "unknown" } else { &s }
                    ),
                )
            }
        }
        _ => (
            "unknown",
            "There is no automatic check for this action type.".into(),
        ),
    }
}

/// Re-check an executed action now and store the result.
pub async fn verify(state: &AppState, id: Uuid) -> anyhow::Result<serde_json::Value> {
    let action = get_action(&state.pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Action not found"))?;
    if action.status != "executed" {
        return Err(anyhow::anyhow!("Only executed actions can be verified"));
    }
    let (status, detail) = check(state, &action).await;
    let result = serde_json::json!({
        "status": status,
        "detail": detail,
        "checked_at": chrono::Utc::now().to_rfc3339(),
    });
    crate::db::query("UPDATE ai_actions SET verify_result = ? WHERE id = ?")
        .bind(&result)
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(result)
}

/// Undo an executed action when its reverse is safe and exact (see [`undo_kind`]).
pub async fn undo(
    state: &AppState,
    id: Uuid,
    actor: &crate::auth::AuthUser,
) -> anyhow::Result<serde_json::Value> {
    let action = get_action(&state.pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Action not found"))?;
    if action.status != "executed" {
        return Err(anyhow::anyhow!("Only executed actions can be undone"));
    }
    let (before, undone_at): (serde_json::Value, Option<String>) =
        crate::db::query_as("SELECT before_state, undone_at FROM ai_actions WHERE id = ?")
            .bind(id)
            .fetch_one(&state.pool)
            .await?;
    if undone_at.is_some() {
        return Err(anyhow::anyhow!("This action was already undone"));
    }
    let kind = undo_kind(&action.action_type, &before)
        .ok_or_else(|| anyhow::anyhow!("This action cannot be undone automatically"))?;
    let vm_id = || {
        vm_id_of(&action.object_ref).ok_or_else(|| anyhow::anyhow!("vm_id missing in object_ref"))
    };

    let message = match kind {
        "delete_stack" | "revert_stack" => crate::api::stacks::deploy_undo(state, actor, &before)
            .await
            .map_err(|e| anyhow::anyhow!(e.message))?,
        "resize_back" => crate::engine::rightsizing::undo(state, &before)
            .await
            .map_err(|e| anyhow::anyhow!(e.message))?,
        "disable_ha" => {
            let vm_id = vm_id()?;
            crate::engine::template::upsert_ha_policy(
                &state.pool,
                vm_id,
                false,
                3,
                "medium",
                false,
                false,
            )
            .await?;
            "High availability turned off again.".to_string()
        }
        "start_vm" => {
            let vm_id = vm_id()?;
            let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
                .bind(vm_id)
                .fetch_optional(&state.pool)
                .await?
                .flatten();
            if host_id.is_none() {
                return Err(anyhow::anyhow!("VM not found or has no assigned host"));
            }
            crate::tasks::enqueue::enqueue_task(
                state,
                "vm.start",
                serde_json::json!({ "vm_id": vm_id.to_string() }),
                Some("vm"),
                Some(vm_id),
                host_id,
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.message))?;
            "Start queued to put the machine back as it was.".to_string()
        }
        "shutdown_vm" => {
            let vm_id = vm_id()?;
            let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
                .bind(vm_id)
                .fetch_optional(&state.pool)
                .await?
                .flatten();
            if host_id.is_none() {
                return Err(anyhow::anyhow!("VM not found or has no assigned host"));
            }
            crate::tasks::enqueue::enqueue_task(
                state,
                "vm.power",
                serde_json::json!({ "vm_id": vm_id.to_string(), "action": "shutdown" }),
                Some("vm"),
                Some(vm_id),
                host_id,
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.message))?;
            "Shutdown queued to put the machine back as it was.".to_string()
        }
        other => return Err(anyhow::anyhow!("Unsupported undo '{other}'")),
    };

    let mut tx = state.pool.begin().await?;
    crate::db::query("UPDATE ai_actions SET undone_at = datetime('now') WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    crate::db::query(
        "INSERT INTO audit_logs (id, actor, action, resource_type, resource_id, detail) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4())
    .bind(&actor.username)
    .bind("zyra.action.undo")
    .bind("zyra")
    .bind(id)
    .bind(serde_json::json!({ "action_type": action.action_type, "undo": kind }))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    state.emit_event("audit", format!("{} zyra.action.undo", actor.username));
    Ok(serde_json::json!({ "message": message, "undo": kind }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn stop_vm_is_undoable_only_when_it_was_running_before() {
        let running = serde_json::json!({"observed_state": "running"});
        let off = serde_json::json!({"observed_state": "shutoff"});
        assert_eq!(undo_kind("stop_vm", &running), Some("start_vm"));
        assert_eq!(undo_kind("stop_vm", &off), None);
    }

    use super::*;

    #[test]
    fn enable_ha_is_undoable_only_when_it_was_off_before() {
        assert_eq!(
            undo_kind("enable_ha", &serde_json::json!({ "ha_enabled": false })),
            Some("disable_ha")
        );
        assert_eq!(
            undo_kind("enable_ha", &serde_json::json!({ "ha_enabled": true })),
            None
        );
    }

    #[test]
    fn start_vm_is_undoable_only_when_it_was_off_before() {
        assert_eq!(
            undo_kind(
                "start_vm",
                &serde_json::json!({ "observed_state": "shutoff" })
            ),
            Some("shutdown_vm")
        );
        assert_eq!(
            undo_kind(
                "start_vm",
                &serde_json::json!({ "observed_state": "running" })
            ),
            None
        );
    }

    #[test]
    fn stack_deploy_undo_deletes_new_stacks_and_reverts_updates() {
        let id = "00000000-0000-0000-0000-000000000001";
        assert_eq!(
            undo_kind(
                "stack.deploy",
                &serde_json::json!({ "stack_id": id, "existed": false })
            ),
            Some("delete_stack")
        );
        assert_eq!(
            undo_kind(
                "stack.deploy",
                &serde_json::json!({ "stack_id": id, "existed": true })
            ),
            Some("revert_stack")
        );
        assert_eq!(undo_kind("stack.deploy", &serde_json::json!({})), None);
    }

    #[test]
    fn resizes_undo_to_the_recorded_size_and_consolidation_does_not() {
        assert_eq!(
            undo_kind(
                "vm.resize",
                &serde_json::json!({ "vm_id": "x", "vcpus": 4, "memory_mib": 8192 })
            ),
            Some("resize_back")
        );
        assert_eq!(undo_kind("vm.resize", &serde_json::json!(null)), None);
        assert_eq!(undo_kind("drs.consolidate", &serde_json::json!({})), None);
    }

    #[test]
    fn irreversible_actions_have_no_undo() {
        assert_eq!(undo_kind("create_backup", &serde_json::json!({})), None);
        assert_eq!(undo_kind("adopt_vm", &serde_json::json!({})), None);
    }
}
