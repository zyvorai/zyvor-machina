// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

use super::forecast as trend;

#[derive(Debug, Serialize)]
pub struct ResourceExhaustionForecast {
    pub vm_id: String,
    pub vm_name: String,
    pub resource: String,
    pub severity: String,
    pub message: String,
    pub hours_until_critical: Option<f64>,
    pub confidence: f64,
}

#[derive(Debug, Serialize)]
pub struct SreForecastReport {
    pub forecasts: Vec<ResourceExhaustionForecast>,
}

pub async fn forecast(pool: &DbPool) -> anyhow::Result<SreForecastReport> {
    let rows: Vec<(Uuid, String, i64, i64, f64)> = crate::db::query_as(
        "SELECT v.id, v.name, v.memory_mib, m.memory_used_mib, m.cpu_percent
         FROM vms v
         JOIN vm_metrics m ON m.vm_id = v.id
         WHERE v.observed_state = 'running'
         ORDER BY v.name
         LIMIT 100",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut forecasts = Vec::new();
    for (vid, name, mem_alloc, mem_used, cpu) in rows {
        if mem_alloc > 0 {
            let ratio = mem_used as f64 / mem_alloc as f64;
            let samples = trend::series(pool, &vid.to_string(), "mem_ratio", 72)
                .await
                .unwrap_or_default();
            if let Some(c) = trend::time_to_threshold(&samples, 0.95) {
                forecasts.push(ResourceExhaustionForecast {
                    vm_id: vid.to_string(),
                    vm_name: name.clone(),
                    resource: "memory".into(),
                    severity: severity_for_hours(c.hours).into(),
                    message: format!(
                        "VM {name} memory is at {:.0}% and growing about {:.0} points a day — full in {} at this rate",
                        ratio * 100.0,
                        c.per_day * 100.0,
                        trend::humanize_hours(c.hours)
                    ),
                    hours_until_critical: Some(c.hours),
                    confidence: c.confidence,
                });
            } else if ratio >= 0.9 {
                // No usable trend yet (or it is flat): say what is true now, with no invented time.
                forecasts.push(ResourceExhaustionForecast {
                    vm_id: vid.to_string(),
                    vm_name: name.clone(),
                    resource: "memory".into(),
                    severity: if ratio >= 0.95 { "critical" } else { "high" }.into(),
                    message: format!(
                        "VM {name} memory is at {:.0}% of what it was given",
                        ratio * 100.0
                    ),
                    hours_until_critical: None,
                    confidence: 0.5,
                });
            }
        }
        if cpu >= 90.0 {
            forecasts.push(ResourceExhaustionForecast {
                vm_id: vid.to_string(),
                vm_name: name.clone(),
                resource: "cpu".into(),
                severity: "high".into(),
                message: format!("VM {name} sustained CPU at {cpu:.0}% — latency risk"),
                hours_until_critical: None,
                confidence: 0.62,
            });
        }
    }

    let pools: Vec<(Uuid, String, f64)> = crate::db::query_as(
        "SELECT id, name, used_gib * 1.0 / capacity_gib FROM storage_pools WHERE capacity_gib > 0",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for (pid, pname, ratio) in pools {
        let samples = trend::series(pool, &format!("pool:{pid}"), "pool_used_ratio", 7 * 24)
            .await
            .unwrap_or_default();
        if let Some(c) = trend::time_to_threshold(&samples, 0.95) {
            forecasts.push(ResourceExhaustionForecast {
                vm_id: "cluster".into(),
                vm_name: format!("(pool {pname})"),
                resource: "storage".into(),
                severity: severity_for_hours(c.hours).into(),
                message: format!(
                    "Storage pool {pname} is {:.0}% full and filling about {:.1} points a day — full in {}",
                    ratio * 100.0,
                    c.per_day * 100.0,
                    trend::humanize_hours(c.hours)
                ),
                hours_until_critical: Some(c.hours),
                confidence: c.confidence,
            });
        } else if ratio >= 0.9 {
            forecasts.push(ResourceExhaustionForecast {
                vm_id: "cluster".into(),
                vm_name: format!("(pool {pname})"),
                resource: "storage".into(),
                severity: "critical".into(),
                message: format!(
                    "Storage pool {pname} is {:.0}% full — VM IO failures are likely if it fills",
                    ratio * 100.0
                ),
                hours_until_critical: None,
                confidence: 0.75,
            });
        }
    }

    // Rank by severity numerically, not by string: lexicographically "high" >
    // "critical" (h > c), so a raw descending string compare sorted `high` ahead
    // of `critical` — and with truncate(20) genuine critical forecasts could be
    // dropped while high ones were kept.
    fn severity_rank(s: &str) -> u8 {
        match s {
            "critical" => 0,
            "high" => 1,
            "medium" => 2,
            "low" => 3,
            _ => 4,
        }
    }
    forecasts.sort_by(|a, b| {
        severity_rank(&a.severity)
            .cmp(&severity_rank(&b.severity))
            .then_with(|| {
                a.hours_until_critical
                    .partial_cmp(&b.hours_until_critical)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    forecasts.truncate(20);

    Ok(SreForecastReport { forecasts })
}

/// Under a day is critical, under three days high, otherwise medium.
fn severity_for_hours(h: f64) -> &'static str {
    if h < 24.0 {
        "critical"
    } else if h < 72.0 {
        "high"
    } else {
        "medium"
    }
}

#[cfg(test)]
mod tests {
    use super::severity_for_hours;

    #[test]
    fn severity_follows_time_left() {
        assert_eq!(severity_for_hours(3.0), "critical");
        assert_eq!(severity_for_hours(40.0), "high");
        assert_eq!(severity_for_hours(200.0), "medium");
    }
}
