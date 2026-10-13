// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! WireGuard overlay between hosts (`machina_bpf::netpol::overlay`): fleet
//! prefixes per host, fleet addresses per VM address, and the peer list
//! pushed to every host's bpfd. Hosts generate their own keys; the
//! controller only learns public keys from the push responses.

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use machina_bpf::api::{Request, VmOverlay, VmOverlayMap, VmOverlayPeer, VmOverlayStatus};
use machina_bpf::netpol::overlay as ov;
use machina_bpf::netpol::NetpolVm;
use serde::{Deserialize, Serialize};
use crate::db::DbPool;

use super::bpf::{self, HostRef};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OverlaySettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_prefix4")]
    pub prefix4: String,
    #[serde(default = "default_prefix6")]
    pub prefix6: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub updated_by: String,
}

fn default_prefix4() -> String {
    ov::DEFAULT_PREFIX4.into()
}
fn default_prefix6() -> String {
    ov::DEFAULT_PREFIX6.into()
}
fn default_port() -> u16 {
    ov::DEFAULT_PORT
}

impl Default for OverlaySettings {
    fn default() -> Self {
        OverlaySettings {
            enabled: false,
            prefix4: default_prefix4(),
            prefix6: default_prefix6(),
            port: default_port(),
            updated_by: String::new(),
        }
    }
}

impl OverlaySettings {
    pub fn validate(&self) -> Result<(), String> {
        let (c4, c6) = ov::host_capacity(&self.prefix4, &self.prefix6);
        if c4 == 0 {
            return Err(format!(
                "prefix4 `{}` must be an IPv4 prefix of /24 or shorter",
                self.prefix4
            ));
        }
        if c6 == 0 {
            return Err(format!(
                "prefix6 `{}` must be an IPv6 prefix of /64 or shorter",
                self.prefix6
            ));
        }
        if self.port == 0 {
            return Err("port must not be 0".into());
        }
        Ok(())
    }
}

/// host id → WireGuard public key, from the last push.
static KEYS: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());
/// Every host was told the overlay is off and nothing changed since.
static IDLE: AtomicBool = AtomicBool::new(false);

pub async fn settings(pool: &DbPool) -> OverlaySettings {
    crate::db::query_scalar::<_, String>("SELECT value FROM vm_netpol_overlay WHERE key = 'settings'")
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default()
}

pub async fn put_settings(pool: &DbPool, s: &OverlaySettings) -> anyhow::Result<()> {
    crate::db::query(
        "INSERT INTO vm_netpol_overlay (key, value) VALUES ('settings', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP",
    )
    .bind(serde_json::to_string(s)?)
    .execute(pool)
    .await?;
    IDLE.store(false, Ordering::Relaxed);
    Ok(())
}

