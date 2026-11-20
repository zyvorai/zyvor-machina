// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! CiliumNetworkPolicy (`cilium.io/v2`, namespaced) and
//! CiliumClusterwideNetworkPolicy → the same identity tuples as Kubernetes
//! NetworkPolicy, so clusters migrated off Cilium keep their policies.
//!
//! Supported: `spec` / `specs`, `endpointSelector`, `ingress`/`egress` with
//! `from|toEndpoints`, `from|toCIDR`, `from|toCIDRSet`, `from|toEntities`
//! (`all`, `world`, `cluster`, `host`, `remote-node`, `kube-apiserver`),
//! `toPorts` (numeric/named, `endPort`, TCP/UDP/ANY) and `enableDefaultDeny`.
//! Selector keys may carry `k8s:`/`any:` prefixes; `io.kubernetes.pod.namespace`
//! and `io.cilium.k8s.namespace.labels.*` select across namespaces.
//!
//! The native datapath is L3/L4 only, so anything finer fails open with a
//! warning: L7 `rules` enforce as their L4 port, `toFQDNs`/`toServices` allow
//! any peer on the rule's ports, `*Deny` rules, `from|toRequires` and host
//! (`nodeSelector`) policies are ignored.

use std::collections::{BTreeMap, BTreeSet};

use machina_bpf::api::CniPolicyEntry;
use machina_bpf_common::{IDENTITY_HOST, IDENTITY_WORLD};
use serde_json::{Map, Value};

use super::{
    cidr_identity, pod_identity, selector_matches, str_at, Pod, RulePort, World, IPPROTO_TCP,
    IPPROTO_UDP, MAX_PORT_RANGE,
};

const NS_LABEL: &str = "io.kubernetes.pod.namespace";
const NS_LABELS_PREFIX: &str = "io.cilium.k8s.namespace.labels.";

pub(super) struct Acc<'a> {
    pub policy: &'a mut BTreeSet<CniPolicyEntry>,
    pub cidrs: &'a mut BTreeMap<String, u32>,
    pub ingress_iso: &'a mut BTreeSet<u32>,
    pub egress_iso: &'a mut BTreeSet<u32>,
    pub warnings: &'a mut Vec<String>,
}

struct Dir {
    egress: bool,
    rules: &'static str,
    deny: &'static str,
    endpoints: &'static str,
    cidr: &'static str,
    cidr_set: &'static str,
    entities: &'static str,
    requires: &'static str,
}

const INGRESS: Dir = Dir {
    egress: false,
    rules: "ingress",
    deny: "ingressDeny",
    endpoints: "fromEndpoints",
    cidr: "fromCIDR",
    cidr_set: "fromCIDRSet",
    entities: "fromEntities",
    requires: "fromRequires",
};

const EGRESS: Dir = Dir {
    egress: true,
    rules: "egress",
    deny: "egressDeny",
    endpoints: "toEndpoints",
    cidr: "toCIDR",
    cidr_set: "toCIDRSet",
    entities: "toEntities",
    requires: "toRequires",
};

/// Egress peers the datapath cannot express; they allow any peer instead.
const EGRESS_FAIL_OPEN: [&str; 3] = ["toFQDNs", "toServices", "toGroups"];

pub(super) fn compile_into(world: &World, objects: &[Value], acc: &mut Acc) {
    for obj in objects {
        let clusterwide = obj["kind"].as_str() == Some("CiliumClusterwideNetworkPolicy");
        let ns = if clusterwide {
            None
        } else {
            Some(str_at(obj, &["metadata", "namespace"]).unwrap_or("default"))
        };
        let pname = str_at(obj, &["metadata", "name"]).unwrap_or("?");
        let name = match ns {
            Some(ns) => format!("cnp {ns}/{pname}"),
            None => format!("ccnp {pname}"),
        };
        let specs: Vec<&Value> = match (obj.get("spec"), obj["specs"].as_array()) {
            (Some(s), Some(list)) if !s.is_null() => std::iter::once(s).chain(list).collect(),
            (Some(s), None) if !s.is_null() => vec![s],
            (_, Some(list)) => list.iter().collect(),
            _ => Vec::new(),
        };
        for spec in specs {
            compile_spec(world, spec, ns, &name, acc);
        }
    }
}

