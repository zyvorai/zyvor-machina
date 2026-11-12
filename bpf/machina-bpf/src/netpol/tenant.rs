// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Fleet Cloud project networking. Default isolation between projects and
//! per-project egress allowlists become generated policies over the
//! `machina.io/project` label, so they compile, trace and replay like any
//! other; ordinary allow policies still open exceptions. Per-project egress
//! IPs become [`super::snat`] rules on each host.

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{threat, NetpolVm, VmNetworkPolicy, LABEL_PROJECT};
use crate::api::VmEgressSnatRule;

pub const LABEL_MANAGED: &str = "machina.io/managed-by";
pub const MANAGED_VALUE: &str = "project-network";
/// The settings row every project inherits isolation from.
pub const DEFAULT_PROJECT: &str = "*";
pub const MAX_EGRESS_ENTRIES: usize = 256;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Isolation {
    /// Follow the default (`*`) row.
    #[default]
    Inherit,
    /// VMs accept connections only from VMs of the same project (and the
    /// host, unless `allow_host` is off).
    Isolated,
    Open,
}

/// One allowlisted egress destination.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct EgressAllow {
    /// A CIDR or IP, a domain (`example.com`, `*.example.com`) or `world`.
    pub to: String,
    /// `443`, `53/udp`, … Empty = every port.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectNet {
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub isolation: Isolation,
    #[serde(default = "yes")]
    pub allow_host: bool,
    /// Limit egress to `egress_allow`, the project's own VMs, the host
    /// (with `allow_host`) and DNS.
    #[serde(default)]
    pub egress_restricted: bool,
    #[serde(default)]
    pub egress_allow: Vec<EgressAllow>,
    /// Host id or name → source address for traffic leaving that host: one
    /// IPv4, one IPv6, or both comma-separated.
    #[serde(default)]
    pub egress_ips: BTreeMap<String, String>,
    /// Block internet egress of the project's VMs on hosts without one of
    /// its egress IPs (instead of leaving with the host's address).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub egress_ip_required: bool,
    #[serde(default)]
    pub updated_by: String,
    #[serde(default)]
    pub updated_at: String,
}

fn yes() -> bool {
    true
}

impl Default for ProjectNet {
    fn default() -> Self {
        ProjectNet {
            project: String::new(),
            isolation: Isolation::Inherit,
            allow_host: true,
            egress_restricted: false,
            egress_allow: Vec::new(),
            egress_ips: BTreeMap::new(),
            egress_ip_required: false,
            updated_by: String::new(),
            updated_at: String::new(),
        }
    }
}

enum Dest {
    Cidr(String),
    Name(String),
    Pattern(String),
    World,
}

fn dest(to: &str) -> Option<Dest> {
    let t = to.trim();
    if matches!(t, "world" | "internet" | "0.0.0.0/0") {
        return Some(Dest::World);
    }
    let (a, p) = t.split_once('/').unwrap_or((t, ""));
    if let Ok(ip) = a.parse::<IpAddr>() {
        let max = if ip.is_ipv4() { 32 } else { 128 };
        let p: u8 = if p.is_empty() { max } else { p.parse().ok()? };
        return (p <= max).then(|| Dest::Cidr(format!("{ip}/{p}")));
    }
    if let Some(rest) = t.strip_prefix("*.") {
        return threat::normalize(rest).map(|d| Dest::Pattern(format!("*.{d}")));
    }
    threat::normalize(t).map(Dest::Name)
}

fn port_entry(s: &str) -> Option<Value> {
    let (n, proto) = s.trim().split_once('/').unwrap_or((s.trim(), "tcp"));
    let n: u16 = n.parse().ok().filter(|n| *n > 0)?;
    let proto = match proto.to_ascii_lowercase().as_str() {
        "tcp" => "TCP",
        "udp" => "UDP",
        "sctp" => "SCTP",
        _ => return None,
    };
    Some(json!({ "port": n.to_string(), "protocol": proto }))
}

