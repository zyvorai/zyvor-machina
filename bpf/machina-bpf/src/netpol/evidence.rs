// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Segmentation evidence for audits: what is configured (policies with
//! content hashes, project isolation, egress controls), what is in force
//! (per-host enforcement and sync), what the policy set allows between
//! groups of VMs (a traced reachability matrix), and what was observed
//! (denied flows, alerts, exceptions and approvals). The report carries a
//! SHA-256 digest of its own content so a stored copy can be checked.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::trace::Tracer;
use super::{NetpolService, NetpolVm, TraceQuery, VmNetworkPolicy};
use crate::api::VmFlowEdge;

pub const KIND: &str = "machina.io/segmentation-evidence/v1";
/// Connections each matrix cell is checked on by default.
pub const PROBES: &[(&str, u16)] = &[
    ("TCP", 22),
    ("TCP", 80),
    ("TCP", 443),
    ("TCP", 3389),
    ("TCP", 5432),
    ("UDP", 53),
];
/// VMs per group tried for each cell.
pub const SAMPLE: usize = 3;
pub const MAX_GROUPS: usize = 40;
pub const TOP_DENIED: usize = 50;
pub const MAX_PROBES: usize = 16;
pub const SIGNATURE_ALG: &str = "ecdsa-p256-sha256";

pub type Probe = (String, u16);

pub fn default_probes() -> Vec<Probe> {
    PROBES.iter().map(|(p, n)| (p.to_string(), *n)).collect()
}

