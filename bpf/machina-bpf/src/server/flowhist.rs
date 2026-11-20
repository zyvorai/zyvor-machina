// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! VM flow history and lateral-movement detection.
//!
//! Every VM edge record is folded into an edge (source, destination,
//! direction, protocol, port, verdict, reason, policy) with counts and a
//! normalised L7 breakdown, kept for seven days across restarts. The same
//! records feed a one-minute sliding window per source that raises port-scan,
//! host-sweep and deny-burst alerts, and a new-peer alert once the history is
//! a day old.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};

use crate::api::{VmFlowAlert, VmFlowEdge, VmFlowL7Stat, VmFlowRecord};

const EDGE_CAP: usize = 20_000;
const L7_PER_EDGE: usize = 32;
const RETAIN_DAYS: i64 = 7;
const ALERT_CAP: usize = 1000;
const WINDOW: Duration = Duration::from_secs(60);
const WINDOW_CAP: usize = 4096;
const SCAN_PORTS: usize = 20;
const SWEEP_HOSTS: usize = 20;
const DENY_BURST: usize = 50;
const ALERT_QUIET: Duration = Duration::from_secs(600);
const NEW_PEER_WARMUP_HOURS: i64 = 24;

type EdgeKey = (String, String, String, String, u16, String, String, String);

const DOMAIN_CAP: usize = 50_000;

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Saved {
    since: Option<String>,
    edges: Vec<VmFlowEdge>,
    /// Base domain → first seen, for `new_domain` alerts.
    #[serde(default)]
    domains: HashMap<String, String>,
}

struct Seen {
    at: Instant,
    dst: String,
    port: u16,
    denied: bool,
}

