// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Learn mode and replay over the flow history.
//!
//! [`learn`] turns observed edges into least-privilege policies: one per
//! group of VMs (by a label, default `app`), allowing exactly the peers,
//! ports and L7 requests seen. [`replay`] evaluates the edges against the
//! current and a draft policy set and lists what would change.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::compile::{NetpolService, NetpolVm};
use super::trace::{TraceQuery, Tracer};
use super::VmNetworkPolicy;
use crate::api::VmFlowEdge;

const VM_NAME_LABEL: &str = "machina.io/vm-name";

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LearnOptions {
    /// Only this VM's group.
    #[serde(default)]
    pub vm: Option<String>,
    /// Only VMs carrying these labels.
    #[serde(default)]
    pub selector: BTreeMap<String, String>,
    /// Label grouping VMs into one policy (default `app`; VMs without it get
    /// a policy of their own).
    #[serde(default)]
    pub group_by: Option<String>,
    /// Ignore edges seen fewer times.
    #[serde(default)]
    pub min_count: u64,
    /// Emit L7 rules for observed HTTP / DNS / TLS requests.
    #[serde(default = "yes")]
    pub l7: bool,
    /// Default-deny a direction with no observed traffic (`- {}`).
    #[serde(default)]
    pub lock_unobserved: bool,
    /// Address → DNS names (the toFQDNs cache), to emit `toFQDNs`.
    #[serde(default)]
    pub fqdn: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LearnResult {
    pub yaml: String,
    pub policies: Vec<LearnedPolicy>,
    pub edges_used: usize,
    pub edges_skipped: usize,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LearnedPolicy {
    pub name: String,
    pub vms: Vec<String>,
    pub ingress_rules: usize,
    pub egress_rules: usize,
}

#[derive(Default)]
struct PortUse {
    http: BTreeSet<(String, String)>,
    dns: BTreeSet<String>,
    sni: BTreeSet<String>,
    plain: bool,
}

#[derive(Default)]
struct Group {
    selector: BTreeMap<String, String>,
    vms: BTreeSet<String>,
    /// (egress, peer) → (proto, port) → use
    rules: BTreeMap<(bool, Peer), BTreeMap<(String, u16), PortUse>>,
    fqdn: bool,
    first: String,
    last: String,
    flows: u64,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Peer {
    Vms(BTreeMap<String, String>),
    Entity(String),
    Fqdn(String),
    Cidr(String),
}

fn sanitize(s: &str) -> String {
    let out: String = s
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    out.trim_matches('-').chars().take(50).collect()
}

fn host_cidr(addr: &str) -> String {
    if addr.contains(':') {
        format!("{addr}/128")
    } else {
        format!("{addr}/32")
    }
}

/// `/users/{id}/x.y` → `/users/[^/]+/x\.y`.
fn path_regex(path: &str) -> String {
    path.split('/')
        .map(|s| {
            if s == "{id}" {
                "[^/]+".to_string()
            } else {
                regex::escape(s)
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

struct Ctx<'a> {
    vms: BTreeMap<&'a str, &'a NetpolVm>,
    group_by: String,
}

impl Ctx<'_> {
    fn labels(&self, vm: &str, seen: &BTreeMap<String, String>) -> BTreeMap<String, String> {
        self.vms
            .get(vm)
            .map(|v| v.labels.clone())
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| seen.clone())
    }

    fn selector(&self, vm: &str, seen: &BTreeMap<String, String>) -> BTreeMap<String, String> {
        match self.labels(vm, seen).get(&self.group_by) {
            Some(v) => BTreeMap::from([(self.group_by.clone(), v.clone())]),
            None => BTreeMap::from([(VM_NAME_LABEL.to_string(), vm.to_string())]),
        }
    }
}

fn usable(e: &VmFlowEdge) -> bool {
    e.verdict != "DROPPED"
        && matches!(e.drop_reason.as_deref(), None | Some("default-deny"))
        && matches!(e.proto.as_str(), "TCP" | "UDP" | "SCTP" | "ICMP" | "ICMPv6")
}

pub fn learn(edges: &[VmFlowEdge], vms: &[NetpolVm], o: &LearnOptions) -> LearnResult {
    let cx = Ctx {
        vms: vms.iter().map(|v| (v.name.as_str(), v)).collect(),
        group_by: o
            .group_by
            .clone()
            .filter(|g| !g.is_empty())
            .unwrap_or_else(|| "app".into()),
    };
    let mut groups: BTreeMap<BTreeMap<String, String>, Group> = BTreeMap::new();
    let mut used = 0;
    let mut skipped = 0;
    let mut notes = BTreeSet::new();
    let target = o.vm.as_ref().map(|v| cx.selector(v, &BTreeMap::new()));

    for e in edges {
        let egress = e.direction == "egress";
        let (subject, subject_seen, peer_vm, peer_seen, peer_addr, peer_entity) = if egress {
            (
                &e.src_vm,
                &e.src_labels,
                &e.dst_vm,
                &e.dst_labels,
                &e.dst,
                &e.dst_entity,
            )
        } else {
            (
                &e.dst_vm,
                &e.dst_labels,
                &e.src_vm,
                &e.src_labels,
                &e.src,
                &e.src_entity,
            )
        };
        let Some(subject) = subject else {
            skipped += 1;
            continue;
        };
        if !usable(e) || e.count < o.min_count {
            skipped += 1;
            continue;
        }
        let labels = cx.labels(subject, subject_seen);
        if !o.selector.iter().all(|(k, v)| labels.get(k) == Some(v)) {
            continue;
        }
        let sel = cx.selector(subject, subject_seen);
        if target.as_ref().is_some_and(|t| *t != sel) {
            continue;
        }
        let peer = match (peer_vm, peer_entity.as_deref()) {
            (Some(p), _) => Peer::Vms(cx.selector(p, peer_seen)),
            // CIDR rules never select the host or other nodes in Cilium.
            (None, Some(n @ ("host" | "remote-node"))) => Peer::Entity(n.to_string()),
            (None, _) => match o.fqdn.get(peer_addr).and_then(|n| n.first()) {
                Some(name) if egress => Peer::Fqdn(name.clone()),
                _ => Peer::Cidr(host_cidr(peer_addr)),
            },
        };
        used += 1;
        let g = groups.entry(sel.clone()).or_default();
        g.selector = sel;
        g.vms.insert(subject.clone());
        g.fqdn |= matches!(peer, Peer::Fqdn(_));
        g.flows += e.count;
        if g.first.is_empty() || e.first_seen < g.first {
            g.first = e.first_seen.clone();
        }
        if e.last_seen > g.last {
            g.last = e.last_seen.clone();
        }
        let pu = g
            .rules
            .entry((egress, peer))
            .or_default()
            .entry((e.proto.clone(), e.port))
            .or_default();
        let l7: Vec<_> = e.l7.iter().filter(|s| s.count > s.denied).collect();
        if l7.is_empty() || !o.l7 {
            pu.plain = true;
        }
        for s in l7.into_iter().filter(|_| o.l7) {
            match s.kind.as_str() {
                "http" => {
                    let (method, rest) = s.request.split_once(' ').unwrap_or(("GET", "/"));
                    let path = rest.find('/').map_or("/", |i| &rest[i..]);
                    pu.http.insert((method.to_string(), path_regex(path)));
                }
                "dns" => {
                    if let Some(n) = s.request.strip_prefix("dns ") {
                        pu.dns.insert(n.to_string());
                    }
                }
                "tls" => {
                    if let Some(n) = s.request.strip_prefix("tls sni=") {
                        pu.sni.insert(n.to_string());
                    }
                }
                other => {
                    pu.plain = true;
                    notes.insert(format!("{other} requests are allowed at L4 only"));
                }
            }
        }
    }

    let mut docs = Vec::new();
    let mut policies = Vec::new();
    for g in groups.values() {
        let name = format!(
            "learned-{}",
            sanitize(&g.selector.values().cloned().collect::<Vec<_>>().join("-"))
        );
        let mut ingress = Vec::new();
        let mut egress = Vec::new();
        for ((eg, peer), ports) in &g.rules {
            let mut rule = serde_json::Map::new();
            let (ep_key, cidr_key, ent_key) = if *eg {
                ("toEndpoints", "toCIDR", "toEntities")
            } else {
                ("fromEndpoints", "fromCIDR", "fromEntities")
            };
            match peer {
                Peer::Vms(sel) => {
                    rule.insert(ep_key.into(), json!([{ "matchLabels": sel }]));
                }
                Peer::Entity(n) => {
                    rule.insert(ent_key.into(), json!([n]));
                }
                Peer::Fqdn(n) => {
                    rule.insert("toFQDNs".into(), json!([{ "matchName": n }]));
                }
                Peer::Cidr(c) => {
                    rule.insert(cidr_key.into(), json!([c]));
                }
            }
            let mut to_ports = Vec::new();
            let mut icmps = Vec::new();
            for ((proto, port), pu) in ports {
                if proto.starts_with("ICMP") {
                    let family = if proto == "ICMPv6" { "IPv6" } else { "IPv4" };
                    icmps.push(json!({ "type": port, "family": family }));
                    continue;
                }
                let mut tp = json!({ "ports": [{ "port": port.to_string(), "protocol": proto }] });
                let dns_port = *eg && *port == 53;
                if !pu.plain {
                    if !pu.http.is_empty() {
                        let rules: Vec<Value> = pu
                            .http
                            .iter()
                            .map(|(m, p)| json!({ "method": m, "path": p }))
                            .collect();
                        tp["rules"] = json!({ "http": rules });
                    } else if !pu.dns.is_empty() {
                        let rules: Vec<Value> =
                            pu.dns.iter().map(|n| json!({ "matchName": n })).collect();
                        tp["rules"] = json!({ "dns": rules });
                    }
                    if !pu.sni.is_empty() {
                        tp["serverNames"] = json!(pu.sni);
                    }
                }
                if dns_port && g.fqdn && tp.get("rules").is_none() {
                    tp["rules"] = json!({ "dns": [{ "matchPattern": "*" }] });
                }
                to_ports.push(tp);
            }
            if !to_ports.is_empty() {
                rule.insert("toPorts".into(), Value::Array(to_ports));
            }
            if !icmps.is_empty() {
                rule.insert("icmps".into(), json!([{ "fields": icmps }]));
            }
            if *eg {
                egress.push(Value::Object(rule));
            } else {
                ingress.push(Value::Object(rule));
            }
        }
        if g.fqdn
            && !g
                .rules
                .iter()
                .any(|((eg, _), ports)| *eg && ports.keys().any(|(_, p)| *p == 53))
        {
            notes.insert(format!(
                "{name}: no DNS traffic in the history; toFQDNs needs the VM's resolver allowed on port 53"
            ));
        }
        let (ni, ne) = (ingress.len(), egress.len());
        let mut spec = json!({
            "description": format!(
                "Learned from {} flows seen {} .. {}",
                g.flows, g.first, g.last
            ),
            "endpointSelector": { "matchLabels": g.selector },
        });
        if !ingress.is_empty() {
            spec["ingress"] = Value::Array(ingress);
        } else if o.lock_unobserved {
            spec["ingress"] = json!([{}]);
        }
        if !egress.is_empty() {
            spec["egress"] = Value::Array(egress);
        } else if o.lock_unobserved {
            spec["egress"] = json!([{}]);
        }
        docs.push(json!({
            "apiVersion": "cilium.io/v2",
            "kind": "CiliumNetworkPolicy",
            "metadata": { "name": name, "labels": { "machina.io/learned": "true" } },
            "spec": spec,
        }));
        policies.push(LearnedPolicy {
            name,
            vms: g.vms.iter().cloned().collect(),
            ingress_rules: ni,
            egress_rules: ne,
        });
    }
    if docs.is_empty() {
        notes.insert("no usable flows in the history for this selection".into());
    }
    let yaml = docs
        .iter()
        .filter_map(|d| serde_yaml::to_string(d).ok())
        .collect::<Vec<_>>()
        .join("---\n");
    LearnResult {
        yaml,
        policies,
        edges_used: used,
        edges_skipped: skipped,
        notes: notes.into_iter().collect(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReplayChange {
    pub src: String,
    pub dst: String,
    pub proto: String,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<String>,
    pub flows: u64,
    pub last_seen: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReplayResult {
    pub evaluated: usize,
    pub unchanged: usize,
    /// Endpoints the tracer could not resolve (e.g. a VM that no longer exists).
    #[serde(default)]
    pub not_evaluated: usize,
    /// Allowed now, denied by the draft.
    pub would_break: Vec<ReplayChange>,
    /// Denied now, allowed by the draft.
    pub would_allow: Vec<ReplayChange>,
    pub flows_breaking: u64,
}

fn query(
    e: &VmFlowEdge,
    req: Option<&str>,
    kind: &str,
    fqdn: &BTreeMap<String, Vec<String>>,
) -> TraceQuery {
    let to = match fqdn.get(&e.dst).and_then(|n| n.first()) {
        Some(name) if e.dst_vm.is_none() => name.clone(),
        _ => e.dst.clone(),
    };
    let mut q = TraceQuery {
        from: e.src.clone(),
        to,
        protocol: e.proto.clone(),
        port: e.port,
        ..Default::default()
    };
    if e.proto.starts_with("ICMP") {
        q.port = 0;
        q.icmp_type = u8::try_from(e.port).ok();
    }
    if let Some(r) = req {
        match kind {
            "http" => {
                let (m, rest) = r.split_once(' ').unwrap_or(("GET", "/"));
                let (host, path) = rest
                    .find('/')
                    .map_or((rest, "/"), |i| (&rest[..i], &rest[i..]));
                q.http_method = Some(m.into());
                q.http_path = Some(path.replace("{id}", "1"));
                q.http_host = (!host.is_empty()).then(|| host.into());
            }
            "dns" => q.dns_name = r.strip_prefix("dns ").map(Into::into),
            "tls" => q.server_name = r.strip_prefix("tls sni=").map(Into::into),
            _ => {}
        }
    }
    q
}

fn verdict(t: &Tracer, q: &TraceQuery) -> Option<(bool, String)> {
    t.trace(q).ok().map(|r| (r.allowed, r.summary))
}

/// Fleet context for [`replay`].
pub struct ReplayInputs<'a> {
    pub vms: &'a [NetpolVm],
    pub services: &'a [NetpolService],
    pub host_addresses: &'a [String],
    pub remote_node_addresses: &'a [String],
    /// Address → DNS names, so traffic to learned addresses meets `toFQDNs`.
    pub fqdn: &'a BTreeMap<String, Vec<String>>,
    pub limit: usize,
}

/// Evaluate the history against the current and the draft policies, once
/// per distinct connection and L7 request.
pub fn replay(
    edges: &[VmFlowEdge],
    current: &[VmNetworkPolicy],
    draft: &[VmNetworkPolicy],
    x: &ReplayInputs,
) -> ReplayResult {
    let tracer = |p| {
        Tracer::new(
            p,
            x.vms,
            x.services,
            x.host_addresses,
            x.remote_node_addresses,
        )
    };
    let (now, next) = (tracer(current), tracer(draft));
    type Conn = (String, String, String, u16, Option<String>);
    let mut seen: BTreeMap<Conn, (u64, String, &str)> = BTreeMap::new();
    for e in edges
        .iter()
        .filter(|e| e.drop_reason.as_deref() != Some("spoofed-source"))
    {
        let mut add = |req: Option<String>, kind: &'static str, n: u64| {
            let k = (e.src.clone(), e.dst.clone(), e.proto.clone(), e.port, req);
            let v = seen.entry(k).or_insert((0, String::new(), kind));
            v.0 += n;
            if e.last_seen > v.1 {
                v.1 = e.last_seen.clone();
            }
        };
        let plain = e.count.saturating_sub(e.l7.iter().map(|s| s.count).sum());
        if plain > 0 || e.l7.is_empty() {
            add(None, "", plain.max(1));
        }
        for s in &e.l7 {
            let kind: &'static str = match s.kind.as_str() {
                "http" => "http",
                "dns" => "dns",
                "tls" => "tls",
                _ => "",
            };
            add(Some(s.request.clone()), kind, s.count);
        }
    }
    let mut out = ReplayResult::default();
    for ((src, dst, proto, port, req), (flows, last, kind)) in seen.into_iter().take(x.limit) {
        let e = VmFlowEdge {
            src: src.clone(),
            dst: dst.clone(),
            proto: proto.clone(),
            port,
            ..Default::default()
        };
        let q = query(&e, req.as_deref(), kind, x.fqdn);
        let (Some((a, before)), Some((b, after))) = (verdict(&now, &q), verdict(&next, &q)) else {
            out.not_evaluated += 1;
            continue;
        };
        out.evaluated += 1;
        if a == b {
            out.unchanged += 1;
            continue;
        }
        let ch = ReplayChange {
            src,
            dst,
            proto,
            port,
            request: req,
            flows,
            last_seen: last,
            before,
            after,
        };
        if a {
            out.flows_breaking += flows;
            out.would_break.push(ch);
        } else {
            out.would_allow.push(ch);
        }
    }
    out.would_break.sort_by_key(|c| std::cmp::Reverse(c.flows));
    out.would_allow.sort_by_key(|c| std::cmp::Reverse(c.flows));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::VmFlowL7Stat;
    use crate::netpol::parse_documents;

    fn vm(name: &str, app: &str, ip: &str) -> NetpolVm {
        NetpolVm {
            name: name.into(),
            labels: BTreeMap::from([("app".into(), app.into())]),
            addresses: vec![ip.into()],
            ..Default::default()
        }
    }

    fn edge(src: &str, dst: &str, dir: &str, port: u16) -> VmFlowEdge {
        VmFlowEdge {
            src: src.into(),
            dst: dst.into(),
            src_vm: (!src.contains('.')).then(|| src.into()),
            dst_vm: (!dst.contains('.')).then(|| dst.into()),
            direction: dir.into(),
            proto: "TCP".into(),
            port,
            verdict: "FORWARDED".into(),
            count: 5,
            first_seen: "2026-10-01T00:00:00Z".into(),
            last_seen: "2026-10-02T00:00:00Z".into(),
            ..Default::default()
        }
    }

    #[test]
    fn learns_least_privilege_and_replays() {
        let vms = vec![
            vm("web-1", "web", "10.0.0.2"),
            vm("web-2", "web", "10.0.0.3"),
            vm("db-1", "db", "10.0.0.9"),
        ];
        let mut api = edge("web-1", "db-1", "egress", 5432);
        api.l7 = vec![VmFlowL7Stat {
            kind: "http".into(),
            request: "GET api/users/{id}".into(),
            count: 3,
            ..Default::default()
        }];
        api.port = 8080;
        let edges = vec![
            edge("web-1", "db-1", "egress", 5432),
            edge("web-2", "db-1", "egress", 5432),
            edge("web-1", "db-1", "ingress", 5432),
            edge("web-2", "db-1", "ingress", 5432),
            edge("web-1", "db-1", "ingress", 8080),
            edge("web-1", "93.184.216.34", "egress", 443),
            VmFlowEdge {
                proto: "UDP".into(),
                dst_entity: Some("host".into()),
                ..edge("web-1", "10.0.0.1", "egress", 53)
            },
            api,
            VmFlowEdge {
                verdict: "DROPPED".into(),
                ..edge("web-1", "db-1", "egress", 22)
            },
        ];
        let o = LearnOptions {
            l7: true,
            fqdn: BTreeMap::from([("93.184.216.34".into(), vec!["example.com".into()])]),
            ..Default::default()
        };
        let r = learn(&edges, &vms, &o);
        assert_eq!(r.edges_skipped, 1, "{:?}", r.notes);
        let names: Vec<&str> = r.policies.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["learned-db", "learned-web"]);
        assert!(r.yaml.contains("toFQDNs"), "{}", r.yaml);
        assert!(
            r.yaml.contains("toEntities:") && r.yaml.contains("- host"),
            "{}",
            r.yaml
        );
        assert!(r.yaml.contains("/users/[^/]+"), "{}", r.yaml);
        assert!(
            !r.notes.iter().any(|n| n.contains("resolver")),
            "{:?}",
            r.notes
        );
        assert!(r.yaml.contains("matchPattern: '*'"), "{}", r.yaml);
        let (learned, v) = parse_documents(&r.yaml);
        assert!(v.errors.is_empty(), "{:?}\n{}", v.errors, r.yaml);

        let hosts = vec!["10.0.0.1".to_string()];
        let x = ReplayInputs {
            vms: &vms,
            services: &[],
            host_addresses: &hosts,
            remote_node_addresses: &[],
            fqdn: &o.fqdn,
            limit: 1000,
        };
        let res = replay(&edges, &[], &learned, &x);
        assert!(
            res.would_break.iter().all(|c| c.port == 22),
            "only the dropped SSH edge is outside the learned policy: {:?}",
            res.would_break
        );
        let (strict, _) = parse_documents(
            "apiVersion: cilium.io/v2\nkind: CiliumNetworkPolicy\nmetadata: {name: x}\nspec:\n  endpointSelector: {matchLabels: {app: db}}\n  ingress: [{}]\n",
        );
        let res = replay(&edges, &[], &strict, &x);
        assert!(res
            .would_break
            .iter()
            .any(|c| c.dst == "db-1" && c.port == 5432));
        assert!(res.flows_breaking > 0);
    }

    #[test]
    fn replay_sees_a_replaced_policy_narrow_ports() {
        let vms = vec![
            vm("np-client", "np-client", "10.0.0.2"),
            vm("np-server", "np-server", "10.0.0.9"),
        ];
        let mut e = edge("np-client", "np-server", "ingress", 80);
        e.l7 = vec![VmFlowL7Stat {
            kind: "http".into(),
            request: "GET 10.0.0.9/ok".into(),
            count: 5,
            ..Default::default()
        }];
        let doc = |port: &str, l7: &str| {
            format!(
                "apiVersion: cilium.io/v2\nkind: CiliumNetworkPolicy\nmetadata: {{name: np-realvm}}\nspec:\n  endpointSelector: {{matchLabels: {{app: np-server}}}}\n  ingress:\n    - fromEndpoints: [{{matchLabels: {{app: np-client}}}}]\n      toPorts:\n        - ports: [{{port: \"{port}\", protocol: TCP}}]\n{l7}"
            )
        };
        let (current, v) = parse_documents(&doc(
            "80",
            "          rules:\n            http: [{method: GET, path: /ok}]\n",
        ));
        assert!(v.errors.is_empty(), "{:?}", v.errors);
        let (draft, v) = parse_documents(&doc("81", ""));
        assert!(v.errors.is_empty(), "{:?}", v.errors);
        let x = ReplayInputs {
            vms: &vms,
            services: &[],
            host_addresses: &[],
            remote_node_addresses: &[],
            fqdn: &BTreeMap::new(),
            limit: 1000,
        };
        let res = replay(&[e], &current, &draft, &x);
        assert_eq!(res.would_break.len(), 1, "{res:?}");
    }
}
