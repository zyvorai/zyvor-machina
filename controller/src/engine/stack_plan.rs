// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Stack v2: instance groups, who-talks-to-whom policies, and per-group
//! sleep, restore point, backup, placement and scaling settings. Everything
//! here is pure: expanding a template into VMs, validating it, compiling its
//! policies to VM network policies, diffing it against what exists, and the
//! rule-based drafter used when no LLM is configured.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use machina_bpf::netpol::VmNetworkPolicy;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const LABEL_STACK: &str = "machina.io/stack";
pub const LABEL_GROUP: &str = "machina.io/stack-group";
pub const MAX_GROUP_COUNT: u32 = 20;
pub const MAX_STACK_VMS: usize = 40;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StackInstances {
    pub name: String,
    #[serde(default = "one")]
    pub count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flavor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_cores: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_gib: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default = "default_network")]
    pub network: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub anti_affinity: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ha: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sleep_after_minutes: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore_points: Option<RestorePointPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<BackupPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scaling: Option<ScalingPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RestorePointPlan {
    pub every_minutes: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackupPlan {
    pub interval_hours: i32,
    #[serde(default = "default_retain")]
    pub retain: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScalingPlan {
    pub min: u32,
    pub max: u32,
    #[serde(default = "default_metric")]
    pub metric: String,
    #[serde(default = "default_target")]
    pub target: f64,
}

/// `from` may be another group, `internet`, `fleet`, `host`, `any` or a CIDR.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StackPolicy {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<u16>,
    #[serde(default = "default_protocol")]
    pub protocol: String,
}

fn one() -> u32 {
    1
}
fn is_false(b: &bool) -> bool {
    !*b
}
fn default_network() -> String {
    "default".into()
}
fn default_retain() -> i32 {
    7
}
fn default_metric() -> String {
    "cpu".into()
}
fn default_target() -> f64 {
    70.0
}
fn default_protocol() -> String {
    "tcp".into()
}

#[derive(Debug, Clone, Copy)]
pub struct Flavor {
    pub vcpus: u32,
    pub memory_mib: i64,
    pub disk_gib: i64,
}

/// What the template's groups resolve against.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub flavors: HashMap<String, Flavor>,
    pub images: BTreeSet<String>,
    pub networks: BTreeSet<String>,
}

/// One VM a group expands to.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlannedVm {
    pub name: String,
    pub group: String,
    pub vcpus: u32,
    pub memory_mib: i64,
    pub disk_gib: i64,
    pub image: Option<String>,
    pub network: String,
    pub labels: BTreeMap<String, String>,
    pub tags: Vec<String>,
    pub ha: bool,
    pub sleep_after_minutes: Option<i32>,
    pub restore_points: Option<RestorePointPlan>,
}

pub fn vm_name(stack: &str, group: &str, count: u32, i: u32) -> String {
    if count == 1 {
        format!("{stack}-{group}")
    } else {
        format!("{stack}-{group}-{i}")
    }
}

pub fn stack_tag(stack: &str) -> String {
    format!("stack:{stack}")
}

pub fn group_tag(stack: &str, group: &str) -> String {
    format!("stack:{stack}/{group}")
}

fn memory_mib(s: &str) -> Option<i64> {
    machina_spec::parse_memory_mib(s)
        .ok()
        .map(|m| m as i64)
        .filter(|m| *m >= 256)
}

/// Expands every group into VMs. Unknown flavors fall back to the explicit
/// sizes (validation reports them).
pub fn expand(stack: &str, groups: &[StackInstances], catalog: &Catalog) -> Vec<PlannedVm> {
    let mut out = Vec::new();
    for g in groups {
        let flavor = g.flavor.as_ref().and_then(|f| catalog.flavors.get(f));
        let vcpus = g.cpu_cores.or(flavor.map(|f| f.vcpus)).unwrap_or(1).max(1);
        let mem = g
            .memory
            .as_deref()
            .and_then(memory_mib)
            .or(flavor.map(|f| f.memory_mib))
            .unwrap_or(1024);
        let disk = g
            .disk_gib
            .or(flavor.map(|f| f.disk_gib))
            .unwrap_or(10)
            .max(1);
        let mut tags = vec![
            "stack".to_string(),
            stack_tag(stack),
            group_tag(stack, &g.name),
        ];
        if g.anti_affinity {
            tags.push(format!("anti-affinity:{stack}-{}", g.name));
        }
        for i in 1..=g.count {
            let mut labels = g.labels.clone();
            labels.insert(LABEL_STACK.into(), stack.into());
            labels.insert(LABEL_GROUP.into(), g.name.clone());
            out.push(PlannedVm {
                name: vm_name(stack, &g.name, g.count, i),
                group: g.name.clone(),
                vcpus,
                memory_mib: mem,
                disk_gib: disk,
                image: g.image.clone(),
                network: g.network.clone(),
                labels,
                tags: tags.clone(),
                ha: g.ha,
                sleep_after_minutes: g.sleep_after_minutes,
                restore_points: g.restore_points.clone(),
            });
        }
    }
    out
}

