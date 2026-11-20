// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct ZyraActionRow {
    pub id: Uuid,
    pub source: String,
    pub action_type: String,
    pub label: String,
    pub review: String,
    pub risk: String,
    pub object_ref: serde_json::Value,
    pub status: String,
    pub requested_by: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateActionBody {
    pub action_type: String,
    pub label: String,
    #[serde(default)]
    pub review: String,
    #[serde(default)]
    pub risk: String,
    #[serde(default)]
    pub object_ref: serde_json::Value,
    #[serde(default)]
    pub source: String,
}

pub async fn list_pending(pool: &DbPool) -> anyhow::Result<Vec<ZyraActionRow>> {
    list_by_status(pool, "pending").await
}

pub async fn list_by_status(pool: &DbPool, status: &str) -> anyhow::Result<Vec<ZyraActionRow>> {
    let rows: Vec<(
        Uuid,
        String,
        String,
        String,
        String,
        String,
        serde_json::Value,
        String,
        String,
        DateTime<Utc>,
    )> = crate::db::query_as(
        "SELECT id, source, action_type, label, review, risk, object_ref, status, requested_by,
                    strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
             FROM ai_actions WHERE status = ? ORDER BY created_at DESC LIMIT 100",
    )
    .bind(status)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(map_row).collect())
}

fn map_row(
    (id, source, action_type, label, review, risk, object_ref, status, requested_by, created_at): (
        Uuid,
        String,
        String,
        String,
        String,
        String,
        serde_json::Value,
        String,
        String,
        DateTime<Utc>,
    ),
) -> ZyraActionRow {
    ZyraActionRow {
        id,
        source,
        action_type,
        label,
        review,
        risk,
        object_ref,
        status,
        requested_by,
        created_at,
    }
}

