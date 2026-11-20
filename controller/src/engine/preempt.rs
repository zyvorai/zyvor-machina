// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Preemptible instances. When a host's free memory falls below its reserve,
//! or a regular VM can't be placed anywhere, preemptible VMs on that host are
//! managed-saved (lowest priority first) instead of being terminated. They
//! stay out of the wake set, so traffic doesn't undo the preemption, and are
//! restored, highest priority first, once the host has room again.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

const TICK: Duration = Duration::from_secs(20);
/// After a preemption or resume on a host, wait for fresh heartbeats before
/// acting on it again.
const SETTLE: Duration = Duration::from_secs(60);
/// A host whose last heartbeat is older than this is left alone.
const FRESH_SECS: f64 = 120.0;
/// Room beyond the reserve a host needs before a preempted VM comes back,
/// as a percent of its total, so it doesn't flap.
pub const RESUME_MARGIN_PCT: i64 = 5;
pub const MAX_RESERVE_PCT: i64 = 90;
pub const MAX_PRIORITY: i64 = 100;

static SETTLED: Mutex<Option<HashMap<Uuid, Instant>>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub enabled: bool,
    pub reserve_pct: i64,
}

#[derive(Debug, Clone)]
pub struct HostCap {
    pub id: Uuid,
    pub name: String,
    pub total_mib: i64,
    pub used_mib: i64,
    pub fresh: bool,
}

impl HostCap {
    fn free(&self) -> i64 {
        (self.total_mib - self.used_mib).max(0)
    }
    pub fn reserve_mib(&self, pct: i64) -> i64 {
        self.total_mib * pct / 100
    }
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: Uuid,
    pub name: String,
    pub host_id: Uuid,
    pub memory_mib: i64,
    pub priority: i64,
}

/// Lowest priority first; within a priority the largest first, so fewer VMs go.
fn victim_order(c: &mut [&Candidate]) {
    c.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then(b.memory_mib.cmp(&a.memory_mib))
            .then(a.name.cmp(&b.name))
    });
}

/// VMs to preempt so `host` is back at its reserve; empty when it already is
/// or nothing running there is preemptible.
pub fn pressure_victims(host: &HostCap, reserve_pct: i64, running: &[Candidate]) -> Vec<Uuid> {
    let floor = host.reserve_mib(reserve_pct);
    let mut free = host.free();
    if free >= floor {
        return Vec::new();
    }
    let mut mine: Vec<&Candidate> = running.iter().filter(|c| c.host_id == host.id).collect();
    victim_order(&mut mine);
    let mut out = Vec::new();
    for c in mine {
        if free >= floor {
            break;
        }
        free += c.memory_mib;
        out.push(c.id);
    }
    out
}

/// The host where preempting the fewest, lowest-priority VMs makes room for
/// `need_mib`, and those VMs.
pub fn room_plan(
    hosts: &[HostCap],
    running: &[Candidate],
    need_mib: i64,
) -> Option<(Uuid, Vec<Uuid>)> {
    let mut best: Option<((i64, usize, i64), Uuid, Vec<Uuid>)> = None;
    for h in hosts.iter().filter(|h| h.fresh) {
        let mut free = h.free();
        let mut mine: Vec<&Candidate> = running.iter().filter(|c| c.host_id == h.id).collect();
        victim_order(&mut mine);
        let mut picked = Vec::new();
        let (mut top, mut freed) = (i64::MIN, 0);
        for c in mine {
            if free >= need_mib {
                break;
            }
            free += c.memory_mib;
            freed += c.memory_mib;
            top = top.max(c.priority);
            picked.push(c.id);
        }
        if free < need_mib || picked.is_empty() {
            continue;
        }
        let cost = (top, picked.len(), freed);
        if best.as_ref().is_none_or(|(b, _, _)| cost < *b) {
            best = Some((cost, h.id, picked));
        }
    }
    best.map(|(_, h, v)| (h, v))
}