/// `tcp/22,udp/53,sctp/3868` → probes (empty = the defaults).
pub fn parse_probes(s: &str) -> Result<Vec<Probe>, String> {
    let mut out: Vec<Probe> = Vec::new();
    for item in s.split(',').map(str::trim).filter(|x| !x.is_empty()) {
        let (proto, port) = item
            .split_once('/')
            .ok_or_else(|| format!("probe `{item}`: use PROTO/PORT, e.g. tcp/443"))?;
        let proto = proto.to_ascii_uppercase();
        if !matches!(proto.as_str(), "TCP" | "UDP" | "SCTP") {
            return Err(format!("probe `{item}`: protocol must be tcp, udp or sctp"));
        }
        let port: u16 = port
            .parse()
            .ok()
            .filter(|p| *p > 0)
            .ok_or_else(|| format!("probe `{item}`: port must be 1-65535"))?;
        if !out.contains(&(proto.clone(), port)) {
            out.push((proto, port));
        }
    }
    if out.len() > MAX_PROBES {
        return Err(format!("at most {MAX_PROBES} probes"));
    }
    Ok(if out.is_empty() {
        default_probes()
    } else {
        out
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Summary {
    pub policies: usize,
    pub generated_policies: usize,
    pub vms: usize,
    /// VMs selected by at least one policy.
    pub vms_selected: usize,
    pub projects: usize,
    pub projects_isolated: usize,
    pub hosts: usize,
    pub hosts_enforcing: usize,
    pub hosts_in_sync: usize,
    pub denied_connections: usize,
    pub alerts: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HostEvidence {
    pub hostname: String,
    pub reachable: bool,
    /// Drops are enforced (enforcement lease held), not only audited.
    pub enforcing: bool,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub synced_at: Option<String>,
    #[serde(default)]
    pub in_sync: bool,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PolicyEvidence {
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    /// Generated from project settings.
    pub generated: bool,
    #[serde(default)]
    pub description: Option<String>,
    /// SHA-256 of the policy's canonical YAML.
    pub sha256: String,
    pub selected_vms: Vec<String>,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MatrixCell {
    pub from: String,
    pub to: String,
    /// Probes allowed (`TCP/443`), empty = fully segmented for the probes.
    pub allowed: Vec<String>,
    pub denied: Vec<String>,
    /// VM pairs the cell was traced on.
    pub samples: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeniedEdge {
    pub src: String,
    pub dst: String,
    pub proto: String,
    pub port: u16,
    pub flows: u64,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub policy: Option<String>,
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Evidence {
    pub kind: String,
    pub generated_at: String,
    pub generated_by: String,
    /// `host` (one daemon) or `fleet` (the controller).
    pub scope: String,
    pub source: String,
    pub summary: Summary,
    pub hosts: Vec<HostEvidence>,
    pub policies: Vec<PolicyEvidence>,
    #[serde(default)]
    pub projects: Vec<Value>,
    /// How groups were formed (`project` or `vm`) and the probes used.
    pub matrix_groups: String,
    pub matrix_probes: Vec<String>,
    pub matrix: Vec<MatrixCell>,
    pub denied: Vec<DeniedEdge>,
    pub alerts: Vec<Value>,
    pub quarantines: Vec<Value>,
    pub temporary_access: Vec<Value>,
    pub threat_feeds: Vec<Value>,
    #[serde(default)]
    pub egress_ips: Vec<Value>,
    #[serde(default)]
    pub approvals: Vec<Value>,
    /// Limits of what the report can show (e.g. projects across NAT).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// SHA-256 over the report with this field empty and no `signature`.
    pub digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<EvidenceSignature>,
}

/// A signature over the hex `digest` by a certificate the fleet's VM
/// network policy CA issued.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvidenceSignature {
    pub alg: String,
    /// DER signature, base64.
    pub value: String,
    pub signer_pem: String,
    pub ca_pem: String,
}

pub fn policy_hash(p: &VmNetworkPolicy) -> String {
    hex(&Sha256::digest(p.to_yaml().as_bytes()))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Groups for the matrix: projects when any VM has one (VMs without a
/// project form `(no project)`), otherwise one group per VM. Capped.
pub fn groups(vms: &[NetpolVm]) -> (String, Vec<(String, Vec<String>)>) {
    let by_project = vms.iter().any(|v| v.project.is_some());
    let mut g: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for v in vms {
        let key = if by_project {
            v.project.clone().unwrap_or_else(|| "(no project)".into())
        } else {
            v.name.clone()
        };
        g.entry(key).or_default().push(v.name.clone());
    }
    let kind = if by_project { "project" } else { "vm" };
    (kind.into(), g.into_iter().take(MAX_GROUPS).collect())
}

pub const OTHER_PROJECTS: &str = "(other projects)";

/// Groups for a report about one project: the project and, sampled one VM
/// per other group in turn, everything else.
pub fn groups_for_project(vms: &[NetpolVm], project: &str) -> (String, Vec<(String, Vec<String>)>) {
    let (_, all) = groups(vms);
    let mine: Vec<String> = all
        .iter()
        .find(|(n, _)| n == project)
        .map(|(_, m)| m.clone())
        .unwrap_or_default();
    let others: Vec<&Vec<String>> = all
        .iter()
        .filter(|(n, _)| n != project)
        .map(|(_, m)| m)
        .collect();
    let mut spread = Vec::new();
    for i in 0..others.iter().map(|m| m.len()).max().unwrap_or(0) {
        spread.extend(others.iter().filter_map(|m| m.get(i)).cloned());
    }
    let mut out = vec![(project.to_string(), mine)];
    if !spread.is_empty() {
        out.push((OTHER_PROJECTS.to_string(), spread));
    }
    ("project".into(), out)
}

fn names_any(v: &Value, keys: &[&str], members: &BTreeSet<String>) -> bool {
    keys.iter()
        .any(|k| v[*k].as_str().is_some_and(|s| members.contains(s)))
}

/// Narrow a fleet report to one project: its VMs' policies, flows and
/// exceptions. Host enforcement state and threat feeds stay; call before
/// [`Evidence::seal`].
pub fn scope_to_project(e: &mut Evidence, project: &str, members: &BTreeSet<String>) {
    e.scope = format!("project {project}");
    for p in &mut e.policies {
        p.selected_vms.retain(|v| members.contains(v));
    }
    e.policies.retain(|p| !p.selected_vms.is_empty());
    e.projects
        .retain(|p| p["project"].as_str() == Some(project));
    e.denied
        .retain(|d| members.contains(&d.src) || members.contains(&d.dst));
    e.alerts
        .retain(|a| names_any(a, &["src_vm", "src"], members));
    e.quarantines.retain(|q| names_any(q, &["vm"], members));
    e.temporary_access
        .retain(|g| names_any(g, &["from", "to"], members));
    e.egress_ips
        .retain(|r| r["project"].as_str() == Some(project));
    let tag = format!("project {project} ");
    e.warnings.retain(|w| w.contains(&tag));
    e.approvals.retain(|a| {
        a["label"]
            .as_str()
            .is_some_and(|l| l.contains(&format!("project {project}")))
    });
    e.summary.vms = members.len();
}

/// Trace every group pair (and the host / internet) on `probes`.
pub fn matrix(
    policies: &[VmNetworkPolicy],
    vms: &[NetpolVm],
    services: &[NetpolService],
    host_addresses: &[String],
    groups: &[(String, Vec<String>)],
    probes: &[Probe],
) -> Vec<MatrixCell> {
    let t = Tracer::new(policies, vms, services, host_addresses, &[]);
    let mut ends: Vec<(String, Vec<String>)> = vec![
        ("host".into(), vec!["host".into()]),
        ("world".into(), vec!["world".into()]),
    ];
    ends.extend(
        groups
            .iter()
            .map(|(n, m)| (n.clone(), m.iter().take(SAMPLE).cloned().collect())),
    );
    let mut out = Vec::new();
    for (from, fm) in &ends {
        for (to, tm) in &ends {
            let pseudo = |n: &str| n == "host" || n == "world";
            if pseudo(from) && pseudo(to) {
                continue;
            }
            let pairs: Vec<(&String, &String)> = fm
                .iter()
                .flat_map(|a| tm.iter().map(move |b| (a, b)))
                .filter(|(a, b)| a != b)
                .collect();
            if pairs.is_empty() {
                continue;
            }
            let mut cell = MatrixCell {
                from: from.clone(),
                to: to.clone(),
                samples: pairs.iter().map(|(a, b)| format!("{a} → {b}")).collect(),
                ..Default::default()
            };
            for (proto, port) in probes {
                let ok = pairs.iter().any(|(a, b)| {
                    t.trace(&TraceQuery {
                        from: (*a).clone(),
                        to: (*b).clone(),
                        protocol: proto.clone(),
                        port: *port,
                        ..Default::default()
                    })
                    .is_ok_and(|r| r.allowed)
                });
                let label = format!("{proto}/{port}");
                if ok {
                    cell.allowed.push(label);
                } else {
                    cell.denied.push(label);
                }
            }
            out.push(cell);
        }
    }
    out
}

/// Denied connections from the flow history, most flows first.
pub fn denied(edges: &[VmFlowEdge]) -> Vec<DeniedEdge> {
    let mut d: Vec<DeniedEdge> = edges
        .iter()
        .filter(|e| matches!(e.verdict.as_str(), "DROPPED" | "AUDIT" | "DENIED"))
        .map(|e| DeniedEdge {
            src: e.src.clone(),
            dst: e.dst.clone(),
            proto: e.proto.clone(),
            port: e.port,
            flows: e.count,
            reason: e.drop_reason.clone(),
            policy: e.policy.clone(),
            last_seen: e.last_seen.clone(),
        })
        .collect();
    d.sort_by(|a, b| b.flows.cmp(&a.flows).then(a.src.cmp(&b.src)));
    d
}

impl Evidence {
    /// Fill the summary and digest; call last.
    pub fn seal(&mut self, denied_total: usize) {
        self.kind = KIND.into();
        let s = &mut self.summary;
        s.policies = self.policies.iter().filter(|p| !p.generated).count();
        s.generated_policies = self.policies.iter().filter(|p| p.generated).count();
        let mut sel: Vec<&String> = self
            .policies
            .iter()
            .filter(|p| p.enabled)
            .flat_map(|p| &p.selected_vms)
            .collect();
        sel.sort();
        sel.dedup();
        s.vms_selected = sel.len();
        s.projects = self.projects.len();
        s.projects_isolated = self
            .projects
            .iter()
            .filter(|p| p["isolated"].as_bool() == Some(true))
            .count();
        s.hosts = self.hosts.len();
        s.hosts_enforcing = self.hosts.iter().filter(|h| h.enforcing).count();
        s.hosts_in_sync = self.hosts.iter().filter(|h| h.in_sync).count();
        s.denied_connections = denied_total;
        s.alerts = self.alerts.len();
        if self.matrix_probes.is_empty() {
            self.matrix_probes = PROBES.iter().map(|(p, n)| format!("{p}/{n}")).collect();
        }
        self.denied.truncate(TOP_DENIED);
        self.signature = None;
        self.digest = String::new();
        self.digest = self.compute_digest();
    }

    /// Sign the digest (after [`Evidence::seal`]).
    pub fn sign(&mut self, signer: &crate::authca::DocSigner, ca_pem: &str) -> anyhow::Result<()> {
        use base64::Engine as _;
        let sig = signer.sign(self.digest.as_bytes())?;
        self.signature = Some(EvidenceSignature {
            alg: SIGNATURE_ALG.into(),
            value: base64::engine::general_purpose::STANDARD.encode(sig),
            signer_pem: signer.cert_pem.clone(),
            ca_pem: ca_pem.to_string(),
        });
        Ok(())
    }

    pub fn compute_digest(&self) -> String {
        let mut c = self.clone();
        c.digest = String::new();
        c.signature = None;
        hex(&Sha256::digest(
            serde_json::to_vec(&c).unwrap_or_default().as_slice(),
        ))
    }

    /// The digest matches the content.
    pub fn verify(&self) -> bool {
        !self.digest.is_empty() && self.digest == self.compute_digest()
    }
}

fn md_escape(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn str_of(v: &Value, k: &str) -> String {
    match &v[k] {
        Value::String(s) => s.clone(),
        Value::Null => "—".into(),
        x => x.to_string(),
    }
}

/// The report as Markdown.
pub fn markdown(e: &Evidence) -> String {
    let s = &e.summary;
    let mut o = String::new();
    o.push_str("# VM network segmentation evidence\n\n");
    o.push_str(&format!(
        "- Generated: {} by {}\n- Scope: {} ({})\n- Digest (SHA-256): `{}`\n\n",
        e.generated_at, e.generated_by, e.scope, e.source, e.digest
    ));
    if let Some(s) = &e.signature {
        o.push_str(&format!(
            "- Signed ({}) by the fleet's VM network policy CA; verify the JSON export with `machinactl netpol evidence verify`\n\n",
            s.alg
        ));
    }
    if !e.warnings.is_empty() {
        o.push_str("## Limits\n\n");
        for w in &e.warnings {
            o.push_str(&format!("- {}\n", md_escape(w)));
        }
        o.push('\n');
    }
    o.push_str("## Summary\n\n| Item | Value |\n|---|---|\n");
    for (k, v) in [
        ("Policies", s.policies.to_string()),
        (
            "Generated project policies",
            s.generated_policies.to_string(),
        ),
        (
            "VMs",
            format!("{} ({} selected by a policy)", s.vms, s.vms_selected),
        ),
        (
            "Projects",
            format!("{} ({} isolated)", s.projects, s.projects_isolated),
        ),
        (
            "Hosts",
            format!(
                "{} ({} enforcing, {} in sync)",
                s.hosts, s.hosts_enforcing, s.hosts_in_sync
            ),
        ),
        (
            "Denied connections in history",
            s.denied_connections.to_string(),
        ),
        ("Recent alerts", s.alerts.to_string()),
    ] {
        o.push_str(&format!("| {k} | {v} |\n"));
    }
    o.push_str("\n## Enforcement per host\n\n| Host | Reachable | Mode | Owner | In sync | Last sync | Error |\n|---|---|---|---|---|---|---|\n");
    for h in &e.hosts {
        o.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            md_escape(&h.hostname),
            if h.reachable { "yes" } else { "no" },
            if h.enforcing {
                "enforce"
            } else {
                "observe (audit only)"
            },
            if h.owner.is_empty() { "—" } else { &h.owner },
            if h.in_sync { "yes" } else { "no" },
            h.synced_at.as_deref().unwrap_or("—"),
            md_escape(h.error.as_deref().unwrap_or("—")),
        ));
    }
    o.push_str("\n## Policies\n\n| Name | Kind | Enabled | Source | Selects | SHA-256 | Description |\n|---|---|---|---|---|---|---|\n");
    for p in &e.policies {
        o.push_str(&format!(
            "| {} | {} | {} | {} | {} | `{}` | {} |\n",
            md_escape(&p.name),
            p.kind,
            if p.enabled { "yes" } else { "no" },
            if p.generated {
                "project settings"
            } else {
                "stored"
            },
            md_escape(&p.selected_vms.join(", ")),
            &p.sha256[..16.min(p.sha256.len())],
            md_escape(p.description.as_deref().unwrap_or("—")),
        ));
    }
    if !e.projects.is_empty() {
        o.push_str("\n## Projects\n\n| Project | VMs | Isolation | Egress | Egress IPs |\n|---|---|---|---|---|\n");
        for p in &e.projects {
            o.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                md_escape(&str_of(p, "project")),
                p["vms"].as_array().map_or(0, Vec::len),
                str_of(p, "isolation"),
                md_escape(&str_of(p, "egress")),
                md_escape(&str_of(p, "egress_ips")),
            ));
        }
    }
    o.push_str(&format!(
        "\n## Reachability matrix\n\nTraced against the policy set, by {}, on {}. A blank *Allowed* cell means the pair is segmented for every probe.\n\n| From | To | Allowed | Samples |\n|---|---|---|---|\n",
        e.matrix_groups,
        e.matrix_probes.join(", ")
    ));
    for c in &e.matrix {
        o.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            md_escape(&c.from),
            md_escape(&c.to),
            c.allowed.join(", "),
            md_escape(&c.samples.join(", ")),
        ));
    }
    o.push_str("\n## Denied connections (flow history)\n\n");
    if e.denied.is_empty() {
        o.push_str("None recorded.\n");
    } else {
        o.push_str("| Source | Destination | Port | Flows | Reason | Policy | Last seen |\n|---|---|---|---|---|---|---|\n");
        for d in &e.denied {
            o.push_str(&format!(
                "| {} | {} | {}/{} | {} | {} | {} | {} |\n",
                md_escape(&d.src),
                md_escape(&d.dst),
                d.proto.to_ascii_lowercase(),
                d.port,
                d.flows,
                d.reason.as_deref().unwrap_or("—"),
                md_escape(d.policy.as_deref().unwrap_or("—")),
                d.last_seen,
            ));
        }
    }
    let list = |o: &mut String, title: &str, items: &[Value], cols: &[&str]| {
        o.push_str(&format!("\n## {title}\n\n"));
        if items.is_empty() {
            o.push_str("None.\n");
            return;
        }
        o.push_str(&format!(
            "| {} |\n|{}\n",
            cols.join(" | "),
            "---|".repeat(cols.len())
        ));
        for i in items {
            let row: Vec<String> = cols.iter().map(|c| md_escape(&str_of(i, c))).collect();
            o.push_str(&format!("| {} |\n", row.join(" | ")));
        }
    };
    list(
        &mut o,
        "Alerts",
        &e.alerts,
        &["ts", "kind", "severity", "src", "detail"],
    );
    list(
        &mut o,
        "Quarantines",
        &e.quarantines,
        &["vm", "since", "until", "by", "reason"],
    );
    list(
        &mut o,
        "Temporary access",
        &e.temporary_access,
        &[
            "name",
            "from",
            "to",
            "port",
            "expires_at",
            "granted_by",
            "reason",
        ],
    );
    list(
        &mut o,
        "DNS threat feeds",
        &e.threat_feeds,
        &["name", "source", "block", "domains", "updated"],
    );
    if !e.egress_ips.is_empty() {
        list(
            &mut o,
            "Egress IPs",
            &e.egress_ips,
            &["hostname", "project", "egress_ip", "sources"],
        );
    }
    if e.scope == "fleet" {
        list(
            &mut o,
            "Approvals (last 90 days)",
            &e.approvals,
            &[
                "created_at",
                "action_type",
                "label",
                "requested_by",
                "approved_by",
                "status",
            ],
        );
    }
    o
}

#[cfg(test)]
mod tests {
    use super::super::parse_documents;
    use super::*;

    fn vm(name: &str, project: Option<&str>, ip: &str) -> NetpolVm {
        NetpolVm {
            name: name.into(),
            host: Some("h1".into()),
            project: project.map(Into::into),
            labels: BTreeMap::new(),
            addresses: vec![ip.into()],
            ..Default::default()
        }
    }

    #[test]
    fn matrix_shows_isolation_and_digest_detects_tampering() {
        let vms = vec![
            vm("a1", Some("shop"), "10.0.0.5"),
            vm("a2", Some("shop"), "10.0.0.6"),
            vm("b1", Some("lab"), "10.0.0.9"),
            vm("c1", None, "10.0.0.20"),
        ];
        let (p, v) = parse_documents(
            "apiVersion: cilium.io/v2\nkind: CiliumNetworkPolicy\nmetadata:\n  name: iso\nspec:\n  endpointSelector:\n    matchLabels:\n      machina.io/project: shop\n  ingress:\n  - fromEndpoints:\n    - matchLabels:\n        machina.io/project: shop\n    toPorts:\n    - ports:\n      - port: \"5432\"\n",
        );
        assert!(v.ok(), "{:?}", v.errors);
        let (kind, g) = groups(&vms);
        assert_eq!(kind, "project");
        assert_eq!(
            g.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(),
            ["(no project)", "lab", "shop"]
        );
        let m = matrix(
            &p,
            &vms,
            &[],
            &["192.168.1.10".into()],
            &g,
            &default_probes(),
        );
        let cell = |f: &str, t: &str| m.iter().find(|c| c.from == f && c.to == t).unwrap();
        assert!(cell("lab", "shop").allowed.is_empty());
        assert_eq!(cell("shop", "shop").allowed, ["TCP/5432"]);
        assert_eq!(cell("shop", "shop").samples, ["a1 → a2", "a2 → a1"]);
        assert_eq!(cell("shop", "lab").allowed.len(), PROBES.len());
        assert!(cell("world", "shop").allowed.is_empty());
        assert!(
            !m.iter().any(|c| c.from == "lab" && c.to == "lab"),
            "one VM: no pair"
        );

        let mut e = Evidence {
            generated_at: "2026-10-04T10:00:00Z".into(),
            scope: "fleet".into(),
            policies: vec![PolicyEvidence {
                name: "iso".into(),
                enabled: true,
                sha256: policy_hash(&p[0]),
                selected_vms: vec!["a1".into(), "a2".into()],
                ..Default::default()
            }],
            matrix_groups: kind,
            matrix: m,
            ..Default::default()
        };
        e.seal(0);
        assert_eq!(e.summary.vms_selected, 2);
        assert_eq!(e.digest.len(), 64);
        assert!(e.verify());
        let md = markdown(&e);
        assert!(md.contains("| lab | shop |  |"), "{md}");
        assert!(md.contains(&e.digest));
        e.policies[0].enabled = false;
        assert!(!e.verify());
    }

    #[test]
    fn a_project_report_shows_only_that_project() {
        let vms = vec![
            vm("a1", Some("shop"), "10.0.0.5"),
            vm("a2", Some("shop"), "10.0.0.6"),
            vm("b1", Some("lab"), "10.0.0.9"),
            vm("b2", Some("lab"), "10.0.0.10"),
            vm("c1", None, "10.0.0.20"),
        ];
        let (kind, g) = groups_for_project(&vms, "shop");
        assert_eq!(kind, "project");
        assert_eq!(g[0], ("shop".into(), vec!["a1".into(), "a2".into()]));
        assert_eq!(g[1].0, OTHER_PROJECTS);
        assert_eq!(g[1].1[..2], ["c1".to_string(), "b1".to_string()], "spread");
        let mut e = Evidence {
            policies: vec![
                PolicyEvidence {
                    name: "both".into(),
                    selected_vms: vec!["a1".into(), "b1".into()],
                    ..Default::default()
                },
                PolicyEvidence {
                    name: "lab-only".into(),
                    selected_vms: vec!["b1".into()],
                    ..Default::default()
                },
            ],
            projects: vec![
                serde_json::json!({"project": "shop"}),
                serde_json::json!({"project": "lab"}),
            ],
            denied: vec![
                DeniedEdge {
                    src: "b1".into(),
                    dst: "a1".into(),
                    ..Default::default()
                },
                DeniedEdge {
                    src: "b1".into(),
                    dst: "c1".into(),
                    ..Default::default()
                },
            ],
            alerts: vec![
                serde_json::json!({"src": "10.0.0.5", "src_vm": "a1"}),
                serde_json::json!({"src": "b2"}),
            ],
            quarantines: vec![serde_json::json!({"vm": "b2"})],
            egress_ips: vec![serde_json::json!({"project": "lab"})],
            warnings: vec![
                "project shop has VMs on h1, h2 behind per-host NAT".into(),
                "project shopping has VMs on h1".into(),
            ],
            ..Default::default()
        };
        let members: BTreeSet<String> = ["a1".to_string(), "a2".to_string()].into();
        scope_to_project(&mut e, "shop", &members);
        assert_eq!(e.scope, "project shop");
        assert_eq!(e.policies.len(), 1);
        assert_eq!(e.policies[0].selected_vms, ["a1"], "other VMs not named");
        assert_eq!(e.projects.len(), 1);
        assert_eq!(e.denied.len(), 1);
        assert_eq!(e.alerts.len(), 1);
        assert!(e.quarantines.is_empty() && e.egress_ips.is_empty());
        assert_eq!(e.warnings.len(), 1);
        assert_eq!(e.summary.vms, 2);
    }

    #[test]
    fn probes_parse_and_default() {
        assert_eq!(parse_probes("").unwrap(), default_probes());
        assert_eq!(
            parse_probes("tcp/8443, udp/53,tcp/8443").unwrap(),
            [("TCP".to_string(), 8443), ("UDP".to_string(), 53)]
        );
        assert!(parse_probes("icmp/1").is_err());
        assert!(parse_probes("tcp/0").is_err());
        assert!(parse_probes("443").is_err());
    }

    #[test]
    fn signature_covers_the_digest_and_leaves_it_alone() {
        use base64::Engine as _;
        let dir = tempfile::tempdir().unwrap();
        let ca = crate::authca::Ca::load_or_create(dir.path()).unwrap();
        let signer = ca.doc_signer(dir.path()).unwrap();
        let mut e = Evidence {
            generated_at: "2026-10-04T10:00:00Z".into(),
            warnings: vec!["projects across NAT".into()],
            ..Default::default()
        };
        e.seal(0);
        let digest = e.digest.clone();
        e.sign(&signer, &ca.cert_pem).unwrap();
        assert_eq!(e.digest, digest);
        assert!(e.verify(), "the signature is outside the digest");
        let s = e.signature.clone().unwrap();
        let sig = base64::engine::general_purpose::STANDARD
            .decode(&s.value)
            .unwrap();
        assert!(signer.verify(digest.as_bytes(), &sig));
        assert!(!signer.verify(b"something else", &sig));
        assert_eq!(
            ca.doc_signer(dir.path()).unwrap().cert_pem,
            signer.cert_pem,
            "reused"
        );
        assert!(markdown(&e).contains("## Limits"));
    }
}
