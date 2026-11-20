// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Deterministic root-cause analysis for a frozen Machina Black Box incident.
//!
//! The engine never asks an LLM to choose a cause. It ranks competing
//! hypotheses from captured bpfd evidence plus VM-intel / network-health
//! snapshots. An LLM can explain the returned evidence later, but cannot
//! change the score or invent evidence.

use std::collections::BTreeMap;

use anyhow::anyhow;
use chrono::{DateTime, Utc};
use machina_bpf::api::{
    BlackBoxEvent, BlackBoxIncident, BlackBoxSnapshot, NetHealth, Request, ScxStatus,
    VmIntelReport,
};
use serde::{Deserialize, Serialize};

use crate::db::DbPool;
use crate::engine::bpf;

#[derive(Debug, Clone, Deserialize)]
pub struct BlackBoxRcaQuery {
    pub vm_name: String,
    #[serde(default)]
    pub host_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RcaStats {
    pub events: usize,
    pub severe_events: usize,
    pub topics: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlackBoxHypothesis {
    pub id: String,
    pub title: String,
    /// Evidence score, 0..100. This is not a probability.
    pub score: f64,
    pub evidence: Vec<String>,
    pub suggested_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlackBoxRcaReport {
    pub vm: String,
    pub host_id: String,
    pub hostname: String,
    pub incident_id: String,
    pub triggered_at: String,
    pub frozen: bool,
    pub deterministic: bool,
    pub root_cause: String,
    /// Confidence that the top-ranked hypothesis is meaningfully ahead of
    /// alternatives, 0..1. It is not a failure probability.
    pub confidence: f64,
    pub hypotheses: Vec<BlackBoxHypothesis>,
    pub evidence: Vec<String>,
    pub suggested_actions: Vec<String>,
    pub stats: RcaStats,
}

#[derive(Debug, Clone)]
struct Candidate {
    id: &'static str,
    title: &'static str,
    score: f64,
    evidence: Vec<String>,
    actions: Vec<String>,
}

impl Candidate {
    fn new(id: &'static str, title: &'static str, actions: &[&str]) -> Self {
        Self {
            id,
            title,
            score: 0.0,
            evidence: Vec::new(),
            actions: actions.iter().map(|x| (*x).to_string()).collect(),
        }
    }

    fn add(&mut self, points: f64, evidence: impl Into<String>) {
        self.score = (self.score + points).min(100.0);
        if self.evidence.len() < 12 {
            let evidence = evidence.into();
            if !self.evidence.contains(&evidence) {
                self.evidence.push(evidence);
            }
        }
    }
}

/// Analyze the freshest Black Box incident for a VM. If `host_id` is absent,
/// every online host is queried and the newest incident wins.
pub async fn analyze(pool: &DbPool, q: &BlackBoxRcaQuery) -> anyhow::Result<BlackBoxRcaReport> {
    let vm = q.vm_name.trim();
    if vm.is_empty() {
        return Err(anyhow!("vm_name is required"));
    }

    let candidates = if let Some(host_id) = q.host_id.as_deref().filter(|x| !x.is_empty()) {
        let host = bpf::host(pool, host_id)
            .await
            .ok_or_else(|| anyhow!("host {host_id} not found"))?;
        vec![(host.clone(), bpf::call(&host, &Request::BlackBoxGet { vm: vm.into() }).await)]
    } else {
        bpf::fan_out(pool, &Request::BlackBoxGet { vm: vm.into() }).await
    };

    let mut chosen: Option<(bpf::HostRef, BlackBoxIncident)> = None;
    for (host, result) in candidates {
        let Ok(value) = result else { continue };
        let Ok(snapshot) = serde_json::from_value::<BlackBoxSnapshot>(value) else { continue };
        let Some(incident) = snapshot.incident else { continue };
        let replace = chosen.as_ref().is_none_or(|(_, old)| {
            incident_time(&incident) > incident_time(old)
        });
        if replace {
            chosen = Some((host, incident));
        }
    }

    let (host, incident) = chosen.ok_or_else(|| anyhow!("no Black Box incident found for VM {vm}"))?;

    // These are diagnostic reads. Failure degrades confidence but never makes
    // the incident unavailable.
    let intel = bpf::call(&host, &Request::VmIntelVm { name: vm.into() })
        .await
        .ok()
        .and_then(|v| serde_json::from_value::<VmIntelReport>(v).ok());
    let net = bpf::call(&host, &Request::NetHealth)
        .await
        .ok()
        .and_then(|v| serde_json::from_value::<NetHealth>(v).ok());
    let scx = bpf::call(&host, &Request::ScxStatus)
        .await
        .ok()
        .and_then(|v| serde_json::from_value::<ScxStatus>(v).ok());

    Ok(score(vm, &host.id, &host.hostname, incident, intel.as_ref(), net.as_ref(), scx.as_ref()))
}

pub fn score(
    vm: &str,
    host_id: &str,
    hostname: &str,
    incident: BlackBoxIncident,
    intel: Option<&VmIntelReport>,
    net: Option<&NetHealth>,
    scx: Option<&ScxStatus>,
) -> BlackBoxRcaReport {
    let trigger = DateTime::parse_from_rfc3339(&incident.status.triggered_at)
        .map(|x| x.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    let mut storage = Candidate::new(
        "storage_stall",
        "Storage / block-I/O stall",
        &["Inspect the VM backing volume and host device latency.", "Check snapshot depth, pool fullness and backup activity.", "Move or throttle the noisy storage workload before retrying the VM operation."],
    );
    let mut scheduler = Candidate::new(
        "scheduler_starvation",
        "vCPU scheduler starvation / noisy neighbour",
        &["Inspect vCPU run-queue latency and physical CPU residency.", "Pin or migrate the VM away from the contending CPUs/NUMA node.", "Review machina-scx latency violations before enabling a scheduling lease."],
    );
    let mut memory = Candidate::new(
        "memory_pressure",
        "Host/VM memory pressure and reclaim",
        &["Inspect reclaim/fault latency and host PSI/OOM events.", "Reduce memory overcommit or migrate the VM to a roomier NUMA node.", "Check hugepage availability and ballooning before restarting workloads."],
    );
    let mut network = Candidate::new(
        "network_path",
        "Network path / policy failure",
        &["Replay the captured flow against current VM network policy.", "Inspect drops, rate limits, DNS failures and route/bridge changes.", "Compare the incident path with a known-good peer before changing policy."],
    );
    let mut security = Candidate::new(
        "security_containment",
        "Security control or compromise signal",
        &["Preserve the frozen Black Box evidence before remediation.", "Review VMM-guard, anomaly and denied-flow events in order.", "If compromise is plausible, clone forensics and use time-boxed quarantine."],
    );
    let mut application = Candidate::new(
        "application_churn",
        "Guest/application process or protocol churn",
        &["Correlate process exec/exit/connect events with the first symptom.", "Check application logs for the first failed dependency or restart.", "Validate DNS/L7 targets before restarting the VM."],
    );

    let mut stats = RcaStats::default();
    stats.events = incident.events.len();

    for event in &incident.events {
        *stats.topics.entry(event.topic.clone()).or_insert(0) += 1;
        if matches!(event.severity.as_str(), "high" | "critical") {
            stats.severe_events += 1;
        }
        score_event(
            event,
            trigger,
            &mut storage,
            &mut scheduler,
            &mut memory,
            &mut network,
            &mut security,
            &mut application,
        );
    }

    if let Some(i) = intel {
        score_intel(i, &mut storage, &mut scheduler, &mut memory);
    }
    if let Some(n) = net {
        score_net_context(n, &mut network);
    }
    if let Some(s) = scx {
        score_scx(vm, s, &mut scheduler);
    }

    let mut all = vec![storage, scheduler, memory, network, security, application];
    all.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(b.id)));

    let top = &all[0];
    let second = all.get(1).map(|x| x.score).unwrap_or(0.0);
    let confidence = confidence(top.score, second);
    let root_cause = if top.score < 10.0 {
        "No dominant cause in the captured evidence; widen the capture or inspect guest logs.".to_string()
    } else {
        format!("Most likely: {}.", top.title)
    };

    let mut evidence = top.evidence.clone();
    if let Some(runner) = all.get(1).filter(|x| x.score >= 10.0) {
        evidence.push(format!(
            "Runner-up: {} scored {:.1}/100 versus {:.1}/100 for the leader.",
            runner.title, runner.score, top.score
        ));
    }
    let suggested_actions = top.actions.clone();
    let hypotheses = all
        .into_iter()
        .map(|c| BlackBoxHypothesis {
            id: c.id.into(),
            title: c.title.into(),
            score: round1(c.score),
            evidence: c.evidence,
            suggested_actions: c.actions,
        })
        .collect();

    BlackBoxRcaReport {
        vm: vm.into(),
        host_id: host_id.into(),
        hostname: hostname.into(),
        incident_id: incident.status.id,
        triggered_at: incident.status.triggered_at,
        frozen: incident.status.frozen,
        deterministic: true,
        root_cause,
        confidence: round3(confidence),
        hypotheses,
        evidence,
        suggested_actions,
        stats,
    }
}

#[allow(clippy::too_many_arguments)]
fn score_event(
    e: &BlackBoxEvent,
    trigger: DateTime<Utc>,
    storage: &mut Candidate,
    scheduler: &mut Candidate,
    memory: &mut Candidate,
    network: &mut Candidate,
    security: &mut Candidate,
    application: &mut Candidate,
) {
    let severity = match e.severity.as_str() {
        "critical" => 1.5,
        "high" => 1.2,
        "medium" => 0.8,
        _ => 0.4,
    };
    let at = DateTime::parse_from_rfc3339(&e.ts)
        .map(|x| x.with_timezone(&Utc))
        .unwrap_or(trigger);
    let delta = (at - trigger).num_seconds().unsigned_abs();
    let proximity = if delta <= 5 { 1.3 } else if delta <= 30 { 1.1 } else if delta <= 120 { 0.8 } else { 0.5 };
    let text = format!("{} {} {} {}", e.topic, e.kind, e.summary, e.event).to_ascii_lowercase();
    let weight = severity * proximity;
    let ev = format!("{} {} — {}", e.ts, e.topic, e.summary);

    if has_any(&text, &["storage", "disk", "block", "nvme", "qcow", "virtio_blk", "i/o"]) {
        storage.add(13.0 * weight, ev.clone());
    }
    if has_any(&text, &["runq", "scheduler", "sched", "vcpu", "irq", "softirq", "starvation", "steal"]) {
        scheduler.add(13.0 * weight, ev.clone());
    }
    if has_any(&text, &["memory", "reclaim", "page fault", "oom", "psi", "hugepage", "balloon"]) {
        memory.add(13.0 * weight, ev.clone());
    }
    if matches!(e.topic.as_str(), "net" | "flow" | "dns" | "l7")
        || has_any(&text, &["drop", "deny", "rate_limit", "qos", "retrans", "dns", "connect", "network", "route", "bridge"])
    {
        let points = if has_any(&text, &["drop", "deny", "rate_limit", "qos", "servfail", "timeout"]) { 15.0 } else { 5.0 };
        network.add(points * weight, ev.clone());
    }
    if matches!(e.topic.as_str(), "guard" | "anomaly" | "alert")
        || has_any(&text, &["denied", "threat", "suspicious", "quarantine", "port_scan", "host_sweep", "w+x"])
    {
        security.add(18.0 * weight, ev.clone());
    }
    if matches!(e.topic.as_str(), "process" | "l7")
        || has_any(&text, &["exec", "exit", "restart", "http", "grpc", "kafka", "postgres", "mysql", "redis"])
    {
        application.add(6.0 * weight, ev);
    }
}

fn score_intel(i: &VmIntelReport, storage: &mut Candidate, scheduler: &mut Candidate, memory: &mut Candidate) {
    add_latency(storage, "block I/O", i.block.p99_ns, i.block.count, &[(100, 45.0), (20, 30.0), (5, 15.0)]);
    add_latency(scheduler, "vCPU run queue", i.runq.p99_ns, i.runq.count, &[(50, 45.0), (10, 30.0), (2, 15.0)]);
    add_latency(memory, "reclaim", i.reclaim.p99_ns, i.reclaim.count, &[(50, 40.0), (10, 25.0), (2, 12.0)]);
    add_latency(memory, "page fault", i.fault.p99_ns, i.fault.count, &[(20, 25.0), (5, 15.0), (1, 8.0)]);
    if i.migrations >= 100 {
        scheduler.add(10.0, format!("VM-intel observed {} vCPU migrations.", i.migrations));
    }
}

fn add_latency(c: &mut Candidate, label: &str, p99_ns: u64, count: u64, levels: &[(u64, f64)]) {
    if count == 0 { return; }
    let ms = p99_ns as f64 / 1_000_000.0;
    for (threshold_ms, points) in levels {
        if ms >= *threshold_ms as f64 {
            c.add(*points, format!("VM-intel {label} p99 {:.3} ms across {count} samples.", ms));
            break;
        }
    }
}

fn score_net_context(n: &NetHealth, network: &mut Candidate) {
    let failures: u64 = n.connect.iter().map(|x| x.failures).sum();
    if failures > 0 {
        network.add(8.0, format!("Host network context: {failures} failed active connects."));
    }
    let retrans: u64 = n.pressure.iter().map(|x| x.retrans_events as u64).sum();
    if retrans > 0 {
        network.add(8.0, format!("Host network context: {retrans} TCP retransmit events."));
    }
    let drops: u64 = n.drop_reasons.iter().map(|x| x.count).sum();
    if drops > 0 {
        network.add(6.0, format!("Host network context: {drops} kernel drop-reason hits."));
    }
}

fn score_scx(vm: &str, s: &ScxStatus, scheduler: &mut Candidate) {
    let Some(v) = s.vms.iter().find(|x| x.name == vm) else { return };
    if v.latency_violations > 0 {
        scheduler.add(22.0, format!("machina-scx recorded {} latency-target violations.", v.latency_violations));
    }
    if v.max_queue_delay_us >= 10_000.0 {
        scheduler.add(15.0, format!("machina-scx max queue delay {:.3} ms.", v.max_queue_delay_us / 1000.0));
    }
}

fn confidence(top: f64, second: f64) -> f64 {
    if top < 10.0 { return 0.25; }
    let strength = (top / 100.0).clamp(0.0, 1.0);
    let margin = ((top - second).max(0.0) / 100.0).clamp(0.0, 1.0);
    (0.30 + strength * 0.45 + margin * 0.25).clamp(0.30, 0.98)
}

fn has_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| text.contains(n))
}