#[derive(Default)]
pub(super) struct FlowHistory {
    edges: HashMap<EdgeKey, VmFlowEdge>,
    since: Option<String>,
    path: Option<PathBuf>,
    dirty: bool,
    windows: HashMap<String, VecDeque<Seen>>,
    quiet: HashMap<(String, String, String), Instant>,
    domains: HashMap<String, String>,
    pub alerts: VecDeque<VmFlowAlert>,
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn ago_rfc3339(d: chrono::Duration) -> String {
    (Utc::now() - d).to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Flow records name reserved identities and CIDR peers like VMs (Hubble
/// style); the history keeps those by address so learn and the map see them.
fn is_entity(name: &str) -> bool {
    matches!(name, "host" | "world" | "remote-node")
        || name.contains('/')
        || name.parse::<std::net::IpAddr>().is_ok()
}

/// (VM, entity) for one side of a record.
fn side(name: &Option<String>) -> (Option<String>, Option<String>) {
    match name {
        Some(n) if is_entity(n) => (None, Some(n.clone())),
        n => (n.clone(), None),
    }
}

fn endpoint(name: &Option<String>, addr: &str) -> String {
    side(name).0.unwrap_or_else(|| addr.to_string())
}

fn edge_port(r: &VmFlowRecord) -> u16 {
    r.icmp_type.map_or(r.dst_port, u16::from)
}

/// Policy spelling (`TCP`, `ICMPv6`), whatever the record producer used.
fn cilium_proto(p: &str) -> String {
    match p.to_ascii_lowercase().as_str() {
        "icmpv6" => "ICMPv6".into(),
        _ => p.to_ascii_uppercase(),
    }
}

fn key_of(r: &VmFlowRecord) -> EdgeKey {
    (
        endpoint(&r.src_vm, &r.src),
        endpoint(&r.dst_vm, &r.dst),
        r.direction.clone(),
        cilium_proto(&r.proto),
        edge_port(r),
        r.verdict.clone(),
        r.drop_reason.clone().unwrap_or_default(),
        r.policy.clone().unwrap_or_default(),
    )
}

/// Drop trailing notes (`(tls intercepted)`, `[denied]`, `(header … added)`).
fn strip_notes(s: &str) -> &str {
    let mut s = s.trim_end();
    loop {
        let cut = if s.ends_with(']') {
            s.rfind(" [")
        } else if s.ends_with(')') {
            s.rfind(" (")
        } else {
            None
        };
        match cut {
            Some(i) => s = s[..i].trim_end(),
            None => return s,
        }
    }
}

fn id_like(seg: &str) -> bool {
    if seg.is_empty() {
        return false;
    }
    let digits = seg.bytes().all(|b| b.is_ascii_digit());
    let hex = seg.len() >= 8 && seg.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-');
    digits || hex
}

/// `GET api.shop/users/42?x=1 (note)` → `GET api.shop/users/{id}`.
pub(crate) fn normalize_l7(kind: &str, summary: &str) -> String {
    let s = strip_notes(summary);
    if kind != "http" {
        return s.to_string();
    }
    let Some((method, rest)) = s.split_once(' ') else {
        return s.to_string();
    };
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let path: Vec<&str> = path
        .split('/')
        .map(|seg| if id_like(seg) { "{id}" } else { seg })
        .collect();
    format!("{method} {host}{}", path.join("/"))
}

fn status_class(code: u16) -> String {
    format!("{}xx", code / 100)
}

impl FlowHistory {
    pub(super) fn load(&mut self, path: &Path) {
        self.path = Some(path.to_path_buf());
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        match serde_json::from_str::<Saved>(&text) {
            Ok(s) => {
                self.since = s.since;
                self.domains = s.domains;
                for mut e in s.edges {
                    e.proto = cilium_proto(&e.proto);
                    if e.src_entity.is_none() {
                        (e.src_vm, e.src_entity) = side(&e.src_vm);
                    }
                    if e.dst_entity.is_none() {
                        (e.dst_vm, e.dst_entity) = side(&e.dst_vm);
                    }
                    let key = (
                        e.src.clone(),
                        e.dst.clone(),
                        e.direction.clone(),
                        e.proto.clone(),
                        e.port,
                        e.verdict.clone(),
                        e.drop_reason.clone().unwrap_or_default(),
                        e.policy.clone().unwrap_or_default(),
                    );
                    self.edges.insert(key, e);
                }
            }
            Err(e) => tracing::warn!("flow history {}: {e}", path.display()),
        }
    }

    fn prune(&mut self) {
        let cutoff = ago_rfc3339(chrono::Duration::days(RETAIN_DAYS));
        let before = self.edges.len();
        self.edges.retain(|_, e| e.last_seen >= cutoff);
        if self.edges.len() > EDGE_CAP {
            let mut ages: Vec<(String, EdgeKey)> = self
                .edges
                .iter()
                .map(|(k, e)| (e.last_seen.clone(), k.clone()))
                .collect();
            ages.sort();
            let drop = self.edges.len() - EDGE_CAP * 9 / 10;
            for (_, k) in ages.into_iter().take(drop) {
                self.edges.remove(&k);
            }
        }
        if self.edges.len() != before {
            self.dirty = true;
        }
    }

    pub(super) fn save(&mut self) {
        self.prune();
        if !self.dirty {
            return;
        }
        let Some(path) = self.path.clone() else {
            return;
        };
        let saved = Saved {
            since: self.since.clone(),
            edges: self.edges.values().cloned().collect(),
            domains: self.domains.clone(),
        };
        let tmp = path.with_extension("json.tmp");
        let res = serde_json::to_vec(&saved)
            .map_err(anyhow::Error::from)
            .and_then(|b| std::fs::write(&tmp, b).map_err(Into::into))
            .and_then(|_| std::fs::rename(&tmp, &path).map_err(Into::into));
        match res {
            Ok(()) => self.dirty = false,
            Err(e) => tracing::warn!("flow history save: {e:#}"),
        }
    }

    pub(super) fn reset(&mut self) {
        self.edges.clear();
        self.since = None;
        self.domains.clear();
        self.windows.clear();
        self.alerts.clear();
        self.dirty = true;
    }

    pub(super) fn edges(&self, vm: Option<&str>) -> Vec<VmFlowEdge> {
        let mut out: Vec<VmFlowEdge> = self
            .edges
            .values()
            .filter(|e| {
                vm.is_none_or(|v| e.src_vm.as_deref() == Some(v) || e.dst_vm.as_deref() == Some(v))
            })
            .cloned()
            .collect();
        out.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
        out
    }

    /// Fold one record in; returns any alerts it raised.
    pub(super) fn observe(&mut self, r: &VmFlowRecord) -> Vec<VmFlowAlert> {
        let now = now_rfc3339();
        if self.since.is_none() {
            self.since = Some(now.clone());
        }
        let key = key_of(r);
        let (src_vm, src_entity) = side(&r.src_vm);
        let (dst_vm, dst_entity) = side(&r.dst_vm);
        let is_new = !self.edges.contains_key(&key);
        let new_pair = is_new
            && src_vm.is_some()
            && dst_vm.is_some()
            && !self
                .edges
                .keys()
                .any(|k| k.0 == key.0 && k.1 == key.1 && k.3 == key.3 && k.4 == key.4);
        let e = self.edges.entry(key.clone()).or_insert_with(|| VmFlowEdge {
            src: key.0.clone(),
            dst: key.1.clone(),
            src_vm: src_vm.clone(),
            dst_vm,
            src_entity,
            dst_entity,
            direction: r.direction.clone(),
            proto: key.3.clone(),
            port: key.4,
            verdict: r.verdict.clone(),
            drop_reason: r.drop_reason.clone(),
            policy: r.policy.clone(),
            first_seen: now.clone(),
            ..Default::default()
        });
        e.src_labels = r.src_labels.clone();
        e.dst_labels = r.dst_labels.clone();
        e.count += 1;
        e.bytes += u64::from(r.bytes);
        e.last_seen = now.clone();
        if let (Some(kind), Some(sum)) = (&r.l7_type, &r.l7) {
            let req = normalize_l7(kind, sum);
            let denied = r.drop_reason.as_deref() == Some("l7-deny");
            match e
                .l7
                .iter()
                .position(|s| s.kind == *kind && s.request == req)
            {
                Some(i) => {
                    e.l7[i].count += 1;
                    e.l7[i].denied += u64::from(denied);
                }
                None if e.l7.len() < L7_PER_EDGE => e.l7.push(VmFlowL7Stat {
                    kind: kind.clone(),
                    request: req,
                    count: 1,
                    denied: u64::from(denied),
                    ..Default::default()
                }),
                None => {}
            }
        }
        self.dirty = true;
        if self.edges.len() > EDGE_CAP {
            self.prune();
        }

        let mut alerts = self.detect(r, &key, &src_vm);
        if new_pair && self.warm() && r.direction == "egress" {
            alerts.extend(self.raise(VmFlowAlert {
                kind: "new_peer".into(),
                severity: "low".into(),
                src: key.0.clone(),
                src_vm: src_vm.clone(),
                dst: Some(key.1.clone()),
                detail: format!(
                    "{} talked to {} on {}/{} for the first time",
                    key.0, key.1, r.proto, key.4
                ),
                count: 1,
                ..Default::default()
            }));
        }
        alerts
    }

    /// Response to a proxied HTTP request on the edge of `r`.
    pub(super) fn observe_response(&mut self, r: &VmFlowRecord, status: u16, latency_ms: u64) {
        let (Some(kind), Some(sum)) = (&r.l7_type, &r.l7) else {
            return;
        };
        let req = normalize_l7(kind, sum);
        let Some(e) = self.edges.get_mut(&key_of(r)) else {
            return;
        };
        if let Some(s) =
            e.l7.iter_mut()
                .find(|s| s.kind == *kind && s.request == req)
        {
            *s.status.entry(status_class(status)).or_insert(0) += 1;
            s.latency_n += 1;
            s.latency_ms_total += latency_ms;
            s.latency_ms_max = s.latency_ms_max.max(latency_ms);
            self.dirty = true;
        }
    }

    fn warm(&self) -> bool {
        self.since.as_deref().is_some_and(|s| {
            s <= ago_rfc3339(chrono::Duration::hours(NEW_PEER_WARMUP_HOURS)).as_str()
        })
    }

    /// A VM resolved a name listed by a threat feed.
    pub(super) fn threat_alert(
        &mut self,
        vm: &str,
        client: &str,
        qname: &str,
        detail: String,
    ) -> Option<VmFlowAlert> {
        self.raise(VmFlowAlert {
            kind: "threat_domain".into(),
            severity: "high".into(),
            src: client.to_string(),
            src_vm: Some(vm.to_string()),
            dst: Some(qname.trim_end_matches('.').to_ascii_lowercase()),
            detail,
            count: 1,
            ..Default::default()
        })
    }

    /// Note the base domain of a successful VM lookup; a `new_domain` alert
    /// the first time one is seen after the warm-up day.
    pub(super) fn observe_domain(
        &mut self,
        vm: &str,
        client: &str,
        qname: &str,
    ) -> Option<VmFlowAlert> {
        let name = crate::netpol::threat::normalize(qname)?;
        if name.ends_with(".arpa") {
            return None;
        }
        let base = crate::netpol::threat::base_domain(&name);
        if self.domains.contains_key(&base) {
            return None;
        }
        if self.since.is_none() {
            self.since = Some(now_rfc3339());
        }
        if self.domains.len() >= DOMAIN_CAP {
            return None;
        }
        self.domains.insert(base.clone(), now_rfc3339());
        self.dirty = true;
        if !self.warm() {
            return None;
        }
        self.raise(VmFlowAlert {
            kind: "new_domain".into(),
            severity: "low".into(),
            src: client.to_string(),
            src_vm: Some(vm.to_string()),
            dst: Some(base.clone()),
            detail: format!("{vm} resolved {name}, the first lookup under {base}"),
            count: 1,
            ..Default::default()
        })
    }

    fn raise(&mut self, mut a: VmFlowAlert) -> Option<VmFlowAlert> {
        let qk = (
            a.kind.clone(),
            a.src.clone(),
            a.dst.clone().unwrap_or_default(),
        );
        let now = Instant::now();
        if self
            .quiet
            .get(&qk)
            .is_some_and(|t| now.duration_since(*t) < ALERT_QUIET)
        {
            return None;
        }
        if self.quiet.len() > 10_000 {
            self.quiet
                .retain(|_, t| now.duration_since(*t) < ALERT_QUIET);
        }
        self.quiet.insert(qk, now);
        if a.ts.is_empty() {
            a.ts = now_rfc3339();
        }
        self.alerts.push_back(a.clone());
        while self.alerts.len() > ALERT_CAP {
            self.alerts.pop_front();
        }
        Some(a)
    }

    fn detect(
        &mut self,
        r: &VmFlowRecord,
        key: &EdgeKey,
        src_vm: &Option<String>,
    ) -> Vec<VmFlowAlert> {
        // A VM-to-VM connection shows up at both taps; count it at the client.
        if r.direction != "egress" && src_vm.is_some() {
            return vec![];
        }
        // Already contained: its drops would only re-alert on it.
        if r.drop_reason.as_deref() == Some("quarantine") {
            return vec![];
        }
        let now = Instant::now();
        let denied = r.verdict == "DROPPED" || r.drop_reason.is_some();
        let w = self.windows.entry(key.0.clone()).or_default();
        w.push_back(Seen {
            at: now,
            dst: key.1.clone(),
            port: key.4,
            denied,
        });
        while w
            .front()
            .is_some_and(|s| now.duration_since(s.at) > WINDOW || w.len() > WINDOW_CAP)
        {
            w.pop_front();
        }
        let ports: HashSet<u16> = w
            .iter()
            .filter(|s| s.dst == key.1)
            .map(|s| s.port)
            .collect();
        let hosts: HashSet<&str> = w
            .iter()
            .filter(|s| s.port == key.4)
            .map(|s| s.dst.as_str())
            .collect();
        let denies = w.iter().filter(|s| s.denied).count();
        let (np, nh) = (ports.len(), hosts.len());
        if self.windows.len() > 50_000 {
            self.windows
                .retain(|_, w| w.back().is_some_and(|s| now.duration_since(s.at) < WINDOW));
        }

        let mut out = Vec::new();
        let base = VmFlowAlert {
            src: key.0.clone(),
            src_vm: src_vm.clone(),
            ..Default::default()
        };
        if np >= SCAN_PORTS {
            out.extend(self.raise(VmFlowAlert {
                kind: "port_scan".into(),
                severity: "high".into(),
                dst: Some(key.1.clone()),
                detail: format!("{} probed {np} ports on {} within a minute", key.0, key.1),
                count: np as u64,
                ..base.clone()
            }));
        }
        if nh >= SWEEP_HOSTS {
            out.extend(self.raise(VmFlowAlert {
                kind: "host_sweep".into(),
                severity: "high".into(),
                dst: Some(format!("*:{}", key.4)),
                detail: format!(
                    "{} reached {nh} hosts on {}/{} within a minute",
                    key.0, r.proto, key.4
                ),
                count: nh as u64,
                ..base.clone()
            }));
        }
        if denies >= DENY_BURST {
            out.extend(self.raise(VmFlowAlert {
                kind: "deny_burst".into(),
                severity: "medium".into(),
                detail: format!("{} had {denies} denied flows within a minute", key.0),
                count: denies as u64,
                ..base
            }));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(src: &str, dst: &str, port: u16, verdict: &str) -> VmFlowRecord {
        VmFlowRecord {
            src: "10.0.0.1".into(),
            dst: format!("10.0.0.{}", port % 250),
            src_vm: Some(src.into()),
            dst_vm: (!dst.is_empty()).then(|| dst.into()),
            direction: "egress".into(),
            proto: "TCP".into(),
            dst_port: port,
            verdict: verdict.into(),
            bytes: 60,
            ..Default::default()
        }
    }

    #[test]
    fn folds_edges_and_l7() {
        let mut h = FlowHistory::default();
        let mut r = rec("web", "db", 5432, "FORWARDED");
        h.observe(&r);
        r.proto = "tcp".into();
        h.observe(&r);
        r.l7_type = Some("http".into());
        r.l7 = Some("GET api/users/42?x=1 (tls intercepted)".into());
        h.observe(&r);
        r.l7 = Some("GET api/users/7 [denied]".into());
        r.drop_reason = Some("l7-deny".into());
        h.observe(&r);
        let e = h.edges(Some("web"));
        assert_eq!(e.len(), 2);
        let ok = e.iter().find(|e| e.drop_reason.is_none()).unwrap();
        assert_eq!(ok.count, 3);
        assert_eq!(ok.proto, "TCP");
        assert_eq!(ok.l7[0].request, "GET api/users/{id}");
        r.drop_reason = None;
        r.l7 = Some("GET api/users/9".into());
        h.observe_response(&r, 503, 40);
        let e = h.edges(Some("web"));
        let ok = e.iter().find(|e| e.drop_reason.is_none()).unwrap();
        assert_eq!(ok.l7[0].status.get("5xx"), Some(&1));
        assert_eq!(ok.l7[0].latency_ms_max, 40);
    }

    #[test]
    fn reserved_peers_keep_their_address() {
        let mut h = FlowHistory::default();
        let mut r = rec("web", "host", 53, "FORWARDED");
        r.dst = "192.168.122.1".into();
        h.observe(&r);
        r.dst_vm = Some("world".into());
        r.dst = "1.1.1.1".into();
        h.observe(&r);
        let e = h.edges(Some("web"));
        assert_eq!(e.len(), 2);
        let host = e.iter().find(|e| e.dst == "192.168.122.1").unwrap();
        assert_eq!(
            (host.dst_vm.as_deref(), host.dst_entity.as_deref()),
            (None, Some("host"))
        );
        assert!(e
            .iter()
            .any(|e| e.dst == "1.1.1.1" && e.dst_entity.as_deref() == Some("world")));
    }

    #[test]
    fn normalizes_requests() {
        assert_eq!(
            normalize_l7("http", "POST h/a/9f8e7d6c5b/b"),
            "POST h/a/{id}/b"
        );
        assert_eq!(normalize_l7("dns", "dns example.com"), "dns example.com");
        assert_eq!(normalize_l7("tls", "tls sni=a.b [denied]"), "tls sni=a.b");
    }

    #[test]
    fn detects_scans_once() {
        let mut h = FlowHistory::default();
        let mut alerts = vec![];
        for p in 1..=25u16 {
            let mut r = rec("bad", "", p, "AUDIT");
            r.dst = "10.0.0.9".into();
            alerts.extend(h.observe(&r));
        }
        assert_eq!(alerts.iter().filter(|a| a.kind == "port_scan").count(), 1);
        let mut sweep = vec![];
        for i in 1..=25u16 {
            let mut r = rec("bad2", "", 22, "FORWARDED");
            r.dst = format!("10.1.0.{i}");
            sweep.extend(h.observe(&r));
        }
        assert!(sweep.iter().any(|a| a.kind == "host_sweep"));
    }

    #[test]
    fn new_domains_alert_after_warmup_once() {
        let mut h = FlowHistory::default();
        assert!(h
            .observe_domain("web", "10.0.0.1", "a.cold.example.")
            .is_none());
        h.since = Some(ago_rfc3339(chrono::Duration::hours(25)));
        assert!(h
            .observe_domain("web", "10.0.0.1", "b.cold.example")
            .is_none());
        assert!(h
            .observe_domain("web", "10.0.0.1", "1.0.0.10.in-addr.arpa")
            .is_none());
        let a = h
            .observe_domain("web", "10.0.0.1", "x.fresh.example")
            .unwrap();
        assert_eq!(
            (a.kind.as_str(), a.dst.as_deref()),
            ("new_domain", Some("fresh.example"))
        );
        assert!(h
            .observe_domain("db", "10.0.0.2", "y.fresh.example")
            .is_none());
        let t = h.threat_alert("web", "10.0.0.1", "Evil.Example.", "x".into());
        assert_eq!(t.unwrap().dst.as_deref(), Some("evil.example"));
        assert!(h
            .threat_alert("web", "10.0.0.1", "evil.example", "x".into())
            .is_none());
    }

    #[test]
    fn persists_and_prunes() {
        let dir = std::env::temp_dir().join(format!("fh-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("flow-history.json");
        let mut h = FlowHistory::default();
        h.load(&p);
        h.observe(&rec("a", "b", 80, "FORWARDED"));
        h.save();
        let mut h2 = FlowHistory::default();
        h2.load(&p);
        let e = h2.edges(None);
        assert_eq!((e.len(), e[0].src.as_str(), e[0].count), (1, "a", 1));
        std::fs::remove_dir_all(dir).ok();
    }
}