fn label_value_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

fn is_cidr(s: &str) -> bool {
    let Some((addr, len)) = s.split_once('/') else {
        return false;
    };
    match (addr.parse::<std::net::IpAddr>(), len.parse::<u8>()) {
        (Ok(std::net::IpAddr::V4(_)), Ok(l)) => l <= 32,
        (Ok(std::net::IpAddr::V6(_)), Ok(l)) => l <= 128,
        _ => false,
    }
}

const PEERS: [&str; 5] = ["internet", "fleet", "host", "any", "world"];

/// Every problem with the v2 part of a template, as plain sentences.
pub fn validate(
    stack: &str,
    groups: &[StackInstances],
    policies: &[StackPolicy],
    catalog: &Catalog,
) -> Vec<String> {
    let mut errors = Vec::new();
    let mut names = BTreeSet::new();
    for g in groups {
        let what = format!("group `{}`", g.name);
        if !label_value_ok(&g.name) || g.name.contains('.') || g.name.contains('_') {
            errors.push(format!("{what}: names use letters, digits and dashes"));
        }
        if !names.insert(g.name.clone()) {
            errors.push(format!("{what} appears twice"));
        }
        if g.count == 0 || g.count > MAX_GROUP_COUNT {
            errors.push(format!("{what}: count must be 1-{MAX_GROUP_COUNT}"));
        }
        if let Some(f) = &g.flavor {
            if !catalog.flavors.contains_key(f) {
                errors.push(format!("{what}: no flavor named `{f}`"));
            }
        }
        if let Some(c) = g.cpu_cores {
            if c == 0 || c > 64 {
                errors.push(format!("{what}: cpu_cores must be 1-64"));
            }
        }
        if let Some(m) = &g.memory {
            if memory_mib(m).is_none_or(|m| m > 512 * 1024) {
                errors.push(format!(
                    "{what}: memory `{m}` is not a size between 256Mi and 512Gi"
                ));
            }
        }
        if let Some(d) = g.disk_gib {
            if !(1..=4096).contains(&d) {
                errors.push(format!("{what}: disk_gib must be 1-4096"));
            }
        }
        if let Some(i) = &g.image {
            if !catalog.images.is_empty()
                && !catalog.images.contains(i.split(':').next().unwrap_or(i))
            {
                errors.push(format!("{what}: no image named `{i}`"));
            }
        }
        if !catalog.networks.is_empty() && !catalog.networks.contains(&g.network) {
            errors.push(format!("{what}: no network named `{}`", g.network));
        }
        for (k, v) in &g.labels {
            if k.starts_with("machina.io/") {
                errors.push(format!("{what}: label `{k}` is reserved"));
            } else if !label_value_ok(k.rsplit('/').next().unwrap_or(k))
                || (!v.is_empty() && !label_value_ok(v))
            {
                errors.push(format!("{what}: label `{k}={v}` is not valid"));
            }
        }
        if let Some(m) = g.sleep_after_minutes {
            if m != 0 && !(5..=10_080).contains(&m) {
                errors.push(format!("{what}: sleep_after_minutes must be 0 or 5-10080"));
            }
        }
        if let Some(r) = &g.restore_points {
            if !(5..=10_080).contains(&r.every_minutes) {
                errors.push(format!("{what}: restore points every 5-10080 minutes"));
            }
            if r.keep.is_some_and(|k| !(1..=168).contains(&k)) {
                errors.push(format!("{what}: keep 1-168 restore points"));
            }
        }
        if let Some(b) = &g.backup {
            if !(1..=720).contains(&b.interval_hours) || !(0..=1000).contains(&b.retain) {
                errors.push(format!("{what}: backups every 1-720 hours, retain 0-1000"));
            }
        }
        if let Some(s) = &g.scaling {
            if s.min == 0 || s.min > g.count || g.count > s.max || s.max > MAX_GROUP_COUNT {
                errors.push(format!(
                    "{what}: scaling needs 1 <= min <= count <= max <= {MAX_GROUP_COUNT}"
                ));
            }
            if !matches!(s.metric.as_str(), "cpu" | "memory" | "network") {
                errors.push(format!("{what}: scaling metric is cpu, memory or network"));
            }
            if !(1.0..=100.0).contains(&s.target) && s.metric != "network" {
                errors.push(format!("{what}: scaling target is a percentage"));
            }
        }
        for i in 1..=g.count.min(MAX_GROUP_COUNT) {
            let n = vm_name(stack, &g.name, g.count, i);
            if let Err(e) = machina_spec::validate_name(&n) {
                errors.push(format!("{what}: VM name `{n}`: {e}"));
                break;
            }
        }
    }
    let total: u32 = groups.iter().map(|g| g.count).sum();
    if total as usize > MAX_STACK_VMS {
        errors.push(format!(
            "{total} VMs is more than the {MAX_STACK_VMS} a stack may have"
        ));
    }
    for p in policies {
        let what = format!("policy {} -> {}", p.from, p.to);
        if !names.contains(&p.to) {
            errors.push(format!("{what}: no group named `{}`", p.to));
        }
        if !names.contains(&p.from) && !PEERS.contains(&p.from.as_str()) && !is_cidr(&p.from) {
            errors.push(format!(
                "{what}: `from` is a group, internet, fleet, host, any or a CIDR"
            ));
        }
        if p.ports.contains(&0) {
            errors.push(format!("{what}: port 0"));
        }
        if !matches!(p.protocol.as_str(), "tcp" | "udp" | "any") {
            errors.push(format!("{what}: protocol is tcp, udp or any"));
        }
    }
    errors
}

