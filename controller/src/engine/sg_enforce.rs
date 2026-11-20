// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Security groups → managed VM network policies.
//!
//! A VM with at least one attached group in `enforce` mode gets one policy `sg-<vm>` pinned to it by
//! `machina.io/vm-name`. Rules of all its groups are unioned (EC2 semantics: allow-only). Both directions
//! are present, so anything not allowed is denied; reply traffic is allowed by the VM edge's conntrack.
//! Groups in `audit` mode produce nothing: they stay advisory and report so.

use std::collections::{BTreeMap, BTreeSet};

use machina_bpf::netpol::VmNetworkPolicy;
use serde_json::{json, Value};
use crate::db::DbPool;

pub const MANAGED_LABEL: &str = "machina.io/managed-by";
pub const MANAGED_VALUE: &str = "security-group";

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Rule {
    pub security_group_id: String,
    pub direction: String,
    pub protocol: Option<String>,
    pub port_min: Option<i64>,
    pub port_max: Option<i64>,
    pub remote_cidr: Option<String>,
    pub remote_sg_id: Option<String>,
}

/// Everything the compiler needs, loaded in one go.
#[derive(Debug, Default, Clone)]
pub struct Model {
    /// sg id → mode (`audit` | `enforce`).
    pub modes: BTreeMap<String, String>,
    pub rules: Vec<Rule>,
    /// vm name → attached sg ids.
    pub attached: BTreeMap<String, BTreeSet<String>>,
}

pub async fn load(pool: &DbPool) -> Model {
    let mut m = Model::default();
    let sgs: Vec<(String, String)> = crate::db::query_as("SELECT lower(hex(id)), mode FROM security_groups")
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    m.modes = sgs.into_iter().collect();
    m.rules = crate::db::query_as(
        "SELECT lower(hex(security_group_id)) AS security_group_id, direction, protocol, port_min, port_max, remote_cidr, remote_sg_id \
         FROM security_group_rules ORDER BY created_at, id",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    let att: Vec<(String, String)> = crate::db::query_as(
        "SELECT v.name, lower(hex(i.sg_id)) FROM instance_security_groups i \
         JOIN vms v ON v.id = i.vm_id",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for (vm, sg) in att {
        m.attached.entry(vm).or_default().insert(sg);
    }
    m
}

pub fn policy_name(vm: &str) -> String {
    format!("sg-{vm}")
}

fn port_entries(r: &Rule) -> Vec<Value> {
    let proto = match r.protocol.as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("tcp") => "TCP",
        Some("udp") => "UDP",
        _ => "ANY",
    };
    match (r.port_min, r.port_max) {
        (Some(lo), hi) if lo > 0 => {
            let mut p = json!({ "port": lo.to_string(), "protocol": proto });
            if let Some(h) = hi.filter(|h| *h > lo) {
                p["endPort"] = json!(h);
            }
            vec![p]
        }
        _ => Vec::new(),
    }
}

/// One Cilium rule for `r`, `egress` selects the field names. `None` when it can't be expressed.
fn rule_json(r: &Rule, egress: bool, sg_vms: &BTreeMap<&str, Vec<&str>>) -> Option<Value> {
    let (ep, cidr, ent) = if egress {
        ("toEndpoints", "toCIDR", "toEntities")
    } else {
        ("fromEndpoints", "fromCIDR", "fromEntities")
    };
    let mut rule = if let Some(sg) = r.remote_sg_id.as_deref().filter(|s| !s.is_empty()) {
        let names = sg_vms.get(sg).cloned().unwrap_or_default();
        if names.is_empty() {
            return None; // an empty group matches nobody
        }
        json!({ ep: [{ "matchExpressions": [{
            "key": "machina.io/vm-name", "operator": "In", "values": names }] }] })
    } else {
        let c = r.remote_cidr.as_deref().unwrap_or("0.0.0.0/0").trim();
        if c == "0.0.0.0/0" || c == "::/0" {
            json!({ ent: ["all"] })
        } else {
            json!({ cidr: [c] })
        }
    };
    match r.protocol.as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("icmp") | Some("icmpv6") | Some("1") | Some("58") => {
            let v6 = matches!(r.protocol.as_deref(), Some(p) if p.eq_ignore_ascii_case("icmpv6") || p == "58");
            let family = if v6 { "IPv6" } else { "IPv4" };
            let types: Vec<i64> = match r.port_min {
                Some(t) if t >= 0 => vec![t],
                _ => vec![0, 3, 4, 8, 11, 12],
            };
            rule["icmps"] = json!([{ "fields": types.iter()
                .map(|t| json!({ "type": t, "family": family })).collect::<Vec<_>>() }]);
        }
        _ => {
            let ports = port_entries(r);
            if !ports.is_empty() {
                rule["toPorts"] = json!([{ "ports": ports }]);
            } else if matches!(r.protocol.as_deref().map(str::to_ascii_lowercase).as_deref(),
                Some("tcp") | Some("udp"))
            {
                let p = r.protocol.as_deref().unwrap().to_ascii_uppercase();
                rule["toPorts"] = json!([{ "ports": [{ "port": "1", "endPort": 65535, "protocol": p }] }]);
            }
        }
    }
    Some(rule)
}

