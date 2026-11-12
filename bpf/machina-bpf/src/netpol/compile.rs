// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Policies + VM inventory → one host's [`VmEdgeState`].
//!
//! Every VM gets its own identity (hash of its name), so selectors on any
//! label — including `machina.io/vm-name` — resolve exactly. Entries are
//! only emitted for subjects on the target host; peers span the fleet
//! (remote VMs are address-only peers).
//!
//! Cilium semantics kept:
//! * a VM selected by a spec with an `ingress`/`ingressDeny` (resp. egress)
//!   section is default-deny in that direction unless that spec sets
//!   `enableDefaultDeny.<dir>: false`;
//! * deny wins over allow, and applies whether or not the VM is default-deny;
//! * an L4-only rule allows every peer, an empty rule (`- {}`) allows none;
//! * `from/toRequires` narrows every allow rule of that direction to peers
//!   carrying the required labels;
//! * CIDR rules cover narrower prefixes from other rules (minus `except`),
//!   and `world` covers every CIDR identity; managed VMs and hypervisors
//!   always resolve to their own identity first;
//! * `toServices` allows the frontends and backends of the selected
//!   services (Fleet Cloud load balancers, Kubernetes services) on their
//!   ports, narrowed by `toPorts` when the rule has any.

use std::collections::{BTreeMap, BTreeSet};

use machina_bpf_common::{IDENTITY_HOST, IDENTITY_WORLD, V4_MAPPED_PREFIX_BITS};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::l7::{self, L7Rules};
use super::{
    effective_labels, fqdn, icmp_type_num, reserved_entity, selector_matches, selector_string,
    VmNetworkPolicy,
};
use super::{LABEL_PORT_PREFIX, UNSUPPORTED_ENTITIES};
use crate::api::{
    VmEdgeFqdnRule, VmEdgeL7Rule, VmEdgePeer, VmEdgeRule, VmEdgeState, VmEdgeVm, AUTH_ALWAYS_FAIL,
    AUTH_REQUIRED,
};
use crate::policy::{parse_prefix, Prefix};

/// Other hypervisors (`remote-node`).
pub const IDENTITY_REMOTE_NODE: u32 = 3;
pub use machina_bpf_common::IDENTITY_THREAT;

const IPPROTO_ICMP: u8 = 1;
const IPPROTO_TCP: u8 = 6;
const IPPROTO_UDP: u8 = 17;
const IPPROTO_ICMPV6: u8 = 58;
const IPPROTO_SCTP: u8 = 132;

/// One VM in the fleet inventory.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct NetpolVm {
    pub name: String,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub addresses: Vec<String>,
    /// Mbit/s from the VM; 0 = unlimited (from `vm_qos_bindings`).
    #[serde(default)]
    pub egress_mbps: u32,
    /// Mbit/s towards the VM; 0 = unlimited.
    #[serde(default)]
    pub ingress_mbps: u32,
    /// Packets/s each direction; 0 = unlimited.
    #[serde(default)]
    pub pps: u32,
}

/// A service `toServices` can select: a Fleet Cloud load balancer
/// (controller) or a Kubernetes service (daemon).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct NetpolService {
    pub name: String,
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// Frontends (cluster IP, VIP, listener) and backends.
    #[serde(default)]
    pub endpoints: Vec<ServiceEndpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ServiceEndpoint {
    pub address: String,
    /// 0 = any port.
    #[serde(default)]
    pub port: u16,
    /// IP protocol number; 0 = any.
    #[serde(default)]
    pub proto: u8,
}