impl ProjectNet {
    pub fn is_default(&self) -> bool {
        self.project == DEFAULT_PROJECT
    }

    pub fn validate(&self) -> Result<(), String> {
        let p = self.project.trim();
        if p.is_empty() || p.len() > 128 || p.chars().any(char::is_control) {
            return Err("project name must be 1–128 printable characters".into());
        }
        if self.is_default() {
            if self.isolation == Isolation::Inherit {
                return Err("the default is either isolated or open".into());
            }
            if self.egress_restricted
                || !self.egress_allow.is_empty()
                || !self.egress_ips.is_empty()
                || self.egress_ip_required
            {
                return Err("egress allowlists and egress IPs are set per project".into());
            }
        }
        if self.egress_allow.len() > MAX_EGRESS_ENTRIES {
            return Err(format!("at most {MAX_EGRESS_ENTRIES} egress destinations"));
        }
        for e in &self.egress_allow {
            if dest(&e.to).is_none() {
                return Err(format!(
                    "egress destination `{}`: use a CIDR, an IP, a domain, *.domain or world",
                    e.to
                ));
            }
            for p in &e.ports {
                if port_entry(p).is_none() {
                    return Err(format!("egress port `{p}`: use N, N/tcp, N/udp or N/sctp"));
                }
            }
        }
        for (h, ip) in &self.egress_ips {
            if h.trim().is_empty() {
                return Err("egress IP needs a host".into());
            }
            let (mut v4, mut v6) = (0, 0);
            for a in egress_ip_list(ip) {
                match a.parse::<std::net::IpAddr>() {
                    Ok(x) if x.is_ipv4() => v4 += 1,
                    Ok(_) => v6 += 1,
                    Err(_) => return Err(format!("egress IP `{a}` for {h} is not an IP address")),
                }
            }
            if v4 + v6 == 0 {
                return Err(format!("egress IP for {h} is empty"));
            }
            if v4 > 1 || v6 > 1 {
                return Err(format!(
                    "{h}: at most one IPv4 and one IPv6 egress IP per host"
                ));
            }
        }
        if self.egress_ip_required && self.egress_ips.is_empty() {
            return Err("requiring the egress IP needs at least one egress IP".into());
        }
        Ok(())
    }

    /// Has an egress IP on the host with this id or name.
    pub fn egress_ip_on(&self, host_id: &str, hostname: &str) -> Option<&String> {
        self.egress_ips
            .get(host_id)
            .or_else(|| self.egress_ips.get(hostname))
    }
}

/// The addresses of one `egress_ips` value.
pub fn egress_ip_list(v: &str) -> impl Iterator<Item = &str> {
    v.split(',').map(str::trim).filter(|a| !a.is_empty())
}

/// A VM of a project with egress IPs, on a host that has none of them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct EgressGap {
    pub project: String,
    pub vm: String,
    pub host: String,
    /// Its internet egress is blocked (`egress_ip_required`).
    pub blocked: bool,
}

/// `hosts`: host id → hostname.
pub fn egress_gaps(
    settings: &[ProjectNet],
    vms: &[NetpolVm],
    hosts: &BTreeMap<String, String>,
) -> Vec<EgressGap> {
    let mut out = Vec::new();
    for s in settings
        .iter()
        .filter(|s| !s.is_default() && !s.egress_ips.is_empty())
    {
        for v in vms
            .iter()
            .filter(|v| v.project.as_deref() == Some(s.project.as_str()))
        {
            let Some(h) = v.host.as_deref() else { continue };
            let name = hosts.get(h).map_or(h, String::as_str);
            if s.egress_ip_on(h, name).is_none() {
                out.push(EgressGap {
                    project: s.project.clone(),
                    vm: v.name.clone(),
                    host: name.to_string(),
                    blocked: s.egress_ip_required,
                });
            }
        }
    }
    out.sort();
    out
}

/// Not `::/0`: the compiler keeps IPv4 as IPv4-mapped IPv6, which `::/0`
/// would cover too.
const GLOBAL_UNICAST6: &str = "2000::/3";

