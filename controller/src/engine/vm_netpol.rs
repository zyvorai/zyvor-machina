// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Fleet VM network policies: compile per host and push the result to each
//! host's machina-bpfd VM edge (`owner = controller`, which makes the
//! host daemon's local policies inactive). Re-pushes when the compiled
//! state changes (policies, labels, addresses, placement) and periodically.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use machina_bpf::api::{Request, VmAuthIdentity, VmEdgeStatus, VmEgressSnat};
use machina_bpf::authca;
use machina_bpf::netpol::tenant::{self, ProjectNet};
use machina_bpf::netpol::{
    self, Inputs, NetpolService, NetpolVm, ServiceEndpoint, VmNetworkPolicy,
};
use serde::Serialize;
use serde_json::Value;
use crate::db::DbPool;
use uuid::Uuid;

use super::bpf::{self, HostRef, LOCAL_HOST_ID};
use crate::state::AppState;

pub const OWNER: &str = "controller";
const TICK_SECS: u64 = 30;
/// Push unchanged state again every this many ticks (bpfd restarts, drift).
const FORCE_EVERY: u32 = 10;
/// Check URL threat feeds for a refetch hourly.
const THREAT_REFRESH_EVERY: u32 = 120;

static LAST_PUSH: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);
/// Egress IP gaps found by the last reconcile.
static EGRESS_GAPS: Mutex<Vec<tenant::EgressGap>> = Mutex::new(Vec::new());
/// host id → global addresses (`address/prefix`) its bpfd reported.
static NODE_ADDRS: Mutex<BTreeMap<String, Vec<String>>> = Mutex::new(BTreeMap::new());
/// host id → VM → source addresses its bpfd saw on the VM's tap.
static LEARNED: Mutex<BTreeMap<String, BTreeMap<String, Vec<String>>>> =
    Mutex::new(BTreeMap::new());
/// Every host has an empty egress SNAT set and nothing asks for one.
static EGRESS_IDLE: AtomicBool = AtomicBool::new(false);

pub async fn policies(
    pool: &DbPool,
) -> anyhow::Result<Vec<(VmNetworkPolicy, bool, i64, String)>> {
    let rows: Vec<(String, bool, i64, String)> = crate::db::query_as(
        "SELECT policy_json, enabled, generation, updated_at FROM vm_network_policies ORDER BY name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(j, en, g, u)| serde_json::from_str(&j).ok().map(|p| (p, en, g, u)))
        .collect())
}

pub async fn enabled_policies(pool: &DbPool) -> Vec<VmNetworkPolicy> {
    let now = chrono::Utc::now();
    policies(pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.1 && !netpol::jit::expired(&p.0, now))
        .map(|p| p.0)
        .collect()
}

/// Delete policies past `machina.io/expires-at`; returns their names.
pub async fn reap_expired(pool: &DbPool) -> Vec<String> {
    let now = chrono::Utc::now();
    let mut gone = Vec::new();
    for (p, ..) in policies(pool).await.unwrap_or_default() {
        if netpol::jit::expired(&p, now) && delete(pool, &p.name).await.unwrap_or(false) {
            gone.push(p.name);
        }
    }
    gone
}

pub async fn upsert(pool: &DbPool, p: &VmNetworkPolicy, actor: &str) -> anyhow::Result<bool> {
    let json = serde_json::to_string(p)?;
    let existed: Option<String> =
        crate::db::query_scalar("SELECT name FROM vm_network_policies WHERE name = ?")
            .bind(&p.name)
            .fetch_optional(pool)
            .await?;
    crate::db::query(
        "INSERT INTO vm_network_policies (name, kind, policy_json, created_by) VALUES (?, ?, ?, ?)
         ON CONFLICT(name) DO UPDATE SET kind = excluded.kind, policy_json = excluded.policy_json,
           generation = vm_network_policies.generation + 1, updated_at = CURRENT_TIMESTAMP",
    )
    .bind(&p.name)
    .bind(&p.kind)
    .bind(json)
    .bind(actor)
    .execute(pool)
    .await?;
    Ok(existed.is_none())
}