/// Managed policies for every VM that has an enforcing group attached.
pub fn policies(m: &Model) -> Vec<VmNetworkPolicy> {
    // sg id → names of the VMs that carry it (for remote_sg_id peers)
    let mut sg_vms: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (vm, sgs) in &m.attached {
        for sg in sgs {
            sg_vms.entry(sg.as_str()).or_default().push(vm.as_str());
        }
    }
    let mut out = Vec::new();
    for (vm, sgs) in &m.attached {
        let enforcing: Vec<&String> = sgs
            .iter()
            .filter(|s| m.modes.get(*s).map(String::as_str) == Some("enforce"))
            .collect();
        if enforcing.is_empty() {
            continue;
        }
        let mut ingress = Vec::new();
        let mut egress = Vec::new();
        for r in m
            .rules
            .iter()
            .filter(|r| enforcing.iter().any(|s| **s == r.security_group_id))
        {
            let is_egress = r.direction == "egress";
            if let Some(j) = rule_json(r, is_egress, &sg_vms) {
                let list = if is_egress { &mut egress } else { &mut ingress };
                if !list.contains(&j) {
                    list.push(j);
                }
            }
        }
        out.push(VmNetworkPolicy {
            name: policy_name(vm),
            kind: "CiliumNetworkPolicy".into(),
            labels: [(MANAGED_LABEL.to_string(), MANAGED_VALUE.to_string())].into(),
            annotations: BTreeMap::new(),
            specs: vec![json!({
                "description": format!("Security groups on {vm}"),
                "endpointSelector": { "matchLabels": { "machina.io/vm-name": vm } },
                "ingress": ingress,
                "egress": egress,
            })],
        });
    }
    out
}

/// What switching `sg_id` to enforce would do to one attached instance.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct VmPreview {
    pub vm: String,
    /// Allowed ingress / egress rules the instance would end up with (all its enforcing groups, plus this one).
    pub ingress_rules: usize,
    pub egress_rules: usize,
    pub warnings: Vec<String>,
}

