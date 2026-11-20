// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Kubernetes objects → [`CniState`]: label-derived identities, NetworkPolicy
//! allow tuples and service frontends. Pure functions over `kubectl -o json`.
//!
//! Identity = hash(namespace, sorted pod labels), so every pod a selector can
//! distinguish gets its own identity and policies compile to identity pairs.
//! Named ports resolve against the destination pod's container ports (the
//! subject for ingress, the peer for egress).
//! Dual-stack: pods carry every `podIPs` entry and services every
//! `clusterIPs` entry; NodePorts emit one frontend per `ipFamilies` family.
//! Limits: ipBlock `except` and named ports towards ipBlock peers
//! are not supported (reported in [`Compiled::warnings`]); overlapping
//! ipBlocks resolve by longest prefix.
//!
//! CiliumNetworkPolicy / CiliumClusterwideNetworkPolicy compile into the same
//! tuples when supplied (see [`cilium`]).

mod cilium;

use std::collections::{BTreeMap, BTreeSet};

use machina_bpf::api::{
    CniBackend, CniIdentity, CniPolicyEntry, CniService, CniState, CNI_STATE_VERSION,
};
use serde_json::Value;

pub const IPPROTO_TCP: u8 = 6;
pub const IPPROTO_UDP: u8 = 17;
const MAX_PORT_RANGE: u32 = 256;

#[derive(Debug, Clone, Default)]
pub struct Pod {
    pub namespace: String,
    /// Every pod address (`status.podIPs`, one per family).
    pub ips: Vec<String>,
    pub labels: BTreeMap<String, String>,
    /// Named container ports: (name, proto) → port.
    pub named_ports: BTreeMap<(String, u8), u16>,
}

#[derive(Debug, Default)]
pub struct Compiled {
    pub state: CniState,
    pub warnings: Vec<String>,
}

fn fnv32(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// Pod identities live in [1024, 2^31); ipBlock identities have the top bit set.
pub fn pod_identity(namespace: &str, labels: &BTreeMap<String, String>) -> u32 {
    let mut key = format!("{namespace}|");
    for (k, v) in labels {
        key.push_str(k);
        key.push('=');
        key.push_str(v);
        key.push(',');
    }
    fnv32(&key) % (0x7fff_0000 - 1024) + 1024
}

pub fn cidr_identity(cidr: &str) -> u32 {
    0x8000_0000 | (fnv32(cidr) & 0x7fff_ffff)
}

fn str_at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut cur = v;
    for p in path {
        cur = cur.get(*p)?;
    }
    cur.as_str()
}