pub fn policy_name(stack: &str, group: &str) -> String {
    format!("stack-{stack}-{group}")
}

fn group_selector(stack: &str, group: &str) -> Value {
    json!({ "matchLabels": { LABEL_STACK: stack, LABEL_GROUP: group } })
}

/// One ingress policy per group that any rule targets. Groups no rule targets
/// get no policy, so their traffic is unaffected.
pub fn compile_policies(stack: &str, policies: &[StackPolicy]) -> Vec<VmNetworkPolicy> {
    let mut by_target: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
    for p in policies {
        let mut rule = serde_json::Map::new();
        match p.from.as_str() {
            "internet" | "world" => {
                rule.insert("fromEntities".into(), json!(["world"]));
            }
            "fleet" => {
                rule.insert("fromEntities".into(), json!(["cluster"]));
            }
            "host" => {
                rule.insert("fromEntities".into(), json!(["host"]));
            }
            "any" => {
                rule.insert("fromEntities".into(), json!(["all"]));
            }
            cidr if is_cidr(cidr) => {
                rule.insert("fromCIDR".into(), json!([cidr]));
            }
            group => {
                rule.insert(
                    "fromEndpoints".into(),
                    json!([group_selector(stack, group)]),
                );
            }
        }
        if !p.ports.is_empty() {
            let protos: &[&str] = match p.protocol.as_str() {
                "udp" => &["UDP"],
                "any" => &["TCP", "UDP"],
                _ => &["TCP"],
            };
            let ports: Vec<Value> = p
                .ports
                .iter()
                .flat_map(|port| {
                    protos
                        .iter()
                        .map(move |pr| json!({ "port": port.to_string(), "protocol": pr }))
                })
                .collect();
            rule.insert("toPorts".into(), json!([{ "ports": ports }]));
        }
        by_target
            .entry(p.to.as_str())
            .or_default()
            .push(Value::Object(rule));
    }
    by_target
        .into_iter()
        .map(|(group, ingress)| VmNetworkPolicy {
            name: policy_name(stack, group),
            kind: "VmNetworkPolicy".into(),
            labels: [(LABEL_STACK.to_string(), stack.to_string())].into(),
            annotations: BTreeMap::new(),
            specs: vec![json!({
                "endpointSelector": group_selector(stack, group),
                "ingress": ingress,
            })],
        })
        .collect()
}

pub fn monthly_cost(vms: &[PlannedVm], vcpu_rate: f64, gib_rate: f64) -> f64 {
    let c: f64 = vms
        .iter()
        .map(|v| {
            crate::engine::ai::idle::monthly_cost(v.vcpus as i64, v.memory_mib, vcpu_rate, gib_rate)
        })
        .sum();
    (c * 100.0).round() / 100.0
}