/// For projects that require their egress IP: deny traffic that would be
/// source-NATed (non-private destinations) from VMs on other hosts.
pub fn egress_ip_guards(
    settings: &[ProjectNet],
    hosts: &BTreeMap<String, String>,
) -> Vec<VmNetworkPolicy> {
    settings
        .iter()
        .filter(|s| !s.is_default() && s.egress_ip_required && !s.egress_ips.is_empty())
        .map(|s| {
            let project = s.project.as_str();
            let mut with: BTreeSet<String> = hosts
                .iter()
                .filter(|(id, name)| s.egress_ip_on(id, name).is_some())
                .map(|(id, _)| id.clone())
                .collect();
            with.extend(
                s.egress_ips
                    .keys()
                    .filter(|k| !hosts.values().any(|n| n == *k))
                    .cloned(),
            );
            policy(
                policy_name("egress-ip", project),
                project,
                json!({
                    "description": format!(
                        "Project {project} requires its egress IP: internet egress is blocked on hosts without one"
                    ),
                    "endpointSelector": {
                        "matchLabels": { LABEL_PROJECT: project },
                        "matchExpressions": [{
                            "key": super::LABEL_HOST,
                            "operator": "NotIn",
                            "values": with.into_iter().collect::<Vec<_>>(),
                        }],
                    },
                    "enableDefaultDeny": { "egress": false },
                    "egressDeny": [{
                        "toCIDRSet": [
                            { "cidr": "0.0.0.0/0", "except": super::snat::DEFAULT_EXCLUDE },
                            { "cidr": GLOBAL_UNICAST6 },
                        ],
                    }],
                }),
            )
        })
        .collect()
}

/// Whether `project` is isolated and lets the host in, after inheritance.
pub fn effective(settings: &[ProjectNet], project: &str) -> (bool, bool) {
    let default = settings.iter().find(|s| s.is_default());
    let own = settings.iter().find(|s| s.project == project);
    let isolated = match own.map(|s| s.isolation) {
        Some(Isolation::Isolated) => true,
        Some(Isolation::Open) => false,
        _ => default.is_some_and(|d| d.isolation == Isolation::Isolated),
    };
    let allow_host = match own {
        Some(s) if s.isolation != Isolation::Inherit => s.allow_host,
        _ => default.is_none_or(|d| d.allow_host),
    };
    (isolated, allow_host)
}

pub fn policy_name(kind: &str, project: &str) -> String {
    let h: String = Sha256::digest(project.as_bytes())[..3]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let slug: String = super::nl::slug(project).chars().take(40).collect();
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        format!("project-{kind}-{h}")
    } else {
        format!("project-{kind}-{slug}-{h}")
    }
}

fn policy(name: String, project: &str, spec: Value) -> VmNetworkPolicy {
    VmNetworkPolicy {
        name,
        kind: "CiliumNetworkPolicy".into(),
        labels: [
            (LABEL_MANAGED.to_string(), MANAGED_VALUE.to_string()),
            (LABEL_PROJECT.to_string(), project.to_string()),
        ]
        .into(),
        annotations: BTreeMap::new(),
        specs: vec![spec],
    }
}

fn members(project: &str) -> Value {
    json!({ "matchLabels": { LABEL_PROJECT: project } })
}

fn isolation_policy(project: &str, allow_host: bool) -> VmNetworkPolicy {
    let mut ingress = vec![json!({ "fromEndpoints": [members(project)] })];
    if allow_host {
        ingress.push(json!({ "fromEntities": ["host"] }));
    }
    policy(
        policy_name("isolation", project),
        project,
        json!({
            "description": format!(
                "Project {project} is isolated: its VMs accept connections only from VMs in the project{}",
                if allow_host { " and the host" } else { "" }
            ),
            "endpointSelector": members(project),
            "ingress": ingress,
        }),
    )
}