pub async fn delete(pool: &DbPool, name: &str) -> anyhow::Result<bool> {
    let r = crate::db::query("DELETE FROM vm_network_policies WHERE name = ?")
        .bind(name)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn set_enabled(pool: &DbPool, name: &str, enabled: bool) -> anyhow::Result<bool> {
    let r = crate::db::query(
        "UPDATE vm_network_policies SET enabled = ?, generation = generation + 1, updated_at = CURRENT_TIMESTAMP WHERE name = ?",
    )
    .bind(enabled)
    .bind(name)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

/// libvirt VMs of the fleet (KubeVirt VMs are pod endpoints, not taps).
pub async fn inventory(pool: &DbPool) -> Vec<NetpolVm> {
    type Row = (
        Uuid,
        String,
        Option<Uuid>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let rows: Vec<Row> = crate::db::query_as(
        "SELECT id, name, host_id, project, labels, tags, guest_ip, guest_ips FROM vms
             WHERE COALESCE(inventory_source, 'libvirt') != 'kubevirt' ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    let qos: BTreeMap<String, (u32, u32, u32)> = load_vm_qos(pool).await;
    rows.into_iter()
        .map(|(id, name, host, project, labels, tags, ip, ips)| {
            let mut l: BTreeMap<String, String> = labels
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            if l.is_empty() {
                let tags: Vec<String> = tags
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_default();
                l = tags
                    .iter()
                    .filter_map(|t| t.split_once('='))
                    .filter(|(k, _)| !k.is_empty())
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();
            }
            let mut addresses: Vec<String> = ip.into_iter().filter(|a| !a.is_empty()).collect();
            let more: Vec<String> = ips
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            for a in more {
                if !a.is_empty() && !addresses.contains(&a) {
                    addresses.push(a);
                }
            }
            let (egress_mbps, ingress_mbps, pps) = qos.get(&id.to_string()).copied().unwrap_or_default();
            NetpolVm {
                name,
                host: host.map(|h| h.to_string()),
                project: project.filter(|p| !p.is_empty()),
                labels: l,
                addresses,
                egress_mbps,
                ingress_mbps,
                pps,
            }
        })
        .collect()
}

async fn load_vm_qos(pool: &DbPool) -> BTreeMap<String, (u32, u32, u32)> {
    let rows: Vec<(String, Option<i64>, Option<i64>, Option<i64>)> = crate::db::query_as(
        "SELECT b.vm_id, q.egress_mbps, q.ingress_mbps, q.pps \
         FROM vm_qos_bindings b JOIN vm_qos_policies q ON q.id = b.policy_id",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    rows.into_iter()
        .map(|(vm_id, eg, ing, pps)| {
            (
                vm_id,
                (
                    eg.unwrap_or(0).clamp(0, u32::MAX as i64) as u32,
                    ing.unwrap_or(0).clamp(0, u32::MAX as i64) as u32,
                    pps.unwrap_or(0).clamp(0, u32::MAX as i64) as u32,
                ),
            )
        })
        .collect()
}

/// host id → management address.
pub async fn host_addresses(pool: &DbPool) -> BTreeMap<String, String> {
    let rows: Vec<(Uuid, String)> = crate::db::query_as("SELECT id, address FROM hosts")
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    rows.into_iter()
        .map(|(id, a)| {
            (
                id.to_string(),
                a.split(':').next().unwrap_or("").to_string(),
            )
        })
        .filter(|(_, a)| a.parse::<std::net::IpAddr>().is_ok())
        .collect()
}

/// Fleet Cloud load balancers as `toServices` targets: name = LB name,
/// namespace = project, endpoints = listener on the owning host plus the
/// enabled members.
pub async fn services(pool: &DbPool) -> Vec<NetpolService> {
    type Row = (
        Uuid,
        String,
        String,
        String,
        i64,
        String,
        Option<String>,
        Option<i64>,
    );
    let rows: Vec<Row> = crate::db::query_as(
        "SELECT lb.id, lb.name, COALESCE(p.name, ''), lb.protocol, lb.listener_port, h.address,
                v.guest_ip, m.port
           FROM load_balancers lb
           JOIN hosts h ON h.id = lb.host_id
           LEFT JOIN projects p ON p.id = lb.project_id
           LEFT JOIN lb_members m ON m.load_balancer_id = lb.id AND m.enabled = TRUE
           LEFT JOIN vms v ON v.id = m.vm_id
          ORDER BY lb.id",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    let mut out: BTreeMap<Uuid, NetpolService> = BTreeMap::new();
    for (id, name, project, protocol, listener, host, member, member_port) in rows {
        let proto = if protocol.eq_ignore_ascii_case("udp") {
            17
        } else {
            6
        };
        let ep = |address: &str, port: i64| ServiceEndpoint {
            address: address.to_string(),
            port: u16::try_from(port).unwrap_or(0),
            proto,
        };
        let s = out.entry(id).or_insert_with(|| NetpolService {
            name,
            namespace: project,
            labels: BTreeMap::new(),
            endpoints: vec![ep(host.split(':').next().unwrap_or(""), listener)],
        });
        if let (Some(ip), Some(port)) = (member.filter(|a| !a.is_empty()), member_port) {
            s.endpoints.push(ep(&ip, port));
        }
    }
    out.into_values().collect()
}

/// Project network settings, the default (`*`) first.
pub async fn project_settings(pool: &DbPool) -> Vec<ProjectNet> {
    let rows: Vec<(String, String, String)> = crate::db::query_as(
        "SELECT settings, updated_by, updated_at FROM vm_netpol_projects
         ORDER BY project != '*', project",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    rows.into_iter()
        .filter_map(|(j, by, at)| {
            let mut s: ProjectNet = serde_json::from_str(&j).ok()?;
            s.updated_by = by;
            s.updated_at = at;
            Some(s)
        })
        .collect()
}

pub async fn project_put(pool: &DbPool, s: &ProjectNet, actor: &str) -> anyhow::Result<()> {
    crate::db::query(
        "INSERT INTO vm_netpol_projects (project, settings, updated_by) VALUES (?, ?, ?)
         ON CONFLICT(project) DO UPDATE SET settings = excluded.settings,
           updated_by = excluded.updated_by, updated_at = CURRENT_TIMESTAMP",
    )
    .bind(&s.project)
    .bind(serde_json::to_string(s)?)
    .bind(actor)
    .execute(pool)
    .await?;
    EGRESS_IDLE.store(false, Ordering::Relaxed);
    Ok(())
}

pub async fn project_delete(pool: &DbPool, project: &str) -> anyhow::Result<bool> {
    let r = crate::db::query("DELETE FROM vm_netpol_projects WHERE project = ?")
        .bind(project)
        .execute(pool)
        .await?;
    EGRESS_IDLE.store(false, Ordering::Relaxed);
    Ok(r.rows_affected() > 0)
}

/// Fleet Cloud projects plus every project a VM names.
pub async fn project_names(pool: &DbPool, vms: &[NetpolVm]) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = crate::db::query_scalar("SELECT name FROM projects")
        .fetch_all(pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect();
    out.extend(vms.iter().filter_map(|v| v.project.clone()));
    out
}

pub fn note_node_addrs(host_id: &str, addrs: &[String]) {
    if addrs.is_empty() {
        return;
    }
    NODE_ADDRS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(host_id.to_string(), addrs.to_vec());
}

fn node_addrs() -> BTreeMap<String, Vec<String>> {
    NODE_ADDRS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Ask each host's bpfd for its addresses and the VM source addresses it
/// learned.
async fn refresh_node_addrs(hosts: &[HostRef]) {
    for h in hosts {
        let Ok(v) = bpf::call(h, &Request::VmEdgeStatus).await else {
            continue;
        };
        if let Ok(s) = serde_json::from_value::<VmEdgeStatus>(v) {
            note_node_addrs(&h.id, &s.node_addrs);
            LEARNED
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(h.id.clone(), s.learned);
        }
    }
}

fn learned() -> BTreeMap<String, BTreeMap<String, Vec<String>>> {
    LEARNED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Add tap-learned addresses to VM identities. Refused: host addresses,
/// addresses another VM on the same host already has, and addresses two
/// VMs on the same host both sent from.
pub fn merge_learned(
    mut vms: Vec<NetpolVm>,
    learned: &BTreeMap<String, BTreeMap<String, Vec<String>>>,
    host_ips: &BTreeSet<String>,
) -> Vec<NetpolVm> {
    let mut claims: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
    for (host, by_vm) in learned {
        for (vm, addrs) in by_vm {
            let Some(i) = vms
                .iter()
                .position(|v| v.name == *vm && v.host.as_deref() == Some(host.as_str()))
            else {
                continue;
            };
            for a in addrs {
                if !host_ips.contains(a) && !vms[i].addresses.contains(a) {
                    claims
                        .entry((host.as_str(), a.as_str()))
                        .or_default()
                        .push(i);
                }
            }
        }
    }
    let mut accepted: Vec<(usize, String)> = Vec::new();
    for ((host, a), who) in claims {
        let owned = vms
            .iter()
            .any(|v| v.host.as_deref() == Some(host) && v.addresses.iter().any(|x| x == a));
        if who.len() == 1 && !owned {
            accepted.push((who[0], a.to_string()));
        }
    }
    for (i, a) in accepted {
        vms[i].addresses.push(a);
    }
    vms
}

fn ipv4_net(cidr: &str) -> Option<(u32, u32)> {
    let (a, l) = cidr.split_once('/')?;
    let a: std::net::Ipv4Addr = a.parse().ok()?;
    let l: u32 = l.parse().ok().filter(|l| *l <= 32)?;
    let mask = if l == 0 { 0 } else { u32::MAX << (32 - l) };
    Some((u32::from(a) & mask, mask))
}

/// A project spread over hosts on per-host NAT networks.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NatSpan {
    pub project: String,
    pub hosts: Vec<String>,
    /// Subnets that several hosts have, holding this project's VMs.
    pub subnets: Vec<String>,
}

/// Everything needed to compile for any host.
pub struct Fleet {
    /// Stored policies plus the generated project policies.
    pub policies: Vec<VmNetworkPolicy>,
    pub vms: Vec<NetpolVm>,
    pub services: Vec<NetpolService>,
    pub host_addrs: BTreeMap<String, String>,
    /// host id → every global address reported by its bpfd.
    pub node_addrs: BTreeMap<String, Vec<String>>,
    pub projects: Vec<ProjectNet>,
    /// host id → hostname.
    pub hostnames: BTreeMap<String, String>,
    /// The WireGuard overlay is on: VMs on other hosts are reached by
    /// fleet address with their identity, NAT or not.
    pub overlay: bool,
}

impl Fleet {
    pub async fn load(pool: &DbPool) -> Self {
        let host_addrs = host_addresses(pool).await;
        let node_addrs = node_addrs();
        let host_ips: BTreeSet<String> = host_addrs
            .values()
            .cloned()
            .chain(
                node_addrs
                    .values()
                    .flatten()
                    .map(|c| c.split('/').next().unwrap_or(c).to_string()),
            )
            .collect();
        let mut vms = merge_learned(inventory(pool).await, &learned(), &host_ips);
        let fleet_addrs = super::vm_overlay::fleet_addresses(pool, &vms).await;
        for v in &mut vms {
            let Some(h) = v.host.clone() else {
                continue;
            };
            let extra: Vec<String> = v
                .addresses
                .iter()
                .filter_map(|a| fleet_addrs.get(&(h.clone(), a.clone())).cloned())
                .collect();
            for e in extra {
                if !v.addresses.contains(&e) {
                    v.addresses.push(e);
                }
            }
        }
        let overlay = super::vm_overlay::enabled(pool).await;
        let projects = project_settings(pool).await;
        let mut policies = enabled_policies(pool).await;
        let hostnames: BTreeMap<String, String> = bpf::hosts(pool)
            .await
            .into_iter()
            .map(|h| (h.id, h.hostname))
            .collect();
        let mut generated = tenant::policies(&projects, &project_names(pool, &vms).await);
        generated.extend(tenant::egress_ip_guards(&projects, &hostnames));
        generated.extend(super::sg_enforce::policies(&super::sg_enforce::load(pool).await));
        for g in generated {
            if !policies.iter().any(|p| p.name == g.name) {
                policies.push(g);
            }
        }
        Fleet {
            policies,
            vms,
            services: services(pool).await,
            host_addrs,
            node_addrs,
            projects,
            hostnames,
            overlay,
        }
    }

    pub fn egress_gaps(&self) -> Vec<tenant::EgressGap> {
        tenant::egress_gaps(&self.projects, &self.vms, &self.hostnames)
    }

    /// host id → its addresses (management first), without prefixes.
    fn addresses_by_host(&self) -> BTreeMap<&str, BTreeSet<String>> {
        let mut out: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
        for (id, a) in &self.host_addrs {
            out.entry(id.as_str()).or_default().insert(a.clone());
        }
        for (id, cidrs) in &self.node_addrs {
            let set = out.entry(id.as_str()).or_default();
            for c in cidrs {
                set.insert(c.split('/').next().unwrap_or(c).to_string());
            }
        }
        out
    }

    /// Addresses more than one host has (e.g. every libvirt `default`
    /// network's 192.168.122.1): they say nothing about which host sent.
    fn shared_addresses(by_host: &BTreeMap<&str, BTreeSet<String>>) -> BTreeSet<String> {
        let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
        for set in by_host.values() {
            for a in set {
                *seen.entry(a.as_str()).or_default() += 1;
            }
        }
        seen.into_iter()
            .filter(|(_, n)| *n > 1)
            .map(|(a, _)| a.to_string())
            .collect()
    }

    /// Projects whose VMs sit on several hosts behind the same gateway
    /// address on each (per-host NAT networks): traffic between
    /// those hosts arrives from the other host's address, so it is
    /// `remote-node`, not a VM, and isolation can't tell projects apart.
    pub fn nat_spans(&self) -> Vec<NatSpan> {
        if self.overlay {
            return Vec::new();
        }
        let mut same: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for (id, cidrs) in &self.node_addrs {
            for c in cidrs {
                same.entry(c.as_str()).or_default().insert(id.as_str());
            }
        }
        let nets: BTreeSet<(u32, u32)> = same
            .into_iter()
            .filter(|(_, hosts)| hosts.len() > 1)
            .filter_map(|(c, _)| ipv4_net(c))
            .collect();
        let mut by_project: BTreeMap<&str, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
        for vm in &self.vms {
            let (Some(p), Some(h)) = (vm.project.as_deref(), vm.host.as_deref()) else {
                continue;
            };
            let e = by_project.entry(p).or_default();
            e.0.insert(h.to_string());
            for a in &vm.addresses {
                let Ok(ip) = a
                    .split('/')
                    .next()
                    .unwrap_or(a)
                    .parse::<std::net::Ipv4Addr>()
                else {
                    continue;
                };
                for (net, mask) in &nets {
                    if u32::from(ip) & mask == *net {
                        let bits = mask.count_ones();
                        e.1.insert(format!("{}/{bits}", std::net::Ipv4Addr::from(*net)));
                    }
                }
            }
        }
        by_project
            .into_iter()
            .filter(|(_, (hosts, subnets))| hosts.len() > 1 && !subnets.is_empty())
            .map(|(p, (hosts, subnets))| NatSpan {
                project: p.to_string(),
                hosts: hosts.into_iter().collect(),
                subnets: subnets.into_iter().collect(),
            })
            .collect()
    }

    /// (`host`, `remote-node`) addresses as seen from `host`.
    pub fn node_identity_addresses(&self, host: Option<&str>) -> (Vec<String>, Vec<String>) {
        let by_host = self.addresses_by_host();
        let shared = Self::shared_addresses(&by_host);
        let own_set = host
            .and_then(|h| by_host.get(h))
            .cloned()
            .unwrap_or_default();
        let mgmt = host.and_then(|h| self.host_addrs.get(h));
        let own: Vec<String> = mgmt
            .cloned()
            .into_iter()
            .chain(own_set.iter().filter(|a| Some(*a) != mgmt).cloned())
            .collect();
        let remote: Vec<String> = by_host
            .iter()
            .filter(|(id, _)| Some(**id) != host)
            .flat_map(|(_, set)| set.iter())
            .filter(|a| !shared.contains(*a) && !own_set.contains(*a))
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        (own, remote)
    }

    /// Compile for one host (`None` / the local pseudo host = every VM).
    pub fn compile(&self, host_id: Option<&str>) -> netpol::Compiled {
        let host = host_id.filter(|h| *h != LOCAL_HOST_ID);
        let (own, remote) = self.node_identity_addresses(host);
        let mut c = netpol::compile(&Inputs {
            policies: &self.policies,
            vms: &self.vms,
            services: &self.services,
            host,
            host_addresses: &own,
            remote_node_addresses: if host.is_some() { &remote } else { &[] },
        });
        if host.is_some() {
            c.state.host_addrs = self.host_addrs.clone();
        }
        c
    }

    pub fn all_host_addresses(&self) -> Vec<String> {
        self.addresses_by_host()
            .into_values()
            .flatten()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HostSync {
    pub host_id: String,
    pub hostname: String,
    pub ok: bool,
    pub pushed: bool,
    pub error: Option<String>,
    pub vms: usize,
    pub rules: usize,
    pub peers: usize,
    pub warnings: Vec<String>,
}

fn hash_value(v: &Value) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.to_string().hash(&mut h);
    h.finish()
}

async fn record(pool: &DbPool, s: &HostSync) {
    let _ = crate::db::query(
        "INSERT INTO vm_netpol_host_status (host_id, hostname, synced_at, ok, error, vms, rules, peers, warnings)
         VALUES (?, ?, CURRENT_TIMESTAMP, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(host_id) DO UPDATE SET hostname = excluded.hostname, synced_at = excluded.synced_at,
           ok = excluded.ok, error = excluded.error, vms = excluded.vms, rules = excluded.rules,
           peers = excluded.peers, warnings = excluded.warnings",
    )
    .bind(&s.host_id)
    .bind(&s.hostname)
    .bind(s.ok)
    .bind(&s.error)
    .bind(s.vms as i64)
    .bind(s.rules as i64)
    .bind(s.peers as i64)
    .bind(serde_json::to_string(&s.warnings).unwrap_or_else(|_| "[]".into()))
    .execute(pool)
    .await;
}

async fn previously_synced(pool: &DbPool, host_id: &str) -> bool {
    crate::db::query_scalar::<_, i64>("SELECT COUNT(*) FROM vm_netpol_host_status WHERE host_id = ?")
        .bind(host_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0)
        > 0
}

static CA: Mutex<Option<std::sync::Arc<authca::Ca>>> = Mutex::new(None);

/// The VM network policy CA (`MACHINA_NETPOL_CA_DIR`, default
/// `/var/lib/machina/netpol-ca`).
fn netpol_ca() -> anyhow::Result<std::sync::Arc<authca::Ca>> {
    let mut g = CA.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(ca) = g.as_ref() {
        return Ok(ca.clone());
    }
    let ca = std::sync::Arc::new(authca::Ca::load_or_create(&netpol_ca_dir())?);
    *g = Some(ca.clone());
    Ok(ca)
}

fn netpol_ca_dir() -> std::path::PathBuf {
    std::env::var("MACHINA_NETPOL_CA_DIR")
        .unwrap_or_else(|_| "/var/lib/machina/netpol-ca".into())
        .into()
}

/// The evidence-signing certificate (issued by the netpol CA) and the CA.
pub fn evidence_signer() -> anyhow::Result<(authca::DocSigner, String)> {
    let ca = netpol_ca()?;
    Ok((ca.doc_signer(&netpol_ca_dir())?, ca.cert_pem.clone()))
}

/// Give the host's bpfd a certificate for bpfd-to-bpfd authentication,
/// re-issued past half its lifetime. The key never leaves the host.
async fn ensure_auth_cert(h: &HostRef) -> anyhow::Result<bool> {
    let id: VmAuthIdentity = serde_json::from_value(bpf::call(h, &Request::VmAuthIdentity).await?)?;
    let fresh = id.cert.as_ref().is_some_and(|c| {
        c.host_id == h.id && c.not_after - authca::unix_now() > authca::HOST_CERT_SECS / 2
    });
    if fresh {
        return Ok(false);
    }
    let ca = netpol_ca()?;
    let (cert_pem, not_after) = ca.sign_host(&id.csr, &h.id)?;
    bpf::call(
        h,
        &Request::VmAuthCert {
            host_id: h.id.clone(),
            ca_pem: ca.cert_pem.clone(),
            cert_pem,
            not_after,
        },
    )
    .await?;
    Ok(true)
}

async fn sync_host(pool: &DbPool, fleet: &Fleet, h: &HostRef, force: bool) -> HostSync {
    let c = fleet.compile(Some(&h.id));
    let mut state = c.state;
    let empty = fleet.policies.is_empty();
    let mut out = HostSync {
        host_id: h.id.clone(),
        hostname: h.hostname.clone(),
        ok: true,
        pushed: false,
        error: None,
        vms: state.vms.len(),
        rules: state.policy.len(),
        peers: state.peers.len(),
        warnings: c.warnings,
    };
    if empty && !previously_synced(pool, &h.id).await {
        return out;
    }
    if netpol::uses_authentication(&fleet.policies) {
        match ensure_auth_cert(h).await {
            Ok(true) => {
                tracing::info!(host = %h.hostname, "issued VM network policy host certificate")
            }
            Ok(false) => {}
            Err(e) => out
                .warnings
                .push(format!("mutual authentication certificate: {e:#}")),
        }
    }
    if empty {
        state.owner = String::new();
        state.flow_log = false;
    } else {
        state.owner = OWNER.into();
        state.node_is_host = true;
    }
    let v = serde_json::to_value(&state).unwrap_or_default();
    let digest = hash_value(&v);
    let unchanged = LAST_PUSH
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .get(&h.id)
        == Some(&digest);
    if unchanged && !force {
        return out;
    }
    match bpf::call(h, &Request::VmEdgeSync { state }).await {
        Ok(_) => {
            out.pushed = true;
            LAST_PUSH
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_with(HashMap::new)
                .insert(h.id.clone(), digest);
        }
        Err(e) => {
            out.ok = false;
            out.error = Some(format!("{e:#}"));
        }
    }
    if empty && out.ok {
        let _ = crate::db::query("DELETE FROM vm_netpol_host_status WHERE host_id = ?")
            .bind(&h.id)
            .execute(pool)
            .await;
    } else {
        record(pool, &out).await;
    }
    out
}

/// Compile and push to every online host.
pub async fn reconcile(pool: &DbPool, force: bool) -> Vec<HostSync> {
    let hosts = bpf::online_hosts(pool).await;
    refresh_node_addrs(&hosts).await;
    let fleet = Fleet::load(pool).await;
    *EGRESS_GAPS.lock().unwrap_or_else(|e| e.into_inner()) = fleet.egress_gaps();
    let mut out = Vec::new();
    for h in &hosts {
        out.push(sync_host(pool, &fleet, h, force).await);
    }
    reconcile_egress(&fleet, &hosts).await;
    super::vm_overlay::reconcile(pool, &hosts, &fleet.host_addrs, &fleet.vms).await;
    out
}

/// Hosts add missing egress IPs to their uplink unless
/// `MACHINA_NETPOL_EGRESS_MANAGE=0`.
fn egress_manage() -> bool {
    !matches!(
        std::env::var("MACHINA_NETPOL_EGRESS_MANAGE").as_deref(),
        Ok("0" | "false" | "no" | "off")
    )
}

/// Push each host its project egress SNAT rules. Once every host holds an
/// empty set and no project has an egress IP, nothing is sent until a
/// project setting changes.
async fn reconcile_egress(fleet: &Fleet, hosts: &[HostRef]) {
    let wanted = fleet.projects.iter().any(|p| !p.egress_ips.is_empty());
    if !wanted && EGRESS_IDLE.load(Ordering::Relaxed) {
        return;
    }
    let mut all_ok = true;
    for h in hosts {
        let rules = tenant::snat_rules(&fleet.projects, &fleet.vms, &h.id, &h.hostname);
        let none = rules.is_empty();
        let req = Request::VmEgressSnatSet {
            config: VmEgressSnat {
                rules,
                exclude: None,
                manage_addresses: Some(egress_manage()),
                interface: std::env::var("MACHINA_NETPOL_EGRESS_INTERFACE")
                    .ok()
                    .filter(|s| !s.is_empty()),
            },
        };
        match bpf::call(h, &req).await {
            Ok(_) => {}
            // A bpfd without egress support has nothing to clear.
            Err(e) if none && format!("{e:#}").contains("unknown variant") => {}
            Err(e) => {
                all_ok = false;
                tracing::warn!(host = %h.hostname, "egress SNAT push: {e:#}");
            }
        }
    }
    if !wanted && all_ok {
        EGRESS_IDLE.store(true, Ordering::Relaxed);
    }
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(TICK_SECS));
        let mut n: u32 = 0;
        let mut alerts_seen: Option<String> = None;
        let mut gaps_seen: BTreeSet<tenant::EgressGap> = BTreeSet::new();
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            n = n.wrapping_add(1);
            for name in reap_expired(&state.pool).await {
                tracing::info!(policy = %name, "temporary VM network policy expired");
                state.emit_event(
                    "netpol.jit",
                    format!("temporary policy {name} expired and was removed"),
                );
            }
            for r in reconcile(&state.pool, n.is_multiple_of(FORCE_EVERY)).await {
                if let Some(e) = r.error {
                    tracing::warn!(host = %r.hostname, "vm network policy sync: {e}");
                }
            }
            if n.is_multiple_of(THREAT_REFRESH_EVERY) {
                for f in refresh_due_threat_feeds(&state.pool).await {
                    for (h, r) in bpf::fan_out(&state.pool, &f.request()).await {
                        if let Err(e) = r {
                            tracing::warn!(host = %h.hostname, feed = %f.name, "threat feed push: {e:#}");
                        }
                    }
                    state.emit_event(
                        "netpol.threat",
                        format!(
                            "threat feed {} refreshed: {} domains",
                            f.name, f.domain_count
                        ),
                    );
                }
            }
            super::sg_enforce::renew(&state).await;
            reconcile_threat(&state.pool).await;
            forward_alerts(&state, &mut alerts_seen).await;
            egress_gap_events(&state, &mut gaps_seen);
            if n.is_multiple_of(FORCE_EVERY) {
                scheduled_evidence(&state).await;
            }
        }
    });
}

/// Scheduled evidence: `MACHINA_NETPOL_EVIDENCE_DIR` (default
/// `/var/lib/machina/netpol-evidence`), every
/// `MACHINA_NETPOL_EVIDENCE_EVERY_HOURS` (default 24, 0 = off), kept
/// `MACHINA_NETPOL_EVIDENCE_KEEP_DAYS` (default 90).
pub fn evidence_dir() -> std::path::PathBuf {
    std::env::var("MACHINA_NETPOL_EVIDENCE_DIR")
        .unwrap_or_else(|_| "/var/lib/machina/netpol-evidence".into())
        .into()
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

pub const EVIDENCE_PREFIX: &str = "segmentation-evidence-";

/// Stored reports, newest first: (file name, bytes, modified).
pub fn evidence_archive() -> Vec<(String, u64, std::time::SystemTime)> {
    let mut out: Vec<_> = std::fs::read_dir(evidence_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with(EVIDENCE_PREFIX) || !name.ends_with(".json") {
                return None;
            }
            let md = e.metadata().ok()?;
            Some((name, md.len(), md.modified().ok()?))
        })
        .collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.2));
    out
}

/// Write a report when the newest is older than the interval, then drop
/// reports past the retention.
async fn scheduled_evidence(state: &AppState) {
    let every = env_u64("MACHINA_NETPOL_EVIDENCE_EVERY_HOURS", 24);
    if every == 0 {
        return;
    }
    let keep = std::time::Duration::from_secs(
        env_u64("MACHINA_NETPOL_EVIDENCE_KEEP_DAYS", 90).max(1) * 86_400,
    );
    let now = std::time::SystemTime::now();
    let archive = evidence_archive();
    let due = archive.first().is_none_or(|(_, _, at)| {
        now.duration_since(*at).unwrap_or_default().as_secs() >= every * 3600
    });
    for (name, _, at) in &archive {
        if now.duration_since(*at).unwrap_or_default() > keep {
            let _ = std::fs::remove_file(evidence_dir().join(name));
        }
    }
    if !due {
        return;
    }
    let probes = netpol::evidence::default_probes();
    let e = match crate::api::vm_network_policies::build_evidence(state, "scheduled", &probes, None)
        .await
    {
        Ok(e) => e,
        Err(err) => {
            tracing::warn!("scheduled segmentation evidence: {err:?}");
            return;
        }
    };
    let dir = evidence_dir();
    let name = format!(
        "{EVIDENCE_PREFIX}{}.json",
        e.generated_at.replace([':', '-'], "")
    );
    let body = serde_json::to_string(&e).unwrap_or_default();
    let written = std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(dir.join(&name), body.as_bytes()));
    match written {
        Ok(()) => state.emit_event(
            "netpol.evidence",
            format!(
                "scheduled segmentation evidence {name} ({})",
                &e.digest[..16]
            ),
        ),
        Err(err) => tracing::warn!(dir = %dir.display(), "scheduled segmentation evidence: {err}"),
    }
}

/// A VM that lands on a host without its project's egress IP (or leaves
/// one) becomes an event, and so a webhook / SIEM record.
fn egress_gap_events(state: &AppState, seen: &mut BTreeSet<tenant::EgressGap>) {
    let now: BTreeSet<tenant::EgressGap> = EGRESS_GAPS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect();
    for g in now.difference(seen) {
        state.emit_event(
            "netpol.egress_gap",
            format!(
                "VM {} of project {} runs on {}, which has none of the project's egress IPs: {}",
                g.vm,
                g.project,
                g.host,
                if g.blocked {
                    "its internet egress is blocked"
                } else {
                    "it leaves with the host's address"
                }
            ),
        );
    }
    for g in seen.difference(&now) {
        state.emit_event(
            "netpol.egress_gap",
            format!(
                "VM {} of project {} no longer lacks an egress IP on {}",
                g.vm, g.project, g.host
            ),
        );
    }
    *seen = now;
}

/// Detection alerts newer than `seen` become events (and so webhooks / SIEM).
/// The first pass only records where the hosts are.
async fn forward_alerts(state: &AppState, seen: &mut Option<String>) {
    let alerts = crate::api::vm_network_policies::fleet_alerts(state, 200).await;
    let newest = alerts.first().map(|a| a.ts.clone());
    if let Some(last) = seen.as_ref() {
        for a in alerts.iter().rev().filter(|a| a.ts > *last) {
            state.emit_event(
                "netpol.alert",
                format!(
                    "[{}] {} on {}: {}",
                    a.severity,
                    a.kind,
                    a.host.as_deref().unwrap_or("?"),
                    a.detail
                ),
            );
            propose_quarantine(state, a).await;
        }
    }
    if newest.is_some() || seen.is_none() {
        *seen = Some(newest.unwrap_or_default());
    }
}

/// A high-severity scan from a VM becomes a pending `vm.quarantine` action
/// (one per VM at a time); nothing happens until someone approves it.
async fn propose_quarantine(state: &AppState, a: &machina_bpf::api::VmFlowAlert) {
    let Some(vm) = a.src_vm.as_deref() else {
        return;
    };
    if a.severity != "high"
        || !matches!(
            a.kind.as_str(),
            "port_scan" | "host_sweep" | "threat_domain"
        )
    {
        return;
    }
    let pending: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM ai_actions WHERE status = 'pending' AND action_type = 'vm.quarantine' AND json_extract(object_ref, '$.vm') = ?",
    )
    .bind(vm)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(1);
    if pending > 0 {
        return;
    }
    let host = a.host.clone().unwrap_or_default();
    let body = crate::engine::ai::actions::CreateActionBody {
        action_type: "vm.quarantine".into(),
        label: format!("Quarantine {vm} for 1 hour"),
        review: format!(
            "{} from {vm} on {host}: {}. Cuts every flow of the VM, including open ones, except SSH from its host; lifts itself after an hour.",
            a.kind, a.detail
        ),
        risk: "Disconnects the VM".into(),
        object_ref: serde_json::json!({
            "vm": vm,
            "host": a.host,
            "secs": 3600,
            "allow_host_ssh": true,
            "reason": format!("{}: {}", a.kind, a.detail),
        }),
        source: "netpol".into(),
    };
    if let Err(e) =
        crate::engine::ai::actions::create_action(&state.pool, &body, "netpol-detector").await
    {
        tracing::warn!("quarantine proposal for {vm}: {e:#}");
    }
}

// ---- DNS threat feeds -------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ThreatFeedRow {
    pub name: String,
    pub source: String,
    pub block: bool,
    #[serde(skip)]
    pub domains: Vec<String>,
    pub domain_count: usize,
    pub updated_by: String,
    pub updated_at: String,
}

impl ThreatFeedRow {
    pub fn request(&self) -> Request {
        Request::VmThreatFeedSet {
            name: self.name.clone(),
            source: self.source.clone(),
            block: self.block,
            domains: self.domains.clone(),
        }
    }
}

pub async fn threat_feeds(pool: &DbPool) -> Vec<ThreatFeedRow> {
    let rows: Vec<(String, String, bool, String, String, String)> = crate::db::query_as(
        "SELECT name, source, block, domains, updated_by, updated_at FROM vm_netpol_threat_feeds ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    rows.into_iter()
        .map(|(name, source, block, d, updated_by, updated_at)| {
            let domains: Vec<String> = serde_json::from_str(&d).unwrap_or_default();
            ThreatFeedRow {
                name,
                source,
                block,
                domain_count: domains.len(),
                domains,
                updated_by,
                updated_at,
            }
        })
        .collect()
}

pub async fn threat_feed_put(
    pool: &DbPool,
    name: &str,
    source: &str,
    block: bool,
    domains: &[String],
    by: &str,
) -> anyhow::Result<()> {
    crate::db::query(
        "INSERT INTO vm_netpol_threat_feeds (name, source, block, domains, updated_by, updated_at)
         VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(name) DO UPDATE SET source = excluded.source, block = excluded.block,
           domains = excluded.domains, updated_by = excluded.updated_by, updated_at = excluded.updated_at",
    )
    .bind(name)
    .bind(source)
    .bind(block)
    .bind(serde_json::to_string(domains)?)
    .bind(by)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn threat_feed_delete(pool: &DbPool, name: &str) -> anyhow::Result<bool> {
    let r = crate::db::query("DELETE FROM vm_netpol_threat_feeds WHERE name = ?")
        .bind(name)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn fetch_feed(url: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let mut resp = client.get(url).send().await?.error_for_status()?;
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        body.extend_from_slice(&chunk);
        if body.len() > netpol::threat::MAX_FEED_BYTES {
            anyhow::bail!(
                "feed {url} is larger than {} MiB",
                netpol::threat::MAX_FEED_BYTES >> 20
            );
        }
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// What a host must change to match `want`: feeds to (re)send, names to drop.
fn threat_diff<'a>(
    want: &'a [ThreatFeedRow],
    have: &[machina_bpf::api::VmThreatFeed],
) -> (Vec<&'a ThreatFeedRow>, Vec<String>) {
    let send = want
        .iter()
        .filter(|w| {
            !have.iter().any(|h| {
                h.name == w.name
                    && h.source == w.source
                    && h.block == w.block
                    && h.domains == w.domain_count
            })
        })
        .collect();
    let drop = have
        .iter()
        .filter(|h| !want.iter().any(|w| w.name == h.name))
        .map(|h| h.name.clone())
        .collect();
    (send, drop)
}

/// With any fleet feeds, every online host carries exactly those.
pub async fn reconcile_threat(pool: &DbPool) {
    let want = threat_feeds(pool).await;
    if want.is_empty() {
        return;
    }
    for h in bpf::online_hosts(pool).await {
        let have: machina_bpf::api::VmThreatStatus =
            match bpf::call(&h, &Request::VmThreatFeeds).await {
                Ok(v) => serde_json::from_value(v).unwrap_or_default(),
                Err(e) => {
                    tracing::debug!(host = %h.hostname, "threat feeds: {e:#}");
                    continue;
                }
            };
        let (send, drop) = threat_diff(&want, &have.feeds);
        for f in send {
            if let Err(e) = bpf::call(&h, &f.request()).await {
                tracing::warn!(host = %h.hostname, feed = %f.name, "threat feed push: {e:#}");
            }
        }
        for name in drop {
            if let Err(e) = bpf::call(&h, &Request::VmThreatFeedRemove { name: name.clone() }).await
            {
                tracing::warn!(host = %h.hostname, feed = %name, "threat feed remove: {e:#}");
            }
        }
    }
}

/// Refetch URL feeds older than [`netpol::threat::REFRESH_SECS`]; returns
/// the refreshed names (pushed to the hosts by the caller).
pub async fn refresh_due_threat_feeds(pool: &DbPool) -> Vec<ThreatFeedRow> {
    let cutoff = (chrono::Utc::now()
        - chrono::Duration::seconds(netpol::threat::REFRESH_SECS as i64))
    .format("%Y-%m-%d %H:%M:%S")
    .to_string();
    let mut out = Vec::new();
    for f in threat_feeds(pool).await {
        if f.source.is_empty() || f.updated_at > cutoff {
            continue;
        }
        match refresh_threat_feed(pool, &f, "threat-feed-refresh").await {
            Ok(r) => out.push(r),
            Err(e) => tracing::warn!(feed = %f.name, "threat feed refresh: {e:#}"),
        }
    }
    out
}

pub async fn refresh_threat_feed(
    pool: &DbPool,
    f: &ThreatFeedRow,
    by: &str,
) -> anyhow::Result<ThreatFeedRow> {
    let body = fetch_feed(&f.source).await?;
    let b = netpol::threat::FeedBody {
        url: Some(f.source.clone()),
        block: f.block,
        ..Default::default()
    };
    let domains = b.domains(Some(&body)).map_err(|e| anyhow::anyhow!(e))?;
    threat_feed_put(pool, &f.name, &f.source, f.block, &domains, by).await?;
    Ok(ThreatFeedRow {
        domain_count: domains.len(),
        domains,
        updated_by: by.to_string(),
        ..f.clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::test_support::{seed_host, test_state};

    fn nvm(name: &str, host: &str, project: &str, ip: &str) -> NetpolVm {
        NetpolVm {
            name: name.into(),
            host: Some(host.into()),
            project: Some(project.into()),
            labels: BTreeMap::new(),
            addresses: vec![ip.into()],
            ..Default::default()
        }
    }

    #[test]
    fn learned_addresses_join_identities_unless_contested() {
        let vms = vec![
            nvm("a", "h1", "p", "192.168.122.5"),
            nvm("b", "h1", "p", "192.168.122.6"),
            nvm("c", "h2", "p", "192.168.122.5"),
        ];
        let l = |v: &[(&str, &[&str])]| {
            v.iter()
                .map(|(vm, a)| (vm.to_string(), a.iter().map(|x| x.to_string()).collect()))
                .collect::<BTreeMap<String, Vec<String>>>()
        };
        let learned: BTreeMap<String, BTreeMap<String, Vec<String>>> = [
            (
                "h1".to_string(),
                l(&[
                    ("a", &["fd00::5", "192.168.122.6", "10.0.0.1", "10.9.9.9"]),
                    ("b", &["10.9.9.9", "fd00::6"]),
                    ("ghost", &["fd00::7"]),
                ]),
            ),
            (
                "h2".to_string(),
                l(&[("a", &["fd00::8"]), ("c", &["fd00::5"])]),
            ),
        ]
        .into_iter()
        .collect();
        let host_ips: BTreeSet<String> = ["10.0.0.1".to_string()].into_iter().collect();
        let out = merge_learned(vms, &learned, &host_ips);
        let addrs = |n: &str| out.iter().find(|v| v.name == n).unwrap().addresses.clone();
        assert_eq!(addrs("a"), ["192.168.122.5", "fd00::5"]);
        assert_eq!(addrs("b"), ["192.168.122.6", "fd00::6"]);
        assert_eq!(addrs("c"), ["192.168.122.5", "fd00::5"]);
    }

    fn two_host_fleet(vms: Vec<NetpolVm>) -> Fleet {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        Fleet {
            policies: vec![],
            vms,
            services: vec![],
            host_addrs: [("h1", "10.0.0.1"), ("h2", "10.0.0.2")]
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
            node_addrs: [
                (
                    "h1".to_string(),
                    s(&["10.0.0.1/24", "192.168.122.1/24", "172.16.5.1/24"]),
                ),
                ("h2".to_string(), s(&["10.0.0.2/24", "192.168.122.1/24"])),
            ]
            .into_iter()
            .collect(),
            projects: vec![],
            hostnames: BTreeMap::new(),
            overlay: false,
        }
    }

    #[test]
    fn node_addresses_split_into_host_and_remote_node() {
        let f = two_host_fleet(vec![]);
        let (own, remote) = f.node_identity_addresses(Some("h1"));
        assert_eq!(own[0], "10.0.0.1", "management address first");
        assert!(own.contains(&"172.16.5.1".to_string()));
        assert!(own.contains(&"192.168.122.1".to_string()));
        assert_eq!(remote, ["10.0.0.2"], "a shared bridge address is no node");
        let (own2, remote2) = f.node_identity_addresses(Some("h2"));
        assert_eq!(own2, ["10.0.0.2", "192.168.122.1"]);
        assert_eq!(remote2, ["10.0.0.1", "172.16.5.1"]);
        assert!(f.node_identity_addresses(None).0.is_empty());
        assert_eq!(f.all_host_addresses().len(), 4);
    }

    #[test]
    fn projects_across_per_host_nat_are_flagged() {
        let f = two_host_fleet(vec![
            nvm("a", "h1", "shop", "192.168.122.10"),
            nvm("b", "h2", "shop", "192.168.122.20"),
            nvm("c", "h1", "lab", "10.0.0.50"),
            nvm("d", "h2", "lab", "10.0.0.51"),
            nvm("e", "h1", "solo", "192.168.122.30"),
        ]);
        let spans = f.nat_spans();
        assert_eq!(
            spans,
            [NatSpan {
                project: "shop".into(),
                hosts: vec!["h1".into(), "h2".into()],
                subnets: vec!["192.168.122.0/24".into()],
            }],
            "a shared LAN (distinct host addresses) and a one-host project are fine"
        );
    }

    #[tokio::test]
    async fn load_balancers_are_services() {
        let (state, _rx) = test_state().await;
        let pool = &state.pool;
        let host = seed_host(pool, Uuid::from_u128(1)).await;
        crate::db::query("UPDATE hosts SET address = '192.0.2.10:50051' WHERE id = ?")
            .bind(host)
            .execute(pool)
            .await
            .unwrap();
        let (vm, lb) = (Uuid::from_u128(2), Uuid::from_u128(3));
        crate::db::query(
            "INSERT INTO vms (id, name, host_id, guest_ip) VALUES (?, 'web-1', ?, '10.0.0.5')",
        )
        .bind(vm)
        .bind(host)
        .execute(pool)
        .await
        .unwrap();
        crate::db::query(
            "INSERT INTO load_balancers (id, name, protocol, host_id, listener_port) VALUES (?, 'web-lb', 'tcp', ?, 8080)",
        )
        .bind(lb)
        .bind(host)
        .execute(pool)
        .await
        .unwrap();
        crate::db::query(
            "INSERT INTO lb_members (id, load_balancer_id, vm_id, port) VALUES (?, ?, ?, 80)",
        )
        .bind(Uuid::from_u128(4))
        .bind(lb)
        .bind(vm)
        .execute(pool)
        .await
        .unwrap();
        let s = services(pool).await;
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "web-lb");
        let eps: Vec<(&str, u16, u8)> = s[0]
            .endpoints
            .iter()
            .map(|e| (e.address.as_str(), e.port, e.proto))
            .collect();
        assert_eq!(eps, [("192.0.2.10", 8080, 6), ("10.0.0.5", 80, 6)]);
    }

    #[tokio::test]
    async fn scans_propose_one_quarantine() {
        let (state, _rx) = test_state().await;
        let alert = |kind: &str, severity: &str| machina_bpf::api::VmFlowAlert {
            kind: kind.into(),
            severity: severity.into(),
            src: "10.0.0.9".into(),
            src_vm: Some("np-bad".into()),
            host: Some("hv1".into()),
            detail: "probed 25 ports".into(),
            ..Default::default()
        };
        propose_quarantine(&state, &alert("new_peer", "low")).await;
        propose_quarantine(&state, &alert("port_scan", "high")).await;
        propose_quarantine(&state, &alert("host_sweep", "high")).await;
        let pending = crate::engine::ai::actions::list_pending(&state.pool)
            .await
            .unwrap();
        let q: Vec<_> = pending
            .iter()
            .filter(|a| a.action_type == "vm.quarantine")
            .collect();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].object_ref["vm"], "np-bad");
        assert_eq!(q[0].object_ref["allow_host_ssh"], true);
        let body: machina_bpf::api::VmQuarantineBody =
            serde_json::from_value(q[0].object_ref.clone()).unwrap();
        assert_eq!(body.secs, Some(3600));
    }

    #[tokio::test]
    async fn jit_needs_a_second_admin_and_expires() {
        use crate::api::vm_network_policies::JIT_ACTION;
        use crate::engine::ai::actions;
        let (state, _rx) = test_state().await;
        let pool = &state.pool;
        for (i, name) in ["np-a", "np-b"].iter().enumerate() {
            crate::db::query("INSERT INTO vms (id, name) VALUES (?, ?)")
                .bind(Uuid::from_u128(10 + i as u128))
                .bind(name)
                .execute(pool)
                .await
                .unwrap();
        }
        let req = netpol::jit::JitRequest {
            from: "np-a".into(),
            to: "np-b".into(),
            port: 22,
            secs: Some(60),
            ..Default::default()
        };
        let body = actions::CreateActionBody {
            action_type: JIT_ACTION.into(),
            label: "jit".into(),
            review: String::new(),
            risk: String::new(),
            object_ref: serde_json::to_value(&req).unwrap(),
            source: "netpol".into(),
        };
        let a = actions::create_action(pool, &body, "alice").await.unwrap();
        let user = |name: &str, role: &str| crate::auth::AuthUser {
            username: name.into(),
            role: role.into(),
            auth_source: None,
        };
        assert!(
            actions::approve_and_execute(&state, a.id, &user("alice", "admin"))
                .await
                .is_err()
        );
        assert!(
            actions::approve_and_execute(&state, a.id, &user("carol", "operator"))
                .await
                .is_err()
        );
        assert_eq!(
            actions::get_action(pool, a.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "pending",
            "refused approvals leave the request pending"
        );
        actions::approve_and_execute(&state, a.id, &user("bob", "admin"))
            .await
            .unwrap();
        let grants = netpol::jit::grants(&enabled_policies(pool).await, chrono::Utc::now());
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].granted_by, "bob");

        let mut p = enabled_policies(pool).await.remove(0);
        p.annotations.insert(
            netpol::jit::ANNOTATION_EXPIRES.into(),
            "2000-01-01T00:00:00Z".into(),
        );
        upsert(pool, &p, "bob").await.unwrap();
        assert!(
            enabled_policies(pool).await.is_empty(),
            "expired is not compiled"
        );
        assert_eq!(reap_expired(pool).await, [p.name]);
        assert!(policies(pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn threat_feeds_are_mirrored_by_name_and_shape() {
        let (state, _rx) = test_state().await;
        let pool = &state.pool;
        let d = vec!["a.example".to_string(), "b.example".to_string()];
        threat_feed_put(pool, "urlhaus", "", true, &d, "alice")
            .await
            .unwrap();
        let want = threat_feeds(pool).await;
        assert_eq!((want.len(), want[0].domain_count), (1, 2));
        let have = |block, domains| machina_bpf::api::VmThreatFeed {
            name: "urlhaus".into(),
            block,
            domains,
            ..Default::default()
        };
        let local = machina_bpf::api::VmThreatFeed {
            name: "local".into(),
            ..Default::default()
        };
        let (send, drop) = threat_diff(&want, &[have(true, 2), local]);
        assert!(send.is_empty());
        assert_eq!(drop, ["local"]);
        let (send, _) = threat_diff(&want, &[have(false, 2)]);
        assert_eq!(send.len(), 1, "block changed");
        let (send, _) = threat_diff(&want, &[have(true, 3)]);
        assert_eq!(send.len(), 1, "list changed");
        assert!(threat_feed_delete(pool, "urlhaus").await.unwrap());
        assert!(threat_feeds(pool).await.is_empty());
    }
}