/// A VM that exists for the stack.
#[derive(Debug, Clone, PartialEq)]
pub struct ActualVm {
    pub name: String,
    pub vcpus: u32,
    pub memory_mib: i64,
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct StackDiff {
    pub create: Vec<String>,
    pub delete: Vec<String>,
    /// VM name → what differs from the template.
    pub resize: Vec<(String, String)>,
    pub relabel: Vec<String>,
    pub policies_upsert: Vec<String>,
    pub policies_delete: Vec<String>,
}

impl StackDiff {
    pub fn is_empty(&self) -> bool {
        self.create.is_empty()
            && self.delete.is_empty()
            && self.resize.is_empty()
            && self.relabel.is_empty()
            && self.policies_upsert.is_empty()
            && self.policies_delete.is_empty()
    }
}

/// What it takes to go from `actual` to `planned`. Policies compare by spec.
pub fn diff(
    planned: &[PlannedVm],
    actual: &[ActualVm],
    want_policies: &[VmNetworkPolicy],
    have_policies: &[VmNetworkPolicy],
) -> StackDiff {
    let mut d = StackDiff::default();
    let have: HashMap<&str, &ActualVm> = actual.iter().map(|a| (a.name.as_str(), a)).collect();
    let want: BTreeSet<&str> = planned.iter().map(|p| p.name.as_str()).collect();
    for p in planned {
        match have.get(p.name.as_str()) {
            None => d.create.push(p.name.clone()),
            Some(a) => {
                let mut what = Vec::new();
                if a.vcpus != p.vcpus {
                    what.push(format!("{} vCPU, template says {}", a.vcpus, p.vcpus));
                }
                if a.memory_mib != p.memory_mib {
                    what.push(format!(
                        "{} MiB, template says {}",
                        a.memory_mib, p.memory_mib
                    ));
                }
                if !what.is_empty() {
                    d.resize.push((p.name.clone(), what.join("; ")));
                }
                if p.labels.iter().any(|(k, v)| a.labels.get(k) != Some(v)) {
                    d.relabel.push(p.name.clone());
                }
            }
        }
    }
    for a in actual {
        if !want.contains(a.name.as_str()) {
            d.delete.push(a.name.clone());
        }
    }
    let have_p: HashMap<&str, &VmNetworkPolicy> =
        have_policies.iter().map(|p| (p.name.as_str(), p)).collect();
    let want_p: BTreeSet<&str> = want_policies.iter().map(|p| p.name.as_str()).collect();
    for p in want_policies {
        if have_p
            .get(p.name.as_str())
            .is_none_or(|h| h.specs != p.specs || h.labels != p.labels)
        {
            d.policies_upsert.push(p.name.clone());
        }
    }
    for h in have_policies {
        if !want_p.contains(h.name.as_str()) {
            d.policies_delete.push(h.name.clone());
        }
    }
    d
}

// ---------------------------------------------------------------------------
// Drafting
// ---------------------------------------------------------------------------

/// The v2 part of a template the drafters produce.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DraftTemplate {
    #[serde(default)]
    pub instances: Vec<StackInstances>,
    #[serde(default)]
    pub policies: Vec<StackPolicy>,
}

struct Role {
    group: &'static str,
    words: &'static [&'static str],
    ports: &'static [u16],
    vcpus: u32,
    memory: &'static str,
    disk: i64,
}

const ROLES: &[Role] = &[
    Role {
        group: "lb",
        words: &[
            "lb",
            "load-balancer",
            "loadbalancer",
            "haproxy",
            "proxy",
            "ingress",
        ],
        ports: &[80, 443],
        vcpus: 1,
        memory: "1Gi",
        disk: 10,
    },
    Role {
        group: "web",
        words: &[
            "web",
            "webserver",
            "webservers",
            "frontend",
            "frontends",
            "nginx",
            "website",
            "site",
        ],
        ports: &[80, 443],
        vcpus: 2,
        memory: "2Gi",
        disk: 20,
    },
    Role {
        group: "api",
        words: &[
            "api",
            "apis",
            "backend",
            "backends",
            "app",
            "apps",
            "service",
            "services",
            "application",
        ],
        ports: &[8080],
        vcpus: 2,
        memory: "4Gi",
        disk: 20,
    },
    Role {
        group: "db",
        words: &[
            "db",
            "dbs",
            "database",
            "databases",
            "postgres",
            "postgresql",
            "mysql",
            "mariadb",
        ],
        ports: &[5432],
        vcpus: 4,
        memory: "8Gi",
        disk: 100,
    },
    Role {
        group: "cache",
        words: &["cache", "caches", "redis", "memcached", "valkey"],
        ports: &[6379],
        vcpus: 1,
        memory: "2Gi",
        disk: 10,
    },
    Role {
        group: "worker",
        words: &[
            "worker",
            "workers",
            "queue",
            "jobs",
            "job",
            "batch",
            "consumer",
            "consumers",
        ],
        ports: &[],
        vcpus: 2,
        memory: "4Gi",
        disk: 20,
    },
];