/// Preempted VMs to restore now: highest priority and longest waiting first,
/// each only while its host keeps the reserve plus a margin afterwards.
/// `preempted` must be ordered by how long each has waited, longest first.
pub fn resume_order(hosts: &[HostCap], reserve_pct: i64, preempted: &[Candidate]) -> Vec<Uuid> {
    let mut free: HashMap<Uuid, (i64, i64)> = hosts
        .iter()
        .filter(|h| h.fresh)
        .map(|h| {
            let floor = h.reserve_mib(reserve_pct + RESUME_MARGIN_PCT);
            (h.id, (h.free(), floor))
        })
        .collect();
    let mut order: Vec<(usize, &Candidate)> = preempted.iter().enumerate().collect();
    order.sort_by(|(ia, a), (ib, b)| b.priority.cmp(&a.priority).then(ia.cmp(ib)));
    let mut out = Vec::new();
    for (_, c) in order {
        if let Some((f, floor)) = free.get_mut(&c.host_id) {
            if *f - c.memory_mib >= *floor {
                *f -= c.memory_mib;
                out.push(c.id);
            }
        }
    }
    out
}

pub async fn settings(pool: &DbPool) -> Settings {
    crate::db::query_as::<_, (bool, i64)>(
        "SELECT enabled, reserve_pct FROM preempt_settings WHERE id = 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .map(|(enabled, reserve_pct)| Settings {
        enabled,
        reserve_pct,
    })
    .unwrap_or(Settings {
        enabled: true,
        reserve_pct: 10,
    })
}

pub async fn hosts(pool: &DbPool) -> anyhow::Result<Vec<HostCap>> {
    let rows: Vec<(Uuid, String, i64, i64, Option<f64>)> = crate::db::query_as(
        "SELECT id, hostname, memory_total_mib, memory_used_mib,
                (julianday('now') - julianday(last_heartbeat_at)) * 86400.0
         FROM hosts WHERE state = 'online' AND memory_total_mib > 0",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, total_mib, used_mib, age)| HostCap {
            id,
            name,
            total_mib,
            used_mib,
            fresh: age.is_some_and(|a| a < FRESH_SECS),
        })
        .collect())
}

const CANDIDATE: &str =
    "SELECT v.id, v.name, v.host_id, v.memory_mib, v.preempt_priority FROM vms v";

async fn busy(pool: &DbPool) -> std::collections::HashSet<Uuid> {
    crate::db::query_scalar::<_, Uuid>(
        "SELECT resource_id FROM tasks WHERE operation IN ('vm.power', 'vm.migrate', 'vm.delete')
         AND status IN ('pending', 'running') AND resource_id IS NOT NULL",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .collect()
}

async fn candidates(pool: &DbPool, filter: &str) -> anyhow::Result<Vec<Candidate>> {
    let busy = busy(pool).await;
    let rows: Vec<(Uuid, String, Uuid, i64, i64)> =
        crate::db::query_as(&format!("{CANDIDATE} {filter}"))
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .filter(|r| !busy.contains(&r.0))
        .map(|(id, name, host_id, memory_mib, priority)| Candidate {
            id,
            name,
            host_id,
            memory_mib,
            priority,
        })
        .collect())
}

async fn running(pool: &DbPool) -> anyhow::Result<Vec<Candidate>> {
    candidates(
        pool,
        "WHERE v.preemptible = TRUE AND v.desired_state = 'running' AND v.observed_state = 'running'
         AND v.host_id IS NOT NULL",
    )
    .await
}

async fn preempted(pool: &DbPool) -> anyhow::Result<Vec<Candidate>> {
    candidates(
        pool,
        "WHERE v.preempted_at IS NOT NULL AND v.desired_state = 'sleeping' AND v.host_id IS NOT NULL
         ORDER BY v.preempted_at",
    )
    .await
}

fn settled(host: Uuid) -> bool {
    SETTLED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|m| m.get(&host))
        .is_none_or(|t| t.elapsed() >= SETTLE)
}