async fn allocations(pool: &DbPool) -> BTreeMap<String, u32> {
    crate::db::query_as::<_, (String, String)>(
        "SELECT key, value FROM vm_netpol_overlay WHERE key != 'settings'",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .filter_map(|(k, v)| v.parse().ok().map(|n| (k, n)))
    .collect()
}

/// The lowest free index in `1..limit` (or `2..limit` with `from = 2`).
fn free_index(used: &BTreeSet<u32>, from: u32, limit: u64) -> Option<u32> {
    (from..)
        .take_while(|n| (*n as u64) < limit)
        .find(|n| !used.contains(n))
}

/// What every host should hold, given the allocations (new ones are added
/// to `alloc`; stale VM slots are dropped from it).
pub fn plan(
    s: &OverlaySettings,
    alloc: &mut BTreeMap<String, u32>,
    hosts: &[(String, String)],
    vms: &[NetpolVm],
    keys: &BTreeMap<String, String>,
) -> BTreeMap<String, VmOverlay> {
    let (c4, c6) = ov::host_capacity(&s.prefix4, &s.prefix6);
    let host_limit = (c4 as u64).min(c6) + 1;
    let mut used: BTreeSet<u32> = alloc
        .iter()
        .filter(|(k, _)| k.starts_with("host:"))
        .map(|(_, v)| *v)
        .collect();
    let mut prefixes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (id, _) in hosts {
        let key = format!("host:{id}");
        let idx = match alloc.get(&key) {
            Some(i) => Some(*i),
            None => free_index(&used, 1, host_limit).inspect(|i| {
                used.insert(*i);
                alloc.insert(key, *i);
            }),
        };
        if let Some(i) = idx {
            let p: Vec<String> = [
                ov::host_prefix4(&s.prefix4, i),
                ov::host_prefix6(&s.prefix6, i),
            ]
            .into_iter()
            .flatten()
            .collect();
            prefixes.insert(id.clone(), p);
        }
    }
    let in_base = |a: &IpAddr| {
        prefixes
            .values()
            .flatten()
            .any(|p| p.split('/').next().is_some_and(|n| same_net(a, n, p)))
    };
    let mut wanted: BTreeSet<String> = BTreeSet::new();
    let mut out = BTreeMap::new();
    for (id, _) in hosts {
        let Some(own) = prefixes.get(id) else {
            continue;
        };
        let mut mappings = Vec::new();
        for v in vms
            .iter()
            .filter(|v| v.host.as_deref() == Some(id.as_str()))
        {
            for a in &v.addresses {
                let Ok(ip) = a.parse::<IpAddr>() else {
                    continue;
                };
                if in_base(&ip) {
                    continue;
                }
                let fam = if ip.is_ipv4() { "a4" } else { "a6" };
                let key = format!("{fam}:{id}:{ip}");
                wanted.insert(key.clone());
                let prefix = if ip.is_ipv4() {
                    &own[0]
                } else {
                    &own[own.len() - 1]
                };
                let slot = match alloc.get(&key) {
                    Some(n) => Some(*n),
                    None => {
                        let taken: BTreeSet<u32> = alloc
                            .iter()
                            .filter(|(k, _)| k.starts_with(&format!("{fam}:{id}:")))
                            .map(|(_, v)| *v)
                            .collect();
                        let limit = if ip.is_ipv4() {
                            ov::VM_SLOTS4 as u64 + 2
                        } else {
                            u32::MAX as u64
                        };
                        free_index(&taken, 2, limit).inspect(|n| {
                            alloc.insert(key.clone(), *n);
                        })
                    }
                };
                if let Some(f) = slot.and_then(|n| ov::addr_in(prefix, n)) {
                    mappings.push(VmOverlayMap {
                        local: ip.to_string(),
                        fleet: f.to_string(),
                        vm: v.name.clone(),
                    });
                }
            }
        }
        let peers = hosts
            .iter()
            .filter(|(o, _)| o != id)
            .filter_map(|(o, addr)| {
                Some(VmOverlayPeer {
                    host: o.clone(),
                    public_key: keys.get(o)?.clone(),
                    endpoint: endpoint(addr, s.port)?,
                    prefixes: prefixes.get(o)?.clone(),
                    routes: prefixes.get(o)?.clone(),
                })
            })
            .collect();
        out.insert(
            id.clone(),
            VmOverlay {
                enabled: true,
                listen_port: s.port,
                prefixes: own.clone(),
                fleet_prefixes: vec![s.prefix4.clone(), s.prefix6.clone()],
                mappings,
                peers,
                direct_routes: Vec::new(),
            },
        );
    }
    let online: BTreeSet<&str> = hosts.iter().map(|(id, _)| id.as_str()).collect();
    alloc.retain(|k, _| {
        let mut it = k.splitn(3, ':');
        match (it.next(), it.next()) {
            (Some("a4" | "a6"), Some(h)) => !online.contains(h) || wanted.contains(k),
            _ => true,
        }
    });
    out
}

fn same_net(a: &IpAddr, net: &str, cidr: &str) -> bool {
    let (Ok(n), Some(Ok(l))) = (
        net.parse::<IpAddr>(),
        cidr.split_once('/').map(|(_, l)| l.parse::<u32>()),
    ) else {
        return false;
    };
    match (a, n) {
        (IpAddr::V4(a), IpAddr::V4(n)) => {
            let m = if l == 0 { 0 } else { u32::MAX << (32 - l) };
            u32::from(*a) & m == u32::from(n) & m
        }
        (IpAddr::V6(a), IpAddr::V6(n)) => {
            let m = if l == 0 { 0 } else { u128::MAX << (128 - l) };
            u128::from(*a) & m == u128::from(n) & m
        }
        _ => false,
    }
}

fn endpoint(addr: &str, port: u16) -> Option<String> {
    match addr.parse::<IpAddr>().ok()? {
        IpAddr::V4(a) => Some(format!("{a}:{port}")),
        IpAddr::V6(a) => Some(format!("[{a}]:{port}")),
    }
}

async fn save_allocations(
    pool: &DbPool,
    before: &BTreeMap<String, u32>,
    after: &BTreeMap<String, u32>,
) {
    for k in before.keys().filter(|k| !after.contains_key(*k)) {
        let _ = crate::db::query("DELETE FROM vm_netpol_overlay WHERE key = ?")
            .bind(k)
            .execute(pool)
            .await;
    }
    for (k, v) in after.iter().filter(|(k, v)| before.get(*k) != Some(v)) {
        let _ = crate::db::query(
            "INSERT INTO vm_netpol_overlay (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP",
        )
        .bind(k)
        .bind(v.to_string())
        .execute(pool)
        .await;
    }
}

/// VM address → fleet address, from the stored allocations (for identities).
pub async fn fleet_addresses(
    pool: &DbPool,
    vms: &[NetpolVm],
) -> BTreeMap<(String, String), String> {
    let s = settings(pool).await;
    if !s.enabled {
        return BTreeMap::new();
    }
    let alloc = allocations(pool).await;
    let mut out = BTreeMap::new();
    for v in vms {
        let Some(h) = v.host.as_deref() else {
            continue;
        };
        let Some(hi) = alloc.get(&format!("host:{h}")) else {
            continue;
        };
        for a in &v.addresses {
            let Ok(ip) = a.parse::<IpAddr>() else {
                continue;
            };
            let (fam, prefix) = if ip.is_ipv4() {
                ("a4", ov::host_prefix4(&s.prefix4, *hi))
            } else {
                ("a6", ov::host_prefix6(&s.prefix6, *hi))
            };
            if let (Some(n), Some(p)) = (alloc.get(&format!("{fam}:{h}:{ip}")), prefix) {
                if let Some(f) = ov::addr_in(&p, *n) {
                    out.insert((h.to_string(), ip.to_string()), f.to_string());
                }
            }
        }
    }
    out
}

pub async fn enabled(pool: &DbPool) -> bool {
    settings(pool).await.enabled
}

/// Push each online host its overlay config; when off, tear down once.
pub async fn reconcile(
    pool: &DbPool,
    hosts: &[HostRef],
    host_addrs: &BTreeMap<String, String>,
    vms: &[NetpolVm],
) {
    let mut s = settings(pool).await;
    // Fleet Cloud VPC peering uses the same authenticated WireGuard fabric.
    // It activates the runtime overlay without changing the user's VM-network-
    // policy preference; removing the last peering route tears it back down.
    if !s.enabled && super::cloud_fabric::needs_overlay(pool).await.unwrap_or(false) {
        s.enabled = true;
    }
    if !s.enabled {
        if IDLE.load(Ordering::Relaxed) {
            return;
        }
        let mut all_ok = true;
        for h in hosts {
            let req = Request::VmOverlaySet {
                config: VmOverlay::default(),
            };
            match bpf::call(h, &req).await {
                Ok(_) => {}
                Err(e) if format!("{e:#}").contains("unknown variant") => {}
                Err(e) => {
                    all_ok = false;
                    tracing::warn!(host = %h.hostname, "overlay teardown: {e:#}");
                }
            }
        }
        KEYS.lock().unwrap_or_else(|e| e.into_inner()).clear();
        if all_ok {
            IDLE.store(true, Ordering::Relaxed);
        }
        return;
    }
    IDLE.store(false, Ordering::Relaxed);
    let before = allocations(pool).await;
    let mut alloc = before.clone();
    let pairs: Vec<(String, String)> = hosts
        .iter()
        .map(|h| {
            (
                h.id.clone(),
                host_addrs.get(&h.id).cloned().unwrap_or_default(),
            )
        })
        .collect();
    let keys = KEYS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut plans = plan(&s, &mut alloc, &pairs, vms, &keys);
    match super::cloud_fabric::overlay_policy(pool).await {
        Ok(policy) => super::cloud_fabric::decorate_overlay(&mut plans, &policy),
        Err(e) => tracing::warn!("cloud fabric overlay policy: {e:#}"),
    }
    save_allocations(pool, &before, &alloc).await;
    for h in hosts {
        let Some(cfg) = plans.get(&h.id) else {
            continue;
        };
        match bpf::call(
            h,
            &Request::VmOverlaySet {
                config: cfg.clone(),
            },
        )
        .await
        {
            Ok(v) => {
                if let Ok(st) = serde_json::from_value::<VmOverlayStatus>(v) {
                    if let Some(e) = &st.error {
                        tracing::warn!(host = %h.hostname, "overlay: {e}");
                    }
                    if !st.public_key.is_empty() {
                        KEYS.lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .insert(h.id.clone(), st.public_key);
                    }
                }
            }
            Err(e) => tracing::warn!(host = %h.hostname, "overlay push: {e:#}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vm(name: &str, host: &str, addrs: &[&str]) -> NetpolVm {
        NetpolVm {
            name: name.into(),
            host: Some(host.into()),
            project: None,
            labels: BTreeMap::new(),
            addresses: addrs.iter().map(|a| a.to_string()).collect(),
        }
    }

    #[test]
    fn plans_prefixes_slots_and_peers() {
        let s = OverlaySettings {
            enabled: true,
            ..Default::default()
        };
        assert!(s.validate().is_ok());
        let hosts = vec![
            ("h1".to_string(), "10.0.0.1".to_string()),
            ("h2".to_string(), "10.0.0.2".to_string()),
        ];
        let vms = vec![
            vm("a", "h1", &["192.168.122.5", "fd00::5"]),
            vm("b", "h2", &["192.168.122.5"]),
            vm("c", "h2", &["192.168.122.6", "100.96.2.9"]),
        ];
        let mut alloc = BTreeMap::new();
        let keys: BTreeMap<String, String> = [("h2".to_string(), ov::private_key([1; 32]))]
            .into_iter()
            .collect();
        let p = plan(&s, &mut alloc, &hosts, &vms, &keys);
        let h1 = &p["h1"];
        assert_eq!(h1.prefixes, ["100.96.1.0/24", "fd6d:6163:6869:1::/64"]);
        assert_eq!(h1.mappings.len(), 2);
        assert_eq!(h1.mappings[0].fleet, "100.96.1.2");
        assert_eq!(h1.mappings[1].fleet, "fd6d:6163:6869:1::2");
        assert_eq!(h1.peers.len(), 1);
        assert_eq!(h1.peers[0].endpoint, "10.0.0.2:51871");
        assert_eq!(
            h1.peers[0].prefixes,
            ["100.96.2.0/24", "fd6d:6163:6869:2::/64"]
        );
        assert!(p["h2"].peers.is_empty(), "h1 has no key yet");
        let h2: Vec<_> = p["h2"]
            .mappings
            .iter()
            .map(|m| (m.vm.as_str(), m.fleet.as_str()))
            .collect();
        assert_eq!(
            h2,
            [("b", "100.96.2.2"), ("c", "100.96.2.3")],
            "fleet addresses are not remapped"
        );
        assert!(ov::validate(h1).is_ok());

        let again = plan(&s, &mut alloc, &hosts, &vms[1..], &keys);
        assert!(again["h1"].mappings.is_empty());
        assert!(
            !alloc.contains_key("a4:h1:192.168.122.5"),
            "stale slot freed"
        );
        assert_eq!(
            again["h2"].mappings[0].fleet, "100.96.2.2",
            "stable across plans"
        );
        assert_eq!(alloc["host:h1"], 1);

        let bad = OverlaySettings {
            prefix4: "100.96.0.0/25".into(),
            ..Default::default()
        };
        assert!(bad.validate().is_err());
    }
}