pub async fn create_action(
    pool: &DbPool,
    body: &CreateActionBody,
    requested_by: &str,
) -> anyhow::Result<ZyraActionRow> {
    let id: Uuid = crate::db::query_scalar(
        "INSERT INTO ai_actions (id, source, action_type, label, review, risk, object_ref, requested_by) VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(if body.source.is_empty() { "zyra" } else { &body.source })
    .bind(&body.action_type)
    .bind(&body.label)
    .bind(&body.review)
    .bind(if body.risk.is_empty() {
        "Review required"
    } else {
        &body.risk
    })
    .bind(&body.object_ref)
    .bind(requested_by)
    .fetch_one(pool)
    .await?;
    get_action(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("action missing"))
}

pub async fn get_action(pool: &DbPool, id: Uuid) -> anyhow::Result<Option<ZyraActionRow>> {
    let row: Option<(
        Uuid,
        String,
        String,
        String,
        String,
        String,
        serde_json::Value,
        String,
        String,
        DateTime<Utc>,
    )> = crate::db::query_as(
        "SELECT id, source, action_type, label, review, risk, object_ref, status, requested_by,
                    strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
             FROM ai_actions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(map_row))
}

pub async fn approve_and_execute(
    state: &crate::state::AppState,
    id: Uuid,
    actor: &crate::auth::AuthUser,
) -> anyhow::Result<serde_json::Value> {
    let action = get_action(&state.pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Action not found"))?;
    if action.status != "pending" {
        return Err(anyhow::anyhow!("Action already {}", action.status));
    }
    let two_person = [
        (
            crate::api::vm_network_policies::JIT_ACTION,
            "temporary access",
        ),
        (
            crate::api::vm_network_policies::APPLY_ACTION,
            "a drafted network policy",
        ),
        (
            crate::api::vm_network_policies::PROJECT_ACTION,
            "a project network change",
        ),
    ];
    if let Some((_, what)) = two_person.iter().find(|(t, _)| *t == action.action_type) {
        crate::auth::require_admin(actor).map_err(|e| anyhow::anyhow!(e.message))?;
        if actor.username == action.requested_by {
            return Err(anyhow::anyhow!(
                "{what} needs a second person: you requested it"
            ));
        }
    }
    let updated = crate::db::query(
        "UPDATE ai_actions SET status = 'approved', approved_by = ? WHERE id = ? AND status = 'pending'",
    )
    .bind(&actor.username)
    .bind(id)
    .execute(&state.pool)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(anyhow::anyhow!(
            "Action already approved or executed by another request"
        ));
    }

    // Remember what the machine looked like so the action can be verified and, if reversible, undone.
    super::action_audit::record_before(state, &action).await;

    let result = match action.action_type.as_str() {
        "start_vm"
        | "stop_vm"
        | "create_backup"
        | "enable_ha"
        | "install_guest_tools"
        | "adopt_vm"
        | "bulk_backup"
        | "bulk_ha"
        | "sync_hosts"
        | "remove_stale_vm"
        =>
        {
            let body = super::autopilot::ExecuteBody {
                action_type: action.action_type.clone(),
                object_ref: action.object_ref.clone(),
            };
            super::autopilot::execute(state, actor, &body)
                .await
                .map(|r| serde_json::json!({"message": r.message, "task_ids": r.task_ids}))
                .map_err(|e| anyhow::anyhow!(e.message))
        }
        // "Describe it, get it": approving builds the planned machines through the same path as the
        // environment-intent API (admin only, placed by the scheduler, created by the task bus).
        "create_environment" => {
            let query = action
                .object_ref
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("query missing in object_ref"))?
                .to_string();
            let max_vms = action
                .object_ref
                .get("max_vms")
                .and_then(|v| v.as_i64())
                .unwrap_or(5) as i32;
            let out = super::environment_intent::execute_environment(
                state,
                actor,
                &super::environment_intent::EnvironmentExecuteBody {
                    query,
                    dry_run: false,
                    max_vms,
                },
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.message))?;
            Ok(serde_json::json!({
                "message": out.summary,
                "task_ids": out.vm_tasks.iter().filter_map(|t| t.task_id.clone()).collect::<Vec<_>>(),
            }))
        }
        "guest.sync_time" | "guest.fstrim" => {
            let vm_id = action
                .object_ref
                .get("vm_id")
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or_else(|| anyhow::anyhow!("vm_id missing in object_ref"))?;
            let op = if action.action_type == "guest.sync_time" {
                "sync_time"
            } else {
                "fstrim"
            };
            let out = crate::engine::host_os::vm_guest_agent_action(
                &state.pool,
                &state.config,
                vm_id,
                op,
            )
            .await?;
            Ok(serde_json::json!({ "message": action.label, "result": out }))
        }
        // Real executors for nl_ops-proposed resource-creation intents — unlike
        // create_vm/migrate_vm below, these call the actual native handler
        // (api::volumes / api::networking), so approving the action genuinely
        // creates the resource rather than only recording an approval.
        "create_volume" => {
            let name = action
                .object_ref
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("name missing in object_ref"))?
                .to_string();
            let size_gib = action
                .object_ref
                .get("size_gib")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| anyhow::anyhow!("size_gib missing in object_ref"))?;
            let volume_class = action
                .object_ref
                .get("volume_class")
                .and_then(|v| v.as_str())
                .unwrap_or("silver")
                .to_string();
            let row = crate::api::volumes::create_volume(
                axum::extract::State(state.clone()),
                axum::Extension(actor.clone()),
                axum::Json(crate::api::volumes::CreateVolumeBody {
                    name,
                    size_gib,
                    project_id: None,
                    volume_class,
                    delete_on_termination: false,
                }),
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.message))?;
            Ok(serde_json::json!({ "message": "Volume created", "volume": row.0 }))
        }
        "create_security_group_allow" => {
            let name = action
                .object_ref
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("name missing in object_ref"))?
                .to_string();
            let protocol = action
                .object_ref
                .get("protocol")
                .and_then(|v| v.as_str())
                .unwrap_or("tcp")
                .to_string();
            let port = action.object_ref.get("port").and_then(|v| v.as_i64()).map(|p| p as i32);
            let group = crate::api::networking::create_security_group(
                axum::extract::State(state.clone()),
                axum::Extension(actor.clone()),
                axum::Json(crate::api::networking::CreateSecurityGroupBody {
                    name,
                    description: String::new(),
                    project_id: None,
                }),
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.message))?;
            let rule = crate::api::networking::create_security_group_rule(
                axum::extract::State(state.clone()),
                axum::Extension(actor.clone()),
                axum::extract::Path(group.0.id),
                axum::Json(crate::api::networking::CreateSecurityGroupRuleBody {
                    direction: "ingress".into(),
                    protocol: Some(protocol),
                    port_min: port,
                    port_max: port,
                    remote_cidr: Some("0.0.0.0/0".into()),
                    remote_sg_id: None,
                    description: String::new(),
                }),
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.message))?;
            Ok(serde_json::json!({
                "message": "Security group created",
                "security_group": group.0,
                "rule": rule.0
            }))
        }
        "vm.quarantine" => {
            let vm = action
                .object_ref
                .get("vm")
                .and_then(|v| v.as_str())
                .filter(|v| !v.is_empty())
                .ok_or_else(|| anyhow::anyhow!("vm missing in object_ref"))?
                .to_string();
            let host = action
                .object_ref
                .get("host")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let body: machina_bpf::api::VmQuarantineBody =
                serde_json::from_value(action.object_ref.clone())
                    .map_err(|e| anyhow::anyhow!("quarantine object_ref: {e}"))?;
            crate::api::vm_network_policies::quarantine_vm(
                state,
                &vm,
                host,
                body,
                &actor.username,
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.message))
        }
        crate::api::vm_network_policies::JIT_ACTION => {
            match serde_json::from_value::<machina_bpf::netpol::jit::JitRequest>(
                action.object_ref.clone(),
            ) {
                Ok(req) => {
                    crate::api::vm_network_policies::jit_grant(state, &req, &actor.username)
                        .await
                        .map_err(|e| anyhow::anyhow!(e.message))
                }
                Err(e) => Err(anyhow::anyhow!("temporary access object_ref: {e}")),
            }
        }
        crate::api::vm_network_policies::APPLY_ACTION => {
            crate::api::vm_network_policies::apply_approved(
                state,
                &action.object_ref,
                &actor.username,
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.message))
        }
        crate::api::stacks::DEPLOY_ACTION => {
            crate::api::stacks::execute_deploy(state, actor, &action.object_ref)
                .await
                .map_err(|e| anyhow::anyhow!(e.message))
        }
        crate::engine::rightsizing::RESIZE_ACTION => {
            crate::engine::rightsizing::execute(state, &action.object_ref)
                .await
                .map_err(|e| anyhow::anyhow!(e.message))
        }
        crate::engine::consolidation::CONSOLIDATE_ACTION => {
            crate::engine::consolidation::execute(state, &action.object_ref)
                .await
                .map_err(|e| anyhow::anyhow!(e.message))
        }
        crate::api::vm_network_policies::PROJECT_ACTION => {
            crate::api::vm_network_policies::project_approved(
                state,
                &action.object_ref,
                &actor.username,
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.message))
        }
        "vm.shutdown_agent" => {
            let vm_id = action
                .object_ref
                .get("vm_id")
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
            let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
                .bind(vm_id)
                .fetch_optional(&state.pool)
                .await?;
            if host_id.is_none() {
                return Err(anyhow::anyhow!("VM not found or has no assigned host"));
            }
            let task_id = crate::tasks::enqueue::enqueue_task(
                state,
                "vm.power",
                serde_json::json!({
                    "vm_id": vm_id.to_string(),
                    "action": "shutdown",
                    "mode": "agent",
                }),
                Some("vm"),
                Some(vm_id),
                host_id,
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.message))?;
            Ok(serde_json::json!({
                "message": "Graceful shutdown queued",
                "task_id": task_id.to_string()
            }))
        }
        // These action_types are proposed by nl_ops / guest_tools and stored as
        // pending ai_actions, but no executor is wired up for them anywhere in
        // the codebase. Falling through to the generic "recorded approval"
        // branch below would mark them 'executed' and audit-log a fabricated
        // success even though nothing actually ran — reject explicitly instead
        // so the action is marked 'failed' and the operator knows to follow up
        // manually rather than trusting a false "done" status.
        //
        // "firewall_change" belongs here too: real firewall approvals go
        // through the dedicated `firewall_approvals` table/workflow
        // (engine/zeus_firewall), which this handler never touches — an
        // ai_actions row of this type had no executor at all, so the old
        // branch claimed "delegated to Zeus Firewall workflow" and still
        // marked it 'executed', reporting success for a change that never
        // happened.
        "create_vm" | "migrate_vm" | "vm.snapshot_quiesce" | "firewall_change" => {
            Err(anyhow::anyhow!(
                "No executor implemented for action type '{}' — this action cannot be auto-executed yet; perform it manually and reject/close this entry",
                action.action_type
            ))
        }
        _ => Ok(serde_json::json!({"message": format!("Recorded approval for {}", action.label)})),
    };

    match &result {
        Ok(msg) => {
            let mut tx = state.pool.begin().await?;
            crate::db::query(
                "UPDATE ai_actions SET status = 'executed', executed_at = datetime('now') WHERE id = ?",
            )
            .bind(id)
            .execute(&mut *tx)
            .await?;
            crate::db::query(
                "INSERT INTO audit_logs (id, actor, action, resource_type, resource_id, detail) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(Uuid::new_v4())
            .bind(&actor.username)
            .bind("zyra.action.execute")
            .bind("zyra")
            .bind(id)
            .bind(serde_json::json!({
                "action_type": action.action_type,
                "result": msg
            }))
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            state.emit_event("audit", format!("{} zyra.action.execute", actor.username));
        }
        Err(_) => {
            crate::db::query("UPDATE ai_actions SET status = 'failed' WHERE id = ?")
                .bind(id)
                .execute(&state.pool)
                .await?;
        }
    }
    result
}

