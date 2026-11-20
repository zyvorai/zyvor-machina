// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Time travel: restore points, rewind and forks on qcow2 backing chains
//! (`machina_core::libvirt::fork`). A restore point records the frozen layer
//! files of each disk; rewinding or forking stacks a new overlay on them.
//! While a fork depends on any of a VM's layers, that VM's points are never
//! merged, and rewinding past a point a fork depends on is refused.

use std::time::Duration;

use machina_agent::pb::{
    DetachForkRequest, DiskLayer, ForkVmRequest, RestorePointCreateRequest,
    RestorePointMergeRequest, RestorePointRewindRequest,
};
use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

use crate::agent_client;
use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;
use crate::tasks::TaskMessage;

pub const MIN_INTERVAL_MINUTES: i64 = 5;
pub const DEFAULT_KEEP: i64 = 24;
pub const MAX_KEEP: i64 = 168;
const TICK: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerRef {
    pub target: String,
    pub file: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestorePoint {
    pub id: Uuid,
    pub label: String,
    pub kind: String,
    pub note: Option<String>,
    pub quiesced: bool,
    pub created_at: String,
    pub layers: Vec<LayerRef>,
    /// Forks whose disks sit on this point's layers.
    pub forks: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForkView {
    pub vm_id: Uuid,
    pub name: Option<String>,
    pub restore_point_id: Option<Uuid>,
    pub memory: bool,
    pub isolated: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForkOf {
    pub vm_id: Uuid,
    pub name: Option<String>,
    pub restore_point_id: Option<Uuid>,
    pub memory: bool,
    pub isolated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimeTravelView {
    pub vm_id: Uuid,
    /// None or 0 = no scheduled restore points.
    pub every_minutes: Option<i64>,
    pub keep: i64,
    /// Oldest first.
    pub points: Vec<RestorePoint>,
    pub forks: Vec<ForkView>,
    pub fork_of: Option<ForkOf>,
}

fn to_pb(v: &[LayerRef]) -> Vec<DiskLayer> {
    v.iter()
        .map(|l| DiskLayer {
            target: l.target.clone(),
            file: l.file.clone(),
        })
        .collect()
}

fn from_pb(v: Vec<DiskLayer>) -> Vec<LayerRef> {
    v.into_iter()
        .map(|l| LayerRef {
            target: l.target,
            file: l.file,
        })
        .collect()
}

pub fn label(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S"),
        &Uuid::new_v4().simple().to_string()[..4]
    )
}

/// Which points to merge away so at most `keep` remain: pairs of (dropped,
/// absorbing) ids, oldest first. Nothing is merged while forks depend on the
/// VM's layers.
pub fn prune_plan(oldest_first: &[Uuid], keep: usize, pinned: bool) -> Vec<(Uuid, Uuid)> {
    if pinned || oldest_first.len() <= keep.max(1) {
        return Vec::new();
    }
    let excess = oldest_first.len() - keep.max(1);
    (0..excess)
        .map(|i| (oldest_first[i], oldest_first[i + 1]))
        .collect()
}

async fn vm_host(pool: &DbPool, vm_id: Uuid) -> anyhow::Result<(String, Uuid)> {
    let (name, host): (String, Option<Uuid>) =
        crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("vm {vm_id} not found"))?;
    Ok((
        name,
        host.ok_or_else(|| anyhow::anyhow!("vm {vm_id} has no host"))?,
    ))
}

async fn client_for(state: &AppState, host: Uuid) -> anyhow::Result<agent_client::AgentClient> {
    let addr = crate::tasks::worker::host_agent_addr(&state.pool, host).await?;
    agent_client::connect(&addr).await
}

async fn points(pool: &DbPool, vm_id: Uuid) -> anyhow::Result<Vec<(Uuid, Vec<LayerRef>)>> {
    let rows: Vec<(Uuid, String)> = crate::db::query_as(
        "SELECT id, layers FROM vm_restore_points WHERE vm_id = ? ORDER BY created_at, rowid",
    )
    .bind(vm_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, l)| (id, serde_json::from_str(&l).unwrap_or_default()))
        .collect())
}