fn egress_policy(s: &ProjectNet, allow_host: bool) -> VmNetworkPolicy {
    let project = s.project.as_str();
    let mut egress = vec![json!({ "toEndpoints": [members(project)] })];
    if allow_host {
        egress.push(json!({ "toEntities": ["host"] }));
    }
    egress.push(json!({
        "toEntities": ["host"],
        "toPorts": [{
            "ports": [{ "port": "53", "protocol": "UDP" }, { "port": "53", "protocol": "TCP" }],
            "rules": { "dns": [{ "matchPattern": "*" }] }
        }]
    }));
    let mut shown = Vec::new();
    for e in &s.egress_allow {
        let Some(d) = dest(&e.to) else { continue };
        let mut rule = match d {
            Dest::Cidr(c) => json!({ "toCIDR": [c] }),
            Dest::Name(n) => json!({ "toFQDNs": [{ "matchName": n }] }),
            Dest::Pattern(p) => json!({ "toFQDNs": [{ "matchPattern": p }] }),
            Dest::World => json!({ "toEntities": ["world"] }),
        };
        let ports: Vec<Value> = e.ports.iter().filter_map(|p| port_entry(p)).collect();
        if !ports.is_empty() {
            rule["toPorts"] = json!([{ "ports": ports }]);
        }
        egress.push(rule);
        shown.push(if e.ports.is_empty() {
            e.to.clone()
        } else {
            format!("{} ({})", e.to, e.ports.join(", "))
        });
    }
    policy(
        policy_name("egress", project),
        project,
        json!({
            "description": format!(
                "Project {project} egress allowlist: {}",
                if shown.is_empty() { "nothing outside the project".to_string() } else { shown.join(", ") }
            ),
            "endpointSelector": members(project),
            "egress": egress,
        }),
    )
}

/// The generated policies for every known project.
pub fn policies(settings: &[ProjectNet], projects: &BTreeSet<String>) -> Vec<VmNetworkPolicy> {
    let mut all: BTreeSet<&str> = projects.iter().map(String::as_str).collect();
    all.extend(
        settings
            .iter()
            .filter(|s| !s.is_default())
            .map(|s| s.project.as_str()),
    );
    let mut out = Vec::new();
    for p in all {
        let (isolated, allow_host) = effective(settings, p);
        if isolated {
            out.push(isolation_policy(p, allow_host));
        }
        if let Some(s) = settings
            .iter()
            .find(|s| s.project == p && s.egress_restricted)
        {
            out.push(egress_policy(s, allow_host));
        }
    }
    out
}

