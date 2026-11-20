// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Scale-to-zero. An idle VM is managed-saved (desired_state='sleeping'),
//! its addresses go into the host bpfd wake set (`machina_bpf::netpol::wake`)
//! and the agent restores it when traffic arrives. The controller learns a
//! VM woke when inventory reports it running while desired is still
//! 'sleeping', so no agent→controller callback is needed.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use machina_bpf::api::{Request, VmWake, VmWakeEntry};
use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

use super::bpf;
use crate::engine::vm_lifecycle;
use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

pub const MIN_SLEEP_AFTER_MINUTES: i64 = 5;
/// Above this guest CPU (percent of its vCPUs) the VM counts as busy.
const ACTIVE_CPU_PERCENT: f32 = 3.0;
/// Above this NIC rate the VM counts as busy; ARP/NTP/DHCP chatter stays well below.
const ACTIVE_NET_BYTES_PER_SEC: f64 = 2048.0;
const TICK: Duration = Duration::from_secs(30);
/// Unchanged wake sets are still re-pushed this often, so a restarted bpfd converges.
const RESYNC: Duration = Duration::from_secs(600);

static PUSHED: Mutex<Option<HashMap<Uuid, (VmWake, Instant)>>> = Mutex::new(None);

/// Whether one inventory sample shows the VM doing real work. `prev` is the
/// previous cumulative NIC byte count and the seconds since it was taken.
pub fn is_active(cpu_percent: f32, prev: Option<(u64, f64)>, net_bytes: u64) -> bool {
    if cpu_percent >= ACTIVE_CPU_PERCENT {
        return true;
    }
    match prev {
        None => true,
        Some((p, _)) if net_bytes < p => true,
        Some((p, secs)) => {
            let secs = secs.max(1.0);
            (net_bytes - p) as f64 / secs >= ACTIVE_NET_BYTES_PER_SEC
        }
    }
}

/// The VM's own setting wins; `None` falls back to the project default; 0 = never.
pub fn effective_minutes(vm: Option<i64>, project: Option<i64>) -> i64 {
    vm.or(project).unwrap_or(0).max(0)
}

/// Stamp `last_active_at` when an inventory sample shows activity (or when
/// the idle clock has not started yet). Runs before vm_metrics is overwritten.
pub async fn observe_activity(pool: &DbPool, vm_id: Uuid, cpu_percent: f32, net_bytes: u64) {
    let prev: Option<(i64, f64)> = crate::db::query_as(
        "SELECT net_bytes, (julianday('now') - julianday(updated_at)) * 86400.0
         FROM vm_metrics WHERE vm_id = ?",
    )
    .bind(vm_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let active = is_active(
        cpu_percent,
        prev.map(|(b, s)| (b.max(0) as u64, s)),
        net_bytes,
    );
    let q = if active {
        "UPDATE vms SET last_active_at = datetime('now') WHERE id = ?"
    } else {
        "UPDATE vms SET last_active_at = datetime('now') WHERE id = ? AND last_active_at IS NULL"
    };
    let _ = crate::db::query(q).bind(vm_id).execute(pool).await;
}

pub async fn record(pool: &DbPool, vm_id: Uuid, kind: &str, reason: &str) {
    let _ = crate::db::query("INSERT INTO vm_sleep_events (vm_id, kind, reason) VALUES (?, ?, ?)")
        .bind(vm_id)
        .bind(kind)
        .bind(reason)
        .execute(pool)
        .await;
}

fn addresses(guest_ip: &str, guest_ips: &str) -> Vec<String> {
    let mut out: Vec<String> = serde_json::from_str::<Vec<String>>(guest_ips).unwrap_or_default();
    if !guest_ip.is_empty() && !out.iter().any(|a| a == guest_ip) {
        out.insert(0, guest_ip.to_string());
    }
    out.retain(|a| {
        a.parse::<std::net::IpAddr>()
            .map(|ip| !ip.is_loopback() && !ip.is_unspecified() && !ip.is_multicast())
            .unwrap_or(false)
    });
    out
}

/// Every sleeping VM on the host with the addresses that wake it.
pub async fn wake_set(pool: &DbPool, host_id: Uuid) -> VmWake {
    let rows: Vec<(String, String, String)> = crate::db::query_as(
        "SELECT name, COALESCE(guest_ip, ''), COALESCE(guest_ips, '[]') FROM vms
         WHERE host_id = ? AND desired_state = 'sleeping' AND preempted_at IS NULL ORDER BY name",
    )
    .bind(host_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    VmWake {
        entries: rows
            .into_iter()
            .map(|(vm, ip, ips)| VmWakeEntry {
                vm,
                addresses: addresses(&ip, &ips)
                    .into_iter()
                    .filter(|a| seen.insert(a.clone()))
                    .collect(),
            })
            .filter(|e| !e.addresses.is_empty())
            .collect(),
    }
}

async fn push(pool: &DbPool, host_id: Uuid, force: bool) -> anyhow::Result<()> {
    let set = wake_set(pool, host_id).await;
    if !force {
        let g = PUSHED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((last, at)) = g.as_ref().and_then(|m| m.get(&host_id)) {
            if *last == set && at.elapsed() < RESYNC {
                return Ok(());
            }
        }
    }
    let Some(host) = bpf::host(pool, &host_id.to_string()).await else {
        return Ok(());
    };
    match bpf::call(
        &host,
        &Request::VmWakeSet {
            config: set.clone(),
        },
    )
    .await
    {
        Ok(_) => {}
        Err(e) if set.entries.is_empty() && format!("{e:#}").contains("unknown variant") => {}
        Err(e) => return Err(e),
    }
    PUSHED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(host_id, (set, Instant::now()));
    Ok(())
}

/// Push the host's wake set now (after a VM slept or woke).
pub async fn sync_host(state: &AppState, host_id: Uuid) -> anyhow::Result<()> {
    push(&state.pool, host_id, true).await
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(TICK).await;
            if !state.leader.is_leader() {
                continue;
            }
            if let Err(e) = tick(&state).await {
                tracing::warn!("vm sleep loop: {e:#}");
            }
        }
    });
}

