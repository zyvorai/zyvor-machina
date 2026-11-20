// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! VM Performance Autopilot.
//!
//! Builds a deterministic, evidence-backed optimization plan per VM from
//! existing Machina inventory + native eBPF VM intelligence. This module is
//! recommendation-first: every mutating action is emitted with
//! `requires_approval=true`.

use std::cmp::Ordering;

use machina_bpf::api::Request;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::db::DbPool;
use crate::engine::bpf;

#[derive(Debug, Clone, Deserialize)]
pub struct PerformanceAutopilotQuery {
    pub vm_id: Uuid,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PerformanceEvidence {
    pub source: String,
    pub metric: String,
    pub value: f64,
    pub unit: String,
    pub threshold: f64,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PerformanceAction {
    pub id: String,
    pub title: String,
    pub action_type: String,
    pub priority: u32,
    pub score: f64,
    pub expected_benefit: String,
    pub risk: String,
    pub requires_approval: bool,
    pub reversible: bool,
    pub object_ref: Value,
    pub evidence: Vec<PerformanceEvidence>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PerformanceHealth {
    pub score: f64,
    pub cpu_pressure: f64,
    pub scheduler_pressure: f64,
    pub storage_pressure: f64,
    pub memory_pressure: f64,
    pub network_pressure: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PerformanceAutopilotReport {
    pub vm_id: Uuid,
    pub vm_name: String,
    pub host_id: Option<Uuid>,
    pub hostname: Option<String>,
    pub generated_at: String,
    pub deterministic: bool,
    pub health: PerformanceHealth,
    pub actions: Vec<PerformanceAction>,
    pub summary: String,
}

#[derive(Debug, Clone)]
struct VmRow {
    name: String,
    host_id: Option<Uuid>,
    vcpus: i64,
    memory_mib: i64,
}

fn val_u64(v: &Value, path: &[&str]) -> u64 {
    let mut cur = v;
    for p in path {
        cur = &cur[*p];
    }
    cur.as_u64().unwrap_or(0)
}

fn val_f64(v: &Value, path: &[&str]) -> f64 {
    let mut cur = v;
    for p in path {
        cur = &cur[*p];
    }
    cur.as_f64()
        .or_else(|| cur.as_u64().map(|n| n as f64))
        .or_else(|| cur.as_i64().map(|n| n as f64))
        .unwrap_or(0.0)
}

fn ns_to_ms(v: u64) -> f64 {
    v as f64 / 1_000_000.0
}

fn pressure(value: f64, warn: f64, bad: f64) -> f64 {
    if value <= warn {
        0.0
    } else if value >= bad {
        100.0
    } else {
        ((value - warn) / (bad - warn) * 100.0).clamp(0.0, 100.0)
    }
}

fn add_action(
    actions: &mut Vec<PerformanceAction>,
    id: &str,
    title: &str,
    action_type: &str,
    score: f64,
    benefit: String,
    risk: &str,
    reversible: bool,
    vm_id: Uuid,
    extra: Value,
    evidence: Vec<PerformanceEvidence>,
) {
    if score < 20.0 {
        return;
    }
    let mut object = serde_json::Map::new();
    object.insert("vm_id".into(), Value::String(vm_id.to_string()));
    if let Value::Object(extra) = extra {
        object.extend(extra);
    }
    actions.push(PerformanceAction {
        id: id.into(),
        title: title.into(),
        action_type: action_type.into(),
        priority: score.round().clamp(1.0, 100.0) as u32,
        score: score.clamp(0.0, 100.0),
        expected_benefit: benefit,
        risk: risk.into(),
        requires_approval: true,
        reversible,
        object_ref: Value::Object(object),
        evidence,
    });
}

async fn best_destination(
    pool: &DbPool,
    source: Uuid,
    memory_mib: i64,
) -> anyhow::Result<Option<(Uuid, String, f64, i64)>> {
    let rows: Vec<(Uuid, String, f32, i64, i64)> = crate::db::query_as(
        "SELECT id, hostname, cpu_percent, memory_total_mib, memory_used_mib
         FROM hosts
         WHERE state = 'online' AND maintenance_mode = FALSE AND id != ?
         ORDER BY cpu_percent ASC
         LIMIT 20",
    )
    .bind(source)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|(id, name, cpu, total, used)| {
            let free = total.saturating_sub(used);
            (free >= memory_mib).then_some((id, name, cpu as f64, free))
        })
        .next())
}

pub async fn analyze(
    pool: &DbPool,
    q: &PerformanceAutopilotQuery,
) -> anyhow::Result<PerformanceAutopilotReport> {
    let row: (String, Option<Uuid>, i64, i64) = crate::db::query_as(
        "SELECT name, host_id, vcpus, memory_mib FROM vms WHERE id = ?",
    )
    .bind(q.vm_id)
    .fetch_one(pool)
    .await?;

    let vm = VmRow {
        name: row.0,
        host_id: row.1,
        vcpus: row.2,
        memory_mib: row.3,
    };

    let mut actions = Vec::new();
    let mut hostname = None;
    let mut host_cpu = 0.0;
    let mut runq_ms = 0.0;
    let mut block_ms = 0.0;
    let mut reclaim_ms = 0.0;
    let mut fault_ms = 0.0;
    let mut migrations = 0.0;
    let mut scx_queue_ms = 0.0;
    let mut scx_violations = 0.0;

    if let Some(host_id) = vm.host_id {
        if let Some((name, cpu)) = crate::db::query_as::<_, (String, f32)>(
            "SELECT hostname, cpu_percent FROM hosts WHERE id = ?",
        )
        .bind(host_id)
        .fetch_optional(pool)
        .await?
        {
            hostname = Some(name);
            host_cpu = cpu as f64;
        }

        if let Some(h) = bpf::host(pool, &host_id.to_string()).await {
            if let Ok(intel) = bpf::call(
                &h,
                &Request::VmIntelVm {
                    name: vm.name.clone(),
                },
            )
            .await
            {
                runq_ms = ns_to_ms(val_u64(&intel, &["runq", "p99_ns"]));
                block_ms = ns_to_ms(val_u64(&intel, &["block", "p99_ns"]));
                reclaim_ms = ns_to_ms(val_u64(&intel, &["reclaim", "p99_ns"]));
                fault_ms = ns_to_ms(val_u64(&intel, &["fault", "p99_ns"]));
                migrations = val_f64(&intel, &["migrations"]);
            }

            if let Ok(scx) = bpf::call(&h, &Request::ScxStatus).await {
                if let Some(v) = scx["vms"]
                    .as_array()
                    .and_then(|xs| xs.iter().find(|v| v["name"].as_str() == Some(vm.name.as_str())))
                {
                    scx_queue_ms = val_f64(v, &["max_queue_delay_us"]) / 1000.0;
                    scx_violations = val_f64(v, &["latency_violations"]);
                }
            }
        }
    }

    let scheduler_metric = runq_ms.max(scx_queue_ms);
    let cpu_pressure = pressure(host_cpu, 70.0, 95.0);
    let scheduler_pressure = pressure(scheduler_metric, 2.0, 40.0);
    let storage_pressure = pressure(block_ms, 5.0, 100.0);
    let memory_pressure = pressure(reclaim_ms.max(fault_ms), 2.0, 50.0);
    let network_pressure = 0.0;

    if scheduler_pressure >= 20.0 {
        let evidence = vec![
            PerformanceEvidence {
                source: "machina-bpfd".into(),
                metric: "scheduler_p99".into(),
                value: scheduler_metric,
                unit: "ms".into(),
                threshold: 2.0,
                summary: format!(
                    "VM scheduler delay is {:.2} ms p99-equivalent (runq/scx).",
                    scheduler_metric
                ),
            },
            PerformanceEvidence {
                source: "host".into(),
                metric: "cpu_percent".into(),
                value: host_cpu,
                unit: "%".into(),
                threshold: 70.0,
                summary: format!("Current host CPU is {:.0}%.", host_cpu),
            },
        ];
        add_action(
            &mut actions,
            "scheduler-slo",
            "Put VM in latency-aware sched_ext class",
            "performance.set_scheduler_slo",
            scheduler_pressure,
            format!(
                "Reduce vCPU queueing for latency-sensitive VM with {} vCPU(s).",
                vm.vcpus
            ),
            "Medium",
            true,
            q.vm_id,
            json!({
                "class": "latency",
                "target_queue_ms": 2.0
            }),
            evidence,
        );
    }

    if storage_pressure >= 20.0 {
        add_action(
            &mut actions,
            "storage-latency",
            "Move VM to lower-latency storage or isolate its I/O path",
            "performance.storage_rebalance",
            storage_pressure,
            format!("Current block-I/O p99 is {:.2} ms.", block_ms),
            "Medium",
            true,
            q.vm_id,
            json!({ "observed_block_p99_ms": block_ms }),
            vec![PerformanceEvidence {
                source: "machina-bpfd".into(),
                metric: "block_p99".into(),
                value: block_ms,
                unit: "ms".into(),
                threshold: 5.0,
                summary: format!("Block latency p99 {:.2} ms exceeds target.", block_ms),
            }],
        );
    }

    if memory_pressure >= 20.0 {
        add_action(
            &mut actions,
            "memory-locality",
            "Improve memory headroom and NUMA locality",
            "performance.memory_rebalance",
            memory_pressure,
            format!(
                "Reduce reclaim/page-fault stalls for {} MiB VM.",
                vm.memory_mib
            ),
            "Medium",
            true,
            q.vm_id,
            json!({
                "memory_mib": vm.memory_mib,
                "prefer_numa_local": true
            }),
            vec![
                PerformanceEvidence {
                    source: "machina-bpfd".into(),
                    metric: "reclaim_p99".into(),
                    value: reclaim_ms,
                    unit: "ms".into(),
                    threshold: 2.0,
                    summary: format!("Reclaim p99 {:.2} ms.", reclaim_ms),
                },
                PerformanceEvidence {
                    source: "machina-bpfd".into(),
                    metric: "fault_p99".into(),
                    value: fault_ms,
                    unit: "ms".into(),
                    threshold: 2.0,
                    summary: format!("Page-fault p99 {:.2} ms.", fault_ms),
                },
            ],
        );
    }

    if migrations >= 20.0 && scheduler_pressure >= 20.0 {
        add_action(
            &mut actions,
            "vcpu-locality",
            "Stabilize vCPU placement",
            "performance.pin_vcpu_numa",
            (scheduler_pressure * 0.7 + pressure(migrations, 20.0, 200.0) * 0.3).clamp(0.0, 100.0),
            format!(
                "Reduce cache/NUMA churn; {} vCPU migrations observed.",
                migrations as u64
            ),
            "Medium",
            true,
            q.vm_id,
            json!({ "prefer_locality": true }),
            vec![PerformanceEvidence {
                source: "machina-bpfd".into(),
                metric: "vcpu_migrations".into(),
                value: migrations,
                unit: "count".into(),
                threshold: 20.0,
                summary: format!("VM vCPUs migrated {} times.", migrations as u64),
            }],
        );
    }

    if scx_violations > 0.0 {
        add_action(
            &mut actions,
            "slo-violations",
            "Protect VM from noisy-neighbour scheduler interference",
            "performance.noisy_neighbor_remediation",
            (35.0 + scx_violations.sqrt() * 8.0).clamp(0.0, 100.0),
            format!(
                "Address {} sched_ext latency-target violation(s).",
                scx_violations as u64
            ),
            "Medium",
            true,
            q.vm_id,
            json!({ "violations": scx_violations as u64 }),
            vec![PerformanceEvidence {
                source: "machina-scx".into(),
                metric: "latency_violations".into(),
                value: scx_violations,
                unit: "count".into(),
                threshold: 0.0,
                summary: format!(
                    "sched_ext recorded {} latency-target violation(s).",
                    scx_violations as u64
                ),
            }],
        );
    }

    if let Some(source) = vm.host_id {
        if host_cpu >= 80.0 {
            if let Some((dest_id, dest_name, dest_cpu, free_mib)) =
                best_destination(pool, source, vm.memory_mib).await?
            {
                let improvement = (host_cpu - dest_cpu).max(0.0);
                let score = (cpu_pressure * 0.65 + (improvement / 50.0 * 35.0)).clamp(0.0, 100.0);
                add_action(
                    &mut actions,
                    "migrate-better-host",
                    "Evaluate predictive live migration to a cooler host",
                    "performance.migrate_vm",
                    score,
                    format!(
                        "Candidate {} is {:.0} percentage points cooler with {} MiB free.",
                        dest_name, improvement, free_mib
                    ),
                    "High",
                    true,
                    q.vm_id,
                    json!({
                        "dest_host_id": dest_id.to_string(),
                        "dest_hostname": dest_name,
                        "source_cpu_percent": host_cpu,
                        "dest_cpu_percent": dest_cpu,
                        "require_migration_oracle": true
                    }),
                    vec![
                        PerformanceEvidence {
                            source: "host".into(),
                            metric: "source_cpu".into(),
                            value: host_cpu,
                            unit: "%".into(),
                            threshold: 80.0,
                            summary: format!("Source host CPU {:.0}%.", host_cpu),
                        },
                        PerformanceEvidence {
                            source: "placement".into(),
                            metric: "dest_cpu".into(),
                            value: dest_cpu,
                            unit: "%".into(),
                            threshold: 80.0,
                            summary: format!("Candidate destination CPU {:.0}%.", dest_cpu),
                        },
                    ],
                );
            }
        }
    }

    actions.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    actions.truncate(q.limit.unwrap_or(10).clamp(1, 50));

    let worst = cpu_pressure
        .max(scheduler_pressure)
        .max(storage_pressure)
        .max(memory_pressure)
        .max(network_pressure);
    let health_score = (100.0 - worst * 0.85).clamp(0.0, 100.0);

    let summary = if let Some(top) = actions.first() {
        format!(
            "{}: top optimization is '{}' (score {:.0}/100).",
            vm.name, top.title, top.score
        )
    } else {
        format!("{}: no material performance optimization is currently indicated.", vm.name)
    };

    Ok(PerformanceAutopilotReport {
        vm_id: q.vm_id,
        vm_name: vm.name,
        host_id: vm.host_id,
        hostname,
        generated_at: chrono::Utc::now().to_rfc3339(),
        deterministic: true,
        health: PerformanceHealth {
            score: health_score,
            cpu_pressure,
            scheduler_pressure,
            storage_pressure,
            memory_pressure,
            network_pressure,
        },
        actions,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_below_warn_is_zero() {
        assert_eq!(pressure(1.0, 2.0, 10.0), 0.0);
    }

    #[test]
    fn pressure_at_bad_is_100() {
        assert_eq!(pressure(10.0, 2.0, 10.0), 100.0);
    }

    #[test]
    fn pressure_midpoint_is_half() {
        assert!((pressure(6.0, 2.0, 10.0) - 50.0).abs() < 0.001);
    }

    #[test]
    fn nanosecond_conversion() {
        assert_eq!(ns_to_ms(25_000_000), 25.0);
    }
}