fn labels_of(v: &Value) -> BTreeMap<String, String> {
    v.pointer("/metadata/labels")
        .and_then(|l| l.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Running, non-hostNetwork pods with at least one address.
pub fn pods(items: &[Value]) -> Vec<Pod> {
    items
        .iter()
        .filter(|p| p["spec"]["hostNetwork"] != true)
        .filter(|p| {
            !matches!(
                str_at(p, &["status", "phase"]),
                Some("Succeeded" | "Failed")
            )
        })
        .filter_map(|p| {
            let mut ips: Vec<String> = p["status"]["podIPs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|i| i["ip"].as_str().map(String::from))
                .collect();
            if ips.is_empty() {
                ips.push(str_at(p, &["status", "podIP"])?.to_string());
            }
            Some(Pod {
                namespace: str_at(p, &["metadata", "namespace"])
                    .unwrap_or("default")
                    .to_string(),
                ips,
                labels: labels_of(p),
                named_ports: named_ports_of(p),
            })
        })
        .collect()
}

fn named_ports_of(p: &Value) -> BTreeMap<(String, u8), u16> {
    let mut out = BTreeMap::new();
    for c in p["spec"]["containers"].as_array().into_iter().flatten() {
        for port in c["ports"].as_array().into_iter().flatten() {
            let (Some(name), Some(num)) = (port["name"].as_str(), port["containerPort"].as_u64())
            else {
                continue;
            };
            if let (Some(proto), Ok(n)) = (proto_num(port["protocol"].as_str()), u16::try_from(num))
            {
                out.insert((name.to_string(), proto), n);
            }
        }
    }
    out
}

/// Kubernetes label selector: `matchLabels` AND every `matchExpressions` term.
/// An empty selector `{}` matches everything.
pub fn selector_matches(sel: &Value, labels: &BTreeMap<String, String>) -> bool {
    if let Some(ml) = sel.get("matchLabels").and_then(|m| m.as_object()) {
        for (k, v) in ml {
            if labels.get(k).map(String::as_str) != v.as_str() {
                return false;
            }
        }
    }
    if let Some(exprs) = sel.get("matchExpressions").and_then(|m| m.as_array()) {
        for e in exprs {
            let key = e["key"].as_str().unwrap_or("");
            let values: Vec<&str> = e["values"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            let have = labels.get(key).map(String::as_str);
            let ok = match e["operator"].as_str().unwrap_or("") {
                "In" => have.is_some_and(|h| values.contains(&h)),
                "NotIn" => have.is_none_or(|h| !values.contains(&h)),
                "Exists" => have.is_some(),
                "DoesNotExist" => have.is_none(),
                _ => false,
            };
            if !ok {
                return false;
            }
        }
    }
    true
}

fn proto_num(p: Option<&str>) -> Option<u8> {
    match p.unwrap_or("TCP") {
        "TCP" => Some(IPPROTO_TCP),
        "UDP" => Some(IPPROTO_UDP),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
enum RulePort {
    /// (proto, port); (0, 0) = everything, (proto, 0) = every port of proto.
    Num(u8, u16),
    /// Resolved against the destination pod's container ports.
    Named(u8, String),
}

/// `ports` of a policy rule.
fn rule_ports(rule: &Value, warnings: &mut Vec<String>, ctx: &str) -> Vec<RulePort> {
    let Some(ports) = rule
        .get("ports")
        .and_then(|p| p.as_array())
        .filter(|p| !p.is_empty())
    else {
        return vec![RulePort::Num(0, 0)];
    };
    let mut out = Vec::new();
    for p in ports {
        let Some(proto) = proto_num(p["protocol"].as_str()) else {
            warnings.push(format!("{ctx}: protocol {} not supported", p["protocol"]));
            continue;
        };
        match &p["port"] {
            Value::Null => out.push(RulePort::Num(proto, 0)),
            Value::Number(n) => {
                let start = n.as_u64().unwrap_or(0) as u32;
                let end = p["endPort"].as_u64().map(|e| e as u32).unwrap_or(start);
                if end < start || end - start >= MAX_PORT_RANGE {
                    warnings.push(format!(
                        "{ctx}: port range {start}-{end} too wide (max {MAX_PORT_RANGE})"
                    ));
                    continue;
                }
                out.extend(
                    (start..=end)
                        .filter(|x| *x > 0 && *x <= 65535)
                        .map(|x| RulePort::Num(proto, x as u16)),
                );
            }
            Value::String(s) => out.push(RulePort::Named(proto, s.clone())),
            _ => {}
        }
    }
    out
}

struct World<'a> {
    pods: &'a [Pod],
    ns_labels: BTreeMap<String, BTreeMap<String, String>>,
}

impl World<'_> {
    /// Port numbers a named port maps to on pods with `identity` (0 = any pod).
    fn resolve_named(&self, identity: u32, name: &str, proto: u8) -> BTreeSet<u16> {
        self.pods
            .iter()
            .filter(|p| identity == 0 || pod_identity(&p.namespace, &p.labels) == identity)
            .filter_map(|p| p.named_ports.get(&(name.to_string(), proto)).copied())
            .collect()
    }

    fn ns_labels(&self, ns: &str) -> BTreeMap<String, String> {
        let mut l = self.ns_labels.get(ns).cloned().unwrap_or_default();
        l.entry("kubernetes.io/metadata.name".into())
            .or_insert_with(|| ns.to_string());
        l
    }

    /// Identities of pods selected by a NetworkPolicy peer.
    fn peer_identities(&self, policy_ns: &str, peer: &Value) -> BTreeSet<u32> {
        let pod_sel = peer.get("podSelector");
        let ns_sel = peer.get("namespaceSelector");
        self.pods
            .iter()
            .filter(|p| match ns_sel {
                Some(s) => selector_matches(s, &self.ns_labels(&p.namespace)),
                None => p.namespace == policy_ns,
            })
            .filter(|p| pod_sel.is_none_or(|s| selector_matches(s, &p.labels)))
            .map(|p| pod_identity(&p.namespace, &p.labels))
            .collect()
    }

    /// Insert subject × peer × port entries, resolving named ports on the
    /// destination side (subject for ingress, peer for egress).
    #[allow(clippy::too_many_arguments)]
    fn emit(
        &self,
        policy: &mut BTreeSet<CniPolicyEntry>,
        warnings: &mut Vec<String>,
        name: &str,
        subjects: &BTreeSet<u32>,
        peers: &BTreeSet<u32>,
        ports: &[RulePort],
        egress: bool,
    ) {
        for s in subjects {
            for peer in peers {
                for rp in ports {
                    let resolved: Vec<(u8, u16)> = match rp {
                        RulePort::Num(proto, port) => vec![(*proto, *port)],
                        RulePort::Named(proto, pname) => {
                            let dst = if egress { *peer } else { *s };
                            if dst & 0x8000_0000 != 0 {
                                warnings.push(format!(
                                    "{name}: named port `{pname}` cannot apply to an ipBlock peer"
                                ));
                                continue;
                            }
                            self.resolve_named(dst, pname, *proto)
                                .into_iter()
                                .map(|n| (*proto, n))
                                .collect()
                        }
                    };
                    for (proto, port) in resolved {
                        policy.insert(CniPolicyEntry {
                            subject: *s,
                            peer: *peer,
                            egress,
                            proto,
                            port,
                        });
                    }
                }
            }
        }
    }
}

pub struct Inputs<'a> {
    pub pods: &'a [Pod],
    pub namespaces: &'a [Value],
    pub policies: &'a [Value],
    pub services: &'a [Value],
    pub endpoint_slices: &'a [Value],
    /// CiliumNetworkPolicy / CiliumClusterwideNetworkPolicy objects (empty
    /// unless the agent runs with `MACHINA_CNI_CILIUM_POLICIES`).
    pub cilium_policies: &'a [Value],
    /// Node name → InternalIP addresses (remote NodePort backends' nodes).
    pub node_ips: &'a BTreeMap<String, Vec<String>>,
    /// This node's name (NodePort frontends only get local backends).
    pub node: &'a str,
}

pub fn compile(inp: &Inputs) -> Compiled {
    let mut warnings = Vec::new();
    let world = World {
        pods: inp.pods,
        ns_labels: inp
            .namespaces
            .iter()
            .filter_map(|n| Some((str_at(n, &["metadata", "name"])?.to_string(), labels_of(n))))
            .collect(),
    };

    let mut policy: BTreeSet<CniPolicyEntry> = BTreeSet::new();
    let mut cidrs: BTreeMap<String, u32> = BTreeMap::new();
    let mut ingress_iso: BTreeSet<u32> = BTreeSet::new();
    let mut egress_iso: BTreeSet<u32> = BTreeSet::new();

    for np in inp.policies {
        let ns = str_at(np, &["metadata", "namespace"]).unwrap_or("default");
        let name = format!("{ns}/{}", str_at(np, &["metadata", "name"]).unwrap_or("?"));
        let spec = &np["spec"];
        let subject_sel = spec
            .get("podSelector")
            .cloned()
            .unwrap_or(Value::Object(Default::default()));
        let subjects: BTreeSet<u32> = inp
            .pods
            .iter()
            .filter(|p| p.namespace == ns && selector_matches(&subject_sel, &p.labels))
            .map(|p| pod_identity(&p.namespace, &p.labels))
            .collect();
        let types: Vec<String> = match spec["policyTypes"].as_array() {
            Some(t) => t
                .iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect(),
            None => {
                let mut t = vec!["Ingress".to_string()];
                if spec.get("egress").is_some() {
                    t.push("Egress".into());
                }
                t
            }
        };
        for (dir, peers_key, rules_key) in
            [("Ingress", "from", "ingress"), ("Egress", "to", "egress")]
        {
            if !types.iter().any(|t| t == dir) {
                continue;
            }
            let egress = dir == "Egress";
            for s in &subjects {
                if egress {
                    egress_iso.insert(*s);
                } else {
                    ingress_iso.insert(*s);
                }
            }
            for rule in spec[rules_key].as_array().into_iter().flatten() {
                let ports = rule_ports(rule, &mut warnings, &name);
                let mut peers: BTreeSet<u32> = BTreeSet::new();
                match rule
                    .get(peers_key)
                    .and_then(|p| p.as_array())
                    .filter(|p| !p.is_empty())
                {
                    None => {
                        peers.insert(0);
                    }
                    Some(list) => {
                        for peer in list {
                            if let Some(block) = peer.get("ipBlock") {
                                let Some(cidr) = block["cidr"].as_str() else {
                                    continue;
                                };
                                if block
                                    .get("except")
                                    .and_then(|e| e.as_array())
                                    .is_some_and(|e| !e.is_empty())
                                {
                                    warnings.push(format!(
                                        "{name}: ipBlock except not supported ({cidr})"
                                    ));
                                }
                                let id = *cidrs
                                    .entry(cidr.to_string())
                                    .or_insert_with(|| cidr_identity(cidr));
                                peers.insert(id);
                            } else {
                                peers.extend(world.peer_identities(ns, peer));
                            }
                        }
                    }
                }
                world.emit(
                    &mut policy,
                    &mut warnings,
                    &name,
                    &subjects,
                    &peers,
                    &ports,
                    egress,
                );
            }
        }
    }

    if !inp.cilium_policies.is_empty() {
        cilium::compile_into(
            &world,
            inp.cilium_policies,
            &mut cilium::Acc {
                policy: &mut policy,
                cidrs: &mut cidrs,
                ingress_iso: &mut ingress_iso,
                egress_iso: &mut egress_iso,
                warnings: &mut warnings,
            },
        );
    }

    let mut identities: Vec<CniIdentity> = inp
        .pods
        .iter()
        .flat_map(|p| {
            let id = pod_identity(&p.namespace, &p.labels);
            let (ingress_isolated, egress_isolated) =
                (ingress_iso.contains(&id), egress_iso.contains(&id));
            p.ips.iter().map(move |ip| CniIdentity {
                ip: ip.clone(),
                identity: id,
                ingress_isolated,
                egress_isolated,
            })
        })
        .collect();
    identities.sort();
    identities.dedup_by(|a, b| a.ip == b.ip);

    Compiled {
        state: CniState {
            version: CNI_STATE_VERSION,
            identities,
            policy: policy.into_iter().collect(),
            cidrs: cidrs.into_iter().collect(),
            services: services(inp, &mut warnings),
        },
        warnings,
    }
}

/// Ready IPv4/IPv6 endpoints of a service, by service port name → (ip, port, node).
fn slice_backends(
    inp: &Inputs,
    ns: &str,
    svc: &str,
) -> BTreeMap<String, Vec<(String, u16, String)>> {
    let mut out: BTreeMap<String, Vec<(String, u16, String)>> = BTreeMap::new();
    for s in inp.endpoint_slices {
        if str_at(s, &["metadata", "namespace"]) != Some(ns)
            || s.pointer("/metadata/labels/kubernetes.io~1service-name")
                .and_then(|v| v.as_str())
                != Some(svc)
            || s["addressType"]
                .as_str()
                .is_some_and(|t| t != "IPv4" && t != "IPv6")
        {
            continue;
        }
        let ports: Vec<(String, u16)> = s["ports"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                Some((
                    p["name"].as_str().unwrap_or("").to_string(),
                    p["port"].as_u64()? as u16,
                ))
            })
            .collect();
        for ep in s["endpoints"].as_array().into_iter().flatten() {
            if ep.pointer("/conditions/ready") == Some(&Value::Bool(false)) {
                continue;
            }
            let node = ep["nodeName"].as_str().unwrap_or("").to_string();
            for addr in ep["addresses"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| a.as_str())
            {
                for (pname, port) in &ports {
                    out.entry(pname.clone()).or_default().push((
                        addr.to_string(),
                        *port,
                        node.clone(),
                    ));
                }
            }
        }
    }
    out
}

fn services(inp: &Inputs, warnings: &mut Vec<String>) -> Vec<CniService> {
    let mut out = Vec::new();
    for svc in inp.services {
        let ns = str_at(svc, &["metadata", "namespace"]).unwrap_or("default");
        let name = str_at(svc, &["metadata", "name"]).unwrap_or("");
        let spec = &svc["spec"];
        let ty = spec["type"].as_str().unwrap_or("ClusterIP");
        if ty == "ExternalName" {
            continue;
        }
        let mut frontends: BTreeSet<String> = BTreeSet::new();
        let cluster_ips = spec["clusterIPs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str());
        for ip in cluster_ips.chain(spec["clusterIP"].as_str()) {
            if !ip.is_empty() && ip != "None" {
                frontends.insert(ip.to_string());
            }
        }
        let affinity_secs = (spec["sessionAffinity"].as_str() == Some("ClientIP")).then(|| {
            spec.pointer("/sessionAffinityConfig/clientIP/timeoutSeconds")
                .and_then(|t| t.as_u64())
                .map(|t| t.clamp(1, 86_400) as u32)
                .unwrap_or(10_800)
        });
        let etp_local = spec["externalTrafficPolicy"].as_str() == Some("Local");
        for ip in spec["externalIPs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
        {
            frontends.insert(ip.to_string());
        }
        for ing in svc
            .pointer("/status/loadBalancer/ingress")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            if let Some(ip) = ing["ip"].as_str() {
                frontends.insert(ip.to_string());
            }
        }
        let backends = slice_backends(inp, ns, name);
        for p in spec["ports"].as_array().into_iter().flatten() {
            let Some(proto) = proto_num(p["protocol"].as_str()) else {
                warnings.push(format!(
                    "service {ns}/{name}: protocol {} not supported",
                    p["protocol"]
                ));
                continue;
            };
            let Some(port) = p["port"].as_u64() else {
                continue;
            };
            let pname = p["name"].as_str().unwrap_or("");
            let all: Vec<&(String, u16, String)> = backends
                .get(pname)
                .map(|v| v.iter().collect())
                .unwrap_or_default();
            // Backends of one family; NodePorts also mark remote-node backends.
            let pick = |v6: bool, nodeport: bool| -> Vec<CniBackend> {
                let mut be: Vec<CniBackend> = all
                    .iter()
                    .filter(|(a, _, _)| a.contains(':') == v6)
                    .filter(|(_, _, node)| !(nodeport && etp_local) || node == inp.node)
                    .map(|(a, port, node)| {
                        let remote = nodeport && node != inp.node;
                        CniBackend {
                            addr: a.clone(),
                            port: *port,
                            remote,
                            node: remote
                                .then(|| {
                                    inp.node_ips
                                        .get(node)?
                                        .iter()
                                        .find(|ip| ip.contains(':') == v6)
                                        .cloned()
                                })
                                .flatten(),
                        }
                    })
                    .collect();
                be.sort();
                be.dedup();
                be
            };
            let label = Some(format!("{ns}/{name}:{pname}"));
            for fe in &frontends {
                out.push(CniService {
                    addr: fe.clone(),
                    port: port as u16,
                    proto,
                    backends: pick(fe.contains(':'), false),
                    name: label.clone(),
                    affinity_secs,
                });
            }
            if let Some(np) = p["nodePort"]
                .as_u64()
                .filter(|_| ty == "NodePort" || ty == "LoadBalancer")
            {
                let families = spec["ipFamilies"]
                    .as_array()
                    .map(|f| f.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>());
                for (v6, fe) in [(false, "0.0.0.0"), (true, "::")] {
                    let fam = if v6 { "IPv6" } else { "IPv4" };
                    if families.as_ref().is_some_and(|f| !f.contains(&fam))
                        || (families.is_none() && v6)
                    {
                        continue;
                    }
                    out.push(CniService {
                        addr: fe.into(),
                        port: np as u16,
                        proto,
                        backends: pick(v6, true),
                        name: label.clone(),
                        affinity_secs,
                    });
                }
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pod(ns: &str, name: &str, ip: &str, labels: &[(&str, &str)]) -> Value {
        let l: serde_json::Map<String, Value> = labels
            .iter()
            .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
            .collect();
        json!({
            "metadata": { "namespace": ns, "name": name, "labels": l },
            "spec": { "nodeName": "n1" },
            "status": { "phase": "Running", "podIP": ip },
        })
    }

    #[test]
    fn selectors() {
        let l: BTreeMap<String, String> =
            [("app".into(), "web".into()), ("tier".into(), "fe".into())].into();
        assert!(selector_matches(&json!({}), &l));
        assert!(selector_matches(
            &json!({"matchLabels": {"app": "web"}}),
            &l
        ));
        assert!(!selector_matches(
            &json!({"matchLabels": {"app": "db"}}),
            &l
        ));
        assert!(selector_matches(
            &json!({"matchExpressions": [{"key": "tier", "operator": "In", "values": ["fe", "be"]}]}),
            &l
        ));
        assert!(!selector_matches(
            &json!({"matchExpressions": [{"key": "tier", "operator": "NotIn", "values": ["fe"]}]}),
            &l
        ));
        assert!(selector_matches(
            &json!({"matchExpressions": [{"key": "x", "operator": "DoesNotExist"}]}),
            &l
        ));
        assert!(!selector_matches(
            &json!({"matchExpressions": [{"key": "x", "operator": "Exists"}]}),
            &l
        ));
    }

    #[test]
    fn policy_compiles_to_identity_pairs() {
        let mut db_pod = pod("prod", "db-1", "10.42.0.6", &[("app", "db")]);
        db_pod["spec"]["containers"] =
            json!([{"ports": [{"name": "metrics", "containerPort": 9187}]}]);
        let items = vec![
            pod("prod", "web-1", "10.42.0.5", &[("app", "web")]),
            db_pod,
            pod("dev", "tool", "10.42.1.9", &[("app", "tool")]),
            json!({ "metadata": {"namespace": "kube-system", "name": "h"}, "spec": {"hostNetwork": true}, "status": {"podIP": "10.0.0.1"} }),
        ];
        let pods = pods(&items);
        assert_eq!(pods.len(), 3, "hostNetwork pods are skipped");
        let namespaces = vec![json!({"metadata": {"name": "dev", "labels": {"env": "dev"}}})];
        let policies = vec![json!({
            "metadata": {"namespace": "prod", "name": "db-allow-web"},
            "spec": {
                "podSelector": {"matchLabels": {"app": "db"}},
                "ingress": [
                    {"from": [{"podSelector": {"matchLabels": {"app": "web"}}}], "ports": [{"port": 5432}]},
                    {"from": [{"namespaceSelector": {"matchLabels": {"env": "dev"}}}, {"ipBlock": {"cidr": "192.168.0.0/16"}}],
                     "ports": [{"protocol": "UDP", "port": 53, "endPort": 54}, {"port": "metrics"}]}
                ]
            }
        })];
        let c = compile(&Inputs {
            pods: &pods,
            namespaces: &namespaces,
            policies: &policies,
            services: &[],
            endpoint_slices: &[],
            cilium_policies: &[],
            node_ips: &BTreeMap::new(),
            node: "n1",
        });
        let web = pod_identity("prod", &pods[0].labels);
        let db = pod_identity("prod", &pods[1].labels);
        let tool = pod_identity("dev", &pods[2].labels);
        let block = cidr_identity("192.168.0.0/16");
        let st = &c.state;
        assert!(st.policy.contains(&CniPolicyEntry {
            subject: db,
            peer: web,
            egress: false,
            proto: IPPROTO_TCP,
            port: 5432
        }));
        for port in [53, 54] {
            assert!(st.policy.contains(&CniPolicyEntry {
                subject: db,
                peer: tool,
                egress: false,
                proto: IPPROTO_UDP,
                port
            }));
            assert!(st.policy.contains(&CniPolicyEntry {
                subject: db,
                peer: block,
                egress: false,
                proto: IPPROTO_UDP,
                port
            }));
        }
        for peer in [tool, block] {
            assert!(st.policy.contains(&CniPolicyEntry {
                subject: db,
                peer,
                egress: false,
                proto: IPPROTO_TCP,
                port: 9187
            }));
        }
        assert_eq!(st.policy.len(), 7);
        assert_eq!(st.cidrs, vec![("192.168.0.0/16".to_string(), block)]);
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        let iso: Vec<(&str, bool, bool)> = st
            .identities
            .iter()
            .map(|i| (i.ip.as_str(), i.ingress_isolated, i.egress_isolated))
            .collect();
        assert!(iso.contains(&("10.42.0.6", true, false)));
        assert!(iso.contains(&("10.42.0.5", false, false)));
        assert!(block & 0x8000_0000 != 0 && db & 0x8000_0000 == 0);
    }

    #[test]
    fn named_ports_resolve_on_the_destination() {
        let mut api = pod("a", "api", "10.42.0.8", &[("app", "api")]);
        api["spec"]["containers"] = json!([{"ports": [
            {"name": "http", "containerPort": 8080},
            {"name": "dns", "containerPort": 5353, "protocol": "UDP"}]}]);
        let items = vec![api, pod("a", "cli", "10.42.0.9", &[("app", "cli")])];
        let pods = pods(&items);
        let policies = vec![json!({
            "metadata": {"namespace": "a", "name": "cli-out"},
            "spec": {"podSelector": {"matchLabels": {"app": "cli"}}, "policyTypes": ["Egress"],
                "egress": [
                    {"to": [{"podSelector": {"matchLabels": {"app": "api"}}}],
                     "ports": [{"port": "http"}, {"port": "dns", "protocol": "UDP"}, {"port": "nope"}]},
                    {"to": [{"ipBlock": {"cidr": "10.0.0.0/8"}}], "ports": [{"port": "http"}]}
                ]}
        })];
        let c = compile(&Inputs {
            pods: &pods,
            namespaces: &[],
            policies: &policies,
            services: &[],
            endpoint_slices: &[],
            cilium_policies: &[],
            node_ips: &BTreeMap::new(),
            node: "n1",
        });
        let api = pod_identity("a", &pods[0].labels);
        let cli = pod_identity("a", &pods[1].labels);
        let got: BTreeSet<(u32, u8, u16)> = c
            .state
            .policy
            .iter()
            .map(|p| (p.peer, p.proto, p.port))
            .collect();
        assert!(c.state.policy.iter().all(|p| p.subject == cli && p.egress));
        assert_eq!(
            got,
            [(api, IPPROTO_TCP, 8080), (api, IPPROTO_UDP, 5353)].into()
        );
        assert!(c.warnings.iter().any(|w| w.contains("ipBlock peer")));
    }

    #[test]
    fn default_deny_egress_and_allow_all_ingress() {
        let items = vec![pod("a", "p", "10.42.0.7", &[("app", "x")])];
        let pods = pods(&items);
        let policies = vec![json!({
            "metadata": {"namespace": "a", "name": "lockdown"},
            "spec": {"podSelector": {}, "policyTypes": ["Ingress", "Egress"], "ingress": [{}]}
        })];
        let c = compile(&Inputs {
            pods: &pods,
            namespaces: &[],
            policies: &policies,
            services: &[],
            endpoint_slices: &[],
            cilium_policies: &[],
            node_ips: &BTreeMap::new(),
            node: "n1",
        });
        let id = pod_identity("a", &pods[0].labels);
        assert_eq!(
            c.state.policy,
            vec![CniPolicyEntry {
                subject: id,
                peer: 0,
                egress: false,
                proto: 0,
                port: 0
            }]
        );
        assert!(c.state.identities[0].ingress_isolated && c.state.identities[0].egress_isolated);
    }

    #[test]
    fn services_and_nodeports() {
        let services = vec![json!({
            "metadata": {"namespace": "prod", "name": "web"},
            "spec": {"type": "NodePort", "clusterIP": "10.43.0.10",
                     "ports": [{"name": "http", "port": 80, "protocol": "TCP", "nodePort": 30080}]}
        })];
        let slices = vec![json!({
            "metadata": {"namespace": "prod", "name": "web-abc", "labels": {"kubernetes.io/service-name": "web"}},
            "addressType": "IPv4",
            "ports": [{"name": "http", "port": 8080}],
            "endpoints": [
                {"addresses": ["10.42.0.5"], "nodeName": "n1", "conditions": {"ready": true}},
                {"addresses": ["10.42.1.5"], "nodeName": "n2"},
                {"addresses": ["10.42.1.6"], "nodeName": "n2", "conditions": {"ready": false}}
            ]
        })];
        let node_ips: BTreeMap<String, Vec<String>> = [(
            "n2".to_string(),
            vec!["192.0.2.2".to_string(), "2001:db8::2".to_string()],
        )]
        .into();
        let c = compile(&Inputs {
            pods: &[],
            namespaces: &[],
            policies: &[],
            services: &services,
            endpoint_slices: &slices,
            cilium_policies: &[],
            node_ips: &node_ips,
            node: "n1",
        });
        let s = &c.state.services;
        assert_eq!(s.len(), 2);
        let np = s.iter().find(|x| x.addr == "0.0.0.0").unwrap();
        assert_eq!(np.port, 30080);
        let local = CniBackend {
            addr: "10.42.0.5".into(),
            port: 8080,
            ..Default::default()
        };
        let remote = CniBackend {
            addr: "10.42.1.5".into(),
            port: 8080,
            remote: true,
            node: Some("192.0.2.2".into()),
        };
        assert_eq!(
            np.backends,
            vec![local.clone(), remote],
            "eTP=Cluster: remote backends via their node"
        );
        let cip = s.iter().find(|x| x.addr == "10.43.0.10").unwrap();
        assert_eq!(cip.backends.len(), 2, "not-ready endpoints are excluded");
        assert!(
            cip.backends.iter().all(|b| !b.remote),
            "ClusterIP backends are plain (socket LB)"
        );
        assert_eq!(cip.affinity_secs, None);

        // eTP=Local, ClientIP affinity, dual-stack.
        let mut svc = services[0].clone();
        svc["spec"]["externalTrafficPolicy"] = json!("Local");
        svc["spec"]["sessionAffinity"] = json!("ClientIP");
        svc["spec"]["sessionAffinityConfig"] = json!({"clientIP": {"timeoutSeconds": 60}});
        svc["spec"]["clusterIPs"] = json!(["10.43.0.10", "fd43::10"]);
        svc["spec"]["ipFamilies"] = json!(["IPv4", "IPv6"]);
        let mut slices6 = slices.clone();
        slices6.push(json!({
            "metadata": {"namespace": "prod", "name": "web-v6", "labels": {"kubernetes.io/service-name": "web"}},
            "addressType": "IPv6",
            "ports": [{"name": "http", "port": 8080}],
            "endpoints": [{"addresses": ["fd42::5"], "nodeName": "n1"}]
        }));
        let c = compile(&Inputs {
            pods: &[],
            namespaces: &[],
            policies: &[],
            services: &[svc],
            endpoint_slices: &slices6,
            cilium_policies: &[],
            node_ips: &node_ips,
            node: "n1",
        });
        let s = &c.state.services;
        assert_eq!(s.len(), 4, "{s:?}");
        assert!(s.iter().all(|x| x.affinity_secs == Some(60)));
        assert_eq!(
            s.iter().find(|x| x.addr == "0.0.0.0").unwrap().backends,
            vec![local]
        );
        let np6 = s.iter().find(|x| x.addr == "::").unwrap();
        assert_eq!(
            np6.backends,
            vec![CniBackend {
                addr: "fd42::5".into(),
                port: 8080,
                ..Default::default()
            }]
        );
        let cip6 = s.iter().find(|x| x.addr == "fd43::10").unwrap();
        assert_eq!(cip6.backends.len(), 1, "v6 frontend only gets v6 backends");
    }

    #[test]
    fn dual_stack_pods_get_one_identity_per_address() {
        let mut p = pod("prod", "web", "10.42.0.5", &[("app", "web")]);
        p["status"]["podIPs"] = json!([{"ip": "10.42.0.5"}, {"ip": "fd42::5"}]);
        let pods = pods(&[p]);
        assert_eq!(pods[0].ips, vec!["10.42.0.5", "fd42::5"]);
        let policies = vec![json!({
            "metadata": {"namespace": "prod", "name": "v6-block"},
            "spec": {"podSelector": {}, "policyTypes": ["Egress"],
                     "egress": [{"to": [{"ipBlock": {"cidr": "2001:db8::/32"}}]}]}
        })];
        let c = compile(&Inputs {
            pods: &pods,
            namespaces: &[],
            policies: &policies,
            services: &[],
            endpoint_slices: &[],
            cilium_policies: &[],
            node_ips: &BTreeMap::new(),
            node: "n1",
        });
        assert_eq!(c.state.identities.len(), 2);
        assert_eq!(
            c.state.identities[0].identity,
            c.state.identities[1].identity
        );
        assert_eq!(
            c.state.cidrs,
            vec![("2001:db8::/32".to_string(), cidr_identity("2001:db8::/32"))]
        );
        assert_eq!(c.state.version, CNI_STATE_VERSION);
    }
}