pub struct Inputs<'a> {
    pub policies: &'a [VmNetworkPolicy],
    pub vms: &'a [NetpolVm],
    pub services: &'a [NetpolService],
    /// Emit entries for VMs on this host only; `None` = every VM is local.
    pub host: Option<&'a str>,
    /// This hypervisor's own addresses (`host`).
    pub host_addresses: &'a [String],
    /// Other hypervisors' addresses (`remote-node`).
    pub remote_node_addresses: &'a [String],
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct EndpointInfo {
    pub name: String,
    #[serde(default)]
    pub host: Option<String>,
    pub identity: u32,
    pub labels: BTreeMap<String, String>,
    pub addresses: Vec<String>,
    pub ingress_enforced: bool,
    pub egress_enforced: bool,
    /// Policies whose selector picks this VM.
    pub policies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SelectorInfo {
    pub policy: String,
    /// `spec[0].endpointSelector`, `spec[0].ingress[1].fromEndpoints[0]`, ...
    pub path: String,
    pub selector: String,
    pub vms: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct Compiled {
    pub state: VmEdgeState,
    pub warnings: Vec<String>,
    pub endpoints: Vec<EndpointInfo>,
    pub selectors: Vec<SelectorInfo>,
}

fn fnv32(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// VM identities live in [1024, 2^31); CIDR identities have the top bit set.
pub fn vm_identity(name: &str) -> u32 {
    fnv32(&format!("vm|{name}")) % (0x7fff_0000 - 1024) + 1024
}

pub fn cidr_identity(p: &Prefix) -> u32 {
    0x8000_0000 | (fnv32(&p.to_display()) & 0x7fff_ffff)
}

fn mask(mut addr: [u8; 16], bits: u32) -> [u8; 16] {
    for (i, b) in addr.iter_mut().enumerate() {
        let start = i as u32 * 8;
        if start >= bits {
            *b = 0;
        } else if start + 8 > bits {
            *b &= 0xffu8 << (8 - (bits - start));
        }
    }
    addr
}

/// `inner` lies within `outer`.
fn contains(outer: &Prefix, inner: &Prefix) -> bool {
    inner.bits >= outer.bits && mask(inner.addr, outer.bits) == outer.addr
}

fn is_v4(p: &Prefix) -> bool {
    p.bits >= V4_MAPPED_PREFIX_BITS
        && p.addr[..10].iter().all(|b| *b == 0)
        && p.addr[10] == 0xff
        && p.addr[11] == 0xff
}

/// `l7`: index into [`Ctx::l7`] when the `toPorts` entry has L7 rules.
#[derive(Debug, Clone, PartialEq)]
enum PortSpec {
    /// proto 0 = any; port 0 = any; ICMP port = type + 1.
    Num {
        proto: u8,
        port: u16,
        end: u16,
        l7: Option<usize>,
    },
    Named {
        proto: u8,
        name: String,
        l7: Option<usize>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    subject: u32,
    peer: u32,
    egress: bool,
    proto: u8,
    port: u16,
    port_end: u16,
    deny: bool,
    auth: u8,
    l7: bool,
}

struct Vm<'a> {
    vm: &'a NetpolVm,
    labels: BTreeMap<String, String>,
    id: u32,
    local: bool,
}

struct Ctx<'a> {
    vms: Vec<Vm<'a>>,
    prefixes: BTreeMap<Prefix, u32>,
    warnings: Vec<String>,
    entries: BTreeMap<Key, String>,
    groups: Vec<&'a VmNetworkPolicy>,
    services: &'a [NetpolService],
    /// Exact addresses of VMs and hypervisors → identity.
    addr_ids: BTreeMap<Prefix, u32>,
    l7: Vec<L7Rules>,
    l7_out: BTreeSet<VmEdgeL7Rule>,
}

fn host_prefix(a: &str) -> Option<Prefix> {
    parse_prefix(a.split('/').next()?).ok()
}

/// Services a `toServices` entry selects (no namespace = any namespace).
fn service_selected(entry: &Value, s: &NetpolService) -> bool {
    let ns_ok = |spec: &Value| spec["namespace"].as_str().is_none_or(|n| n == s.namespace);
    if let Some(k) = entry.get("k8sService") {
        k["serviceName"].as_str() == Some(s.name.as_str()) && ns_ok(k)
    } else if let Some(k) = entry.get("k8sServiceSelector") {
        selector_matches(&k["selector"], &s.labels) && ns_ok(k)
    } else {
        false
    }
}

/// A rule port narrowed to one service endpoint port; None when disjoint.
fn port_for_endpoint(p: &PortSpec, proto: u8, port: u16) -> Option<PortSpec> {
    match p {
        PortSpec::Num {
            proto: pp,
            port: lo,
            end,
            l7,
        } => {
            if *pp != 0 && proto != 0 && *pp != proto {
                return None;
            }
            if port == 0 {
                return Some(p.clone());
            }
            let hi = (*end).max(*lo);
            if *lo != 0 && !(*lo..=hi).contains(&port) {
                return None;
            }
            Some(PortSpec::Num {
                proto: if *pp != 0 { *pp } else { proto },
                port,
                end: 0,
                l7: *l7,
            })
        }
        PortSpec::Named { .. } => Some(p.clone()),
    }
}

/// CiliumCIDRGroups a `toGroups` / `fromGroups` entry selects (any provider:
/// there is no cloud API on a hypervisor, groups are CiliumCIDRGroup objects).
fn group_selected(entry: &Value, g: &VmNetworkPolicy) -> bool {
    let Some(spec) = entry.get("aws").or_else(|| entry.get("machina")) else {
        return false;
    };
    let strs = |k: &str| -> Vec<&str> { arr(spec, k).iter().filter_map(Value::as_str).collect() };
    let names: Vec<&str> = [strs("names"), strs("securityGroupsNames")].concat();
    let ids = strs("securityGroupsIds");
    let id = g.annotations.get("machina.io/group-id").map(String::as_str);
    let named = names.is_empty() && ids.is_empty()
        || names.contains(&g.name.as_str())
        || ids.iter().any(|i| *i == g.name || Some(*i) == id);
    let labels_ok = spec
        .get("labels")
        .and_then(Value::as_object)
        .is_none_or(|m| {
            m.iter()
                .all(|(k, v)| g.labels.get(k).map(String::as_str) == v.as_str())
        });
    let region_ok = spec["region"]
        .as_str()
        .is_none_or(|r| r.is_empty() || g.labels.get("machina.io/region").is_none_or(|x| x == r));
    named && labels_ok && region_ok
}

struct Dir {
    egress: bool,
    allow: &'static str,
    deny: &'static str,
    pre: &'static str,
}

const DIRS: [Dir; 2] = [
    Dir {
        egress: false,
        allow: "ingress",
        deny: "ingressDeny",
        pre: "from",
    },
    Dir {
        egress: true,
        allow: "egress",
        deny: "egressDeny",
        pre: "to",
    },
];

fn arr<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn spec_paths(p: &VmNetworkPolicy) -> impl Iterator<Item = (String, &Value)> {
    let one = p.specs.len() == 1;
    p.specs.iter().enumerate().map(move |(i, s)| {
        (
            if one {
                "spec".to_string()
            } else {
                format!("specs[{i}]")
            },
            s,
        )
    })
}

impl<'a> Ctx<'a> {
    fn vm_by_id(&self, id: u32) -> Option<&Vm<'a>> {
        self.vms.iter().find(|v| v.id == id)
    }

    fn all_vm_ids(&self) -> BTreeSet<u32> {
        self.vms.iter().map(|v| v.id).collect()
    }

    fn select(&self, sel: &Value) -> BTreeSet<u32> {
        self.vms
            .iter()
            .filter(|v| selector_matches(sel, &v.labels))
            .map(|v| v.id)
            .collect()
    }

    fn world(&self, family: Option<bool>) -> BTreeSet<u32> {
        let mut s: BTreeSet<u32> = [IDENTITY_WORLD].into();
        s.extend(
            self.prefixes
                .iter()
                .filter(|(p, _)| family.is_none_or(|v4| is_v4(p) == v4))
                .map(|(_, id)| *id),
        );
        s
    }

    fn entity(&mut self, e: &str, ctx: &str) -> BTreeSet<u32> {
        match e {
            "all" => [0].into(),
            "world" | "unmanaged" => self.world(None),
            "world-ipv4" => self.world(Some(true)),
            "world-ipv6" => self.world(Some(false)),
            "host" => [IDENTITY_HOST].into(),
            "remote-node" => [IDENTITY_REMOTE_NODE].into(),
            "cluster" | "fleet" => {
                let mut s = self.all_vm_ids();
                s.extend([IDENTITY_HOST, IDENTITY_REMOTE_NODE]);
                s
            }
            other => {
                if UNSUPPORTED_ENTITIES.contains(&other) {
                    self.warnings.push(format!(
                        "{ctx}: entity `{other}` matches nothing on the VM edge"
                    ));
                }
                BTreeSet::new()
            }
        }
    }

    fn cidr_peers(&self, cidr: &str, except: &[Value]) -> BTreeSet<u32> {
        let Ok(q) = parse_prefix(cidr) else {
            return BTreeSet::new();
        };
        let ex: Vec<Prefix> = except
            .iter()
            .filter_map(Value::as_str)
            .filter_map(|e| parse_prefix(e).ok())
            .collect();
        self.prefixes
            .iter()
            .filter(|(p, _)| contains(&q, p) && !ex.iter().any(|e| contains(e, p)))
            .map(|(_, id)| *id)
            .collect()
    }

    /// Peers of one rule and whether it named any peer at all.
    fn rule_peers(&mut self, rule: &Value, d: &Dir, ctx: &str) -> (BTreeSet<u32>, bool) {
        let mut peers = BTreeSet::new();
        let mut named = false;
        let key = |s: &str| format!("{}{s}", d.pre);
        for sel in arr(rule, &key("Endpoints")) {
            named = true;
            match reserved_entity(sel) {
                Some(e) => peers.extend(self.entity(&e, ctx)),
                None => peers.extend(self.select(sel)),
            }
        }
        for c in arr(rule, &key("CIDR")).iter().filter_map(Value::as_str) {
            named = true;
            peers.extend(self.cidr_peers(c, &[]));
        }
        for set in arr(rule, &key("CIDRSet")) {
            named = true;
            if let Some(c) = set["cidr"].as_str() {
                peers.extend(self.cidr_peers(c, arr(set, "except")));
            }
            if let Some(g) = set["cidrGroupRef"].as_str() {
                match self
                    .groups
                    .iter()
                    .find(|p| p.name == g)
                    .map(|p| p.group_cidrs())
                {
                    Some(cidrs) => {
                        for c in cidrs {
                            peers.extend(self.cidr_peers(&c, arr(set, "except")));
                        }
                    }
                    None => self.warnings.push(format!(
                        "{ctx}: CiliumCIDRGroup `{g}` does not exist; it matches nothing"
                    )),
                }
            }
        }
        for entry in arr(rule, &key("Groups")) {
            named = true;
            let cidrs: Vec<String> = self
                .groups
                .iter()
                .filter(|g| group_selected(entry, g))
                .flat_map(|g| g.group_cidrs())
                .collect();
            if cidrs.is_empty() {
                self.warnings.push(format!(
                    "{ctx}: {} selects no CiliumCIDRGroup",
                    key("Groups")
                ));
            }
            for c in cidrs {
                peers.extend(self.cidr_peers(&c, &[]));
            }
        }
        for e in arr(rule, &key("Entities")).iter().filter_map(Value::as_str) {
            named = true;
            peers.extend(self.entity(e, ctx));
        }
        if d.egress && !arr(rule, "toFQDNs").is_empty() {
            named = true;
        }
        if d.egress && !arr(rule, "toServices").is_empty() {
            named = true;
        }
        let k = key("Nodes");
        if !arr(rule, &k).is_empty() {
            named = true;
            self.warnings.push(format!(
                "{ctx}: {k} is not enforced natively yet; it matches nothing"
            ));
        }
        if !named && (!arr(rule, "toPorts").is_empty() || !arr(rule, "icmps").is_empty()) {
            peers.insert(0);
        }
        (peers, named)
    }

    /// (peer, proto, port) of the service endpoints a rule's `toServices`
    /// selects.
    fn service_targets(&self, rule: &Value) -> BTreeSet<(u32, u8, u16)> {
        let mut out = BTreeSet::new();
        for entry in arr(rule, "toServices") {
            let hits = self.services.iter().filter(|s| service_selected(entry, s));
            for e in hits.flat_map(|s| &s.endpoints) {
                let Some(p) = host_prefix(&e.address) else {
                    continue;
                };
                if let Some(id) = self.addr_ids.get(&p).or_else(|| self.prefixes.get(&p)) {
                    out.insert((*id, e.proto, e.port));
                }
            }
        }
        out
    }

    fn named_port(&self, dest: Option<u32>, name: &str) -> BTreeSet<u16> {
        let label = format!("{LABEL_PORT_PREFIX}{name}");
        self.vms
            .iter()
            .filter(|v| dest.is_none_or(|d| d == v.id))
            .filter_map(|v| v.labels.get(&label).and_then(|p| p.parse::<u16>().ok()))
            .filter(|p| *p > 0)
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        subject: u32,
        peers: &BTreeSet<u32>,
        ports: &[PortSpec],
        egress: bool,
        deny: bool,
        auth: u8,
        source: &str,
    ) {
        for &peer in peers {
            for p in ports {
                let l7 = match p {
                    PortSpec::Num { l7, .. } | PortSpec::Named { l7, .. } => *l7,
                };
                let nums: Vec<(u8, u16, u16)> = match p {
                    PortSpec::Num {
                        proto, port, end, ..
                    } => vec![(*proto, *port, *end)],
                    PortSpec::Named { proto, name, .. } => {
                        let dest = if egress {
                            (peer != 0).then_some(peer)
                        } else {
                            Some(subject)
                        };
                        if dest.is_some_and(|d| self.vm_by_id(d).is_none()) {
                            self.warnings.push(format!(
                                "{source}: named port `{name}` towards a non-VM peer is skipped"
                            ));
                            continue;
                        }
                        let found = self.named_port(dest, name);
                        if found.is_empty() {
                            self.warnings.push(format!(
                                "{source}: no VM defines port `{name}` (label {LABEL_PORT_PREFIX}{name})"
                            ));
                        }
                        found.into_iter().map(|n| (*proto, n, 0)).collect()
                    }
                };
                for (proto, port, end) in nums {
                    let port_end = if end > port { end } else { 0 };
                    let l7 = l7.filter(|_| !deny);
                    let k = Key {
                        subject,
                        peer,
                        egress,
                        proto,
                        port,
                        port_end,
                        deny,
                        auth: if deny { 0 } else { auth },
                        l7: l7.is_some(),
                    };
                    self.entries.entry(k).or_insert_with(|| source.to_string());
                    if let Some(i) = l7 {
                        self.l7_out.insert(VmEdgeL7Rule {
                            subject_identity: subject,
                            peer_identity: peer,
                            egress,
                            proto,
                            port,
                            port_end,
                            rules: self.l7[i].clone(),
                            source: Some(source.to_string()),
                        });
                    }
                }
            }
        }
    }
}

fn protos(p: &str, port: u16) -> &'static [u8] {
    match p {
        "TCP" => &[IPPROTO_TCP],
        "UDP" => &[IPPROTO_UDP],
        "SCTP" => &[IPPROTO_SCTP],
        _ if port == 0 => &[0],
        _ => &[IPPROTO_TCP, IPPROTO_UDP, IPPROTO_SCTP],
    }
}

