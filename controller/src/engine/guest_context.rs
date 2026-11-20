// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Normalized QEMU guest-agent snapshots for AI features (insights, fleet query, copilot).

use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use tokio::sync::{RwLock, Semaphore};
use uuid::Uuid;

use crate::config::ControllerConfig;
use crate::engine::host_os::{self, VmGuestHealthReport};

const CACHE_TTL: Duration = Duration::from_secs(60);
const FLEET_CONCURRENCY: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestIpRow {
    pub address: String,
    pub interface: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestUserRow {
    pub username: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub login_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestFsRow {
    pub mountpoint: String,
    pub used_pct: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestAiSnapshot {
    pub vm_id: String,
    pub vm_name: String,
    pub observed_state: String,
    pub inventory_source: String,
    pub install_state: String,
    pub agent_ping: bool,
    pub agent_reachable: bool,
    pub healthy: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub os_pretty_name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub os_kernel: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub os_arch: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub guest_hostname: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub guest_ip: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub agent_version: String,
    pub ipv4_addresses: Vec<GuestIpRow>,
    pub users: Vec<GuestUserRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_delta_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fs_frozen: Option<bool>,
    pub filesystems: Vec<GuestFsRow>,
    pub issues: Vec<String>,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_init_status: Option<String>,
    pub collected_at: String,
}

struct CacheEntry {
    snapshot: GuestAiSnapshot,
    at: Instant,
}

static SNAPSHOT_CACHE: LazyLock<RwLock<HashMap<Uuid, CacheEntry>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub fn from_health_report(
    health: &VmGuestHealthReport,
    observed_state: &str,
    inventory_source: &str,
) -> GuestAiSnapshot {
    let obs = health.guest_observability.as_ref();
    let mut ipv4_addresses = Vec::new();
    let mut users = Vec::new();
    let mut filesystems = Vec::new();
    let mut time_delta_ms = None;
    let mut fs_frozen = None;
    let mut os_kernel = String::new();
    let mut os_arch = String::new();
    let mut cloud_init_status = None;

    if let Some(o) = obs {
        if let Some(arr) = o.get("ip_addresses").and_then(|v| v.as_array()) {
            for item in arr {
                let ip_type = item
                    .get("ip_type")
                    .and_then(|x| x.as_str())
                    .unwrap_or("ipv4");
                if ip_type != "ipv4" {
                    continue;
                }
                let address = item.get("address").and_then(|x| x.as_str()).unwrap_or("");
                if address.is_empty() || address.starts_with("127.") {
                    continue;
                }
                ipv4_addresses.push(GuestIpRow {
                    address: address.to_string(),
                    interface: item
                        .get("name")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                    source: item
                        .get("source")
                        .and_then(|x| x.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                });
            }
        }
        if let Some(arr) = o.get("users").and_then(|v| v.as_array()) {
            for u in arr {
                users.push(GuestUserRow {
                    username: u
                        .get("username")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                    login_time: u
                        .get("login_time")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string()),
                });
            }
        }
        if let Some(t) = o.get("time") {
            time_delta_ms = t.get("delta_ms").and_then(|x| x.as_i64());
        }
        if let Some(fz) = o.get("fs_freeze") {
            fs_frozen = fz.get("frozen").and_then(|x| x.as_bool());
        }
        os_kernel = o
            .get("os_kernel")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        os_arch = o
            .get("os_arch")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        cloud_init_status = o
            .get("cloud_init_status")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        if let Some(arr) = o.get("filesystems").and_then(|v| v.as_array()) {
            for fs in arr {
                let total = fs.get("total_bytes").and_then(|x| x.as_u64()).unwrap_or(0);
                let used = fs.get("used_bytes").and_then(|x| x.as_u64()).unwrap_or(0);
                let used_pct = if total > 0 {
                    ((used as f64 / total as f64) * 100.0).round() as u32
                } else {
                    0
                };
                filesystems.push(GuestFsRow {
                    mountpoint: fs
                        .get("mountpoint")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                    used_pct,
                });
            }
        }
    }

    GuestAiSnapshot {
        vm_id: health.vm_id.clone(),
        vm_name: health.vm_name.clone(),
        observed_state: observed_state.to_string(),
        inventory_source: inventory_source.to_string(),
        install_state: health.install_state.clone(),
        agent_ping: health.agent_ping,
        agent_reachable: health.agent_reachable,
        healthy: health.healthy,
        os_pretty_name: health.os_pretty_name.clone(),
        os_kernel,
        os_arch,
        guest_hostname: health.guest_hostname.clone(),
        guest_ip: health.guest_ip.clone(),
        agent_version: health.agent_version.clone(),
        ipv4_addresses,
        users,
        time_delta_ms,
        fs_frozen,
        filesystems,
        issues: health.issues.clone(),
        summary: health.summary.clone(),
        cloud_init_status,
        collected_at: chrono::Utc::now().to_rfc3339(),
    }
}

/// One-line chip for copilot / UI headers.
pub fn context_chip(s: &GuestAiSnapshot) -> String {
    let mut parts = Vec::new();
    if !s.os_pretty_name.is_empty() {
        parts.push(s.os_pretty_name.clone());
    }
    if !s.os_kernel.is_empty() {
        parts.push(s.os_kernel.clone());
    }
    if s.agent_ping {
        parts.push("QGA active".into());
    } else if s.install_state == "channel_only" {
        parts.push("QGA channel only".into());
    }
    if let Some(ms) = s.time_delta_ms {
        if ms.abs() > 5000 {
            parts.push(format!("Δ{}s", ms / 1000));
        }
    }
    if !s.users.is_empty() {
        parts.push(format!("{} user(s)", s.users.len()));
    }
    if parts.is_empty() {
        s.vm_name.clone()
    } else {
        parts.join(" · ")
    }
}

pub async fn snapshot_for_vm(
    pool: &DbPool,
    cfg: &ControllerConfig,
    vm_id: Uuid,
    refresh: bool,
) -> anyhow::Result<GuestAiSnapshot> {
    if !refresh {
        let cache = SNAPSHOT_CACHE.read().await;
        if let Some(entry) = cache.get(&vm_id) {
            if entry.at.elapsed() < CACHE_TTL {
                return Ok(entry.snapshot.clone());
            }
        }
    }

    let row: (String, String, String) = crate::db::query_as(
        "SELECT name, observed_state, COALESCE(inventory_source, 'libvirt') FROM vms WHERE id = ?",
    )
    .bind(vm_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("vm not found"))?;

    let health = host_os::vm_guest_health(pool, cfg, vm_id).await?;
    let snap = from_health_report(&health, &row.1, &row.2);

    let mut cache = SNAPSHOT_CACHE.write().await;
    // Evict expired entries while we hold the write lock. The read path only *skips*
    // expired entries (never removes them) and invalidate_vm() has no callers, so
    // without this the map would retain one snapshot per distinct VM UUID ever queried
    // — including long-deleted VMs — growing unbounded on a churning fleet. Sweeping
    // here bounds it to VMs queried within CACHE_TTL.
    cache.retain(|_, e| e.at.elapsed() < CACHE_TTL);
    cache.insert(
        vm_id,
        CacheEntry {
            snapshot: snap.clone(),
            at: Instant::now(),
        },
    );
    Ok(snap)
}

pub async fn invalidate_vm(vm_id: Uuid) {
    SNAPSHOT_CACHE.write().await.remove(&vm_id);
}

pub async fn gather_fleet_snapshots(
    pool: &DbPool,
    cfg: &ControllerConfig,
    vm_ids: Vec<Uuid>,
    refresh: bool,
) -> Vec<(Uuid, Result<GuestAiSnapshot, String>)> {
    let sem = std::sync::Arc::new(Semaphore::new(FLEET_CONCURRENCY));
    let mut handles = Vec::new();
    for id in vm_ids {
        let pool = pool.clone();
        let cfg = cfg.clone();
        let sem = sem.clone();
        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.ok();
            let row: Option<(String, String)> = crate::db::query_as(
                "SELECT observed_state, COALESCE(inventory_source, 'libvirt') FROM vms WHERE id = ?",
            )
            .bind(id)
            .fetch_optional(&pool)
            .await
            .ok()
            .flatten();
            let Some((state, source)) = row else {
                return (id, Err("vm not found".into()));
            };
            if source == "kubevirt" {
                return (id, Err("kubevirt VM — no libvirt guest agent".into()));
            }
            if state != "running" && state != "paused" {
                return (id, Err(format!("VM is {state}")));
            }
            match snapshot_for_vm(&pool, &cfg, id, refresh).await {
                Ok(s) => (id, Ok(s)),
                Err(e) => (id, Err(e.to_string())),
            }
        }));
    }
    let mut out = Vec::new();
    for h in handles {
        if let Ok(r) = h.await {
            out.push(r);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chip_includes_os_and_qga() {
        let s = GuestAiSnapshot {
            vm_id: "id".into(),
            vm_name: "vm1".into(),
            observed_state: "running".into(),
            inventory_source: "libvirt".into(),
            install_state: "running".into(),
            agent_ping: true,
            agent_reachable: true,
            healthy: true,
            os_pretty_name: "Ubuntu 24.04".into(),
            os_kernel: "6.8.0".into(),
            os_arch: String::new(),
            guest_hostname: String::new(),
            guest_ip: "10.0.0.1".into(),
            agent_version: String::new(),
            ipv4_addresses: vec![],
            users: vec![GuestUserRow {
                username: "root".into(),
                login_time: None,
            }],
            time_delta_ms: Some(8000),
            fs_frozen: None,
            filesystems: vec![],
            issues: vec![],
            summary: String::new(),
            cloud_init_status: None,
            collected_at: String::new(),
        };
        let chip = context_chip(&s);
        assert!(chip.contains("Ubuntu"));
        assert!(chip.contains("QGA"));
        assert!(chip.contains("user"));
    }
}
