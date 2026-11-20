// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Security waste + SRE×FinOps joint remediate items (Phase 22).

use serde::Serialize;
use crate::db::DbPool;

use crate::config::ControllerConfig;

#[derive(Debug, Clone, Serialize)]
pub struct ExposureWasteItem {
    pub id: String,
    pub label: String,
    pub review: String,
    pub action: String,
    pub monthly_waste_usd: f64,
    pub priority: u8,
    pub risk: String,
}

#[derive(Debug, Serialize)]
pub struct ExposureWasteProposal {
    pub items: Vec<ExposureWasteItem>,
    pub summary: String,
}

pub async fn propose_waste(
    pool: &DbPool,
    cfg: &ControllerConfig,
) -> anyhow::Result<ExposureWasteProposal> {
    let report = crate::engine::zeus_firewall::finops::exposure_rollup(pool, cfg).await?;
    let mut items = Vec::new();

    for t in report
        .targets
        .iter()
        .filter(|t| t.exposure_monthly_usd > 20.0)
    {
        items.push(ExposureWasteItem {
            id: format!("finops-waste-{}", t.target_id),
            label: format!(
                "${:.0}/mo exposure on {} ({})",
                t.exposure_monthly_usd, t.name, t.kind
            ),
            review: t
                .alert
                .clone()
                .unwrap_or_else(|| format!("{} open ports · apply Zyra profile", t.open_ports)),
            action: "navigate_firewall".into(),
            monthly_waste_usd: t.exposure_monthly_usd,
            priority: if t.critical_ports > 0 { 1 } else { 2 },
            risk: if t.critical_ports > 0 {
                "Critical".into()
            } else {
                "Warning".into()
            },
        });
    }

    for v in report.vm_idle_ranking.iter().take(3) {
        items.push(ExposureWasteItem {
            id: format!("finops-vm-idle-{}", v.vm_id),
            label: format!("VM {} — ${:.0}/mo idle port waste", v.vm_name, v.waste_usd),
            review: format!(
                "{} public idle port(s) on team:{} — close or move behind LB",
                v.idle_ports, v.team
            ),
            action: "vm_guest_ports".into(),
            monthly_waste_usd: v.waste_usd,
            priority: 3,
            risk: "Info".into(),
        });
    }

    if report.cloud_sg_monthly_usd > 5.0 {
        items.push(ExposureWasteItem {
            id: "finops-cloud-sg".into(),
            label: format!(
                "Cloud SG exposure ${:.0}/mo ({} public rules)",
                report.cloud_sg_monthly_usd, report.cloud_attribution.public_rules
            ),
            review: "Review 0.0.0.0/0 ingress on cloud security groups".into(),
            action: "cloud_firewall".into(),
            monthly_waste_usd: report.cloud_sg_monthly_usd,
            priority: 2,
            risk: "Warning".into(),
        });
    }

    items.sort_by(|a, b| {
        b.monthly_waste_usd
            .partial_cmp(&a.monthly_waste_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total_waste: f64 = items.iter().map(|i| i.monthly_waste_usd).sum();
    let summary = if items.is_empty() {
        "No material exposure waste detected.".into()
    } else {
        format!(
            "${:.0}/mo security waste across {} item(s)",
            total_waste,
            items.len()
        )
    };

    Ok(ExposureWasteProposal { items, summary })
}

pub async fn joint_sre_finops(
    pool: &DbPool,
    cfg: &ControllerConfig,
) -> anyhow::Result<Vec<ExposureWasteItem>> {
    let waste = propose_waste(pool, cfg).await?;
    let sre = super::sre_remediate::propose(pool).await?;
    let mut joint = Vec::new();

    for s in sre.remediations.iter().take(3) {
        let exposure = waste
            .items
            .iter()
            .find(|w| w.label.contains(&s.vm_name) || w.review.contains(&s.vm_name))
            .map(|w| w.monthly_waste_usd)
            .unwrap_or(0.0);
        if exposure > 0.0 {
            joint.push(ExposureWasteItem {
                id: format!("joint-{}-{}", s.id, exposure as u32),
                label: format!("SRE + FinOps: {} (${:.0}/mo exposure)", s.label, exposure),
                review: format!("{} · {}", s.review, waste.summary),
                action: s.action.clone(),
                monthly_waste_usd: exposure,
                priority: 1,
                risk: s.risk.clone(),
            });
        }
    }

    Ok(joint)
}