/// `toPorts` + `icmps` of a rule; neither = every port of every protocol.
/// L7 sections are appended to `l7s` and referenced by index.
fn rule_ports(rule: &Value, l7s: &mut Vec<L7Rules>) -> Vec<PortSpec> {
    let to_ports = arr(rule, "toPorts");
    let icmps = arr(rule, "icmps");
    if to_ports.is_empty() && icmps.is_empty() {
        return vec![PortSpec::Num {
            proto: 0,
            port: 0,
            end: 0,
            l7: None,
        }];
    }
    let mut out = Vec::new();
    for tp in to_ports {
        let l7 = l7::from_to_ports(tp).map(|r| {
            l7s.push(r);
            l7s.len() - 1
        });
        let ports = arr(tp, "ports");
        if ports.is_empty() {
            out.push(PortSpec::Num {
                proto: 0,
                port: 0,
                end: 0,
                l7,
            });
        }
        for p in ports {
            let proto = p["protocol"].as_str().unwrap_or("ANY");
            let port = match &p["port"] {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => "0".into(),
            };
            match port.parse::<u16>() {
                Ok(n) => {
                    let end = p["endPort"]
                        .as_u64()
                        .and_then(|e| u16::try_from(e).ok())
                        .unwrap_or(0);
                    for pr in protos(proto, n) {
                        out.push(PortSpec::Num {
                            proto: *pr,
                            port: n,
                            end,
                            l7,
                        });
                    }
                }
                Err(_) => {
                    let list: &[u8] = match proto {
                        "TCP" => &[IPPROTO_TCP],
                        "UDP" => &[IPPROTO_UDP],
                        "SCTP" => &[IPPROTO_SCTP],
                        _ => &[IPPROTO_TCP, IPPROTO_UDP, IPPROTO_SCTP],
                    };
                    out.extend(list.iter().map(|pr| PortSpec::Named {
                        proto: *pr,
                        name: port.clone(),
                        l7,
                    }));
                }
            }
        }
    }
    for ic in icmps {
        for f in arr(ic, "fields") {
            let v6 = f["family"].as_str() == Some("IPv6");
            if let Some(t) = icmp_type_num(&f["type"], v6) {
                out.push(PortSpec::Num {
                    proto: if v6 { IPPROTO_ICMPV6 } else { IPPROTO_ICMP },
                    port: t as u16 + 1,
                    end: 0,
                    l7: None,
                });
            }
        }
    }
    out
}