/// Dry run: the policy each attached instance would get if `sg_id` were enforcing, with plain-language warnings.
pub fn preview(m: &Model, sg_id: &str) -> Vec<VmPreview> {
    let mut what_if = m.clone();
    what_if.modes.insert(sg_id.to_string(), "enforce".into());
    let policies = policies(&what_if);
    let mut out = Vec::new();
    for (vm, sgs) in &m.attached {
        if !sgs.contains(sg_id) {
            continue;
        }
        let spec = policies
            .iter()
            .find(|p| p.name == policy_name(vm))
            .and_then(|p| p.specs.first().cloned())
            .unwrap_or(Value::Null);
        let ing = spec["ingress"].as_array().cloned().unwrap_or_default();
        let eg = spec["egress"].as_array().cloned().unwrap_or_default();
        let mut warnings = Vec::new();
        if ing.is_empty() {
            warnings.push("no ingress rule: every inbound connection will be refused, including SSH and the console's network paths".into());
        } else if !allows_port(&ing, 22) {
            warnings.push("no ingress rule allows TCP 22 (SSH)".into());
        }
        if eg.is_empty() {
            warnings.push("no egress rule: the instance will not be able to start any outbound connection (DNS, updates, the guest agent's network paths)".into());
        }
        if ing.iter().any(|r| r.get("fromEntities").is_some_and(|e| e == &json!(["all"])) && r.get("toPorts").is_none() && r.get("icmps").is_none()) {
            warnings.push("an ingress rule allows every port from everywhere".into());
        }
        out.push(VmPreview { vm: vm.clone(), ingress_rules: ing.len(), egress_rules: eg.len(), warnings });
    }
    out
}

fn allows_port(rules: &[Value], port: u16) -> bool {
    rules.iter().any(|r| match r.get("toPorts").and_then(|t| t.as_array()) {
        None => r.get("icmps").is_none(), // no port restriction and not ICMP-only: every port
        Some(tp) => tp.iter().flat_map(|t| t["ports"].as_array().cloned().unwrap_or_default()).any(|p| {
            let lo = p["port"].as_str().and_then(|s| s.parse::<u16>().ok()).unwrap_or(0);
            let hi = p["endPort"].as_u64().map_or(lo, |e| e as u16);
            (lo..=hi).contains(&port) && p["protocol"].as_str() != Some("UDP")
        }),
    })
}