fn touch(host: Uuid) {
    SETTLED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(host, Instant::now());
}

async fn preempt(state: &AppState, c: &Candidate, reason: &str) -> anyhow::Result<()> {
    tracing::info!(vm = %c.name, reason, "preempting vm");
    enqueue_task(
        state,
        "vm.power",
        serde_json::json!({
            "vm_id": c.id.to_string(),
            "action": "managedsave",
            "preempt": true,
            "reason": format!("preempted: {reason}"),
        }),
        Some("vm"),
        Some(c.id),
        Some(c.host_id),
    )
    .await
    .map_err(|e| anyhow::anyhow!(e.message))?;
    state.emit_event("vm.preempt", format!("VM {} preempted: {reason}", c.name));
    Ok(())
}

/// Preempt VMs so a regular VM of `need_mib` fits; the host it will fit on.
/// None when preemption is off or no amount of it would make room.
pub async fn make_room(
    state: &AppState,
    need_mib: i64,
    for_vm: &str,
) -> anyhow::Result<Option<Uuid>> {
    if !settings(&state.pool).await.enabled {
        return Ok(None);
    }
    let hosts = hosts(&state.pool).await?;
    let schedulable: std::collections::HashSet<Uuid> = crate::db::query_scalar(
        "SELECT id FROM hosts WHERE maintenance_mode = FALSE AND schedulable = TRUE",
    )
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .collect();
    let hosts: Vec<HostCap> = hosts
        .into_iter()
        .filter(|h| schedulable.contains(&h.id))
        .collect();
    let running = running(&state.pool).await?;
    let Some((host, victims)) = room_plan(&hosts, &running, need_mib) else {
        return Ok(None);
    };
    let name = hosts
        .iter()
        .find(|h| h.id == host)
        .map(|h| h.name.clone())
        .unwrap_or_default();
    let mut freed = 0;
    for v in &victims {
        if let Some(c) = running.iter().find(|c| c.id == *v) {
            preempt(state, c, &format!("room for {for_vm} on {name}")).await?;
            freed += c.memory_mib;
        }
    }
    crate::db::query("UPDATE hosts SET memory_used_mib = MAX(0, memory_used_mib - ?) WHERE id = ?")
        .bind(freed)
        .bind(host)
        .execute(&state.pool)
        .await?;
    touch(host);
    Ok(Some(host))
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(TICK).await;
            if !state.leader.is_leader() {
                continue;
            }
            if let Err(e) = tick(&state).await {
                tracing::warn!("preemption loop: {e:#}");
            }
        }
    });
}

