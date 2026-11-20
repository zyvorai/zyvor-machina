// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Idle-machine detection for FinOps. A running VM is idle when, over at least a day of recorded samples, its CPU
//! averaged under 5% and never went above 25%. Savings are the VM's compute cost at the cluster's FinOps rates.

use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

use super::forecast;

pub const MIN_HOURS: f64 = 24.0;
pub const AVG_CPU_MAX: f64 = 5.0;
pub const PEAK_CPU_MAX: f64 = 25.0;
const HOURS_PER_MONTH: f64 = 730.0;

#[derive(Debug, Serialize, PartialEq)]
pub struct IdleVm {
    pub vm_id: String,
    pub name: String,
    pub vcpus: i64,
    pub memory_mib: i64,
    pub avg_cpu_percent: f64,
    pub peak_cpu_percent: f64,
    pub hours_observed: f64,
    pub monthly_usd: f64,
}

pub fn monthly_cost(vcpus: i64, memory_mib: i64, vcpu_rate: f64, gib_rate: f64) -> f64 {
    let hourly = vcpus as f64 * vcpu_rate + (memory_mib as f64 / 1024.0) * gib_rate;
    (hourly * HOURS_PER_MONTH * 100.0).round() / 100.0
}

/// (avg, peak, hours observed) for (epoch seconds, cpu%) samples; None when there is too little to judge.
pub fn summarize(samples: &[(i64, f64)]) -> Option<(f64, f64, f64)> {
    if samples.len() < forecast::MIN_SAMPLES {
        return None;
    }
    let hours = (samples.last()?.0 - samples.first()?.0) as f64 / 3600.0;
    let avg = samples.iter().map(|s| s.1).sum::<f64>() / samples.len() as f64;
    let peak = samples.iter().map(|s| s.1).fold(0.0, f64::max);
    Some((avg, peak, hours))
}

pub fn is_idle(avg: f64, peak: f64, hours: f64) -> bool {
    hours >= MIN_HOURS && avg < AVG_CPU_MAX && peak < PEAK_CPU_MAX
}

/// Idle running machines, biggest saving first.
pub async fn find(pool: &DbPool, limit: usize) -> anyhow::Result<Vec<IdleVm>> {
    let (vcpu_rate, gib_rate): (f64, f64) = crate::db::query_as(
        "SELECT finops_vcpu_hour_usd, finops_gib_hour_usd FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_one(pool)
    .await?;
    let vms: Vec<(Uuid, String, i64, i64)> = crate::db::query_as(
        "SELECT id, name, vcpus, memory_mib FROM vms WHERE observed_state = 'running' ORDER BY name LIMIT 200",
    )
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for (id, name, vcpus, memory_mib) in vms {
        let samples = forecast::series(pool, &id.to_string(), "cpu_percent", 7 * 24).await?;
        let Some((avg, peak, hours)) = summarize(&samples) else {
            continue;
        };
        if is_idle(avg, peak, hours) {
            out.push(IdleVm {
                vm_id: id.to_string(),
                name,
                vcpus,
                memory_mib,
                avg_cpu_percent: (avg * 10.0).round() / 10.0,
                peak_cpu_percent: (peak * 10.0).round() / 10.0,
                hours_observed: hours.round(),
                monthly_usd: monthly_cost(vcpus, memory_mib, vcpu_rate, gib_rate),
            });
        }
    }
    out.sort_by(|a, b| {
        b.monthly_usd
            .partial_cmp(&a.monthly_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(limit);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(n: i64, step: i64, v: f64) -> Vec<(i64, f64)> {
        (0..n).map(|i| (i * step, v)).collect()
    }

    #[test]
    fn a_day_of_near_zero_cpu_is_idle() {
        let (avg, peak, hours) = summarize(&flat(100, 1000, 1.0)).unwrap();
        assert!(hours > 24.0 && is_idle(avg, peak, hours));
    }

    #[test]
    fn one_busy_burst_is_not_idle() {
        let mut s = flat(100, 1000, 1.0);
        s[50].1 = 80.0;
        let (avg, peak, hours) = summarize(&s).unwrap();
        assert!(!is_idle(avg, peak, hours));
    }

    #[test]
    fn short_history_is_never_called_idle() {
        assert!(summarize(&flat(5, 1000, 0.0)).is_none());
        let (avg, peak, hours) = summarize(&flat(20, 60, 0.0)).unwrap();
        assert!(!is_idle(avg, peak, hours)); // 20 minutes of history
    }

    #[test]
    fn monthly_cost_uses_cpu_and_memory_rates() {
        // (2 vCPU * 0.02 + 4 GiB * 0.005) = 0.06/h * 730
        assert_eq!(monthly_cost(2, 4096, 0.02, 0.005), 43.8);
    }
}