async fn has_forks(pool: &DbPool, vm_id: Uuid) -> anyhow::Result<bool> {
    let n: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM vm_forks WHERE source_vm_id = ?")
        .bind(vm_id)
        .fetch_one(pool)
        .await?;
    Ok(n > 0)
}

async fn insert_point(
    pool: &DbPool,
    vm_id: Uuid,
    label: &str,
    kind: &str,
    note: Option<&str>,
    layers: &[LayerRef],
    quiesced: bool,
) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO vm_restore_points (id, vm_id, label, kind, note, layers, quiesced)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(vm_id)
    .bind(label)
    .bind(kind)
    .bind(note)
    .bind(serde_json::to_string(layers)?)
    .bind(quiesced)
    .execute(pool)
    .await?;
    Ok(id)
}

/// Merge away the oldest points beyond the VM's `keep`. Errors stop pruning
/// (the chain is only longer, never wrong) and are logged.
async fn prune(state: &AppState, vm_id: Uuid, name: &str, host: Uuid) -> anyhow::Result<usize> {
    let keep: Option<i64> = crate::db::query_scalar("SELECT restore_point_keep FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_one(&state.pool)
        .await?;
    let keep = keep.unwrap_or(DEFAULT_KEEP).clamp(1, MAX_KEEP) as usize;
    let all = points(&state.pool, vm_id).await?;
    let ids: Vec<Uuid> = all.iter().map(|(id, _)| *id).collect();
    let plan = prune_plan(&ids, keep, has_forks(&state.pool, vm_id).await?);
    if plan.is_empty() {
        return Ok(0);
    }
    let mut client = client_for(state, host).await?;
    let mut merged = 0;
    for (drop_id, next_id) in plan {
        let cur = points(&state.pool, vm_id).await?;
        let base = cur
            .iter()
            .find(|(id, _)| *id == drop_id)
            .map(|p| p.1.clone());
        let top = cur
            .iter()
            .find(|(id, _)| *id == next_id)
            .map(|p| p.1.clone());
        let (Some(base), Some(top)) = (base, top) else {
            break;
        };
        let mut tops = Vec::new();
        let mut bases = Vec::new();
        for t in &top {
            if let Some(b) = base.iter().find(|b| b.target == t.target) {
                if b.file != t.file {
                    tops.push(t.clone());
                    bases.push(b.clone());
                }
            }
        }
        if !tops.is_empty() {
            if let Err(e) = client
                .restore_point_merge(RestorePointMergeRequest {
                    vm_name: name.to_string(),
                    top: to_pb(&tops),
                    base: to_pb(&bases),
                })
                .await
            {
                tracing::warn!(vm = %name, "restore point merge failed: {}", e.message());
                break;
            }
        }
        let mut tx = state.pool.begin().await?;
        crate::db::query("UPDATE vm_restore_points SET layers = ? WHERE id = ?")
            .bind(serde_json::to_string(&base)?)
            .bind(next_id)
            .execute(&mut *tx)
            .await?;
        crate::db::query("DELETE FROM vm_restore_points WHERE id = ?")
            .bind(drop_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        merged += 1;
    }
    Ok(merged)
}

pub async fn task_restore_point(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id = payload_vm(msg)?;
    let kind = msg.payload["kind"].as_str().unwrap_or("manual").to_string();
    let note = msg.payload["note"].as_str().map(str::to_string);
    let (name, host) = vm_host(&state.pool, vm_id).await?;
    let label = label("rp");
    let mut client = client_for(state, host).await?;
    let resp = client
        .restore_point_create(RestorePointCreateRequest {
            vm_name: name.clone(),
            label: label.clone(),
        })
        .await
        .map_err(|e| anyhow::anyhow!("restore point: {}", e.message()))?
        .into_inner();
    let layers = from_pb(resp.layers);
    insert_point(
        &state.pool,
        vm_id,
        &label,
        &kind,
        note.as_deref(),
        &layers,
        resp.quiesced,
    )
    .await?;
    let merged = prune(state, vm_id, &name, host).await.unwrap_or_else(|e| {
        tracing::warn!(vm = %name, "restore point prune: {e:#}");
        0
    });
    state.emit_event(
        "vm.restore_point",
        format!(
            "Restore point {label} on {name}{}",
            if merged > 0 {
                format!(" ({merged} merged away)")
            } else {
                String::new()
            }
        ),
    );
    crate::tasks::worker::update_task_progress(&state.pool, msg.task_id, 100, &label).await?;
    Ok(())
}

/// Forks pinned to points created after `point`, which a rewind would break.
pub async fn forks_after(
    pool: &DbPool,
    vm_id: Uuid,
    point: Uuid,
) -> anyhow::Result<Vec<String>> {
    Ok(crate::db::query_scalar(
        "SELECT COALESCE(v.name, CAST(f.fork_vm_id AS TEXT)) FROM vm_forks f
         JOIN vm_restore_points p ON p.id = f.restore_point_id
         LEFT JOIN vms v ON v.id = f.fork_vm_id
         WHERE p.vm_id = ?1 AND p.created_at > (SELECT created_at FROM vm_restore_points WHERE id = ?2)",
    )
    .bind(vm_id)
    .bind(point)
    .fetch_all(pool)
    .await?)
}

pub async fn task_rewind(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id = payload_vm(msg)?;
    let point: Uuid = msg.payload["restore_point_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("restore_point_id missing"))?;
    let (name, host) = vm_host(&state.pool, vm_id).await?;
    let all = points(&state.pool, vm_id).await?;
    let idx = all
        .iter()
        .position(|(id, _)| *id == point)
        .ok_or_else(|| anyhow::anyhow!("restore point {point} not found on {name}"))?;
    let blocking = forks_after(&state.pool, vm_id, point).await?;
    if !blocking.is_empty() {
        anyhow::bail!(
            "forks {} depend on later restore points; detach or delete them first",
            blocking.join(", ")
        );
    }
    let newer: Vec<Uuid> = all[idx + 1..].iter().map(|(id, _)| *id).collect();
    let discard: Vec<String> = all[idx + 1..]
        .iter()
        .flat_map(|(_, l)| l.iter().map(|x| x.file.clone()))
        .collect();
    let label = label("rw");
    let mut client = client_for(state, host).await?;
    let resp = client
        .restore_point_rewind(RestorePointRewindRequest {
            vm_name: name.clone(),
            layers: to_pb(&all[idx].1),
            label: label.clone(),
            discard,
        })
        .await
        .map_err(|e| anyhow::anyhow!("rewind: {}", e.message()))?
        .into_inner();
    let mut tx = state.pool.begin().await?;
    for id in &newer {
        crate::db::query("DELETE FROM vm_restore_points WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    crate::db::query(
        "UPDATE vms SET desired_state = 'stopped', slept_at = NULL
         WHERE id = ? AND desired_state = 'sleeping'",
    )
    .bind(vm_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    state.emit_event(
        "vm.rewind",
        format!(
            "Rewound {name} to {}{}",
            all[idx].0,
            if resp.restarted {
                " and restarted it"
            } else {
                ""
            }
        ),
    );
    crate::tasks::worker::update_task_progress(&state.pool, msg.task_id, 100, "rewound").await?;
    Ok(())
}

pub async fn task_fork(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id = payload_vm(msg)?;
    let new_name = msg.payload["new_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("new_name missing"))?
        .to_string();
    let point: Option<Uuid> = msg.payload["restore_point_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok());
    let memory = msg.payload["memory"].as_bool().unwrap_or(false);
    let isolate = memory || msg.payload["isolate"].as_bool().unwrap_or(false);
    let reseed = msg.payload["reseed"].as_bool().unwrap_or(true);
    let start = memory || msg.payload["start"].as_bool().unwrap_or(true);

    #[allow(clippy::type_complexity)]
    let src: (
        String,
        Option<Uuid>,
        Option<Uuid>,
        serde_json::Value,
        i32,
        i64,
        Option<String>,
        String,
    ) = crate::db::query_as(
        "SELECT name, host_id, cluster_id, spec_json, vcpus, memory_mib, project, COALESCE(labels, '{}')
         FROM vms WHERE id = ?",
    )
    .bind(vm_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("vm {vm_id} not found"))?;
    let host = src
        .1
        .ok_or_else(|| anyhow::anyhow!("vm {vm_id} has no host"))?;
    let layers = match point {
        Some(p) => {
            let l: String = crate::db::query_scalar(
                "SELECT layers FROM vm_restore_points WHERE id = ? AND vm_id = ?",
            )
            .bind(p)
            .bind(vm_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("restore point {p} not found"))?;
            serde_json::from_str::<Vec<LayerRef>>(&l)?
        }
        None => Vec::new(),
    };

    let new_id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO vms (id, cluster_id, host_id, name, project, labels, spec_json, desired_state, observed_state, vcpus, memory_mib)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'creating', ?, ?)",
    )
    .bind(new_id)
    .bind(src.2)
    .bind(host)
    .bind(&new_name)
    .bind(&src.6)
    .bind(&src.7)
    .bind(&src.3)
    .bind(if start { "running" } else { "stopped" })
    .bind(src.4)
    .bind(src.5)
    .execute(&state.pool)
    .await?;

    let label = label("fk");
    let resp = match async {
        let mut client = client_for(state, host).await?;
        client
            .fork_vm(ForkVmRequest {
                source_name: src.0.clone(),
                new_name: new_name.clone(),
                layers: to_pb(&layers),
                memory,
                isolate,
                reseed,
                start,
                label: label.clone(),
            })
            .await
            .map(|r| r.into_inner())
            .map_err(|e| anyhow::anyhow!("fork: {}", e.message()))
    }
    .await
    {
        Ok(r) => r,
        Err(e) => {
            let _ = crate::db::query("DELETE FROM vms WHERE id = ? AND observed_state = 'creating'")
                .bind(new_id)
                .execute(&state.pool)
                .await;
            return Err(e);
        }
    };

    let point = match point {
        Some(p) => p,
        None => {
            insert_point(
                &state.pool,
                vm_id,
                &label,
                "fork",
                Some(&format!("fork {new_name}")),
                &from_pb(resp.frozen.clone()),
                resp.quiesced,
            )
            .await?
        }
    };
    let mut tx = state.pool.begin().await?;
    crate::db::query("UPDATE vms SET uuid = ?, observed_state = ?, desired_state = ? WHERE id = ?")
        .bind(&resp.uuid)
        .bind(if resp.running { "running" } else { "shutoff" })
        .bind(if resp.running { "running" } else { "stopped" })
        .bind(new_id)
        .execute(&mut *tx)
        .await?;
    crate::db::query(
        "INSERT INTO vm_forks (fork_vm_id, source_vm_id, restore_point_id, memory, isolated)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(new_id)
    .bind(vm_id)
    .bind(point)
    .bind(memory)
    .bind(isolate)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    state.emit_event(
        "vm.fork",
        format!(
            "Forked {} -> {new_name}{}",
            src.0,
            if memory {
                " (with memory, isolated)"
            } else {
                ""
            }
        ),
    );
    crate::tasks::worker::update_task_progress(&state.pool, msg.task_id, 100, "forked").await?;
    Ok(())
}

pub async fn task_detach(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id = payload_vm(msg)?;
    let (name, host) = vm_host(&state.pool, vm_id).await?;
    let mut client = client_for(state, host).await?;
    client
        .detach_fork(DetachForkRequest {
            vm_name: name.clone(),
        })
        .await
        .map_err(|e| anyhow::anyhow!("detach: {}", e.message()))?;
    crate::db::query("DELETE FROM vm_forks WHERE fork_vm_id = ?")
        .bind(vm_id)
        .execute(&state.pool)
        .await?;
    state.emit_event(
        "vm.fork.detach",
        format!("{name} no longer depends on its source"),
    );
    crate::tasks::worker::update_task_progress(&state.pool, msg.task_id, 100, "detached").await?;
    Ok(())
}

fn payload_vm(msg: &TaskMessage) -> anyhow::Result<Uuid> {
    msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))
}

pub async fn view(pool: &DbPool, vm_id: Uuid) -> anyhow::Result<Option<TimeTravelView>> {
    let row: Option<(Option<i64>, Option<i64>)> =
        crate::db::query_as("SELECT restore_point_minutes, restore_point_keep FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_optional(pool)
            .await?;
    let Some((every, keep)) = row else {
        return Ok(None);
    };
    #[allow(clippy::type_complexity)]
    let rows: Vec<(Uuid, String, String, Option<String>, bool, String, String)> = crate::db::query_as(
        "SELECT id, label, kind, note, quiesced, created_at, layers FROM vm_restore_points
         WHERE vm_id = ? ORDER BY created_at, rowid",
    )
    .bind(vm_id)
    .fetch_all(pool)
    .await?;
    let forks: Vec<(Uuid, Option<String>, Option<Uuid>, bool, bool, String)> = crate::db::query_as(
        "SELECT f.fork_vm_id, v.name, f.restore_point_id, f.memory, f.isolated, f.created_at
         FROM vm_forks f LEFT JOIN vms v ON v.id = f.fork_vm_id
         WHERE f.source_vm_id = ? ORDER BY f.created_at",
    )
    .bind(vm_id)
    .fetch_all(pool)
    .await?;
    let fork_of: Option<(Uuid, Option<String>, Option<Uuid>, bool, bool)> = crate::db::query_as(
        "SELECT f.source_vm_id, v.name, f.restore_point_id, f.memory, f.isolated
         FROM vm_forks f LEFT JOIN vms v ON v.id = f.source_vm_id WHERE f.fork_vm_id = ?",
    )
    .bind(vm_id)
    .fetch_optional(pool)
    .await?;
    Ok(Some(TimeTravelView {
        vm_id,
        every_minutes: every,
        keep: keep.unwrap_or(DEFAULT_KEEP),
        points: rows
            .into_iter()
            .map(
                |(id, label, kind, note, quiesced, created_at, layers)| RestorePoint {
                    id,
                    label,
                    kind,
                    note,
                    quiesced,
                    created_at,
                    layers: serde_json::from_str(&layers).unwrap_or_default(),
                    forks: forks
                        .iter()
                        .filter(|f| f.2 == Some(id))
                        .map(|f| f.1.clone().unwrap_or_else(|| f.0.to_string()))
                        .collect(),
                },
            )
            .collect(),
        forks: forks
            .into_iter()
            .map(
                |(vm_id, name, restore_point_id, memory, isolated, created_at)| ForkView {
                    vm_id,
                    name,
                    restore_point_id,
                    memory,
                    isolated,
                    created_at,
                },
            )
            .collect(),
        fork_of: fork_of.map(|(vm_id, name, restore_point_id, memory, isolated)| ForkOf {
            vm_id,
            name,
            restore_point_id,
            memory,
            isolated,
        }),
    }))
}

/// VMs whose newest restore point is older than their interval.
pub async fn due(pool: &DbPool) -> anyhow::Result<Vec<(Uuid, Option<Uuid>)>> {
    Ok(crate::db::query_as(
        "SELECT v.id, v.host_id FROM vms v
         WHERE COALESCE(v.restore_point_minutes, 0) > 0
           AND v.desired_state = 'running' AND v.observed_state = 'running'
           AND NOT EXISTS (
             SELECT 1 FROM vm_restore_points p WHERE p.vm_id = v.id
               AND p.created_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-' || v.restore_point_minutes || ' minutes'))
           AND NOT EXISTS (
             SELECT 1 FROM tasks t WHERE t.resource_id = v.id
               AND t.operation IN ('vm.restore_point', 'vm.rewind', 'vm.fork')
               AND t.status IN ('pending', 'running'))",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn tick(state: &AppState) -> anyhow::Result<usize> {
    let due = due(&state.pool).await?;
    let n = due.len();
    for (vm_id, host) in due {
        enqueue_task(
            state,
            "vm.restore_point",
            serde_json::json!({ "vm_id": vm_id.to_string(), "kind": "scheduled" }),
            Some("vm"),
            Some(vm_id),
            host,
        )
        .await
        .map_err(|e| anyhow::anyhow!("enqueue restore point: {e:?}"))?;
    }
    Ok(n)
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(TICK).await;
            if !state.leader.is_leader() {
                continue;
            }
            if let Err(e) = tick(&state).await {
                tracing::warn!("restore point scheduler: {e:#}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::test_support::{seed_host, test_state};

    fn ids(n: u128) -> Vec<Uuid> {
        (1..=n).map(Uuid::from_u128).collect()
    }

    #[test]
    fn prune_keeps_the_newest() {
        let p = ids(5);
        assert_eq!(prune_plan(&p, 3, false), vec![(p[0], p[1]), (p[1], p[2])]);
        assert!(prune_plan(&p, 5, false).is_empty());
        assert!(prune_plan(&p, 9, false).is_empty());
        assert_eq!(prune_plan(&p, 0, false).len(), 4);
    }

    #[test]
    fn forks_suspend_pruning() {
        assert!(prune_plan(&ids(30), 3, true).is_empty());
    }

    #[test]
    fn labels_are_valid_overlay_labels() {
        let l = label("rp");
        assert!(l.starts_with("rp-"));
        assert!(l.len() <= 48);
        assert!(l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }

    async fn seed_vm(pool: &DbPool, host: Uuid, id: u128, every: Option<i64>) -> Uuid {
        let vm = Uuid::from_u128(id);
        crate::db::query(
            "INSERT INTO vms (id, host_id, name, desired_state, observed_state, restore_point_minutes)
             VALUES (?, ?, ?, 'running', 'running', ?)",
        )
        .bind(vm)
        .bind(host)
        .bind(format!("vm{id}"))
        .bind(every)
        .execute(pool)
        .await
        .unwrap();
        vm
    }

    #[tokio::test]
    async fn scheduler_enqueues_only_due_vms() {
        let (state, mut rx) = test_state().await;
        let host = seed_host(&state.pool, Uuid::from_u128(1)).await;
        let due_vm = seed_vm(&state.pool, host, 2, Some(15)).await;
        let fresh = seed_vm(&state.pool, host, 3, Some(15)).await;
        let off = seed_vm(&state.pool, host, 4, None).await;
        insert_point(&state.pool, fresh, "rp-x", "scheduled", None, &[], false)
            .await
            .unwrap();
        crate::db::query(
            "INSERT INTO vm_restore_points (id, vm_id, label, kind, layers, created_at)
             VALUES (?, ?, 'rp-old', 'scheduled', '[]', strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-20 minutes'))",
        )
        .bind(Uuid::new_v4())
        .bind(due_vm)
        .execute(&state.pool)
        .await
        .unwrap();
        assert_eq!(tick(&state).await.unwrap(), 1);
        let msg = rx.try_recv().unwrap();
        assert_eq!(msg.operation, "vm.restore_point");
        assert_eq!(msg.payload["vm_id"], due_vm.to_string());
        assert_eq!(
            tick(&state).await.unwrap(),
            0,
            "pending task suppresses a second one"
        );
        let _ = off;
    }

    #[tokio::test]
    async fn rewind_is_blocked_by_forks_on_later_points() {
        let (state, _rx) = test_state().await;
        let host = seed_host(&state.pool, Uuid::from_u128(1)).await;
        let vm = seed_vm(&state.pool, host, 2, None).await;
        let fork = seed_vm(&state.pool, host, 3, None).await;
        let mut pts = Vec::new();
        for (i, ago) in [30, 20, 10].iter().enumerate() {
            let id = Uuid::from_u128(100 + i as u128);
            crate::db::query(
                "INSERT INTO vm_restore_points (id, vm_id, label, kind, layers, created_at)
                 VALUES (?, ?, ?, 'manual', '[]', strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?))",
            )
            .bind(id)
            .bind(vm)
            .bind(format!("rp-{i}"))
            .bind(format!("-{ago} minutes"))
            .execute(&state.pool)
            .await
            .unwrap();
            pts.push(id);
        }
        crate::db::query(
            "INSERT INTO vm_forks (fork_vm_id, source_vm_id, restore_point_id) VALUES (?, ?, ?)",
        )
        .bind(fork)
        .bind(vm)
        .bind(pts[1])
        .execute(&state.pool)
        .await
        .unwrap();
        assert_eq!(
            forks_after(&state.pool, vm, pts[0]).await.unwrap(),
            vec!["vm3"]
        );
        assert!(forks_after(&state.pool, vm, pts[1])
            .await
            .unwrap()
            .is_empty());
        let v = view(&state.pool, vm).await.unwrap().unwrap();
        assert_eq!(v.points.len(), 3);
        assert_eq!(v.points[1].forks, vec!["vm3"]);
        assert_eq!(v.forks.len(), 1);
        let f = view(&state.pool, fork).await.unwrap().unwrap();
        assert_eq!(f.fork_of.unwrap().vm_id, vm);
    }
}