pub async fn tick(state: &AppState) -> anyhow::Result<()> {
    let s = settings(&state.pool).await;
    if !s.enabled {
        return Ok(());
    }
    let hosts: Vec<HostCap> = hosts(&state.pool)
        .await?
        .into_iter()
        .filter(|h| h.fresh && settled(h.id))
        .collect();
    let running = running(&state.pool).await?;
    let mut acted = std::collections::HashSet::new();
    for h in &hosts {
        let victims = pressure_victims(h, s.reserve_pct, &running);
        for v in &victims {
            if let Some(c) = running.iter().find(|c| c.id == *v) {
                let why = format!("{} below {}% free memory", h.name, s.reserve_pct);
                preempt(state, c, &why).await?;
            }
        }
        if !victims.is_empty() {
            acted.insert(h.id);
        }
    }
    let calm: Vec<HostCap> = hosts
        .into_iter()
        .filter(|h| !acted.contains(&h.id))
        .collect();
    let waiting = preempted(&state.pool).await?;
    for id in resume_order(&calm, s.reserve_pct, &waiting) {
        let Some(c) = waiting.iter().find(|c| c.id == id) else {
            continue;
        };
        tracing::info!(vm = %c.name, "resuming preempted vm");
        enqueue_task(
            state,
            "vm.power",
            serde_json::json!({
                "vm_id": c.id.to_string(),
                "action": "start",
                "resume_preempted": true,
            }),
            Some("vm"),
            Some(c.id),
            Some(c.host_id),
        )
        .await
        .map_err(|e| anyhow::anyhow!(e.message))?;
        state.emit_event(
            "vm.resume",
            format!("VM {} resumed: capacity freed", c.name),
        );
        acted.insert(c.host_id);
    }
    for h in acted {
        touch(h);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::test_support::{seed_host, test_state};

    fn host(id: u128, total: i64, used: i64) -> HostCap {
        HostCap {
            id: Uuid::from_u128(id),
            name: format!("h{id}"),
            total_mib: total,
            used_mib: used,
            fresh: true,
        }
    }

    fn vm(id: u128, host: u128, mem: i64, prio: i64) -> Candidate {
        Candidate {
            id: Uuid::from_u128(id),
            name: format!("vm{id}"),
            host_id: Uuid::from_u128(host),
            memory_mib: mem,
            priority: prio,
        }
    }

    #[test]
    fn pressure_takes_lowest_priority_until_reserve() {
        let h = host(1, 10_000, 9_500);
        let vms = [
            vm(10, 1, 300, 5),
            vm(11, 1, 400, 0),
            vm(12, 1, 200, 0),
            vm(13, 2, 4000, 0),
        ];
        assert_eq!(
            pressure_victims(&h, 10, &vms),
            vec![Uuid::from_u128(11), Uuid::from_u128(12)]
        );
        assert!(pressure_victims(&host(1, 10_000, 8_000), 10, &vms).is_empty());
        assert_eq!(pressure_victims(&h, 90, &vms).len(), 3);
    }

    #[test]
    fn room_plan_prefers_lowest_priority_then_fewest() {
        let hosts = [host(1, 8_000, 7_000), host(2, 8_000, 6_000)];
        let vms = [
            vm(10, 1, 3_000, 0),
            vm(20, 2, 1_000, 0),
            vm(21, 2, 1_000, 0),
            vm(22, 2, 2_000, 9),
        ];
        assert_eq!(
            room_plan(&hosts, &vms, 4_000),
            Some((Uuid::from_u128(1), vec![Uuid::from_u128(10)]))
        );
        let vms = [
            vm(10, 1, 3_000, 50),
            vm(20, 2, 1_000, 0),
            vm(21, 2, 1_000, 0),
        ];
        assert_eq!(
            room_plan(&hosts, &vms, 4_000),
            Some((
                Uuid::from_u128(2),
                vec![Uuid::from_u128(20), Uuid::from_u128(21)]
            ))
        );
        assert_eq!(room_plan(&hosts, &vms, 9_000), None);
        let stale = [HostCap {
            fresh: false,
            ..host(1, 8_000, 7_000)
        }];
        assert_eq!(room_plan(&stale, &[vm(10, 1, 3_000, 0)], 2_000), None);
    }

    #[test]
    fn resume_keeps_reserve_and_margin() {
        let hosts = [host(1, 10_000, 7_000)];
        let waiting = [
            vm(10, 1, 1_000, 0),
            vm(11, 1, 1_000, 9),
            vm(12, 1, 1_000, 0),
        ];
        assert_eq!(
            resume_order(&hosts, 10, &waiting),
            vec![Uuid::from_u128(11)]
        );
        assert_eq!(
            resume_order(&[host(1, 10_000, 9_000)], 10, &waiting),
            Vec::<Uuid>::new()
        );
        assert_eq!(
            resume_order(&[host(1, 10_000, 2_000)], 10, &waiting).len(),
            3
        );
    }

    async fn seed(
        pool: &DbPool,
        host: Uuid,
        id: u128,
        mem: i64,
        prio: i64,
        preemptible: bool,
    ) -> Uuid {
        let v = Uuid::from_u128(id);
        crate::db::query(
            "INSERT INTO vms (id, host_id, name, desired_state, observed_state, memory_mib, preemptible, preempt_priority, guest_ip, lifecycle_phase, inventory_source)
             VALUES (?, ?, ?, 'running', 'running', ?, ?, ?, ?, 'running', 'libvirt')",
        )
        .bind(v)
        .bind(host)
        .bind(format!("vm{id}"))
        .bind(mem)
        .bind(preemptible)
        .bind(prio)
        .bind(format!("10.0.0.{id}"))
        .execute(pool)
        .await
        .unwrap();
        v
    }

    async fn power_tasks(pool: &DbPool) -> Vec<(Uuid, serde_json::Value)> {
        crate::db::query_as(
            "SELECT resource_id, payload FROM tasks WHERE operation = 'vm.power' ORDER BY rowid",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn pressure_preempts_and_capacity_resumes() {
        let (state, _rx) = test_state().await;
        let h = seed_host(&state.pool, Uuid::from_u128(101)).await;
        crate::db::query("UPDATE hosts SET memory_total_mib = 10000, memory_used_mib = 9500, last_heartbeat_at = datetime('now') WHERE id = ?")
            .bind(h)
            .execute(&state.pool)
            .await
            .unwrap();
        let low = seed(&state.pool, h, 10, 1000, 0, true).await;
        seed(&state.pool, h, 11, 1000, 50, true).await;
        seed(&state.pool, h, 12, 4000, 0, false).await;
        tick(&state).await.unwrap();
        let t = power_tasks(&state.pool).await;
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].0, low);
        assert_eq!(t[0].1["action"], "managedsave");
        assert_eq!(t[0].1["preempt"], true);

        crate::db::query("UPDATE tasks SET status = 'completed'")
            .execute(&state.pool)
            .await
            .unwrap();
        crate::db::query("UPDATE vms SET desired_state = 'sleeping', observed_state = 'shutoff', preempted_at = datetime('now') WHERE id = ?")
            .bind(low)
            .execute(&state.pool)
            .await
            .unwrap();
        let wake = crate::engine::vm_sleep::wake_set(&state.pool, h).await;
        assert!(
            wake.entries.is_empty(),
            "a preempted VM must not wake on traffic"
        );

        crate::db::query("UPDATE hosts SET memory_used_mib = 3000 WHERE id = ?")
            .bind(h)
            .execute(&state.pool)
            .await
            .unwrap();
        SETTLED.lock().unwrap().as_mut().unwrap().remove(&h);
        tick(&state).await.unwrap();
        let t = power_tasks(&state.pool).await;
        assert_eq!(t.len(), 2);
        assert_eq!(t[1].0, low);
        assert_eq!(t[1].1["action"], "start");
        assert_eq!(t[1].1["resume_preempted"], true);
    }

    #[tokio::test]
    async fn make_room_preempts_on_the_cheapest_host() {
        let (state, _rx) = test_state().await;
        let h = seed_host(&state.pool, Uuid::from_u128(1)).await;
        crate::db::query("UPDATE hosts SET memory_total_mib = 8000, memory_used_mib = 7500, last_heartbeat_at = datetime('now') WHERE id = ?")
            .bind(h)
            .execute(&state.pool)
            .await
            .unwrap();
        let p = seed(&state.pool, h, 10, 3000, 0, true).await;
        assert_eq!(make_room(&state, 9000, "big").await.unwrap(), None);
        assert_eq!(make_room(&state, 3000, "web").await.unwrap(), Some(h));
        let t = power_tasks(&state.pool).await;
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].0, p);
        assert_eq!(t[0].1["reason"], "preempted: room for web on h1");
        let used: i64 = crate::db::query_scalar("SELECT memory_used_mib FROM hosts WHERE id = ?")
            .bind(h)
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert_eq!(used, 4500);
        crate::db::query("UPDATE preempt_settings SET enabled = FALSE")
            .execute(&state.pool)
            .await
            .unwrap();
        assert_eq!(make_room(&state, 100, "x").await.unwrap(), None);
    }
}
