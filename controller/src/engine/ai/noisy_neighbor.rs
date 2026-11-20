// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Deterministic VM-to-VM noisy-neighbour attribution.
//! Observe/recommend-only: never throttles, migrates or changes policy.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::Utc;
use machina_bpf::api::{
    AccountingRecord, Request, ScxStatus, ScxVmStatus, VmIntelReport, VmIntelStatus,
};
use serde::{Deserialize, Serialize};

use crate::db::DbPool;
use crate::engine::bpf::{call, host, online_hosts, HostRef};

const MIN_EDGE_SCORE: f64 = 20.0;
const MAX_EDGES: usize = 50;

#[derive(Debug, Clone, Deserialize)]
pub struct NoisyNeighborQuery {
    pub host_id: Option<String>,
    pub victim_vm: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VmPressure {
    pub vm: String,
    pub vcpus: usize,
    pub cpu_residency_ns: u64,
    pub cpu_set: Vec<u32>,
    pub runq_p99_ms: f64,
    pub block_p99_ms: f64,
    pub reclaim_p99_ms: f64,
    pub block_samples: u64,
    pub migrations: u64,
    pub scx_avg_queue_us: f64,
    pub scx_max_queue_us: f64,
    pub scx_latency_violations: u64,
    pub runtime_ms: u64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub drops: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InterferenceEdge {
    pub host_id: String,
    pub hostname: String,
    pub aggressor: String,
    pub victim: String,
    pub score: f64,
    pub confidence: f64,
    pub estimated_latency_impact_pct: f64,
    pub resources: Vec<String>,
    pub evidence: Vec<String>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostInterference {
    pub host_id: String,
    pub hostname: String,
    pub vm_count: usize,
    pub edges: Vec<InterferenceEdge>,
    pub pressures: Vec<VmPressure>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NoisyNeighborReport {
    pub generated_at: String,
    pub deterministic: bool,
    pub summary: String,
    pub hosts: Vec<HostInterference>,
    pub top_edges: Vec<InterferenceEdge>,
}

#[derive(Debug, Clone)]
struct Sample {
    pressure: VmPressure,
    cpu_weights: BTreeMap<u32, u64>,
}

fn ns_ms(v: u64) -> f64 { v as f64 / 1_000_000.0 }

fn overlap(a: &BTreeMap<u32, u64>, b: &BTreeMap<u32, u64>) -> f64 {
    if a.is_empty() || b.is_empty() { return 0.0; }
    let aa: BTreeSet<u32> = a.keys().copied().collect();
    let bb: BTreeSet<u32> = b.keys().copied().collect();
    let union = aa.union(&bb).count();
    if union == 0 { 0.0 } else { aa.intersection(&bb).count() as f64 / union as f64 }
}

fn share(n: u64, total: u64) -> f64 {
    if total == 0 { 0.0 } else { n as f64 / total as f64 }
}

fn recommendations(resources: &[String], aggressor: &str, victim: &str) -> Vec<String> {
    let mut out = Vec::new();
    if resources.iter().any(|r| r == "cpu") {
        out.push(format!("Separate {aggressor} and {victim} onto disjoint pCPUs/NUMA domains before adding vCPU."));
        out.push(format!("Give {victim} a sched_ext latency SLO and keep {aggressor} in throughput/batch class after approval."));
    }
    if resources.iter().any(|r| r == "storage") {
        out.push(format!("Separate {aggressor} and {victim} across storage pools/hosts or apply storage QoS after approval."));
    }
    if resources.iter().any(|r| r == "network") {
        out.push(format!("Review {aggressor} bandwidth and apply VM QoS only with an explicit operator-approved limit."));
    }
    out.push("Run Predictive Migration before moving either VM; choose a destination with lower runqueue and block p99.".into());
    out
}

fn edge(
    h: &HostRef,
    aggressor: &Sample,
    victim: &Sample,
    total_runtime: u64,
    total_block_samples: u64,
    total_net: u64,
) -> Option<InterferenceEdge> {
    if aggressor.pressure.vm == victim.pressure.vm { return None; }

    let cpu_overlap = overlap(&aggressor.cpu_weights, &victim.cpu_weights);
    let runtime_share = share(aggressor.pressure.runtime_ms, total_runtime);
    let block_share = share(aggressor.pressure.block_samples, total_block_samples);
    let net_bytes = aggressor.pressure.tx_bytes.saturating_add(aggressor.pressure.rx_bytes);
    let net_share = share(net_bytes, total_net);

    let mut score = 0.0;
    let mut attribution = 0.0;
    let mut resources = Vec::new();
    let mut evidence = Vec::new();

    let victim_runq = victim.pressure.runq_p99_ms.max(victim.pressure.scx_max_queue_us / 1000.0);
    if cpu_overlap >= 0.25 && victim_runq >= 5.0 {
        let latency = (victim_runq / 50.0).clamp(0.0, 1.0);
        let activity = (runtime_share / 0.50).clamp(0.0, 1.0);
        score += 50.0 * cpu_overlap * (0.55 + 0.45 * activity) * latency.max(0.25);
        attribution += 0.48 * cpu_overlap + 0.20 * activity;
        resources.push("cpu".to_string());
        evidence.push(format!(
            "{:.0}% pCPU-set overlap; victim runqueue p99 {:.2} ms; aggressor runtime share {:.0}%.",
            cpu_overlap * 100.0, victim_runq, runtime_share * 100.0
        ));
    }

    if victim.pressure.block_p99_ms >= 10.0 && block_share >= 0.20 {
        let latency = (victim.pressure.block_p99_ms / 100.0).clamp(0.0, 1.0);
        let activity = (block_share / 0.60).clamp(0.0, 1.0);
        score += 35.0 * latency.max(0.25) * (0.45 + 0.55 * activity);
        attribution += 0.30 * activity;
        resources.push("storage".to_string());
        evidence.push(format!(
            "Victim block p99 {:.2} ms; aggressor contributes {:.0}% of VM block samples.",
            victim.pressure.block_p99_ms, block_share * 100.0
        ));
    }

    if victim.pressure.drops > 0 && net_share >= 0.35 {
        let activity = (net_share / 0.70).clamp(0.0, 1.0);
        score += 20.0 * activity;
        attribution += 0.15 * activity;
        resources.push("network".to_string());
        evidence.push(format!(
            "Aggressor carries {:.0}% of accounted VM traffic while victim has {} drops.",
            net_share * 100.0, victim.pressure.drops
        ));
    }

    if resources.iter().any(|r| r == "cpu") && victim.pressure.migrations >= 10 {
        score += (victim.pressure.migrations as f64).log10().min(2.0) * 5.0;
        evidence.push(format!("Victim vCPUs migrated {} times, consistent with poor locality.", victim.pressure.migrations));
    }

    if resources.iter().any(|r| r == "cpu") && victim.pressure.scx_latency_violations > 0 {
        score += (victim.pressure.scx_latency_violations as f64).sqrt().min(10.0);
        attribution += 0.08;
        evidence.push(format!("sched_ext recorded {} victim latency-target violations.", victim.pressure.scx_latency_violations));
    }

    score = score.clamp(0.0, 100.0);
    if score < MIN_EDGE_SCORE { return None; }

    resources.sort();
    resources.dedup();
    let confidence = (0.25 + attribution + ((score - MIN_EDGE_SCORE) / 160.0)).clamp(0.0, 0.98);

    Some(InterferenceEdge {
        host_id: h.id.clone(),
        hostname: h.hostname.clone(),
        aggressor: aggressor.pressure.vm.clone(),
        victim: victim.pressure.vm.clone(),
        score,
        confidence,
        estimated_latency_impact_pct: (score * 0.72).clamp(0.0, 100.0),
        recommendations: recommendations(&resources, &aggressor.pressure.vm, &victim.pressure.vm),
        resources,
        evidence,
    })
}

fn scx_by_vm(st: &ScxStatus) -> HashMap<String, ScxVmStatus> {
    st.vms.iter().cloned().map(|v| (v.name.clone(), v)).collect()
}

fn accounting_by_vm(rows: Vec<AccountingRecord>) -> HashMap<String, AccountingRecord> {
    rows.into_iter().filter_map(|r| r.vm.clone().map(|vm| (vm, r))).collect()
}

fn make_sample(
    report: VmIntelReport,
    tracked_vcpus: usize,
    scx: Option<&ScxVmStatus>,
    accounting: Option<&AccountingRecord>,
) -> Sample {
    let cpu_weights: BTreeMap<u32, u64> = report.residency.iter()
        .filter(|r| r.ns > 0).map(|r| (r.cpu, r.ns)).collect();

    let pressure = VmPressure {
        vm: report.name,
        vcpus: tracked_vcpus,
        cpu_residency_ns: cpu_weights.values().sum(),
        cpu_set: cpu_weights.keys().copied().collect(),
        runq_p99_ms: ns_ms(report.runq.p99_ns),
        block_p99_ms: ns_ms(report.block.p99_ns),
        reclaim_p99_ms: ns_ms(report.reclaim.p99_ns),
        block_samples: report.block.count,
        migrations: report.migrations,
        scx_avg_queue_us: scx.map(|v| v.avg_queue_delay_us).unwrap_or(0.0),
        scx_max_queue_us: scx.map(|v| v.max_queue_delay_us).unwrap_or(0.0),
        scx_latency_violations: scx.map(|v| v.latency_violations).unwrap_or(0),
        runtime_ms: scx.map(|v| v.runtime_ms).unwrap_or(0),
        tx_bytes: accounting.map(|a| a.tx_bytes).unwrap_or(0),
        rx_bytes: accounting.map(|a| a.rx_bytes).unwrap_or(0),
        drops: accounting.map(|a| a.drops).unwrap_or(0),
    };
    Sample { pressure, cpu_weights }
}

async fn one_host(h: HostRef, victim_filter: Option<&str>) -> HostInterference {
    let intel_status: VmIntelStatus = call(&h, &Request::VmIntelStatus).await.ok()
        .and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let scx: ScxStatus = call(&h, &Request::ScxStatus).await.ok()
        .and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let accounting: Vec<AccountingRecord> = call(&h, &Request::Accounting { vm: None }).await.ok()
        .and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();

    let scx_map = scx_by_vm(&scx);
    let acct_map = accounting_by_vm(accounting);
    let mut samples = Vec::new();

    for tracked in &intel_status.vms {
        let Ok(v) = call(&h, &Request::VmIntelVm { name: tracked.name.clone() }).await else { continue; };
        let Ok(report) = serde_json::from_value::<VmIntelReport>(v) else { continue; };
        samples.push(make_sample(report, tracked.vcpus, scx_map.get(&tracked.name), acct_map.get(&tracked.name)));
    }

    let total_runtime: u64 = samples.iter().map(|s| s.pressure.runtime_ms).sum();
    let total_block_samples: u64 = samples.iter().map(|s| s.pressure.block_samples).sum();
    let total_net: u64 = samples.iter().map(|s| s.pressure.tx_bytes.saturating_add(s.pressure.rx_bytes)).sum();

    let mut edges = Vec::new();
    for aggressor in &samples {
        for victim in &samples {
            if victim_filter.is_some_and(|v| victim.pressure.vm != v) { continue; }
            if let Some(e) = edge(&h, aggressor, victim, total_runtime, total_block_samples, total_net) {
                edges.push(e);
            }
        }
    }
    edges.sort_by(|a, b| b.score.total_cmp(&a.score));
    edges.truncate(MAX_EDGES);

    HostInterference {
        host_id: h.id,
        hostname: h.hostname,
        vm_count: samples.len(),
        edges,
        pressures: samples.into_iter().map(|s| s.pressure).collect(),
    }
}

pub async fn analyze(pool: &DbPool, q: &NoisyNeighborQuery) -> anyhow::Result<NoisyNeighborReport> {
    let hosts = if let Some(id) = q.host_id.as_deref() {
        host(pool, id).await.into_iter().collect()
    } else {
        online_hosts(pool).await
    };

    let mut host_reports = Vec::new();
    for h in hosts {
        host_reports.push(one_host(h, q.victim_vm.as_deref()).await);
    }

    let mut top_edges: Vec<InterferenceEdge> =
        host_reports.iter().flat_map(|h| h.edges.iter().cloned()).collect();
    top_edges.sort_by(|a, b| b.score.total_cmp(&a.score));
    top_edges.truncate(q.limit.unwrap_or(20).clamp(1, MAX_EDGES));

    let summary = top_edges.first().map(|e| format!(
        "{} is the strongest noisy-neighbour candidate for {} on {} (score {:.1}, confidence {:.0}%).",
        e.aggressor, e.victim, e.hostname, e.score, e.confidence * 100.0
    )).unwrap_or_else(|| "No strong VM-to-VM interference edge was found in current telemetry.".into());

    Ok(NoisyNeighborReport {
        generated_at: Utc::now().to_rfc3339(),
        deterministic: true,
        summary,
        hosts: host_reports,
        top_edges,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_host() -> HostRef {
        HostRef { id: "h1".into(), hostname: "node-1".into(), addr: String::new(), state: "online".into() }
    }

    fn s(name: &str, cpus: &[(u32,u64)], runq: f64, runtime: u64, block: u64) -> Sample {
        let cpu_weights = cpus.iter().copied().collect();
        Sample {
            pressure: VmPressure {
                vm: name.into(), vcpus: 2, cpu_residency_ns: cpus.iter().map(|x|x.1).sum(),
                cpu_set: cpus.iter().map(|x|x.0).collect(), runq_p99_ms: runq,
                block_p99_ms: 0.0, reclaim_p99_ms: 0.0, block_samples: block, migrations: 0,
                scx_avg_queue_us: 0.0, scx_max_queue_us: 0.0, scx_latency_violations: 0,
                runtime_ms: runtime, tx_bytes: 0, rx_bytes: 0, drops: 0,
            },
            cpu_weights,
        }
    }

    #[test]
    fn cpu_overlap_attributes_aggressor() {
        let a=s("batch",&[(2,50),(3,50)],1.0,900,1);
        let v=s("db",&[(2,50),(3,50)],80.0,100,1);
        let e=edge(&test_host(),&a,&v,1000,2,0).unwrap();
        assert!(e.score >= 40.0);
        assert!(e.resources.iter().any(|r| r=="cpu"));
    }

    #[test]
    fn no_overlap_no_cpu_blame() {
        let a=s("batch",&[(0,100)],1.0,900,1);
        let v=s("db",&[(7,100)],80.0,100,1);
        assert!(edge(&test_host(),&a,&v,1000,2,0).is_none());
    }

    #[test]
    fn quiet_victim_not_a_case() {
        let a=s("batch",&[(2,100)],1.0,900,1);
        let v=s("db",&[(2,100)],1.0,100,1);
        assert!(edge(&test_host(),&a,&v,1000,2,0).is_none());
    }

    #[test]
    fn storage_activity_can_form_edge() {
        let a=s("backup",&[(0,100)],1.0,0,900);
        let mut v=s("db",&[(7,100)],1.0,0,100);
        v.pressure.block_p99_ms=150.0;
        let e=edge(&test_host(),&a,&v,0,1000,0).unwrap();
        assert!(e.resources.iter().any(|r| r=="storage"));
    }
}
