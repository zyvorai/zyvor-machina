// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Durable cloud jobs and elastic groups. The existing worker claims tasks and
//! VM reconciler performs power changes. Scale-in stops and retains disks.
use crate::{agent_client, auth::AuthUser, state::AppState, tasks::TaskMessage};
use axum::{extract::State, Extension, Json};
use machina_spec::{LbBinding, NetworkAttachmentSpec, ScaleIn, ScalingPolicy, VirtualMachine};
use serde_json::Value;
use std::time::Duration;
use uuid::Uuid;

/// A reconcile pass may keep acting only while we still lead AND the leadership epoch has not moved since the pass began.
/// (A deposed leader that is slow to notice would otherwise keep creating VMs next to the new leader.)
pub(crate) fn tick_still_valid(is_leader: bool, started_epoch: i64, now_epoch: i64) -> bool {
    is_leader && now_epoch == started_epoch
}

/// `None` = unfenced (direct calls in tests); `Some(epoch)` = a loop pass that began at that leadership epoch.
pub(crate) fn fence_ok(state: &AppState, epoch: Option<i64>) -> bool {
    match epoch {
        None => true,
        Some(e) => tick_still_valid(state.leader.is_leader(), e, crate::leader::current_epoch()),
    }
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            if let Err(e) = reconcile_once(&state).await {
                tracing::warn!("cloud reconcile: {e:#}");
            }
        }
    });
}