pub async fn reject(pool: &DbPool, id: Uuid, actor: &str) -> anyhow::Result<bool> {
    let r = crate::db::query(
        "UPDATE ai_actions SET status = 'rejected', approved_by = ? WHERE id = ? AND status = 'pending'",
    )
    .bind(actor)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn approval_hub(pool: &DbPool) -> anyhow::Result<serde_json::Value> {
    let zyra = list_pending(pool).await?;
    let firewall_count: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM firewall_approvals WHERE status = 'pending'")
            .fetch_one(pool)
            .await
            .unwrap_or(0);
    let autopilot = super::autopilot::propose(pool, None).await?;
    // total_pending must only count items actually present in `zyra_actions`
    // (rendered by the approvals queue UI) plus firewall_pending (explicitly
    // broken out in the UI subtitle and reviewed on the dedicated firewall
    // approvals page). autopilot_proposals are ephemeral recommendations
    // surfaced through their own /api/v1/ai/autopilot/* endpoints and UI
    // surfaces (ZyraAssistant, PlatformControlCenter) — they are never
    // rendered by this hub's consumers, so including their count here made
    // the nav badge / page header report pending items that the approvals
    // list could never show (e.g. "1 total" with an empty list).
    Ok(serde_json::json!({
        "zyra_actions": zyra,
        "firewall_pending": firewall_count,
        "autopilot_proposals": autopilot.actions,
        "total_pending": zyra.len() as i64 + firewall_count
    }))
}
