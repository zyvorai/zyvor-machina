// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Predictive Migration Oracle.
//!
//! Read-only estimator: predicts whether pre-copy live migration converges,
//! expected transfer time/downtime, required bandwidth, and destination benefit.
//! It never enqueues or executes a migration.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbPool;

#[derive(Debug, Clone, Deserialize)]
pub struct MigrationOracleRequest {
    pub dest_host_id: Uuid,
    #[serde(default)]
    pub live: bool,
    /// Optional measured dirty-page rate in MiB/s.
    pub dirty_rate_mib_s: Option<f64>,
    /// Optional effective migration link capacity in Mbit/s.
    pub bandwidth_mbps: Option<f64>,
    /// Target maximum stop-and-copy downtime. Default: 250 ms.
    pub max_downtime_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MigrationRound {
    pub round: u32,
    pub remaining_mib: f64,
    pub seconds: f64,
    pub dirtied_mib: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MigrationOracleReport {
    pub vm_id: Uuid,
    pub vm_name: String,
    pub source_host_id: Uuid,
    pub source_hostname: String,
    pub dest_host_id: Uuid,
    pub dest_hostname: String,
    pub live: bool,
    pub precheck_ok: bool,
    pub convergence: String,
    pub convergence_score: f64,
    pub estimated_total_seconds: f64,
    pub estimated_downtime_ms: f64,
    pub estimated_transferred_gib: f64,
    pub dirty_rate_mib_s: f64,
    pub effective_bandwidth_mib_s: f64,
    pub minimum_bandwidth_mbps: f64,
    pub destination_benefit_score: f64,
    pub predicted_cpu_improvement_pct: f64,
    pub memory_headroom_after_mib: i64,
    pub rounds: Vec<MigrationRound>,
    pub risks: Vec<String>,
    pub evidence: Vec<String>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone)]
struct Inputs {
    memory_mib: f64,
    dirty_mib_s: f64,
    bw_mib_s: f64,
    max_downtime_ms: f64,
}

fn simulate_precopy(i: &Inputs) -> (Vec<MigrationRound>, f64, f64, f64, String, f64) {
    if i.bw_mib_s <= 0.0 {
        return (vec![], f64::INFINITY, f64::INFINITY, 0.0, "blocked".into(), 0.0);
    }

    let ratio = i.dirty_mib_s / i.bw_mib_s;
    let mut rounds = Vec::new();
    let mut remaining = i.memory_mib.max(1.0);
    let mut total_s = 0.0;
    let mut transferred = 0.0;
    let downtime_budget_s = (i.max_downtime_ms.max(25.0) / 1000.0).min(5.0);
    let stop_threshold = (i.bw_mib_s * downtime_budget_s).max(16.0);

    for r in 1..=8 {
        let seconds = remaining / i.bw_mib_s;
        let dirtied = (seconds * i.dirty_mib_s).min(i.memory_mib * 2.0);
        transferred += remaining;
        total_s += seconds;
        rounds.push(MigrationRound {
            round: r,
            remaining_mib: remaining,
            seconds,
            dirtied_mib: dirtied,
        });
        remaining = dirtied;
        if remaining <= stop_threshold {
            break;
        }
        if ratio >= 1.0 && r >= 2 {
            break;
        }
    }

    let downtime_ms = (remaining / i.bw_mib_s * 1000.0).max(1.0);
    transferred += remaining;
    total_s += remaining / i.bw_mib_s;

    let (label, score) = if ratio >= 1.0 {
        ("will_not_converge".into(), 0.0)
    } else if downtime_ms > i.max_downtime_ms.max(25.0) * 2.0 {
        ("high_risk".into(), ((1.0 - ratio) * 45.0).clamp(10.0, 55.0))
    } else if ratio >= 0.70 {
        ("marginal".into(), (65.0 - ratio * 35.0).clamp(35.0, 60.0))
    } else if ratio >= 0.40 {
        ("likely".into(), (78.0 - ratio * 20.0).clamp(60.0, 78.0))
    } else {
        ("strong".into(), (94.0 - ratio * 18.0).clamp(78.0, 96.0))
    };

    (rounds, total_s, downtime_ms, transferred / 1024.0, label, score)
}

fn estimate_dirty(cpu_pct: f64, memory_mib: i64) -> f64 {
    let mem_factor = (memory_mib as f64 / 8192.0).clamp(0.5, 8.0);
    let cpu_factor = (cpu_pct / 100.0).clamp(0.0, 1.5);
    (8.0 + 42.0 * cpu_factor) * mem_factor.sqrt()
}

fn effective_bandwidth_mib_s(mbps: f64) -> f64 {
    (mbps.max(1.0) / 8.0) * 0.82
}

fn minimum_bandwidth_mbps(dirty_mib_s: f64) -> f64 {
    dirty_mib_s * 8.0 / 0.82 * 1.35
}

fn destination_benefit(src_cpu: f64, dst_cpu: f64, mem_headroom_after: i64, vm_mem: i64) -> (f64, f64) {
    let cpu_improve = (src_cpu - dst_cpu).max(0.0);
    let cpu_score = (cpu_improve / 50.0 * 70.0).clamp(0.0, 70.0);
    let mem_ratio = if vm_mem <= 0 { 0.0 } else { mem_headroom_after.max(0) as f64 / vm_mem as f64 };
    let mem_score = (mem_ratio / 2.0 * 30.0).clamp(0.0, 30.0);
    (cpu_score + mem_score, cpu_improve)
}

pub async fn predict(
    pool: &DbPool,
    vm_id: Uuid,
    req: &MigrationOracleRequest,
) -> anyhow::Result<MigrationOracleReport> {
    let vm: (String, Option<Uuid>, i64) = crate::db::query_as(
        "SELECT name, host_id, memory_mib FROM vms WHERE id = ?",
    )
    .bind(vm_id)
    .fetch_one(pool)
    .await?;
    let source_id = vm.1.ok_or_else(|| anyhow::anyhow!("VM has no source host"))?;

    let source: (String, f32) = crate::db::query_as(
        "SELECT hostname, cpu_percent FROM hosts WHERE id = ?",
    )
    .bind(source_id)
    .fetch_one(pool)
    .await?;

    let dest: (String, String, bool, i64, i64, f32) = crate::db::query_as(
        "SELECT hostname, state, maintenance_mode, memory_total_mib, memory_used_mib, cpu_percent
         FROM hosts WHERE id = ?",
    )
    .bind(req.dest_host_id)
    .fetch_one(pool)
    .await?;

    let pre = crate::engine::migrate_precheck::run_migrate_precheck(
        pool, vm_id, req.dest_host_id, req.live,
    )
    .await?;

    let recent_cpu: Option<f64> = crate::db::query_scalar(
        "SELECT cpu_percent FROM vm_metrics WHERE vm_id = ?",
    )
    .bind(vm_id)
    .fetch_optional(pool)
    .await
    .unwrap_or(None);

    let dirty = req.dirty_rate_mib_s.unwrap_or_else(|| {
        estimate_dirty(recent_cpu.unwrap_or(source.1 as f64), vm.2)
    });

    let bw_mbps = req.bandwidth_mbps.unwrap_or(10_000.0);
    let bw_mib_s = effective_bandwidth_mib_s(bw_mbps);
    let downtime_target = req.max_downtime_ms.unwrap_or(250.0).clamp(25.0, 5000.0);

    let inputs = Inputs {
        memory_mib: vm.2 as f64,
        dirty_mib_s: dirty,
        bw_mib_s,
        max_downtime_ms: downtime_target,
    };
    let (rounds, total_s, downtime_ms, transferred_gib, convergence, mut convergence_score) =
        simulate_precopy(&inputs);

    if !pre.ok {
        convergence_score = convergence_score.min(20.0);
    }

    let headroom_after = dest.3.saturating_sub(dest.4).saturating_sub(vm.2);
    let (benefit, cpu_improvement) =
        destination_benefit(source.1 as f64, dest.5 as f64, headroom_after, vm.2);

    let mut risks = Vec::new();
    let mut evidence = Vec::new();
    let mut recommendations = Vec::new();

    // The precheck's failed checks are the first thing a person needs to read: say which one and how to fix it.
    for c in pre.checks.iter().filter(|c| !c.passed) {
        risks.push(format!("Precheck '{}' failed: {}", c.name, c.message));
        if let Some(r) = &c.remediation {
            recommendations.push(r.clone());
        }
    }

    evidence.push(format!(
        "VM memory {} MiB; dirty rate {:.1} MiB/s; effective migration bandwidth {:.1} MiB/s.",
        vm.2, dirty, bw_mib_s
    ));
    evidence.push(format!(
        "Source CPU {:.0}%; destination CPU {:.0}%; destination memory headroom after move {} MiB.",
        source.1, dest.5, headroom_after
    ));

    if dirty >= bw_mib_s {
        risks.push("Dirty-page rate is at or above effective transfer rate; stable pre-copy cannot converge.".into());
        recommendations.push(format!(
            "Provide at least {:.0} Mbit/s effective migration capacity, reduce write rate, or use post-copy/offline migration.",
            minimum_bandwidth_mbps(dirty)
        ));
    }
    if downtime_ms > downtime_target {
        risks.push(format!(
            "Estimated stop-and-copy downtime {:.0} ms exceeds target {:.0} ms.",
            downtime_ms, downtime_target
        ));
    }
    if headroom_after < 0 {
        risks.push("Destination does not have enough current memory headroom.".into());
    }
    if dest.1 != "online" || dest.2 {
        risks.push("Destination is offline or in maintenance mode.".into());
    }
    if benefit < 20.0 {
        risks.push("Destination benefit is weak; migration may add risk without meaningful SLO improvement.".into());
        recommendations.push("Prefer a destination with materially lower CPU pressure and stronger memory headroom.".into());
    } else {
        recommendations.push(format!(
            "Destination benefit score {:.0}/100; expected CPU-pressure improvement about {:.0} percentage points.",
            benefit, cpu_improvement
        ));
    }
    if req.dirty_rate_mib_s.is_none() {
        recommendations.push("For production admission control, feed a measured QEMU/libvirt dirty-page rate instead of the fallback estimate.".into());
    }

    Ok(MigrationOracleReport {
        vm_id,
        vm_name: vm.0,
        source_host_id: source_id,
        source_hostname: source.0,
        dest_host_id: req.dest_host_id,
        dest_hostname: dest.0,
        live: req.live,
        precheck_ok: pre.ok,
        convergence,
        convergence_score,
        estimated_total_seconds: total_s,
        estimated_downtime_ms: downtime_ms,
        estimated_transferred_gib: transferred_gib,
        dirty_rate_mib_s: dirty,
        effective_bandwidth_mib_s: bw_mib_s,
        minimum_bandwidth_mbps: minimum_bandwidth_mbps(dirty),
        destination_benefit_score: benefit,
        predicted_cpu_improvement_pct: cpu_improvement,
        memory_headroom_after_mib: headroom_after,
        rounds,
        risks,
        evidence,
        recommendations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(mem: f64, dirty: f64, bw: f64) -> Inputs {
        Inputs { memory_mib: mem, dirty_mib_s: dirty, bw_mib_s: bw, max_downtime_ms: 250.0 }
    }

    #[test]
    fn low_dirty_rate_converges() {
        let (_, _, dt, _, state, score) = simulate_precopy(&inputs(8192.0, 40.0, 800.0));
        assert_eq!(state, "strong");
        assert!(score > 80.0);
        assert!(dt < 250.0);
    }

    #[test]
    fn dirty_rate_above_bandwidth_does_not_converge() {
        let (_, _, _, _, state, score) = simulate_precopy(&inputs(8192.0, 900.0, 800.0));
        assert_eq!(state, "will_not_converge");
        assert_eq!(score, 0.0);
    }

    #[test]
    fn marginal_ratio_scores_lower() {
        let (_, _, _, _, _, a) = simulate_precopy(&inputs(8192.0, 100.0, 800.0));
        let (_, _, _, _, _, b) = simulate_precopy(&inputs(8192.0, 600.0, 800.0));
        assert!(a > b);
    }

    #[test]
    fn faster_destination_has_benefit() {
        let (s, cpu) = destination_benefit(90.0, 30.0, 16384, 8192);
        assert!(s > 70.0);
        assert_eq!(cpu, 60.0);
    }
}