pub async fn provision_subnet(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let id: Uuid = serde_json::from_value(msg.payload["subnet_id"].clone())?;
    let result=async {
        let row:(String,String)=crate::db::query_as("SELECT s.cidr,h.agent_grpc_addr FROM cloud_subnets s JOIN cloud_vpcs v ON v.id=s.vpc_id JOIN hosts h ON h.id=v.host_id WHERE s.id=? AND h.state='online'").bind(id).fetch_one(&state.pool).await?;
        let mut client=agent_client::connect(&row.1).await?;
        agent_client::provision_cloud_subnet(&mut client,id,&row.0).await?;
        crate::db::query("UPDATE cloud_subnets SET status='ready',last_error='' WHERE id=?").bind(id).execute(&state.pool).await?;
        Ok::<(),anyhow::Error>(())
    }.await;
    if let Err(ref e) = result {
        crate::db::query("UPDATE cloud_subnets SET status='error',last_error=? WHERE id=?")
            .bind(format!("{e:#}"))
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    result
}

pub async fn reconcile_once(state: &AppState) -> anyhow::Result<()> {
    let epoch = crate::leader::current_epoch();
    // Publish committed pending rows again after process/bus failure. Worker
    // claim_task's compare-and-swap makes duplicate deliveries harmless.
    let jobs:Vec<(Uuid,Value)>=crate::db::query_as("SELECT id,payload FROM tasks WHERE operation='cloud.subnet.provision' AND status='pending' ORDER BY created_at LIMIT 100").fetch_all(&state.pool).await?;
    for (task_id, payload) in jobs {
        state
            .task_bus
            .publish(
                "machina.tasks",
                &TaskMessage {
                    task_id,
                    operation: "cloud.subnet.provision".into(),
                    payload,
                },
            )
            .await?;
    }
    // Bootstrap reaps orphaned tasks through the generic worker. Propagate that
    // failure to the cloud resource so its retry endpoint is usable.
    crate::db::query("UPDATE cloud_subnets SET status='error',last_error='Provisioning task failed; inspect task history and retry' WHERE status='pending' AND EXISTS(SELECT 1 FROM tasks WHERE resource_id=cloud_subnets.id AND operation='cloud.subnet.provision' AND status='failed') AND NOT EXISTS(SELECT 1 FROM tasks WHERE resource_id=cloud_subnets.id AND operation='cloud.subnet.provision' AND status IN ('pending','running'))").execute(&state.pool).await?;
    let groups:Vec<Uuid>=crate::db::query_scalar("SELECT g.id FROM cloud_instance_groups g JOIN projects p ON p.id=g.project_id WHERE g.paused = FALSE AND p.enabled = TRUE ORDER BY g.id LIMIT 100").fetch_all(&state.pool).await?;
    for id in groups {
        if !fence_ok(state, Some(epoch)) {
            break;
        }
        let error = reconcile_group_fenced(state, id, Some(epoch))
            .await
            .err()
            .map(|e| format!("{e:#}"))
            .unwrap_or_default();
        // A pass that lost its fence must not write: the new leader owns this group now.
        if !fence_ok(state, Some(epoch)) {
            break;
        }
        // Only write when the message changed, so an unchanged failure is not a database write every tick.
        crate::db::query("UPDATE cloud_instance_groups SET last_error=? WHERE id=? AND last_error<>?1")
            .bind(error)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    Ok(())
}

/// Public for focused tests; normal entry is the leader-gated loop.
pub async fn reconcile_group(state: &AppState, id: Uuid) -> anyhow::Result<()> {
    reconcile_group_fenced(state, id, None).await
}

async fn reconcile_group_fenced(
    state: &AppState,
    id: Uuid,
    epoch: Option<i64>,
) -> anyhow::Result<()> {
    type Definition = (Uuid, String, String, Uuid, String, String, bool);
    let (_project_id,project,raw,host,network,template,paused):Definition=crate::db::query_as("SELECT g.project_id,p.name,g.policy_json,v.host_id,n.name,t.spec_json,g.paused FROM cloud_instance_groups g JOIN projects p ON p.id=g.project_id AND p.enabled = TRUE JOIN cloud_launch_templates t ON t.id=g.template_id AND t.project_id=g.project_id JOIN cloud_subnets s ON s.id=g.subnet_id AND s.status='ready' JOIN cloud_vpcs v ON v.id=s.vpc_id AND v.project_id=g.project_id JOIN networks n ON n.id=s.network_id WHERE g.id=?").bind(id).fetch_one(&state.pool).await?;
    if paused {
        return Ok(());
    }
    let mut policy: ScalingPolicy = serde_json::from_str(&raw)?;
    policy.validate().map_err(anyhow::Error::msg)?;
    // Autoscaling is only allowed on a fully converged group with a fresh
    // sample from EVERY active member. Averages over a partial set are unsafe.
    let (count,cpu):(i64,Option<f64>)=crate::db::query_as("SELECT COUNT(*),AVG(m.cpu_percent) FROM cloud_group_members gm JOIN vms v ON v.id=gm.vm_id JOIN vm_metrics m ON m.vm_id=v.id WHERE gm.group_id=? AND gm.slot<? AND v.observed_state='running' AND m.updated_at>datetime('now','-2 minutes')").bind(id).bind(i64::from(policy.desired)).fetch_one(&state.pool).await?;
    let elapsed:i64=crate::db::query_scalar("SELECT CAST(strftime('%s','now') AS INTEGER)-CAST(strftime('%s',last_scaled_at) AS INTEGER) FROM cloud_instance_groups WHERE id=?").bind(id).fetch_one(&state.pool).await?;
    let peak = if policy.predictive {
        crate::engine::ai::forecast::group_demand_peak(&state.pool, id).await
    } else {
        None
    };
    let desired = policy.next_desired_with_forecast(
        if count == i64::from(policy.desired) && count > 0 {
            cpu
        } else {
            None
        },
        elapsed,
        peak,
    );
    if desired != policy.desired {
        policy.desired = desired;
        let changed=crate::db::query("UPDATE cloud_instance_groups SET policy_json=?,last_scaled_at=CURRENT_TIMESTAMP WHERE id=? AND paused = FALSE AND policy_json=?").bind(serde_json::to_string(&policy)?).bind(id).bind(&raw).execute(&state.pool).await?.rows_affected();
        if changed == 0 {
            return Ok(());
        }
        let actor = AuthUser {
            username: "cloud-reconciler".into(),
            role: "admin".into(),
            auth_source: None,
        };
        let mut conn = state.pool.acquire().await?;
        crate::api::cloud::audit(&mut conn, &actor, "cloud.group.autoscale", id)
            .await
            .map_err(|e| anyhow::anyhow!("audit: {e:?}"))?;
    }
    let expected = serde_json::to_string(&policy)?;
    for slot in 0..policy.max {
        if !fence_ok(state, epoch) {
            return Ok(());
        }
        // A PATCH/pause that races this tick wins; do not continue with stale
        // desired counts or resurrect a paused group's stopped VMs.
        let current: Option<String> = crate::db::query_scalar(
            "SELECT policy_json FROM cloud_instance_groups WHERE id=? AND paused = FALSE",
        )
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
        if current.as_deref() != Some(expected.as_str()) {
            return Ok(());
        }
        let existing: Option<Member> = crate::db::query_as(
            "SELECT gm.vm_id, COALESCE(v.desired_state, ''), COALESCE(v.observed_state, ''),
                    COALESCE(v.guest_ip, ''),
                    CAST(strftime('%s', 'now') AS INTEGER) - CAST(strftime('%s', gm.draining_since) AS INTEGER)
             FROM cloud_group_members gm JOIN vms v ON v.id = gm.vm_id
             WHERE gm.group_id = ? AND gm.slot = ?",
        )
        .bind(id)
        .bind(i64::from(slot))
        .fetch_optional(&state.pool)
        .await?;
        let guard = Guard {
            group: id,
            project: &project,
            policy: &expected,
        };
        if let Some(m) = existing {
            if slot < policy.desired {
                // Ordinary VM reconciliation starts it (restoring a sleeping
                // one) and retries.
                guard.set_desired(state, m.0, "running").await?;
                set_draining(state, id, slot, false).await?;
                if let Some(lb) = policy.load_balancer {
                    if m.2 == "running" && !m.3.is_empty() {
                        lb_member(state, lb, m.0, true).await?;
                    }
                }
            } else {
                scale_in(state, &policy, &guard, slot, &m).await?;
            }
            continue;
        }
        if slot >= policy.desired {
            continue;
        }
        let mut vm: VirtualMachine = serde_json::from_str(&template)?;
        vm.metadata.name = format!("asg-{}-{slot}", id.simple());
        vm.metadata.project = Some(project.clone());
        vm.spec.network = vec![NetworkAttachmentSpec {
            network: network.clone(),
            ip_mode: "dhcp".into(),
            firewall_profile: None,
        }];
        // Deterministic name allows recovery if creation committed but recording
        // group membership was interrupted. Only adopt the exact stored spec.
        let prior: Option<(Uuid, Value)> =
            crate::db::query_as("SELECT id,spec_json FROM vms WHERE name=? AND project=?")
                .bind(&vm.metadata.name)
                .bind(&project)
                .fetch_optional(&state.pool)
                .await?;
        let vm_id = if let Some((vid, spec)) = prior {
            anyhow::ensure!(
                spec == serde_json::to_value(&vm)?,
                "instance slot has conflicting spec; refusing adoption"
            );
            vid
        } else {
            let actor = AuthUser {
                username: "cloud-reconciler".into(),
                role: "admin".into(),
                auth_source: None,
            };
            let body = crate::api::vms::CreateVmBody {
                flavor_id: None,
                vm: vm.clone(),
                host_id: Some(host),
                tags: vec![format!("cloud.group={id}")],
                desired_state: "running".into(),
                atlas_root_disk: false,
                atlas_policy: None,
                preemptible: false,
                preempt_priority: 0,
            };
            let _ = crate::api::vms::create_vm(State(state.clone()), Extension(actor), Json(body))
                .await
                .map_err(|e| anyhow::anyhow!("instance create: {e:?}"))?;
            crate::db::query_scalar("SELECT id FROM vms WHERE name=? AND project=?")
                .bind(&vm.metadata.name)
                .bind(&project)
                .fetch_one(&state.pool)
                .await?
        };
        crate::db::query("INSERT INTO cloud_group_members (group_id,slot,vm_id) VALUES (?,?,?) ON CONFLICT(group_id,slot) DO UPDATE SET vm_id=excluded.vm_id WHERE cloud_group_members.vm_id IS NULL").bind(id).bind(i64::from(slot)).bind(vm_id).execute(&state.pool).await?;
    }
    // If max was lowered, stopped retained slots above the new max remain owned.
    crate::db::query("UPDATE vms SET desired_state='stopped' WHERE id IN (SELECT vm_id FROM cloud_group_members WHERE group_id=? AND slot>=?) AND desired_state<>'sleeping' AND EXISTS(SELECT 1 FROM cloud_instance_groups WHERE id=? AND paused = FALSE AND policy_json=?)").bind(id).bind(i64::from(policy.max)).bind(id).bind(&expected).execute(&state.pool).await?;
    Ok(())
}

/// vm_id, desired_state, observed_state, guest_ip, seconds draining.
type Member = (Uuid, String, String, String, Option<i64>);

/// Writes only while the group still has the policy this pass read.
struct Guard<'a> {
    group: Uuid,
    project: &'a str,
    policy: &'a str,
}

impl Guard<'_> {
    async fn set_desired(&self, state: &AppState, vm: Uuid, target: &str) -> anyhow::Result<()> {
        crate::db::query("UPDATE vms SET desired_state=? WHERE id=? AND project=? AND EXISTS(SELECT 1 FROM cloud_instance_groups WHERE id=? AND paused = FALSE AND policy_json=?)")
            .bind(target)
            .bind(vm)
            .bind(self.project)
            .bind(self.group)
            .bind(self.policy)
            .execute(&state.pool)
            .await?;
        Ok(())
    }
}