fn compile_spec(world: &World, spec: &Value, ns: Option<&str>, name: &str, acc: &mut Acc) {
    let Some(sel) = spec.get("endpointSelector") else {
        if spec.get("nodeSelector").is_some() {
            acc.warnings.push(format!(
                "{name}: host policies (nodeSelector) are not supported"
            ));
        }
        return;
    };
    let Some(subject_sel) = normalize(sel) else {
        acc.warnings.push(format!(
            "{name}: endpointSelector with reserved: labels is not supported"
        ));
        return;
    };
    let subjects: BTreeSet<u32> = world
        .pods
        .iter()
        .filter(|p| ns.is_none_or(|ns| p.namespace == ns))
        .filter(|p| selector_matches(&subject_sel, &cilium_labels(world, p)))
        .map(|p| pod_identity(&p.namespace, &p.labels))
        .collect();

    for dir in [INGRESS, EGRESS] {
        if spec[dir.deny].as_array().is_some_and(|d| !d.is_empty()) {
            acc.warnings
                .push(format!("{name}: {} rules are not enforced", dir.deny));
        }
        let Some(rules) = spec.get(dir.rules).filter(|r| !r.is_null()) else {
            continue;
        };
        if spec["enableDefaultDeny"][dir.rules].as_bool() != Some(false) {
            let iso = if dir.egress {
                &mut *acc.egress_iso
            } else {
                &mut *acc.ingress_iso
            };
            iso.extend(subjects.iter().copied());
        }
        for rule in rules.as_array().into_iter().flatten() {
            let ports = to_ports(rule, acc.warnings, name);
            let peers = rule_peers(world, rule, &dir, ns, acc, name);
            world.emit(
                acc.policy,
                acc.warnings,
                name,
                &subjects,
                &peers,
                &ports,
                dir.egress,
            );
        }
    }
}

fn rule_peers(
    world: &World,
    rule: &Value,
    dir: &Dir,
    ns: Option<&str>,
    acc: &mut Acc,
    name: &str,
) -> BTreeSet<u32> {
    let nonempty = |key: &str| {
        rule.get(key)
            .and_then(Value::as_array)
            .filter(|a| !a.is_empty())
    };
    let mut peers = BTreeSet::new();
    let mut has_peer_field = false;

    if let Some(list) = nonempty(dir.endpoints) {
        has_peer_field = true;
        for sel in list {
            match endpoints(world, sel, ns) {
                Some(ids) => peers.extend(ids),
                None => acc.warnings.push(format!(
                    "{name}: {} with reserved: labels is not supported",
                    dir.endpoints
                )),
            }
        }
    }
    let mut add_cidr = |cidr: &str, acc: &mut Acc| {
        peers.insert(
            *acc.cidrs
                .entry(cidr.to_string())
                .or_insert_with(|| cidr_identity(cidr)),
        );
    };
    if let Some(list) = nonempty(dir.cidr) {
        has_peer_field = true;
        for c in list.iter().filter_map(Value::as_str) {
            add_cidr(c, acc);
        }
    }
    if let Some(list) = nonempty(dir.cidr_set) {
        has_peer_field = true;
        for set in list {
            if set["except"].as_array().is_some_and(|e| !e.is_empty()) {
                acc.warnings
                    .push(format!("{name}: {} except is not supported", dir.cidr_set));
            }
            match set["cidr"].as_str() {
                Some(c) => add_cidr(c, acc),
                None => acc.warnings.push(format!(
                    "{name}: {} without cidr is not supported",
                    dir.cidr_set
                )),
            }
        }
    }
    if let Some(list) = nonempty(dir.entities) {
        has_peer_field = true;
        for e in list.iter().filter_map(Value::as_str) {
            match entity(world, e) {
                Some(ids) => peers.extend(ids),
                None => acc
                    .warnings
                    .push(format!("{name}: entity `{e}` is not supported")),
            }
        }
    }
    if dir.egress {
        for key in EGRESS_FAIL_OPEN {
            if nonempty(key).is_some() {
                has_peer_field = true;
                acc.warnings.push(format!(
                    "{name}: {key} is not enforced natively; allowing any peer on its ports"
                ));
                peers.insert(0);
            }
        }
    }
    if nonempty(dir.requires).is_some() {
        acc.warnings
            .push(format!("{name}: {} is ignored", dir.requires));
    }
    // An L4-only rule allows every peer; a fully empty rule (`- {}`) allows nothing.
    if !has_peer_field && nonempty("toPorts").is_some() {
        peers.insert(0);
    }
    peers
}

