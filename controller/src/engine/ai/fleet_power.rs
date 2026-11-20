// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Serialize)]
pub struct PowerOptimization {
    pub host: String,
    pub action: String,
    pub estimated_savings_usd_month: f64,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct FleetPowerReport {
    pub optimizations: Vec<PowerOptimization>,
    pub total_savings_usd_month: f64,
    pub summary: String,
}

pub async fn optimize(pool: &DbPool) -> anyhow::Result<FleetPowerReport> {
    let heat = super::fleet_heatmap::heatmap(pool).await?;
    let rates: (f64, f64) = crate::db::query_as(
        "SELECT finops_vcpu_hour_usd, finops_gib_hour_usd FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .unwrap_or((0.02, 0.005));

    let idle_host_monthly = (16.0 * rates.0 + 64.0 * rates.1) * 730.0 * 0.15;

    let mut optimizations = Vec::new();
    for host in &heat.power_waste_hosts {
        optimizations.push(PowerOptimization {
            host: host.clone(),
            action: "consolidate_or_power_down".into(),
            estimated_savings_usd_month: idle_host_monthly,
            reason: "Cold host with zero VMs — candidate for consolidation or power-down.".into(),
        });
    }

    for host in heat.hotspots.iter().take(3) {
        optimizations.push(PowerOptimization {
            host: host.clone(),
            action: "rebalance_vms".into(),
            estimated_savings_usd_month: idle_host_monthly * 0.4,
            reason: "Hotspot host — live-migrate VMs to cold nodes.".into(),
        });
    }

    let total_savings_usd_month = optimizations
        .iter()
        .map(|o| o.estimated_savings_usd_month)
        .sum();

    let summary = if optimizations.is_empty() {
        "Fleet power profile balanced — no waste optimizations.".into()
    } else {
        format!(
            "{} optimization(s) · ~${:.0}/mo estimated savings",
            optimizations.len(),
            total_savings_usd_month
        )
    };

    Ok(FleetPowerReport {
        optimizations,
        total_savings_usd_month,
        summary,
    })
}