pub async fn tick(state: &AppState) -> anyhow::Result<()> {
    let pool = &state.pool;
    let woken: Vec<(Uuid, String)> = crate::db::query_as(
        "SELECT id, name FROM vms
         WHERE desired_state = 'sleeping' AND observed_state IN ('running', 'blocked')",
    )
    .fetch_all(pool)
    .await?;
    for (vm_id, name) in woken {
        crate::db::query(
            "UPDATE vms SET desired_state = 'running', slept_at = NULL, preempted_at = NULL,
             last_active_at = datetime('now') WHERE id = ? AND desired_state = 'sleeping'",
        )
        .bind(vm_id)
        .execute(pool)
        .await?;
        let _ = vm_lifecycle::sync_phase_from_observed(pool, vm_id).await;
        record(pool, vm_id, "wake", "traffic").await;
        state.emit_event("vm.wake", format!("VM {name} woke on traffic"));
    }

    type Candidate = (
        Uuid,
        String,
        Uuid,
        Option<i64>,
        Option<i64>,
        f64,
        String,
        String,
    );
    let rows: Vec<Candidate> = crate::db::query_as(
        "SELECT v.id, v.name, v.host_id, v.sleep_after_minutes, p.sleep_after_minutes,
                (julianday('now') - julianday(v.last_active_at)) * 1440.0,
                COALESCE(v.guest_ip, ''), COALESCE(v.guest_ips, '[]')
         FROM vms v
         LEFT JOIN vm_sleep_project_policies p ON p.project = v.project
         WHERE v.desired_state = 'running' AND v.observed_state = 'running'
           AND v.inventory_source = 'libvirt' AND v.host_id IS NOT NULL
           AND v.lifecycle_phase = 'running' AND v.last_active_at IS NOT NULL
           AND (v.sleep_after_minutes > 0 OR (v.sleep_after_minutes IS NULL AND p.sleep_after_minutes > 0))",
    )
    .fetch_all(pool)
    .await?;
    for (vm_id, name, host_id, own, project, idle_min, ip, ips) in rows {
        let after = effective_minutes(own, project);
        if after <= 0 || idle_min < after as f64 {
            continue;
        }
        if addresses(&ip, &ips).is_empty() {
            continue;
        }
        let inflight: i64 = crate::db::query_scalar(
            "SELECT COUNT(*) FROM tasks WHERE resource_id = ? AND operation = 'vm.power'
             AND status IN ('pending', 'running')",
        )
        .bind(vm_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);
        if inflight > 0 {
            continue;
        }
        tracing::info!(vm = %name, idle_min, after, "auto-sleep idle vm");
        if let Err(e) = enqueue_task(
            state,
            "vm.power",
            serde_json::json!({
                "vm_id": vm_id.to_string(),
                "action": "managedsave",
                "auto_sleep": true,
                "idle_minutes": idle_min.round() as i64,
            }),
            Some("vm"),
            Some(vm_id),
            Some(host_id),
        )
        .await
        {
            tracing::warn!(vm = %name, "auto-sleep enqueue failed: {}", e.message);
        }
    }

    let hosts: Vec<(Uuid,)> = crate::db::query_as(
        "SELECT DISTINCT host_id FROM vms WHERE host_id IS NOT NULL AND desired_state = 'sleeping'",
    )
    .fetch_all(pool)
    .await?;
    let mut targets: Vec<Uuid> = hosts.into_iter().map(|(h,)| h).collect();
    {
        let g = PUSHED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(m) = g.as_ref() {
            for (h, (set, _)) in m {
                if !set.entries.is_empty() && !targets.contains(h) {
                    targets.push(*h);
                }
            }
        }
    }
    for h in targets {
        if let Err(e) = push(pool, h, false).await {
            tracing::debug!(host = %h, "wake set push: {e:#}");
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct SleepEvent {
    pub kind: String,
    pub reason: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SleepPolicyView {
    pub vm_id: Uuid,
    pub desired_state: String,
    pub observed_state: String,
    /// The VM's own setting (`null` = inherit the project default).
    pub sleep_after_minutes: Option<i64>,
    pub project: Option<String>,
    pub project_default: Option<i64>,
    /// 0 = never sleeps.
    pub effective_minutes: i64,
    pub last_active_at: Option<String>,
    pub idle_minutes: Option<f64>,
    pub slept_at: Option<String>,
    /// Traffic can wake it (a guest address is known).
    pub wakeable: bool,
    pub events: Vec<SleepEvent>,
}

pub async fn policy_view(
    pool: &DbPool,
    vm_id: Uuid,
) -> anyhow::Result<Option<SleepPolicyView>> {
    type Row = (
        String,
        String,
        Option<i64>,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<f64>,
        Option<String>,
        String,
        String,
    );
    let row: Option<Row> = crate::db::query_as(
        "SELECT v.desired_state, v.observed_state, v.sleep_after_minutes, v.project,
                p.sleep_after_minutes, v.last_active_at,
                (julianday('now') - julianday(v.last_active_at)) * 1440.0,
                v.slept_at, COALESCE(v.guest_ip, ''), COALESCE(v.guest_ips, '[]')
         FROM vms v LEFT JOIN vm_sleep_project_policies p ON p.project = v.project
         WHERE v.id = ?",
    )
    .bind(vm_id)
    .fetch_optional(pool)
    .await?;
    let Some((desired, observed, own, project, pdef, last, idle, slept, ip, ips)) = row else {
        return Ok(None);
    };
    let events: Vec<(String, String, String)> = crate::db::query_as(
        "SELECT kind, reason, at FROM vm_sleep_events WHERE vm_id = ? ORDER BY id DESC LIMIT 20",
    )
    .bind(vm_id)
    .fetch_all(pool)
    .await?;
    Ok(Some(SleepPolicyView {
        vm_id,
        desired_state: desired,
        observed_state: observed,
        sleep_after_minutes: own,
        project,
        project_default: pdef,
        effective_minutes: effective_minutes(own, pdef),
        last_active_at: last,
        idle_minutes: idle.map(|m| (m * 10.0).round() / 10.0),
        slept_at: slept,
        wakeable: !addresses(&ip, &ips).is_empty(),
        events: events
            .into_iter()
            .map(|(kind, reason, at)| SleepEvent { kind, reason, at })
            .collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::test_support::{seed_host, test_state};

    #[test]
    fn activity_rule() {
        assert!(is_active(0.0, None, 0));
        assert!(is_active(5.0, Some((0, 30.0)), 0));
        assert!(!is_active(0.5, Some((1000, 30.0)), 1000 + 30 * 100));
        assert!(is_active(0.5, Some((1000, 30.0)), 1000 + 30 * 4096));
        assert!(is_active(0.5, Some((1_000_000, 30.0)), 10));
        assert!(!is_active(0.5, Some((1000, 0.0)), 1500));
    }

    #[test]
    fn policy_inheritance() {
        assert_eq!(effective_minutes(None, None), 0);
        assert_eq!(effective_minutes(None, Some(30)), 30);
        assert_eq!(effective_minutes(Some(0), Some(30)), 0);
        assert_eq!(effective_minutes(Some(15), Some(30)), 15);
    }

    #[test]
    fn address_filtering() {
        assert_eq!(
            addresses("10.0.0.5", r#"["10.0.0.5","fd00::5","127.0.0.1","junk"]"#),
            vec!["10.0.0.5".to_string(), "fd00::5".to_string()]
        );
        assert_eq!(addresses("10.0.0.7", "[]"), vec!["10.0.0.7".to_string()]);
        assert!(addresses("", "[]").is_empty());
    }

    async fn seed_vm(
        pool: &DbPool,
        host: Uuid,
        id: u128,
        desired: &str,
        observed: &str,
    ) -> Uuid {
        let vm_id = Uuid::from_u128(id);
        crate::db::query(
            "INSERT INTO vms (id, host_id, name, desired_state, observed_state, guest_ip, lifecycle_phase, inventory_source)
             VALUES (?, ?, ?, ?, ?, ?, 'running', 'libvirt')",
        )
        .bind(vm_id)
        .bind(host)
        .bind(format!("vm{id}"))
        .bind(desired)
        .bind(observed)
        .bind(format!("10.0.0.{id}"))
        .execute(pool)
        .await
        .unwrap();
        vm_id
    }

    #[tokio::test]
    async fn woken_vm_returns_to_running() {
        let (state, _rx) = test_state().await;
        let host = seed_host(&state.pool, Uuid::from_u128(1)).await;
        let vm = seed_vm(&state.pool, host, 2, "sleeping", "running").await;
        let asleep = seed_vm(&state.pool, host, 3, "sleeping", "shutoff").await;
        tick(&state).await.unwrap();
        let d: String = crate::db::query_scalar("SELECT desired_state FROM vms WHERE id = ?")
            .bind(vm)
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert_eq!(d, "running");
        let d: String = crate::db::query_scalar("SELECT desired_state FROM vms WHERE id = ?")
            .bind(asleep)
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert_eq!(d, "sleeping");
        let set = wake_set(&state.pool, host).await;
        assert_eq!(set.entries.len(), 1);
        assert_eq!(set.entries[0].vm, "vm3");
        assert_eq!(set.entries[0].addresses, vec!["10.0.0.3".to_string()]);
    }

    #[tokio::test]
    async fn idle_vm_is_put_to_sleep_once() {
        let (state, _rx) = test_state().await;
        let host = seed_host(&state.pool, Uuid::from_u128(1)).await;
        let idle = seed_vm(&state.pool, host, 4, "running", "running").await;
        let busy = seed_vm(&state.pool, host, 5, "running", "running").await;
        let never = seed_vm(&state.pool, host, 6, "running", "running").await;
        crate::db::query(
            "UPDATE vms SET sleep_after_minutes = 10, last_active_at = datetime('now', '-30 minutes') WHERE id = ?",
        )
        .bind(idle)
        .execute(&state.pool)
        .await
        .unwrap();
        crate::db::query(
            "UPDATE vms SET sleep_after_minutes = 10, last_active_at = datetime('now', '-2 minutes') WHERE id = ?",
        )
        .bind(busy)
        .execute(&state.pool)
        .await
        .unwrap();
        crate::db::query(
            "UPDATE vms SET sleep_after_minutes = 0, last_active_at = datetime('now', '-300 minutes') WHERE id = ?",
        )
        .bind(never)
        .execute(&state.pool)
        .await
        .unwrap();
        tick(&state).await.unwrap();
        tick(&state).await.unwrap();
        let tasks: Vec<(Option<Uuid>, serde_json::Value)> =
            crate::db::query_as("SELECT resource_id, payload FROM tasks WHERE operation = 'vm.power'")
                .fetch_all(&state.pool)
                .await
                .unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].0, Some(idle));
        assert_eq!(tasks[0].1["action"], serde_json::json!("managedsave"));
    }

    #[tokio::test]
    async fn project_default_applies() {
        let (state, _rx) = test_state().await;
        let host = seed_host(&state.pool, Uuid::from_u128(1)).await;
        let vm = seed_vm(&state.pool, host, 7, "running", "running").await;
        crate::db::query(
            "UPDATE vms SET project = 'dev', last_active_at = datetime('now', '-45 minutes') WHERE id = ?",
        )
        .bind(vm)
        .execute(&state.pool)
        .await
        .unwrap();
        crate::db::query("INSERT INTO vm_sleep_project_policies (project, sleep_after_minutes) VALUES ('dev', 30)")
            .execute(&state.pool)
            .await
            .unwrap();
        let v = policy_view(&state.pool, vm).await.unwrap().unwrap();
        assert_eq!(v.effective_minutes, 30);
        assert!(v.wakeable);
        tick(&state).await.unwrap();
        let n: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM tasks WHERE operation = 'vm.power'")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
    }
}