/// Pod labels plus the namespace labels Cilium exposes to selectors.
fn cilium_labels(world: &World, p: &Pod) -> BTreeMap<String, String> {
    let mut l = p.labels.clone();
    l.insert(NS_LABEL.into(), p.namespace.clone());
    for (k, v) in world.ns_labels(&p.namespace) {
        l.insert(format!("{NS_LABELS_PREFIX}{k}"), v);
    }
    l
}

fn strip_source(k: &str) -> &str {
    k.strip_prefix("k8s:")
        .or_else(|| k.strip_prefix("any:"))
        .unwrap_or(k)
}

/// The selector with `k8s:`/`any:` key prefixes removed; `None` when it
/// matches on `reserved:` labels (entities, not pods).
fn normalize(sel: &Value) -> Option<Value> {
    let mut out = Map::new();
    if let Some(ml) = sel.get("matchLabels").and_then(Value::as_object) {
        let mut m = Map::new();
        for (k, v) in ml {
            if k.starts_with("reserved:") {
                return None;
            }
            m.insert(strip_source(k).to_string(), v.clone());
        }
        out.insert("matchLabels".into(), Value::Object(m));
    }
    if let Some(exprs) = sel.get("matchExpressions").and_then(Value::as_array) {
        let mut list = Vec::new();
        for e in exprs {
            let key = e["key"].as_str().unwrap_or("");
            if key.starts_with("reserved:") {
                return None;
            }
            let mut e = e.clone();
            e["key"] = Value::String(strip_source(key).to_string());
            list.push(e);
        }
        out.insert("matchExpressions".into(), Value::Array(list));
    }
    Some(Value::Object(out))
}

fn names_namespace(sel: &Value) -> bool {
    let cross = |k: &str| {
        let k = strip_source(k);
        k == NS_LABEL || k.starts_with(NS_LABELS_PREFIX)
    };
    sel.get("matchLabels")
        .and_then(Value::as_object)
        .is_some_and(|m| m.keys().any(|k| cross(k)))
        || sel
            .get("matchExpressions")
            .and_then(Value::as_array)
            .is_some_and(|a| a.iter().any(|e| e["key"].as_str().is_some_and(cross)))
}

/// Pods selected by a `from|toEndpoints` entry: the policy's namespace unless
/// the selector names one (or the policy is clusterwide).
fn endpoints(world: &World, sel: &Value, ns: Option<&str>) -> Option<BTreeSet<u32>> {
    let norm = normalize(sel)?;
    let scope = if names_namespace(sel) { None } else { ns };
    Some(
        world
            .pods
            .iter()
            .filter(|p| scope.is_none_or(|ns| p.namespace == ns))
            .filter(|p| selector_matches(&norm, &cilium_labels(world, p)))
            .map(|p| pod_identity(&p.namespace, &p.labels))
            .collect(),
    )
}

fn entity(world: &World, e: &str) -> Option<BTreeSet<u32>> {
    Some(match e {
        "all" => [0].into(),
        "world" | "world-ipv4" => [IDENTITY_WORLD].into(),
        "host" | "remote-node" | "kube-apiserver" => [IDENTITY_HOST].into(),
        "cluster" => world
            .pods
            .iter()
            .map(|p| pod_identity(&p.namespace, &p.labels))
            .chain([IDENTITY_HOST])
            .collect(),
        _ => return None,
    })
}