fn incident_time(i: &BlackBoxIncident) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&i.status.triggered_at)
        .map(|x| x.with_timezone(&Utc))
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

fn round1(v: f64) -> f64 { (v * 10.0).round() / 10.0 }
fn round3(v: f64) -> f64 { (v * 1000.0).round() / 1000.0 }

#[cfg(test)]
mod tests {
    use super::*;
    use machina_bpf::api::{BlackBoxIncidentStatus, VmIntelHist};
    use serde_json::json;

    fn incident(events: Vec<BlackBoxEvent>) -> BlackBoxIncident {
        BlackBoxIncident {
            vm: "db".into(),
            status: BlackBoxIncidentStatus {
                id: "db-1".into(),
                triggered_at: "2026-10-07T00:00:10Z".into(),
                frozen: true,
                events: events.len(),
                ..Default::default()
            },
            events,
        }
    }

    fn ev(ts: &str, topic: &str, kind: &str, severity: &str, summary: &str) -> BlackBoxEvent {
        BlackBoxEvent { ts: ts.into(), topic: topic.into(), kind: kind.into(), severity: severity.into(), summary: summary.into(), event: json!({}) }
    }

    #[test]
    fn block_latency_wins_storage() {
        let mut intel = VmIntelReport::default();
        intel.block = VmIntelHist { count: 100, p99_ns: 187_000_000, ..Default::default() };
        let r = score("db", "h1", "host1", incident(vec![]), Some(&intel), None, None);
        assert_eq!(r.hypotheses[0].id, "storage_stall");
        assert!(r.confidence > 0.5);
    }

    #[test]
    fn guard_denial_wins_security() {
        let r = score("db", "h1", "host1", incident(vec![ev("2026-10-07T00:00:09Z", "guard", "mprotect", "critical", "W+X denied")]), None, None, None);
        assert_eq!(r.hypotheses[0].id, "security_containment");
    }

    #[test]
    fn flow_drop_wins_network() {
        let r = score("db", "h1", "host1", incident(vec![ev("2026-10-07T00:00:09Z", "flow", "flow", "high", "DROPPED egress to database")]), None, None, None);
        assert_eq!(r.hypotheses[0].id, "network_path");
    }

    #[test]
    fn runq_wins_scheduler() {
        let mut intel = VmIntelReport::default();
        intel.runq = VmIntelHist { count: 100, p99_ns: 80_000_000, ..Default::default() };
        let r = score("db", "h1", "host1", incident(vec![]), Some(&intel), None, None);
        assert_eq!(r.hypotheses[0].id, "scheduler_starvation");
    }

    #[test]
    fn unknown_is_low_confidence() {
        let r = score("db", "h1", "host1", incident(vec![]), None, None, None);
        assert_eq!(r.confidence, 0.25);
        assert!(r.root_cause.starts_with("No dominant"));
    }
}