fn number_word(w: &str) -> Option<u32> {
    let n = match w {
        "a" | "an" | "one" | "single" => 1,
        "two" | "pair" | "couple" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        _ => return w.parse().ok(),
    };
    Some(n)
}

/// Turns a sentence like "3 web servers behind a load balancer, an API and a
/// postgres database with daily backups" into groups and policies.
pub fn draft_rules(prompt: &str) -> (DraftTemplate, Vec<String>) {
    let lower = prompt
        .to_lowercase()
        .replace("load balancer", "load-balancer");
    let words: Vec<String> = lower
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect();
    let has = |ws: &[&str]| words.iter().any(|w| ws.contains(&w.as_str()));
    let mut counts: BTreeMap<&'static str, u32> = BTreeMap::new();
    let mut order: Vec<&'static str> = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let Some(role) = ROLES.iter().find(|r| r.words.contains(&w.as_str())) else {
            continue;
        };
        let n = (i.saturating_sub(3)..i)
            .rev()
            .find_map(|j| number_word(&words[j]))
            .unwrap_or(1)
            .clamp(1, MAX_GROUP_COUNT);
        let e = counts.entry(role.group).or_insert(0);
        *e = (*e).max(n);
        if !order.contains(&role.group) {
            order.push(role.group);
        }
    }
    let mut notes = Vec::new();
    if order.is_empty() {
        order.push("api");
        counts.insert("api", 1);
        notes.push("No roles recognised; drafted a single app group.".to_string());
    }
    let mysql = has(&["mysql", "mariadb"]);
    let ha = has(&["ha", "highly", "redundant", "resilient", "failover"]);
    let size = if has(&["large", "big", "heavy"]) {
        2
    } else if has(&["small", "tiny", "minimal"]) {
        0
    } else {
        1
    };
    let dev = has(&[
        "dev",
        "development",
        "test",
        "testing",
        "sandbox",
        "staging",
    ]);
    let backups = has(&["backup", "backups", "backed"]);
    let rewind = has(&["restore", "rewind", "snapshot", "snapshots", "undo"]);
    let scale = has(&["autoscale", "autoscaling", "scale", "scaling", "elastic"]);

    let mut instances = Vec::new();
    for group in &order {
        let role = ROLES.iter().find(|r| r.group == *group).unwrap();
        let mut count = counts[group];
        if ha && count == 1 && matches!(*group, "web" | "api" | "lb") {
            count = 2;
        }
        let (vcpus, memory) = match size {
            0 => (role.vcpus.div_ceil(2), half(role.memory)),
            2 => (role.vcpus * 2, double(role.memory)),
            _ => (role.vcpus, role.memory.to_string()),
        };
        let stateful = matches!(*group, "db" | "cache");
        instances.push(StackInstances {
            name: group.to_string(),
            count,
            flavor: None,
            cpu_cores: Some(vcpus),
            memory: Some(memory),
            disk_gib: Some(role.disk),
            image: None,
            network: default_network(),
            labels: BTreeMap::new(),
            anti_affinity: count > 1,
            ha: ha && stateful,
            sleep_after_minutes: dev.then_some(30),
            restore_points: (rewind || (*group == "db" && !dev)).then_some(RestorePointPlan {
                every_minutes: 60,
                keep: Some(24),
            }),
            backup: (backups && (stateful || order.len() == 1)).then_some(BackupPlan {
                interval_hours: 24,
                retain: 7,
            }),
            scaling: (scale && matches!(*group, "web" | "api" | "worker")).then(|| ScalingPlan {
                min: count,
                max: (count * 3).min(MAX_GROUP_COUNT),
                metric: default_metric(),
                target: default_target(),
            }),
        });
    }
    if backups && !instances.iter().any(|i| i.backup.is_some()) {
        notes.push("Backups requested but no database group; none scheduled.".into());
    }

    let present = |g: &str| order.contains(&g);
    let mut policies = Vec::new();
    let mut allow = |from: &str, to: &str, ports: &[u16]| {
        policies.push(StackPolicy {
            from: from.into(),
            to: to.into(),
            ports: ports.to_vec(),
            protocol: default_protocol(),
        })
    };
    let front = ["lb", "web", "api"].into_iter().find(|g| present(g));
    if let Some(f) = front {
        allow(
            "internet",
            f,
            ROLES.iter().find(|r| r.group == f).unwrap().ports,
        );
    }
    if present("lb") && present("web") {
        allow("lb", "web", &[80, 443]);
    }
    if present("api") && front != Some("api") {
        let from = if present("web") { "web" } else { "lb" };
        allow(from, "api", &[8080]);
    }
    let db_port: &[u16] = if mysql { &[3306] } else { &[5432] };
    for client in ["api", "worker", "web"] {
        if present(client) && present("db") && (client != "web" || !present("api")) {
            allow(client, "db", db_port);
        }
    }
    for client in ["api", "worker", "web"] {
        if present(client) && present("cache") && (client != "web" || !present("api")) {
            allow(client, "cache", &[6379]);
        }
    }
    for g in &order {
        allow("host", g, &[22]);
    }
    (
        DraftTemplate {
            instances,
            policies,
        },
        notes,
    )
}