fn collect_prefixes(policies: &[VmNetworkPolicy]) -> BTreeMap<Prefix, u32> {
    let mut out = BTreeMap::new();
    let mut add = |s: &str| {
        if let Ok(p) = parse_prefix(s) {
            out.insert(p, cidr_identity(&p));
        }
    };
    for p in policies {
        if p.is_cidr_group() {
            p.group_cidrs().iter().for_each(|c| add(c));
            continue;
        }
        for spec in &p.specs {
            for sec in ["ingress", "egress", "ingressDeny", "egressDeny"] {
                for rule in arr(spec, sec) {
                    for k in ["fromCIDR", "toCIDR"] {
                        arr(rule, k)
                            .iter()
                            .filter_map(Value::as_str)
                            .for_each(&mut add);
                    }
                    for k in ["fromCIDRSet", "toCIDRSet"] {
                        for set in arr(rule, k) {
                            if let Some(c) = set["cidr"].as_str() {
                                add(c);
                            }
                            arr(set, "except")
                                .iter()
                                .filter_map(Value::as_str)
                                .for_each(&mut add);
                        }
                    }
                }
            }
        }
    }
    out
}

pub fn compile(inp: &Inputs) -> Compiled {
    let mut sorted: Vec<&NetpolVm> = inp.vms.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    sorted.dedup_by(|a, b| a.name == b.name);
    let mut used = BTreeSet::new();
    let mut vms = Vec::new();
    for vm in sorted {
        let mut id = vm_identity(&vm.name);
        while !used.insert(id) {
            id = if id + 1 >= 0x7fff_0000 { 1024 } else { id + 1 };
        }
        let local = inp.host.is_none_or(|h| vm.host.as_deref() == Some(h));
        vms.push(Vm {
            vm,
            labels: effective_labels(vm),
            id,
            local,
        });
    }
    let mut addr_ids = BTreeMap::new();
    for v in &vms {
        for p in v.vm.addresses.iter().filter_map(|a| host_prefix(a)) {
            addr_ids.insert(p, v.id);
        }
    }
    for (list, id) in [
        (inp.host_addresses, IDENTITY_HOST),
        (inp.remote_node_addresses, IDENTITY_REMOTE_NODE),
    ] {
        for p in list.iter().filter_map(|a| host_prefix(a)) {
            addr_ids.entry(p).or_insert(id);
        }
    }
    let mut prefixes = collect_prefixes(inp.policies);
    for e in inp.services.iter().flat_map(|s| &s.endpoints) {
        if let Some(p) = host_prefix(&e.address).filter(|p| !addr_ids.contains_key(p)) {
            prefixes.insert(p, cidr_identity(&p));
        }
    }
    let mut cx = Ctx {
        vms,
        prefixes,
        warnings: Vec::new(),
        entries: BTreeMap::new(),
        groups: inp.policies.iter().filter(|p| p.is_cidr_group()).collect(),
        services: inp.services,
        addr_ids,
        l7: Vec::new(),
        l7_out: BTreeSet::new(),
    };

    // Pass 1: subjects, default deny, requires, selector listings.
    let mut iso: BTreeMap<u32, (bool, bool)> = BTreeMap::new();
    let mut requires: BTreeMap<(u32, bool), Vec<Value>> = BTreeMap::new();
    let mut selected_by: BTreeMap<u32, BTreeSet<String>> = BTreeMap::new();
    let mut selectors = Vec::new();
    let mut specs: Vec<(String, String, &Value, BTreeSet<u32>)> = Vec::new();
    for p in inp.policies.iter().filter(|p| !p.is_cidr_group()) {
        for (sp, spec) in spec_paths(p) {
            let ctx = format!("{} {sp}", p.name);
            let Some(sel) = spec.get("endpointSelector") else {
                cx.warnings.push(format!(
                    "{ctx}: nodeSelector (host policy) is not enforced on the VM edge"
                ));
                continue;
            };
            if reserved_entity(sel).is_some() {
                cx.warnings.push(format!(
                    "{ctx}: endpointSelector on reserved: labels selects no VM"
                ));
                continue;
            }
            let subjects = cx.select(sel);
            selectors.push(SelectorInfo {
                policy: p.name.clone(),
                path: format!("{sp}.endpointSelector"),
                selector: selector_string(sel),
                vms: names(&cx, &subjects),
            });
            for s in &subjects {
                selected_by.entry(*s).or_default().insert(p.name.clone());
            }
            for d in &DIRS {
                let section = spec.get(d.allow).is_some_and(|x| !x.is_null())
                    || spec.get(d.deny).is_some_and(|x| !x.is_null());
                if section && spec["enableDefaultDeny"][d.allow].as_bool() != Some(false) {
                    for s in &subjects {
                        let e = iso.entry(*s).or_default();
                        if d.egress {
                            e.1 = true;
                        } else {
                            e.0 = true;
                        }
                    }
                }
                for (ri, rule) in arr(spec, d.allow).iter().enumerate() {
                    let req = arr(rule, &format!("{}Requires", d.pre));
                    for s in &subjects {
                        requires
                            .entry((*s, d.egress))
                            .or_default()
                            .extend(req.iter().cloned());
                    }
                    for (si, sel) in arr(rule, &format!("{}Endpoints", d.pre)).iter().enumerate() {
                        let ids = cx.select(sel);
                        selectors.push(SelectorInfo {
                            policy: p.name.clone(),
                            path: format!("{sp}.{}[{ri}].{}Endpoints[{si}]", d.allow, d.pre),
                            selector: selector_string(sel),
                            vms: names(&cx, &ids),
                        });
                    }
                }
                for rule in arr(spec, d.deny) {
                    if !arr(rule, &format!("{}Requires", d.pre)).is_empty() {
                        cx.warnings
                            .push(format!("{ctx}: {}Requires in {} is ignored", d.pre, d.deny));
                    }
                }
            }
            specs.push((p.name.clone(), sp, spec, subjects));
        }
    }

    for (pname, sp, spec, _) in &specs {
        for sec in ["egress", "egressDeny"] {
            for (ri, rule) in arr(spec, sec).iter().enumerate() {
                for (i, entry) in arr(rule, "toServices").iter().enumerate() {
                    if !inp.services.iter().any(|s| service_selected(entry, s)) {
                        cx.warnings.push(format!(
                            "{pname} {sp}.{sec}[{ri}]: toServices[{i}] selects no service"
                        ));
                    }
                }
            }
        }
    }

    // Pass 2: entries for local subjects.
    let mut fqdn: BTreeSet<VmEdgeFqdnRule> = BTreeSet::new();
    for (pname, sp, spec, subjects) in &specs {
        let local: Vec<u32> = subjects
            .iter()
            .copied()
            .filter(|s| cx.vm_by_id(*s).is_some_and(|v| v.local))
            .collect();
        if local.is_empty() {
            continue;
        }
        for d in &DIRS {
            for (deny, sec) in [(false, d.allow), (true, d.deny)] {
                for (ri, rule) in arr(spec, sec).iter().enumerate() {
                    let source = format!("{pname} {sp}.{sec}[{ri}]");
                    let (peers, _) = cx.rule_peers(rule, d, &source);
                    let ports = rule_ports(rule, &mut cx.l7);
                    let auth = match rule["authentication"]["mode"].as_str() {
                        Some("required") => AUTH_REQUIRED,
                        Some("test-always-fail") => AUTH_ALWAYS_FAIL,
                        _ => 0,
                    };
                    for s in &local {
                        let peers = if deny {
                            peers.clone()
                        } else {
                            narrow(&cx, &requires, *s, d.egress, &peers)
                        };
                        cx.emit(*s, &peers, &ports, d.egress, deny, auth, &source);
                    }
                    let svc = if d.egress {
                        cx.service_targets(rule)
                    } else {
                        BTreeSet::new()
                    };
                    let explicit =
                        !arr(rule, "toPorts").is_empty() || !arr(rule, "icmps").is_empty();
                    for (peer, proto, port) in &svc {
                        let svc_ports: Vec<PortSpec> = if explicit {
                            ports
                                .iter()
                                .filter_map(|p| port_for_endpoint(p, *proto, *port))
                                .collect()
                        } else {
                            vec![PortSpec::Num {
                                proto: *proto,
                                port: *port,
                                end: 0,
                                l7: None,
                            }]
                        };
                        let one = BTreeSet::from([*peer]);
                        for s in &local {
                            let peers = if deny {
                                one.clone()
                            } else {
                                narrow(&cx, &requires, *s, true, &one)
                            };
                            cx.emit(*s, &peers, &svc_ports, true, deny, auth, &source);
                        }
                    }
                    let fq = fqdn::selectors(arr(rule, "toFQDNs"));
                    if d.egress && !deny && !fq.is_empty() {
                        for s in &local {
                            if requires.get(&(*s, true)).is_some_and(|r| !r.is_empty()) {
                                cx.warnings
                                    .push(format!("{source}: toRequires excludes toFQDNs peers"));
                                continue;
                            }
                            for p in &ports {
                                let PortSpec::Num {
                                    proto,
                                    port,
                                    end,
                                    l7,
                                } = p
                                else {
                                    cx.warnings.push(format!(
                                        "{source}: named ports do not apply to toFQDNs"
                                    ));
                                    continue;
                                };
                                for n in &fq {
                                    fqdn.insert(VmEdgeFqdnRule {
                                        pattern: n.clone(),
                                        subject_identity: *s,
                                        proto: *proto,
                                        port: *port,
                                        port_end: if end > port { *end } else { 0 },
                                        source: Some(source.clone()),
                                        l7: l7.map(|i| cx.l7[i].clone()),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Deny wins: drop allow entries shadowed by an identical deny key.
    let denies: BTreeSet<Key> = cx.entries.keys().filter(|k| k.deny).copied().collect();
    cx.entries.retain(|k, _| {
        k.deny
            || !denies.contains(&Key {
                deny: true,
                auth: 0,
                l7: false,
                ..*k
            })
    });

    let mut state = VmEdgeState {
        flow_log: true,
        host_id: inp.host.unwrap_or_default().to_string(),
        fqdn: fqdn.into_iter().collect(),
        l7: std::mem::take(&mut cx.l7_out).into_iter().collect(),
        ..Default::default()
    };
    let mut endpoints = Vec::new();
    for v in &cx.vms {
        let (ing, eg) = iso.get(&v.id).copied().unwrap_or_default();
        endpoints.push(EndpointInfo {
            name: v.vm.name.clone(),
            host: v.vm.host.clone(),
            identity: v.id,
            labels: v.labels.clone(),
            addresses: v.vm.addresses.clone(),
            ingress_enforced: ing,
            egress_enforced: eg,
            policies: selected_by
                .get(&v.id)
                .map(|s| s.iter().cloned().collect())
                .unwrap_or_default(),
        });
        if v.local {
            state.vms.push(VmEdgeVm {
                name: v.vm.name.clone(),
                addresses: v.vm.addresses.clone(),
                isolate_ingress: ing,
                isolate_egress: eg,
                egress_mbps: v.vm.egress_mbps,
                ingress_mbps: v.vm.ingress_mbps,
                pps: v.vm.pps,
                identity: Some(v.id),
                labels: v.labels.clone(),
                ..Default::default()
            });
        } else {
            for a in &v.vm.addresses {
                state.peers.push(VmEdgePeer {
                    cidr: a.clone(),
                    identity: v.id,
                    name: v.vm.name.clone(),
                    host: v.vm.host.clone().unwrap_or_default(),
                });
            }
        }
    }
    for a in inp.host_addresses {
        state.peers.push(VmEdgePeer {
            cidr: a.clone(),
            identity: IDENTITY_HOST,
            name: "host".into(),
            ..Default::default()
        });
    }
    for a in inp.remote_node_addresses {
        state.peers.push(VmEdgePeer {
            cidr: a.clone(),
            identity: IDENTITY_REMOTE_NODE,
            name: "remote-node".into(),
            ..Default::default()
        });
    }
    for (p, id) in &cx.prefixes {
        let mut c = p.to_display();
        if !c.contains('/') {
            c.push_str(if is_v4(p) { "/32" } else { "/128" });
        }
        state.peers.push(VmEdgePeer {
            cidr: c.clone(),
            identity: *id,
            name: c,
            ..Default::default()
        });
    }
    for (k, source) in &cx.entries {
        state.policy.push(VmEdgeRule {
            egress: k.egress,
            proto: k.proto,
            port: k.port,
            port_end: k.port_end,
            deny: k.deny,
            subject_identity: Some(k.subject),
            peer_identity: Some(k.peer),
            source: Some(source.clone()),
            auth: k.auth,
            l7: k.l7,
            ..Default::default()
        });
    }
    let mut seen = BTreeSet::new();
    cx.warnings.retain(|w| seen.insert(w.clone()));
    Compiled {
        state,
        warnings: cx.warnings,
        endpoints,
        selectors,
    }
}

fn names(cx: &Ctx, ids: &BTreeSet<u32>) -> Vec<String> {
    cx.vms
        .iter()
        .filter(|v| ids.contains(&v.id))
        .map(|v| v.vm.name.clone())
        .collect()
}

/// Apply `from/toRequires` of `subject`: only VMs carrying every required
/// label set remain (non-VM peers drop out; "any" expands to those VMs).
fn narrow(
    cx: &Ctx,
    requires: &BTreeMap<(u32, bool), Vec<Value>>,
    subject: u32,
    egress: bool,
    peers: &BTreeSet<u32>,
) -> BTreeSet<u32> {
    let Some(req) = requires.get(&(subject, egress)).filter(|r| !r.is_empty()) else {
        return peers.clone();
    };
    let ok = |v: &Vm| req.iter().all(|s| selector_matches(s, &v.labels));
    if peers.contains(&0) {
        return cx.vms.iter().filter(|v| ok(v)).map(|v| v.id).collect();
    }
    peers
        .iter()
        .copied()
        .filter(|p| cx.vm_by_id(*p).is_some_and(ok))
        .collect()
}