/// SNAT rules for one host: each project with an egress IP for `host_id`
/// (or `hostname`) covers its VMs running there.
pub fn snat_rules(
    settings: &[ProjectNet],
    vms: &[NetpolVm],
    host_id: &str,
    hostname: &str,
) -> Vec<VmEgressSnatRule> {
    let mut out = Vec::new();
    for s in settings.iter().filter(|s| !s.is_default()) {
        let Some(ips) = s.egress_ip_on(host_id, hostname) else {
            continue;
        };
        for ip in egress_ip_list(ips) {
            let addrs = vms
                .iter()
                .filter(|v| v.project.as_deref() == Some(s.project.as_str()))
                .filter(|v| v.host.as_deref() == Some(host_id))
                .flat_map(|v| v.addresses.iter().cloned());
            let r = super::snat::rules_for(&s.project, ip, addrs);
            if !r.sources.is_empty() {
                out.push(r);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::{parse_documents, trace, TraceQuery};
    use super::*;

    fn vm(name: &str, project: &str, ip: &str) -> NetpolVm {
        NetpolVm {
            name: name.into(),
            host: Some("h1".into()),
            project: (!project.is_empty()).then(|| project.into()),
            labels: BTreeMap::new(),
            addresses: vec![ip.into()],
            ..Default::default()
        }
    }

    fn inv() -> Vec<NetpolVm> {
        vec![
            vm("shop-web", "shop", "10.0.0.5"),
            vm("shop-db", "shop", "10.0.0.6"),
            vm("lab-1", "lab", "10.0.0.9"),
            vm("loose", "", "10.0.0.20"),
        ]
    }

    fn reach(p: &[VmNetworkPolicy], from: &str, to: &str, port: u16) -> bool {
        let r = trace(
            p,
            &inv(),
            &[],
            &["192.168.1.10".into()],
            &[],
            &TraceQuery {
                from: from.into(),
                to: to.into(),
                protocol: "tcp".into(),
                port,
                ..Default::default()
            },
        )
        .unwrap();
        r.allowed
    }

    fn projects() -> BTreeSet<String> {
        ["shop".to_string(), "lab".to_string()].into()
    }

    #[test]
    fn default_isolation_keeps_projects_apart() {
        let settings = vec![
            ProjectNet {
                project: "*".into(),
                isolation: Isolation::Isolated,
                ..Default::default()
            },
            ProjectNet {
                project: "lab".into(),
                isolation: Isolation::Open,
                ..Default::default()
            },
        ];
        let p = policies(&settings, &projects());
        assert_eq!(p.len(), 1);
        assert!(p[0].name.starts_with("project-isolation-shop-"));
        let (_, v) = parse_documents(&super::super::nl::to_yaml(&p));
        assert!(v.ok(), "{:?}", v.errors);
        assert!(reach(&p, "shop-web", "shop-db", 5432));
        assert!(!reach(&p, "lab-1", "shop-db", 5432));
        assert!(!reach(&p, "loose", "shop-web", 80));
        assert!(reach(&p, "host", "shop-web", 22));
        assert!(reach(&p, "shop-web", "lab-1", 80), "lab is open");

        let no_host = vec![ProjectNet {
            project: "shop".into(),
            isolation: Isolation::Isolated,
            allow_host: false,
            ..Default::default()
        }];
        let p = policies(&no_host, &projects());
        assert!(!reach(&p, "host", "shop-web", 22));
        assert_eq!(effective(&no_host, "lab"), (false, true));
    }

    #[test]
    fn egress_allowlist_limits_the_internet() {
        let settings = vec![ProjectNet {
            project: "shop".into(),
            egress_restricted: true,
            egress_allow: vec![
                EgressAllow {
                    to: "203.0.113.0/24".into(),
                    ports: vec!["443".into()],
                },
                EgressAllow {
                    to: "*.example.com".into(),
                    ports: vec![],
                },
            ],
            ..Default::default()
        }];
        settings[0].validate().unwrap();
        let p = policies(&settings, &projects());
        assert_eq!(p.len(), 1);
        let (_, v) = parse_documents(&super::super::nl::to_yaml(&p));
        assert!(v.ok(), "{:?}", v.errors);
        assert!(reach(&p, "shop-web", "203.0.113.7", 443));
        assert!(!reach(&p, "shop-web", "203.0.113.7", 80));
        assert!(!reach(&p, "shop-web", "8.8.8.8", 443));
        assert!(reach(&p, "shop-web", "shop-db", 5432));
        assert!(!reach(&p, "shop-web", "lab-1", 80));
        assert!(
            reach(&p, "lab-1", "8.8.8.8", 443),
            "other projects unaffected"
        );
        let spec = &p[0].specs[0];
        assert_eq!(spec["egress"][3]["toPorts"][0]["ports"][0]["port"], "443");
        assert_eq!(
            spec["egress"][4]["toFQDNs"][0]["matchPattern"],
            "*.example.com"
        );
    }

    #[test]
    fn validation_and_snat_plan() {
        let mut s = ProjectNet {
            project: "*".into(),
            isolation: Isolation::Inherit,
            ..Default::default()
        };
        assert!(s.validate().is_err());
        s.isolation = Isolation::Isolated;
        s.validate().unwrap();
        s.egress_ips.insert("h1".into(), "203.0.113.10".into());
        assert!(s.validate().is_err());
        let bad = ProjectNet {
            project: "shop".into(),
            egress_allow: vec![EgressAllow {
                to: "not a host!".into(),
                ports: vec![],
            }],
            ..Default::default()
        };
        assert!(bad.validate().is_err());
        let bad = ProjectNet {
            project: "shop".into(),
            egress_allow: vec![EgressAllow {
                to: "world".into(),
                ports: vec!["70000".into()],
            }],
            ..Default::default()
        };
        assert!(bad.validate().is_err());

        let settings = vec![ProjectNet {
            project: "shop".into(),
            egress_ips: [("hv1".to_string(), "203.0.113.10".to_string())].into(),
            ..Default::default()
        }];
        let r = snat_rules(&settings, &inv(), "h1", "hv1");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].sources, ["10.0.0.5", "10.0.0.6"]);
        assert!(snat_rules(&settings, &inv(), "h2", "hv2").is_empty());
        assert_ne!(policy_name("egress", "a b"), policy_name("egress", "a-b"));

        let mut dual = settings[0].clone();
        dual.egress_ips
            .insert("hv1".into(), "203.0.113.10, 2001:db8::10".into());
        dual.validate().unwrap();
        let mut vms = inv();
        vms[0].addresses.push("fd00::5".into());
        let r = snat_rules(&[dual.clone()], &vms, "h1", "hv1");
        assert_eq!(r.len(), 2, "{r:?}");
        assert_eq!(r[1].egress_ip, "2001:db8::10");
        assert_eq!(r[1].sources, ["fd00::5"]);
        for bad in [
            "203.0.113.10,203.0.113.11",
            "2001:db8::1,2001:db8::2",
            " , ",
            "x",
        ] {
            dual.egress_ips.insert("hv1".into(), bad.into());
            assert!(dual.validate().is_err(), "{bad}");
        }
    }

    #[test]
    fn egress_ip_gaps_are_reported_and_optionally_blocked() {
        let mut vms = inv();
        vms.push(NetpolVm {
            host: Some("h2".into()),
            ..vm("shop-moved", "shop", "10.0.0.7")
        });
        let hosts: BTreeMap<String, String> = [
            ("h1".to_string(), "hv1".to_string()),
            ("h2".to_string(), "hv2".to_string()),
        ]
        .into();
        let mut s = ProjectNet {
            project: "shop".into(),
            egress_ip_required: true,
            ..Default::default()
        };
        assert!(s.validate().is_err(), "required needs an egress IP");
        s.egress_ips.insert("hv1".into(), "203.0.113.10".into());
        s.validate().unwrap();
        let settings = vec![s];
        assert_eq!(
            egress_gaps(&settings, &vms, &hosts),
            [EgressGap {
                project: "shop".into(),
                vm: "shop-moved".into(),
                host: "hv2".into(),
                blocked: true,
            }]
        );
        let p = egress_ip_guards(&settings, &hosts);
        assert_eq!(p.len(), 1);
        assert!(p[0].name.starts_with("project-egress-ip-shop-"));
        let (_, v) = parse_documents(&super::super::nl::to_yaml(&p));
        assert!(v.ok(), "{:?}", v.errors);
        let go = |from: &str, to: &str| {
            trace(
                &p,
                &vms,
                &[],
                &[],
                &[],
                &TraceQuery {
                    from: from.into(),
                    to: to.into(),
                    protocol: "tcp".into(),
                    port: 443,
                    ..Default::default()
                },
            )
            .unwrap()
            .allowed
        };
        assert!(!go("shop-moved", "8.8.8.8"), "no egress IP on hv2");
        assert!(go("shop-moved", "10.20.0.1"), "private: not NATed anyway");
        assert!(!go("shop-moved", "2001:4860::8888"), "global IPv6 too");
        assert!(go("shop-moved", "fd00::9"), "ULA stays local");
        assert!(go("shop-web", "8.8.8.8"), "hv1 has the egress IP");
        assert!(go("lab-1", "8.8.8.8"));

        let open = vec![ProjectNet {
            egress_ip_required: false,
            ..settings[0].clone()
        }];
        assert!(egress_ip_guards(&open, &hosts).is_empty());
        assert!(!egress_gaps(&open, &vms, &hosts)[0].blocked);
    }
}