async fn set_draining(state: &AppState, group: Uuid, slot: u32, on: bool) -> anyhow::Result<()> {
    let sql = if on {
        "UPDATE cloud_group_members SET draining_since = CURRENT_TIMESTAMP WHERE group_id = ? AND slot = ? AND draining_since IS NULL"
    } else {
        "UPDATE cloud_group_members SET draining_since = NULL WHERE group_id = ? AND slot = ? AND draining_since IS NOT NULL"
    };
    crate::db::query(sql)
        .bind(group)
        .bind(i64::from(slot))
        .execute(&state.pool)
        .await?;
    Ok(())
}

/// A member the group no longer needs: out of the load balancer first, then
/// after the drain period stopped, or managed-saved when the policy says sleep
/// and traffic could wake it (a known address).
async fn scale_in(
    state: &AppState,
    policy: &ScalingPolicy,
    guard: &Guard<'_>,
    slot: u32,
    m: &Member,
) -> anyhow::Result<()> {
    let (vm, desired, observed, ip, draining) = m;
    if let Some(lb) = policy.load_balancer {
        lb_member(state, lb, *vm, false).await?;
    }
    if desired != "running" {
        return set_draining(state, guard.group, slot, false).await;
    }
    let drain = i64::from(policy.drain());
    if drain > 0 && observed == "running" {
        match draining {
            None => return set_draining(state, guard.group, slot, true).await,
            Some(s) if *s < drain => return Ok(()),
            Some(_) => {}
        }
    }
    let sleep = policy.scale_in == ScaleIn::Sleep && observed == "running" && !ip.is_empty();
    if sleep {
        let inflight: i64 = crate::db::query_scalar(
            "SELECT COUNT(*) FROM tasks WHERE resource_id = ? AND operation = 'vm.power' AND status IN ('pending', 'running')",
        )
        .bind(vm)
        .fetch_one(&state.pool)
        .await?;
        if inflight == 0 {
            let host: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
                .bind(vm)
                .fetch_one(&state.pool)
                .await?;
            crate::tasks::enqueue::enqueue_task(
                state,
                "vm.power",
                serde_json::json!({ "vm_id": vm.to_string(), "action": "managedsave", "reason": "scale-in" }),
                Some("vm"),
                Some(*vm),
                host,
            )
            .await
            .map_err(|e| anyhow::anyhow!("scale-in sleep: {}", e.message))?;
        }
    } else {
        guard.set_desired(state, *vm, "stopped").await?;
    }
    set_draining(state, guard.group, slot, false).await
}

