// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Serialize)]
pub struct RemediationItem {
    pub id: String,
    pub source: String,
    pub label: String,
    pub review: String,
    pub action: String,
    pub priority: u8,
    pub risk: String,
}

#[derive(Debug, Serialize)]
pub struct RemediateHub {
    pub items: Vec<RemediationItem>,
    pub summary: String,
}

pub async fn hub(pool: &DbPool) -> anyhow::Result<RemediateHub> {
    let cfg = crate::config::ControllerConfig::default();
    let sre = super::sre_remediate::propose(pool).await?;
    let compliance = super::compliance_remediate::propose(pool).await?;
    let power = super::fleet_power::optimize(pool).await?;
    let firewall = super::firewall_remediate::propose(pool).await?;
    let exposure_waste = super::exposure_finops::propose_waste(pool, &cfg).await.ok();
    let joint = super::exposure_finops::joint_sre_finops(pool, &cfg)
        .await
        .ok();

    let mut items = Vec::new();

    let sre_count = sre.remediations.len();
    let compliance_count = compliance.remediations.len();
    let power_count = power.optimizations.len().min(5);
    let firewall_count = firewall.remediations.len();
    let finops_count = exposure_waste.as_ref().map(|w| w.items.len()).unwrap_or(0);
    let joint_count = joint.as_ref().map(|j| j.len()).unwrap_or(0);

    for r in sre.remediations {
        items.push(RemediationItem {
            id: r.id,
            source: "sre".into(),
            label: r.label,
            review: r.review,
            action: r.action,
            priority: r.priority,
            risk: r.risk,
        });
    }

    for r in firewall.remediations {
        items.push(RemediationItem {
            id: r.id,
            source: "firewall".into(),
            label: r.label,
            review: r.review,
            action: r.action,
            priority: r.priority,
            risk: r.risk,
        });
    }

    if let Some(waste) = exposure_waste {
        for w in waste.items {
            items.push(RemediationItem {
                id: w.id,
                source: "finops".into(),
                label: w.label,
                review: w.review,
                action: w.action,
                priority: w.priority,
                risk: w.risk,
            });
        }
    }

    if let Some(joints) = joint {
        for j in joints {
            items.push(RemediationItem {
                id: j.id,
                source: "sre_finops".into(),
                label: j.label,
                review: j.review,
                action: j.action,
                priority: j.priority,
                risk: j.risk,
            });
        }
    }

    for (i, r) in compliance.remediations.iter().enumerate() {
        items.push(RemediationItem {
            id: r.id.clone(),
            source: "compliance".into(),
            label: r.label.clone(),
            review: r.review.clone(),
            action: r.action.clone(),
            priority: (2usize + i).min(u8::MAX as usize) as u8,
            risk: r.risk.clone(),
        });
    }

    for (i, o) in power.optimizations.iter().enumerate().take(5) {
        items.push(RemediationItem {
            id: format!("power-{}", i),
            source: "fleet".into(),
            label: format!("Optimize {}", o.host),
            review: o.reason.clone(),
            action: o.action.clone(),
            priority: 3,
            risk: "Low".into(),
        });
    }

    items.sort_by_key(|i| i.priority);

    let summary = if items.is_empty() {
        "Remediation hub clear — no open SRE, compliance, firewall, FinOps, or fleet actions."
            .into()
    } else {
        format!(
            "{} unified remediation(s): {} SRE · {} firewall · {} FinOps · {} joint · {} compliance · {} fleet",
            items.len(),
            sre_count,
            firewall_count,
            finops_count,
            joint_count,
            compliance_count,
            power_count
        )
    };

    Ok(RemediateHub { items, summary })
}
