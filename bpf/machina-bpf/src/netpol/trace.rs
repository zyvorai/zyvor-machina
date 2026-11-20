// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Policy trace (`cilium policy trace` for VMs): evaluate one connection
//! against the compiled fleet state with the datapath's lookup order —
//! egress at the source VM, then ingress at the destination VM.

use std::collections::HashMap;

use machina_bpf_common::{IDENTITY_HOST, IDENTITY_WORLD};
use serde::{Deserialize, Serialize};

use super::compile::{compile, Inputs, NetpolService, NetpolVm, IDENTITY_REMOTE_NODE};
use super::{fqdn, l7, VmNetworkPolicy};
use crate::api::VmEdgeState;
use crate::policy::parse_prefix;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TraceQuery {
    /// VM name, IP address, DNS name, `world`, `host` or `remote-node`.
    pub from: String,
    pub to: String,
    /// TCP (default), UDP, SCTP, ICMP, ICMPv6 or ANY.
    #[serde(default)]
    pub protocol: String,
    #[serde(default)]
    pub port: u16,
    /// For ICMP / ICMPv6.
    #[serde(default)]
    pub icmp_type: Option<u8>,
    /// L7 request to evaluate against L7 rules (HTTP, Kafka, TLS, DNS).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_host: Option<String>,
    /// `Name: value`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub http_headers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kafka_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kafka_api_version: Option<i16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kafka_client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kafka_topic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns_name: Option<String>,
}