/// host id → VM names with an enforcing group on that host.
async fn enforcing_hosts(pool: &DbPool) -> BTreeMap<String, Vec<String>> {
    let rows: Vec<(String, String)> = crate::db::query_as(
        "SELECT DISTINCT COALESCE(lower(hex(v.host_id)), ''), v.name FROM instance_security_groups i \
         JOIN security_groups g ON g.id = i.sg_id AND g.mode = 'enforce' \
         JOIN vms v ON v.id = i.vm_id",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (h, vm) in rows {
        out.entry(h).or_default().push(vm);
    }
    out
}

/// Re-assert enforce mode with a fresh lease on every host that has an enforcing group attached:
/// bpfd forgets the mode on restart and the datapath stops dropping when the lease lapses.
pub async fn renew(state: &crate::state::AppState) {
    use super::bpf::{self, LOCAL_HOST_ID};
    use machina_bpf::api::{Mode, Request};
    let want = enforcing_hosts(&state.pool).await;
    if want.is_empty() {
        return;
    }
    let lease = state.config.bpf_enforce_lease_secs.max(60);
    for h in bpf::hosts(&state.pool).await {
        let key = h.id.replace('-', "").to_lowercase();
        let has = want.contains_key(&key) || (h.id == LOCAL_HOST_ID && want.contains_key(""));
        if !has {
            continue;
        }
        let req = Request::SetMode { mode: Mode::Enforce, lease_secs: Some(lease) };
        if let Err(e) = bpf::call(&h, &req).await {
            tracing::warn!(host = %h.hostname, "security-group enforce lease: {e:#}");
        }
    }
}

/// `sg_id` is the group's UUID as simple hex.
/// Honest per-group state from what each host's VM edge reports.
/// `advisory` (audit mode), `pending` (nothing attached), `enforced`, `auditing` (edge not enforcing), `failed` (host unreachable).
pub async fn enforcement(pool: &DbPool, sg_id: &str) -> Value {
    use super::bpf;
    use machina_bpf::api::{Request, VmEdgeStatus};
    let mode: Option<String> = crate::db::query_scalar("SELECT mode FROM security_groups WHERE lower(hex(id)) = ?")
        .bind(sg_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
    let vms: Vec<(String, Option<String>)> = crate::db::query_as(
        "SELECT v.name, lower(hex(v.host_id)) FROM instance_security_groups i \
         JOIN vms v ON v.id = i.vm_id WHERE lower(hex(i.sg_id)) = ?",
    )
    .bind(sg_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    let names: Vec<&str> = vms.iter().map(|(n, _)| n.as_str()).collect();
    if mode.as_deref() != Some("enforce") {
        return json!({ "state": "advisory", "reason": "group is in audit mode: rules are stored but not applied", "vms": names });
    }
    if vms.is_empty() {
        return json!({ "state": "pending", "reason": "no instance has this group attached", "vms": names });
    }
    let mut enforcing: BTreeMap<String, bool> = BTreeMap::new();
    for (h, res) in bpf::fan_out(pool, &Request::VmEdgeStatus).await {
        let e: Option<VmEdgeStatus> = res.ok().and_then(|v| serde_json::from_value(v).ok());
        let key = if h.is_local() { String::new() } else { h.id.replace('-', "").to_lowercase() };
        enforcing.insert(key, e.is_some_and(|e| e.enforcing));
    }
    let mut unreachable = 0;
    let mut auditing = 0;
    for (_, h) in &vms {
        match enforcing.get(h.as_deref().unwrap_or("")) {
            None => unreachable += 1,
            Some(false) => auditing += 1,
            Some(true) => {}
        }
    }
    let (state, reason) = if unreachable > 0 {
        ("failed", format!("{unreachable} instance(s) are on a host whose VM edge did not answer"))
    } else if auditing > 0 {
        ("auditing", format!("{auditing} instance(s) are on a host whose VM edge is not enforcing (lease lapsed or bpfd restarted); it is re-asserted every 30 s"))
    } else {
        ("enforced", "every attached instance is on a host that reports enforcement".to_string())
    };
    json!({ "state": state, "reason": reason, "vms": names })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(sg: &str, dir: &str, proto: Option<&str>, lo: Option<i64>, hi: Option<i64>, cidr: Option<&str>, rsg: Option<&str>) -> Rule {
        Rule {
            security_group_id: sg.into(),
            direction: dir.into(),
            protocol: proto.map(Into::into),
            port_min: lo,
            port_max: hi,
            remote_cidr: cidr.map(Into::into),
            remote_sg_id: rsg.map(Into::into),
        }
    }

    fn model() -> Model {
        let mut m = Model::default();
        m.modes.insert("a".into(), "enforce".into());
        m.modes.insert("b".into(), "audit".into());
        m.attached.entry("web".into()).or_default().insert("a".into());
        m.attached.entry("db".into()).or_default().insert("b".into());
        m
    }

    #[test]
    fn only_enforcing_groups_make_policies() {
        let m = model();
        let p = policies(&m);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].name, "sg-web");
        assert_eq!(p[0].labels[MANAGED_LABEL], MANAGED_VALUE);
    }

    #[test]
    fn no_rules_means_default_deny_both_ways() {
        let p = policies(&model());
        let s = &p[0].specs[0];
        assert_eq!(s["ingress"], json!([]));
        assert_eq!(s["egress"], json!([]));
    }

    #[test]
    fn cidr_ports_and_ranges() {
        let mut m = model();
        m.rules.push(rule("a", "ingress", Some("tcp"), Some(22), Some(22), Some("10.0.0.0/8"), None));
        m.rules.push(rule("a", "ingress", Some("tcp"), Some(8000), Some(8100), Some("0.0.0.0/0"), None));
        let s = &policies(&m)[0].specs[0];
        let i = s["ingress"].as_array().unwrap();
        assert_eq!(i[0]["fromCIDR"], json!(["10.0.0.0/8"]));
        assert_eq!(i[0]["toPorts"][0]["ports"][0]["port"], "22");
        assert!(i[0]["toPorts"][0]["ports"][0].get("endPort").is_none());
        assert_eq!(i[1]["fromEntities"], json!(["all"]));
        assert_eq!(i[1]["toPorts"][0]["ports"][0]["endPort"], 8100);
    }

    #[test]
    fn egress_all_and_icmp() {
        let mut m = model();
        m.rules.push(rule("a", "egress", None, None, None, None, None));
        m.rules.push(rule("a", "ingress", Some("icmp"), Some(8), None, Some("0.0.0.0/0"), None));
        let s = &policies(&m)[0].specs[0];
        assert_eq!(s["egress"][0], json!({ "toEntities": ["all"] }));
        assert_eq!(s["ingress"][0]["icmps"][0]["fields"][0]["type"], 8);
    }

    #[test]
    fn sg_reference_selects_member_vms_and_empty_group_matches_nobody() {
        let mut m = model();
        m.attached.entry("app".into()).or_default().insert("a".into());
        m.rules.push(rule("a", "ingress", Some("tcp"), Some(80), None, None, Some("a")));
        m.rules.push(rule("a", "ingress", Some("tcp"), Some(81), None, None, Some("ghost")));
        let p = policies(&m);
        let web = p.iter().find(|p| p.name == "sg-web").unwrap();
        let i = web.specs[0]["ingress"].as_array().unwrap();
        assert_eq!(i.len(), 1, "a reference to an empty group adds no rule");
        let names = &i[0]["fromEndpoints"][0]["matchExpressions"][0]["values"];
        assert_eq!(names, &json!(["app", "web"]));
    }

    #[test]
    fn rules_of_several_groups_are_unioned_without_duplicates() {
        let mut m = model();
        m.modes.insert("c".into(), "enforce".into());
        m.attached.get_mut("web").unwrap().insert("c".into());
        let r = rule("a", "ingress", Some("tcp"), Some(443), None, Some("1.2.3.0/24"), None);
        m.rules.push(r.clone());
        m.rules.push(Rule { security_group_id: "c".into(), ..r });
        let s = &policies(&m)[0].specs[0];
        assert_eq!(s["ingress"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn preview_warns_about_lockout_and_open_rules() {
        let mut m = model();
        let p = preview(&m, "a");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].vm, "web");
        assert_eq!((p[0].ingress_rules, p[0].egress_rules), (0, 0));
        assert!(p[0].warnings.iter().any(|w| w.contains("no ingress rule")));
        assert!(p[0].warnings.iter().any(|w| w.contains("no egress rule")));

        m.rules.push(rule("a", "ingress", Some("tcp"), Some(22), Some(22), Some("10.0.0.0/8"), None));
        m.rules.push(rule("a", "egress", None, None, None, None, None));
        let p = preview(&m, "a");
        assert!(p[0].warnings.is_empty(), "{:?}", p[0].warnings);

        m.rules.push(rule("a", "ingress", Some("tcp"), Some(80), None, Some("0.0.0.0/0"), None));
        m.rules.retain(|r| r.port_min != Some(22));
        let p = preview(&m, "a");
        assert!(p[0].warnings.iter().any(|w| w.contains("TCP 22")));
    }

    #[test]
    fn preview_ignores_instances_without_the_group_and_does_not_change_the_model() {
        let m = model();
        assert!(preview(&m, "b").iter().all(|v| v.vm == "db"));
        assert_eq!(m.modes["a"], "enforce");
        assert_eq!(m.modes["b"], "audit");
    }

    #[test]
    fn compiled_policy_validates_in_the_netpol_compiler() {
        let mut m = model();
        m.rules.push(rule("a", "ingress", Some("tcp"), Some(22), Some(22), Some("10.0.0.0/8"), None));
        m.rules.push(rule("a", "ingress", Some("icmp"), None, None, None, None));
        m.rules.push(rule("a", "egress", None, None, None, None, None));
        let p = policies(&m);
        let v = machina_bpf::netpol::validate_policy(&p[0]);
        assert!(v.errors.is_empty(), "{:?}", v.errors);
    }
}
