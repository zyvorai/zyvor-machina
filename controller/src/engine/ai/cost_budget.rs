// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Serialize)]
pub struct BudgetAlert {
    pub id: String,
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct CostBudgetReport {
    pub monthly_budget_usd: f64,
    pub current_spend_usd: f64,
    pub predicted_spend_usd: f64,
    pub utilization_pct: f32,
    pub status: String,
    pub alerts: Vec<BudgetAlert>,
    pub summary: String,
}

pub async fn analyze(pool: &DbPool) -> anyhow::Result<CostBudgetReport> {
    let cost = super::cost::analyze(pool).await?;
    let attribution = super::cost_attribution::attribute(pool).await?;

    let monthly_budget_usd = (cost.estimated_monthly_usd * 1.15).max(1000.0);
    let current_spend_usd = cost.estimated_monthly_usd;
    let predicted_spend_usd = cost.predicted_next_month_usd;
    let utilization_pct = if monthly_budget_usd > 0.0 {
        (current_spend_usd / monthly_budget_usd * 100.0) as f32
    } else {
        0.0
    };

    let mut alerts = Vec::new();
    if utilization_pct >= 95.0 {
        alerts.push(BudgetAlert {
            id: "budget-critical".into(),
            severity: "critical".into(),
            message: format!("Spend at {:.0}% of monthly budget", utilization_pct),
        });
    } else if utilization_pct >= 80.0 {
        alerts.push(BudgetAlert {
            id: "budget-warning".into(),
            severity: "warning".into(),
            message: format!("Spend at {:.0}% of monthly budget", utilization_pct),
        });
    }

    if cost.idle_vm_count > 2 {
        alerts.push(BudgetAlert {
            id: "idle-waste".into(),
            severity: "info".into(),
            message: format!("{} idle VMs contributing to waste", cost.idle_vm_count),
        });
    }

    if attribution.unattributed_monthly_usd > current_spend_usd * 0.3 {
        alerts.push(BudgetAlert {
            id: "unattributed".into(),
            severity: "info".into(),
            message: "Over 30% spend lacks team/project attribution tags".into(),
        });
    }

    let cfg = crate::config::ControllerConfig::default();
    if let Ok(exp) = crate::engine::zeus_firewall::finops::exposure_rollup(pool, &cfg).await {
        if exp.fleet_exposure_monthly_usd > current_spend_usd * 0.05 {
            alerts.push(BudgetAlert {
                id: "firewall-exposure-overlap".into(),
                severity: if exp.fleet_exposure_monthly_usd > current_spend_usd * 0.1 {
                    "warning".into()
                } else {
                    "info".into()
                },
                message: format!(
                    "Firewall exposure est ${:.0}/mo ({:.0}% of infra spend) — FinOps × Security overlap",
                    exp.fleet_exposure_monthly_usd,
                    if current_spend_usd > 0.0 {
                        exp.fleet_exposure_monthly_usd / current_spend_usd * 100.0
                    } else {
                        0.0
                    }
                ),
            });
        }
        if exp.idle_port_waste_usd > 50.0 {
            alerts.push(BudgetAlert {
                id: "idle-port-waste".into(),
                severity: "info".into(),
                message: format!(
                    "${:.0}/mo idle open port waste across fleet",
                    exp.idle_port_waste_usd
                ),
            });
        }
    }

    let status = if alerts.iter().any(|a| a.severity == "critical") {
        "over_budget"
    } else if alerts.iter().any(|a| a.severity == "warning") {
        "watch"
    } else {
        "on_track"
    };

    let summary = format!(
        "${:.0} / ${:.0} budget ({:.0}%) · predicted ${:.0} next month",
        current_spend_usd, monthly_budget_usd, utilization_pct, predicted_spend_usd
    );

    Ok(CostBudgetReport {
        monthly_budget_usd,
        current_spend_usd,
        predicted_spend_usd,
        utilization_pct,
        status: status.into(),
        alerts,
        summary,
    })
}