impl TraceQuery {
    fn l7_request(&self) -> Option<l7::Request> {
        if let Some(n) = &self.dns_name {
            return Some(l7::Request::Dns {
                name: fqdn::normalize(n),
            });
        }
        if let Some(n) = &self.server_name {
            return Some(l7::Request::Tls {
                server_name: fqdn::normalize(n),
            });
        }
        if let Some(k) = &self.kafka_api_key {
            return Some(l7::Request::Kafka(l7::KafkaRequest {
                api_key: l7::kafka_api_key(k).unwrap_or(-1),
                api_version: self.kafka_api_version.unwrap_or(0),
                client_id: self.kafka_client_id.clone().unwrap_or_default(),
                topics: Some(self.kafka_topic.iter().cloned().collect()),
            }));
        }
        if self.http_method.is_some() || self.http_path.is_some() || self.http_host.is_some() {
            return Some(l7::Request::Http(l7::HttpRequest {
                method: self
                    .http_method
                    .clone()
                    .unwrap_or_else(|| "GET".into())
                    .to_ascii_uppercase(),
                path: self.http_path.clone().unwrap_or_else(|| "/".into()),
                host: self.http_host.clone().unwrap_or_default(),
                headers: self
                    .http_headers
                    .iter()
                    .filter_map(|h| h.split_once(':'))
                    .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_string()))
                    .collect(),
            }));
        }
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TraceEndpoint {
    pub input: String,
    #[serde(default)]
    pub vm: Option<String>,
    pub identity: u32,
    /// `vm`, `host`, `remote-node`, `cidr`, `fqdn` or `world`.
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TraceSide {
    /// `egress` (at the source VM) or `ingress` (at the destination VM).
    pub direction: String,
    #[serde(default)]
    pub vm: Option<String>,
    /// The VM is default-deny in this direction.
    pub enforced: bool,
    /// `allowed`, `denied`, `default-deny`, `l7-denied`, `auth-failed`,
    /// `no-policy` or `not-a-vm`.
    pub verdict: String,
    #[serde(default)]
    pub rule: Option<String>,
    /// `required` / `test-always-fail` when the allow needs mutual authentication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<String>,
    /// L7 enforcement on this hop: the request verdict, or which rules apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l7: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TraceResult {
    pub allowed: bool,
    pub from: TraceEndpoint,
    pub to: TraceEndpoint,
    pub protocol: String,
    pub port: u16,
    pub egress: TraceSide,
    pub ingress: TraceSide,
    pub summary: String,
}

fn proto_num(p: &str) -> Result<u8, String> {
    match p.to_ascii_uppercase().as_str() {
        "" | "TCP" => Ok(6),
        "UDP" => Ok(17),
        "SCTP" => Ok(132),
        "ICMP" => Ok(1),
        "ICMPV6" => Ok(58),
        "ANY" => Ok(0),
        o => Err(format!("unknown protocol `{o}`")),
    }
}

type Key = (u32, u32, bool, u8, u16);

struct Table {
    /// (subject, peer, egress, proto, port) → (deny, source)
    map: super::RuleIndex,
    /// Allow keys → (auth mode OR-ed, every allow entry has L7 rules).
    flags: HashMap<Key, (u8, bool)>,
}

/// One evaluated hop: deny?, deciding rule, auth mode, L7 applies.
struct Hit {
    deny: bool,
    source: String,
    auth: u8,
    l7: bool,
}

impl Table {
    fn new(state: &VmEdgeState) -> Self {
        let mut map = HashMap::new();
        let mut flags: HashMap<Key, (u8, bool)> = HashMap::new();
        for r in &state.policy {
            let (Some(s), Some(p)) = (r.subject_identity, r.peer_identity) else {
                continue;
            };
            let end = if r.port_end > r.port {
                r.port_end
            } else {
                r.port
            };
            for port in r.port..=end {
                let k = (s, p, r.egress, r.proto, port);
                let e = map
                    .entry(k)
                    .or_insert((r.deny, r.source.clone().unwrap_or_default()));
                if r.deny && !e.0 {
                    *e = (true, r.source.clone().unwrap_or_default());
                }
                if !r.deny {
                    let f = flags.entry(k).or_insert((0, true));
                    f.0 |= r.auth;
                    f.1 &= r.l7;
                }
            }
        }
        Table { map, flags }
    }

    /// Same wildcard order as the kernel; any deny match wins; auth is
    /// required when any matching allow requires it; L7 applies only when
    /// every matching allow carries L7 rules.
    fn eval(&self, subject: u32, peer: u32, egress: bool, proto: u8, port: u16) -> Option<Hit> {
        let combos = [
            (peer, proto, port),
            (peer, proto, 0),
            (0, proto, port),
            (0, proto, 0),
            (peer, 0, 0),
            (0, 0, 0),
        ];
        let keys: Vec<Key> = combos
            .iter()
            .map(|(pe, pr, po)| (subject, *pe, egress, *pr, *po))
            .filter(|k| self.map.contains_key(k))
            .collect();
        if let Some(k) = keys.iter().find(|k| self.map[*k].0) {
            return Some(Hit {
                deny: true,
                source: self.map[k].1.clone(),
                auth: 0,
                l7: false,
            });
        }
        let first = keys.first()?;
        let mut auth = 0;
        let mut l7 = true;
        for k in &keys {
            let (a, l) = self.flags.get(k).copied().unwrap_or((0, false));
            auth |= a;
            l7 &= l;
        }
        Some(Hit {
            deny: false,
            source: self.map[first].1.clone(),
            auth,
            l7,
        })
    }
}

/// L7 rule sets that apply to one hop (same keys the kernel matched).
fn l7_rules(
    state: &VmEdgeState,
    subject: u32,
    peer: u32,
    egress: bool,
    proto: u8,
    port: u16,
) -> Vec<&l7::L7Rules> {
    state
        .l7
        .iter()
        .filter(|r| {
            r.subject_identity == subject
                && r.egress == egress
                && (r.peer_identity == 0 || r.peer_identity == peer)
        })
        .filter(|r| r.proto == 0 || r.proto == proto)
        .filter(|r| r.port == 0 || (r.port..=r.port_end.max(r.port)).contains(&port))
        .map(|r| &r.rules)
        .collect()
}

fn resolve(
    input: &str,
    vms: &[NetpolVm],
    state: &VmEdgeState,
    compiled_ids: &HashMap<String, u32>,
) -> TraceEndpoint {
    let ep = |vm: Option<String>, identity, kind: &str| TraceEndpoint {
        input: input.into(),
        vm,
        identity,
        kind: kind.into(),
    };
    if let Some(id) = compiled_ids.get(input) {
        return ep(Some(input.into()), *id, "vm");
    }
    match input {
        "world" => return ep(None, IDENTITY_WORLD, "world"),
        "host" => return ep(None, IDENTITY_HOST, "host"),
        "remote-node" => return ep(None, IDENTITY_REMOTE_NODE, "remote-node"),
        _ => {}
    }
    let Ok(addr) = parse_prefix(input) else {
        let kind = if fqdn::invalid(input, false).is_none() && input.contains('.') {
            "fqdn"
        } else {
            "world"
        };
        return ep(None, IDENTITY_WORLD, kind);
    };
    if let Some(vm) = vms.iter().find(|v| {
        v.addresses
            .iter()
            .any(|a| parse_prefix(a).ok() == Some(addr))
    }) {
        return ep(
            Some(vm.name.clone()),
            compiled_ids.get(&vm.name).copied().unwrap_or(0),
            "vm",
        );
    }
    let mut best: Option<(u32, u32)> = None;
    for p in &state.peers {
        let Ok(pp) = parse_prefix(&p.cidr) else {
            continue;
        };
        let inside = addr.bits >= pp.bits && {
            let mut a = addr.addr;
            for (i, b) in a.iter_mut().enumerate() {
                let start = i as u32 * 8;
                if start >= pp.bits {
                    *b = 0;
                } else if start + 8 > pp.bits {
                    *b &= 0xffu8 << (8 - (pp.bits - start));
                }
            }
            a == pp.addr
        };
        if inside && best.is_none_or(|(bits, _)| pp.bits > bits) {
            best = Some((pp.bits, p.identity));
        }
    }
    match best {
        Some((_, IDENTITY_HOST)) => ep(None, IDENTITY_HOST, "host"),
        Some((_, IDENTITY_REMOTE_NODE)) => ep(None, IDENTITY_REMOTE_NODE, "remote-node"),
        Some((_, id)) => ep(None, id, "cidr"),
        None => ep(None, IDENTITY_WORLD, "world"),
    }
}

/// Trace one connection across the fleet.
pub fn trace(
    policies: &[VmNetworkPolicy],
    vms: &[NetpolVm],
    services: &[NetpolService],
    host_addresses: &[String],
    remote_node_addresses: &[String],
    q: &TraceQuery,
) -> Result<TraceResult, String> {
    Tracer::new(
        policies,
        vms,
        services,
        host_addresses,
        remote_node_addresses,
    )
    .trace(q)
}

/// The fleet compiled once, for tracing many connections.
pub struct Tracer {
    vms: Vec<NetpolVm>,
    state: VmEdgeState,
    ids: HashMap<String, u32>,
    iso: HashMap<u32, (bool, bool)>,
    table: Table,
}

impl Tracer {
    pub fn new(
        policies: &[VmNetworkPolicy],
        vms: &[NetpolVm],
        services: &[NetpolService],
        host_addresses: &[String],
        remote_node_addresses: &[String],
    ) -> Self {
        let c = compile(&Inputs {
            policies,
            vms,
            services,
            host: None,
            host_addresses,
            remote_node_addresses,
        });
        let ids = c
            .endpoints
            .iter()
            .map(|e| (e.name.clone(), e.identity))
            .collect();
        let iso = c
            .endpoints
            .iter()
            .map(|e| (e.identity, (e.ingress_enforced, e.egress_enforced)))
            .collect();
        let table = Table::new(&c.state);
        Tracer {
            vms: vms.to_vec(),
            state: c.state,
            ids,
            iso,
            table,
        }
    }

    pub fn trace(&self, q: &TraceQuery) -> Result<TraceResult, String> {
        trace_with(self, q)
    }
}

fn trace_with(t: &Tracer, q: &TraceQuery) -> Result<TraceResult, String> {
    let proto = proto_num(&q.protocol)?;
    let port = if proto == 1 || proto == 58 {
        q.icmp_type.map_or(0, |t| t as u16 + 1)
    } else {
        q.port
    };
    let (c_state, ids, iso, table) = (&t.state, &t.ids, &t.iso, &t.table);
    let from = resolve(&q.from, &t.vms, c_state, ids);
    let to = resolve(&q.to, &t.vms, c_state, ids);
    let side = |subject: &TraceEndpoint, peer: &TraceEndpoint, egress: bool| -> TraceSide {
        let direction = if egress { "egress" } else { "ingress" }.to_string();
        if subject.kind != "vm" {
            return TraceSide {
                direction,
                verdict: "not-a-vm".into(),
                ..Default::default()
            };
        }
        let enforced = iso
            .get(&subject.identity)
            .is_some_and(|(i, e)| if egress { *e } else { *i });
        let mut hit = table.eval(subject.identity, peer.identity, egress, proto, port);
        let mut rules = l7_rules(
            c_state,
            subject.identity,
            peer.identity,
            egress,
            proto,
            port,
        );
        if egress && peer.kind == "fqdn" && !hit.as_ref().is_some_and(|h| h.deny) {
            let by_name: Vec<_> = c_state
                .fqdn
                .iter()
                .filter(|r| {
                    let end = if r.port_end > r.port {
                        r.port_end
                    } else {
                        r.port
                    };
                    r.subject_identity == subject.identity
                        && (r.proto == 0 || r.proto == proto)
                        && (r.port == 0 || (r.port..=end).contains(&port))
                        && fqdn::matches(&r.pattern, &peer.input)
                })
                .collect();
            if let Some(r) = by_name.first() {
                let plain =
                    by_name.iter().any(|r| r.l7.is_none()) || hit.as_ref().is_some_and(|h| !h.l7);
                rules.extend(by_name.iter().filter_map(|r| r.l7.as_ref()));
                hit = Some(Hit {
                    deny: false,
                    source: r.source.clone().unwrap_or_default(),
                    auth: hit.as_ref().map_or(0, |h| h.auth),
                    l7: !plain,
                });
            }
        }
        let mut side = TraceSide {
            direction,
            vm: subject.vm.clone(),
            enforced,
            ..Default::default()
        };
        match hit {
            Some(h) if h.deny => {
                side.verdict = "denied".into();
                side.rule = Some(h.source);
            }
            Some(h) => {
                side.verdict = "allowed".into();
                side.rule = Some(h.source);
                side.auth = match h.auth {
                    0 => None,
                    a if a & crate::api::AUTH_ALWAYS_FAIL != 0 => Some("test-always-fail".into()),
                    _ => Some("required".into()),
                };
                if side.auth.as_deref() == Some("test-always-fail") {
                    side.verdict = "auth-failed".into();
                }
                if h.l7 && side.verdict == "allowed" {
                    let kinds: std::collections::BTreeSet<&str> =
                        rules.iter().map(|r| r.kind()).collect();
                    let kinds = kinds.into_iter().collect::<Vec<_>>().join("/");
                    side.l7 = Some(match q.l7_request() {
                        Some(req) => {
                            let (ok, note) = l7::Matcher::new(rules.iter().copied()).check(&req);
                            if !ok {
                                side.verdict = "l7-denied".into();
                            }
                            let mut s = format!("{} {}", req.summary(), if ok { "allowed" } else { "denied" });
                            if let Some(n) = note {
                                s.push_str(&format!(" ({n})"));
                            }
                            s
                        }
                        None => format!("{kinds} rules apply; pass a request (e.g. --http-method GET --http-path /) to evaluate it"),
                    });
                }
            }
            None if enforced => side.verdict = "default-deny".into(),
            None => side.verdict = "no-policy".into(),
        }
        side
    };
    let egress = side(&from, &to, true);
    let ingress = side(&to, &from, false);
    let pass = |s: &TraceSide| matches!(s.verdict.as_str(), "allowed" | "no-policy" | "not-a-vm");
    let allowed = pass(&egress) && pass(&ingress);
    let why = |s: &TraceSide| match (&s.verdict[..], &s.rule) {
        ("denied", Some(r)) => format!("denied at {} by {r}", s.direction),
        ("l7-denied", _) => format!(
            "L7 denied at {}: {}",
            s.direction,
            s.l7.as_deref().unwrap_or("")
        ),
        ("auth-failed", _) => format!("authentication fails at {} (test-always-fail)", s.direction),
        ("default-deny", _) => format!("no {} rule allows it (default deny)", s.direction),
        _ => String::new(),
    };
    let summary = if allowed {
        let mut notes = Vec::new();
        if egress.auth.is_some() || ingress.auth.is_some() {
            notes.push("once both identities are authenticated");
        }
        if egress.l7.is_some() || ingress.l7.is_some() {
            notes.push("per request by L7 rules");
        }
        if notes.is_empty() {
            "ALLOWED".to_string()
        } else {
            format!("ALLOWED ({})", notes.join(", "))
        }
    } else {
        let reasons: Vec<String> = [why(&egress), why(&ingress)]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();
        format!("DENIED: {}", reasons.join("; "))
    };
    Ok(TraceResult {
        allowed,
        from,
        to,
        protocol: if q.protocol.is_empty() {
            "TCP".into()
        } else {
            q.protocol.to_ascii_uppercase()
        },
        port: q.port,
        egress,
        ingress,
        summary,
    })
}