/// Enables or disables `vm` in the group's load balancer, adding it on first
/// join, and pushes the rule set only when something changed.
async fn lb_member(state: &AppState, lb: LbBinding, vm: Uuid, enabled: bool) -> anyhow::Result<()> {
    let row: Option<(Uuid, bool)> = crate::db::query_as(
        "SELECT id, enabled FROM lb_members WHERE load_balancer_id = ? AND vm_id = ? AND port = ?",
    )
    .bind(lb.id)
    .bind(vm)
    .bind(i64::from(lb.port))
    .fetch_optional(&state.pool)
    .await?;
    let changed = match row {
        Some((member, on)) if on != enabled => {
            crate::db::query("UPDATE lb_members SET enabled = ? WHERE id = ?")
                .bind(enabled)
                .bind(member)
                .execute(&state.pool)
                .await?;
            true
        }
        Some(_) => false,
        None if enabled => {
            crate::db::query(
                "INSERT INTO lb_members (id, load_balancer_id, vm_id, port, weight, enabled) VALUES (?, ?, ?, ?, 1, TRUE)",
            )
            .bind(Uuid::new_v4())
            .bind(lb.id)
            .bind(vm)
            .bind(i64::from(lb.port))
            .execute(&state.pool)
            .await?;
            true
        }
        None => false,
    };
    // A host that can't take the rule set marks the load balancer `error`;
    // that mustn't stop the group from scaling.
    if changed {
        if let Err(e) = crate::engine::load_balancer::apply(&state.pool, &state.config, lb.id).await
        {
            tracing::warn!(lb = %lb.id, "group load balancer push: {e:#}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod fence_tests {
    use super::tick_still_valid;

    #[test]
    fn a_pass_continues_only_while_leading_at_the_same_epoch() {
        assert!(tick_still_valid(true, 7, 7));
        // lost the lease
        assert!(!tick_still_valid(false, 7, 7));
        // someone else took over (epoch moved) even if our cached flag has not caught up
        assert!(!tick_still_valid(true, 7, 8));
        assert!(!tick_still_valid(false, 7, 8));
    }
}