fn half(m: &str) -> String {
    let mib = memory_mib(m).unwrap_or(1024) / 2;
    format!("{}Gi", (mib / 1024).max(1))
}

fn double(m: &str) -> String {
    let mib = memory_mib(m).unwrap_or(1024) * 2;
    format!("{}Gi", mib / 1024)
}

pub fn llm_system_prompt(stack: &str, catalog: &Catalog) -> String {
    let flavors: Vec<String> = {
        let mut f: Vec<_> = catalog.flavors.iter().collect();
        f.sort_by(|a, b| a.0.cmp(b.0));
        f.iter()
            .map(|(n, f)| {
                format!(
                    "{n} ({} vCPU, {} MiB, {} GiB)",
                    f.vcpus, f.memory_mib, f.disk_gib
                )
            })
            .collect()
    };
    format!(
        "You design application stacks for the Machina private cloud. Reply with one JSON object and nothing else:\n\
{{\"instances\": [{{\"name\": \"web\", \"count\": 2, \"flavor\"?: string, \"cpu_cores\"?: int, \"memory\"?: \"4Gi\", \"disk_gib\"?: int, \"image\"?: string, \"network\"?: string, \"anti_affinity\"?: bool, \"ha\"?: bool, \"sleep_after_minutes\"?: int, \"restore_points\"?: {{\"every_minutes\": int, \"keep\": int}}, \"backup\"?: {{\"interval_hours\": int, \"retain\": int}}, \"scaling\"?: {{\"min\": int, \"max\": int, \"metric\": \"cpu\"|\"memory\"|\"network\", \"target\": number}}}}],\n \
\"policies\": [{{\"from\": group | \"internet\" | \"fleet\" | \"host\" | \"any\" | CIDR, \"to\": group, \"ports\": [int], \"protocol\": \"tcp\"|\"udp\"|\"any\"}}]}}\n\
Rules: group names are short lowercase words with dashes (VMs are named {stack}-<group>-<n>). count 1-{MAX_GROUP_COUNT}, at most {MAX_STACK_VMS} VMs in total. \
A group that is the `to` of any policy accepts only the listed traffic, so list every flow the application needs, plus `host` on port 22 for each group. \
Use anti_affinity for groups with more than one VM. sleep_after_minutes (0 or 5-10080) only for dev/test. restore_points every 5-10080 minutes, keep 1-168. backup interval 1-720 hours.\n\
Flavors: {}\nImages: {}\nNetworks: {}\n\
If the request is not about infrastructure, reply `# cannot: <reason>`.",
        if flavors.is_empty() { "none (use cpu_cores and memory)".into() } else { flavors.join(", ") },
        if catalog.images.is_empty() { "none (omit image)".into() } else { catalog.images.iter().cloned().collect::<Vec<_>>().join(", ") },
        if catalog.networks.is_empty() { "default".into() } else { catalog.networks.iter().cloned().collect::<Vec<_>>().join(", ") },
    )
}