/// `toPorts` of a Cilium rule; port `0` or no ports means every port.
fn to_ports(rule: &Value, warnings: &mut Vec<String>, ctx: &str) -> Vec<RulePort> {
    let Some(list) = rule
        .get("toPorts")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
    else {
        return vec![RulePort::Num(0, 0)];
    };
    let mut out = Vec::new();
    for tp in list {
        if tp.get("rules").is_some_and(|r| !r.is_null()) {
            warnings.push(format!("{ctx}: L7 rules are enforced as L4 only"));
        }
        let Some(ports) = tp["ports"].as_array().filter(|p| !p.is_empty()) else {
            out.push(RulePort::Num(0, 0));
            continue;
        };
        for p in ports {
            let protos: &[u8] = match p["protocol"].as_str().unwrap_or("ANY") {
                "TCP" => &[IPPROTO_TCP],
                "UDP" => &[IPPROTO_UDP],
                "ANY" => &[IPPROTO_TCP, IPPROTO_UDP],
                other => {
                    warnings.push(format!("{ctx}: protocol {other} not supported"));
                    continue;
                }
            };
            let port = match &p["port"] {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => "0".into(),
            };
            let Ok(start) = port.parse::<u32>() else {
                out.extend(
                    protos
                        .iter()
                        .map(|proto| RulePort::Named(*proto, port.clone())),
                );
                continue;
            };
            if start == 0 {
                out.extend(protos.iter().map(|proto| RulePort::Num(*proto, 0)));
                continue;
            }
            let end = p["endPort"]
                .as_u64()
                .map(|e| e as u32)
                .filter(|e| *e > 0)
                .unwrap_or(start);
            if end < start || end - start >= MAX_PORT_RANGE {
                warnings.push(format!(
                    "{ctx}: port range {start}-{end} too wide (max {MAX_PORT_RANGE})"
                ));
                continue;
            }
            for proto in protos {
                out.extend((start..=end.min(65535)).map(|x| RulePort::Num(*proto, x as u16)));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::{compile, pods, Inputs};
    use super::*;
    use serde_json::json;

    fn pod(ns: &str, name: &str, ip: &str, labels: Value) -> Value {
        json!({
            "metadata": { "namespace": ns, "name": name, "labels": labels },
            "spec": { "nodeName": "n1", "containers": [{"ports": [{"name": "http", "containerPort": 8080}]}] },
            "status": { "phase": "Running", "podIP": ip },
        })
    }

    fn run(policies: &[Value]) -> (Vec<Pod>, super::super::Compiled) {
        let items = vec![
            pod("prod", "web", "10.42.0.5", json!({"app": "web"})),
            pod("prod", "db", "10.42.0.6", json!({"app": "db"})),
            pod("mon", "prom", "10.42.0.7", json!({"app": "prometheus"})),
        ];
        let pods = pods(&items);
        let namespaces = vec![json!({"metadata": {"name": "mon", "labels": {"team": "obs"}}})];
        let c = compile(&Inputs {
            pods: &pods,
            namespaces: &namespaces,
            policies: &[],
            services: &[],
            endpoint_slices: &[],
            cilium_policies: policies,
            node_ips: &BTreeMap::new(),
            node: "n1",
        });
        (pods, c)
    }

    fn id(p: &Pod) -> u32 {
        pod_identity(&p.namespace, &p.labels)
    }

    fn entry(subject: u32, peer: u32, egress: bool, proto: u8, port: u16) -> CniPolicyEntry {
        CniPolicyEntry {
            subject,
            peer,
            egress,
            proto,
            port,
        }
    }

    #[test]
    fn cnp_endpoints_ports_and_cross_namespace() {
        let cnp = json!({
            "kind": "CiliumNetworkPolicy",
            "metadata": {"namespace": "prod", "name": "db"},
            "spec": {
                "endpointSelector": {"matchLabels": {"k8s:app": "db"}},
                "ingress": [
                    {"fromEndpoints": [{"matchLabels": {"app": "web"}}],
                     "toPorts": [{"ports": [{"port": "5432", "protocol": "TCP"}], "rules": {"http": [{}]}}]},
                    {"fromEndpoints": [{"matchLabels": {"k8s:io.kubernetes.pod.namespace": "mon"}}],
                     "toPorts": [{"ports": [{"port": "http"}]}]},
                    {"fromEndpoints": [{"matchLabels": {"app": "prometheus"}}]}
                ]
            }
        });
        let (pods, c) = run(&[cnp]);
        let (web, db, prom) = (id(&pods[0]), id(&pods[1]), id(&pods[2]));
        let p = &c.state.policy;
        assert!(p.contains(&entry(db, web, false, IPPROTO_TCP, 5432)));
        assert!(
            p.contains(&entry(db, prom, false, IPPROTO_TCP, 8080)),
            "named port resolves on db"
        );
        assert!(
            !p.iter().any(|e| e.proto == IPPROTO_UDP),
            "db exposes `http` over TCP only"
        );
        assert_eq!(
            p.iter().filter(|e| e.port == 0).count(),
            0,
            "un-namespaced selector stays in prod"
        );
        let db_id = c
            .state
            .identities
            .iter()
            .find(|i| i.identity == db)
            .unwrap();
        assert!(db_id.ingress_isolated && !db_id.egress_isolated);
        assert!(c.warnings.iter().any(|w| w.contains("L7 rules")));
    }

    #[test]
    fn ccnp_entities_cidrs_and_fail_open() {
        let ccnp = json!({
            "kind": "CiliumClusterwideNetworkPolicy",
            "metadata": {"name": "egress"},
            "specs": [{
                "endpointSelector": {},
                "enableDefaultDeny": {"ingress": false},
                "egress": [
                    {"toEntities": ["kube-apiserver", "cluster"]},
                    {"toCIDRSet": [{"cidr": "1.1.1.0/24", "except": ["1.1.1.1/32"]}], "toPorts": [{"ports": [{"port": "53", "protocol": "UDP"}]}]},
                    {"toFQDNs": [{"matchName": "example.com"}], "toPorts": [{"ports": [{"port": "443", "protocol": "TCP"}]}]}
                ],
                "ingress": [{"fromEntities": ["world"]}],
                "egressDeny": [{"toEntities": ["world"]}]
            }]
        });
        let (pods, c) = run(&[ccnp]);
        let web = id(&pods[0]);
        let prom = id(&pods[2]);
        let p = &c.state.policy;
        assert!(p.contains(&entry(web, IDENTITY_HOST, true, 0, 0)));
        assert!(
            p.contains(&entry(web, prom, true, 0, 0)),
            "clusterwide: cluster entity spans namespaces"
        );
        assert!(p.contains(&entry(
            prom,
            cidr_identity("1.1.1.0/24"),
            true,
            IPPROTO_UDP,
            53
        )));
        assert!(
            p.contains(&entry(web, 0, true, IPPROTO_TCP, 443)),
            "toFQDNs fails open on its ports"
        );
        assert!(p.contains(&entry(web, IDENTITY_WORLD, false, 0, 0)));
        assert!(c
            .state
            .identities
            .iter()
            .all(|i| i.egress_isolated && !i.ingress_isolated));
        assert_eq!(
            c.state.cidrs,
            vec![("1.1.1.0/24".to_string(), cidr_identity("1.1.1.0/24"))]
        );
        for w in ["except", "toFQDNs", "egressDeny"] {
            assert!(
                c.warnings.iter().any(|x| x.contains(w)),
                "missing warning {w}: {:?}",
                c.warnings
            );
        }
    }

    #[test]
    fn empty_rule_denies_and_l4_only_allows_all() {
        let cnp = json!({
            "kind": "CiliumNetworkPolicy",
            "metadata": {"namespace": "prod", "name": "lockdown"},
            "spec": {
                "endpointSelector": {"matchLabels": {"app": "web"}},
                "ingress": [{}],
                "egress": [{"toPorts": [{"ports": [{"port": "53", "protocol": "UDP"}]}]}]
            }
        });
        let (pods, c) = run(&[cnp]);
        let web = id(&pods[0]);
        assert_eq!(c.state.policy, vec![entry(web, 0, true, IPPROTO_UDP, 53)]);
        let w = c
            .state
            .identities
            .iter()
            .find(|i| i.identity == web)
            .unwrap();
        assert!(w.ingress_isolated && w.egress_isolated);
    }

    #[test]
    fn namespace_label_selectors() {
        let cnp = json!({
            "kind": "CiliumNetworkPolicy",
            "metadata": {"namespace": "prod", "name": "obs"},
            "spec": {
                "endpointSelector": {},
                "ingress": [{"fromEndpoints": [{"matchLabels": {"k8s:io.cilium.k8s.namespace.labels.team": "obs"}}]}]
            }
        });
        let (pods, c) = run(&[cnp]);
        let prom = id(&pods[2]);
        assert!(c.state.policy.iter().all(|e| e.peer == prom));
        assert_eq!(c.state.policy.len(), 2, "both prod pods allow prom");
    }
}