pub fn llm_repair_prompt(request: &str, reply: &str, errors: &[String]) -> String {
    format!(
        "Request: {request}\n\nYour JSON:\n{reply}\n\nIt has these problems:\n- {}\n\nReply with the corrected JSON object only.",
        errors.join("\n- ")
    )
}

pub fn llm_declined(reply: &str) -> Option<String> {
    reply
        .trim()
        .strip_prefix("# cannot:")
        .map(|s| s.trim().to_string())
}

pub fn extract_json(reply: &str) -> &str {
    let r = reply.trim();
    if let Some(start) = r.find("```") {
        let body = &r[start + 3..];
        let body = body.strip_prefix("json").unwrap_or(body);
        if let Some(end) = body.find("```") {
            return body[..end].trim();
        }
    }
    match (r.find('{'), r.rfind('}')) {
        (Some(a), Some(b)) if b > a => &r[a..=b],
        _ => r,
    }
}

/// Parses and validates an LLM reply.
pub fn accept_llm(
    stack: &str,
    reply: &str,
    catalog: &Catalog,
) -> Result<DraftTemplate, Vec<String>> {
    let t: DraftTemplate = serde_json::from_str(extract_json(reply))
        .map_err(|e| vec![format!("not valid JSON for the schema: {e}")])?;
    if t.instances.is_empty() {
        return Err(vec!["no instances".into()]);
    }
    let errors = validate(stack, &t.instances, &t.policies, catalog);
    if errors.is_empty() {
        Ok(t)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(name: &str, count: u32) -> StackInstances {
        serde_json::from_value(json!({ "name": name, "count": count })).unwrap()
    }

    #[test]
    fn expand_names_sizes_and_labels() {
        let mut cat = Catalog::default();
        cat.flavors.insert(
            "m1.small".into(),
            Flavor {
                vcpus: 2,
                memory_mib: 2048,
                disk_gib: 20,
            },
        );
        let mut web = group("web", 2);
        web.flavor = Some("m1.small".into());
        web.anti_affinity = true;
        let mut db = group("db", 1);
        db.memory = Some("8Gi".into());
        let vms = expand("shop", &[web, db], &cat);
        let names: Vec<_> = vms.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, ["shop-web-1", "shop-web-2", "shop-db"]);
        assert_eq!(
            (vms[0].vcpus, vms[0].memory_mib, vms[0].disk_gib),
            (2, 2048, 20)
        );
        assert_eq!(
            (vms[2].vcpus, vms[2].memory_mib, vms[2].disk_gib),
            (1, 8192, 10)
        );
        assert_eq!(vms[0].labels[LABEL_GROUP], "web");
        assert!(vms[0].tags.contains(&"anti-affinity:shop-web".to_string()));
        assert!(!vms[2].tags.iter().any(|t| t.starts_with("anti-affinity:")));
    }

    #[test]
    fn validate_reports_bad_references() {
        let cat = Catalog::default();
        let mut g = group("web", 0);
        g.flavor = Some("huge".into());
        g.sleep_after_minutes = Some(2);
        let p = StackPolicy {
            from: "nowhere".into(),
            to: "db".into(),
            ports: vec![0],
            protocol: "sctp".into(),
        };
        let e = validate("shop", &[g], &[p], &cat).join("\n");
        for want in [
            "count must be",
            "no flavor named `huge`",
            "sleep_after_minutes",
            "no group named `db`",
            "`from` is a group",
            "port 0",
            "protocol",
        ] {
            assert!(e.contains(want), "{want} missing in {e}");
        }
        let ok = StackPolicy {
            from: "10.0.0.0/8".into(),
            to: "web".into(),
            ports: vec![443],
            protocol: "tcp".into(),
        };
        assert!(validate("shop", &[group("web", 2)], &[ok], &cat).is_empty());
    }

    #[test]
    fn policies_compile_and_validate_as_netpol() {
        let ps = vec![
            StackPolicy {
                from: "internet".into(),
                to: "web".into(),
                ports: vec![443],
                protocol: "tcp".into(),
            },
            StackPolicy {
                from: "web".into(),
                to: "db".into(),
                ports: vec![5432],
                protocol: "tcp".into(),
            },
            StackPolicy {
                from: "host".into(),
                to: "db".into(),
                ports: vec![],
                protocol: "tcp".into(),
            },
        ];
        let compiled = compile_policies("shop", &ps);
        assert_eq!(compiled.len(), 2);
        let db = compiled.iter().find(|p| p.name == "stack-shop-db").unwrap();
        let ingress = db.specs[0]["ingress"].as_array().unwrap();
        assert_eq!(
            ingress[0]["fromEndpoints"][0]["matchLabels"][LABEL_GROUP],
            "web"
        );
        assert_eq!(ingress[0]["toPorts"][0]["ports"][0]["port"], "5432");
        assert_eq!(ingress[1]["fromEntities"][0], "host");
        assert!(ingress[1].get("toPorts").is_none());
        let yaml = machina_bpf::netpol::nl::to_yaml(&compiled);
        let (parsed, v) = machina_bpf::netpol::parse_documents(&yaml);
        assert!(v.errors.is_empty(), "{:?}", v.errors);
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn diff_finds_creates_deletes_resizes_and_policy_changes() {
        let cat = Catalog::default();
        let planned = expand("s", &[group("web", 2)], &cat);
        let mut labels = planned[0].labels.clone();
        let actual = vec![
            ActualVm {
                name: "s-web-1".into(),
                vcpus: 2,
                memory_mib: 1024,
                labels: labels.clone(),
            },
            ActualVm {
                name: "s-old".into(),
                vcpus: 1,
                memory_mib: 1024,
                labels: BTreeMap::new(),
            },
        ];
        labels.clear();
        let want = compile_policies(
            "s",
            &[StackPolicy {
                from: "host".into(),
                to: "web".into(),
                ports: vec![22],
                protocol: "tcp".into(),
            }],
        );
        let mut stale = want.clone();
        stale[0].specs[0]["ingress"] = json!([]);
        stale.push(VmNetworkPolicy {
            name: "stack-s-gone".into(),
            ..stale[0].clone()
        });
        let d = diff(&planned, &actual, &want, &stale);
        assert_eq!(d.create, ["s-web-2"]);
        assert_eq!(d.delete, ["s-old"]);
        assert_eq!(d.resize[0].0, "s-web-1");
        assert_eq!(d.policies_upsert, ["stack-s-web"]);
        assert_eq!(d.policies_delete, ["stack-s-gone"]);
        assert!(diff(&planned[..1], &actual[..1], &want, &want).resize.len() == 1);
    }

    #[test]
    fn rule_drafter_builds_a_three_tier_stack() {
        let (t, notes) = draft_rules(
            "3 web servers behind a load balancer, an API, and a highly available postgres database with daily backups",
        );
        assert!(notes.is_empty(), "{notes:?}");
        let g = |n: &str| t.instances.iter().find(|i| i.name == n).unwrap();
        assert_eq!(g("web").count, 3);
        assert!(g("web").anti_affinity);
        assert_eq!(g("lb").count, 2);
        assert_eq!(g("api").count, 2);
        assert!(g("db").backup.is_some() && g("db").ha);
        let flows: Vec<String> = t
            .policies
            .iter()
            .map(|p| format!("{}>{}", p.from, p.to))
            .collect();
        for f in ["internet>lb", "lb>web", "web>api", "api>db", "host>db"] {
            assert!(flows.contains(&f.to_string()), "{f} missing in {flows:?}");
        }
        assert!(validate("shop", &t.instances, &t.policies, &Catalog::default()).is_empty());
    }

    #[test]
    fn rule_drafter_dev_sleeps_and_unknown_falls_back() {
        let (t, _) = draft_rules("small dev environment with two workers and redis");
        assert!(t
            .instances
            .iter()
            .all(|i| i.sleep_after_minutes == Some(30)));
        assert_eq!(
            t.instances
                .iter()
                .find(|i| i.name == "worker")
                .unwrap()
                .count,
            2
        );
        let (t, notes) = draft_rules("something");
        assert_eq!(t.instances.len(), 1);
        assert!(!notes.is_empty());
    }

    #[test]
    fn llm_reply_is_extracted_and_checked() {
        let cat = Catalog::default();
        let reply = "Here you go:\n```json\n{\"instances\":[{\"name\":\"web\",\"count\":2}],\"policies\":[{\"from\":\"internet\",\"to\":\"web\",\"ports\":[443]}]}\n```";
        let t = accept_llm("shop", reply, &cat).unwrap();
        assert_eq!(t.instances[0].count, 2);
        let bad = "{\"instances\":[{\"name\":\"web\",\"count\":99}],\"policies\":[]}";
        assert!(accept_llm("shop", bad, &cat).unwrap_err()[0].contains("count"));
        assert_eq!(
            llm_declined("# cannot: not infra").as_deref(),
            Some("not infra")
        );
    }
}
