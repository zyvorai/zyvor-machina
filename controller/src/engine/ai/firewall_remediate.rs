// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct FirewallRemediation {
    pub id: String,
    pub host_id: String,
    pub hostname: String,
    pub label: String,
    pub review: String,
    pub action: String,
    pub risk: String,
    pub priority: u8,
}

#[derive(Debug, Serialize)]
pub struct FirewallRemediateProposal {
    pub remediations: Vec<FirewallRemediation>,
    pub summary: String,
}

pub async fn propose(pool: &DbPool) -> anyhow::Result<FirewallRemediateProposal> {
    let hosts: Vec<(Uuid, String)> = crate::db::query_as(
        "SELECT id, hostname FROM hosts WHERE state = 'online' ORDER BY hostname LIMIT 200",
    )
    .fetch_all(pool)
    .await?;

    let mut remediations = Vec::new();
    for (host_id, hostname) in hosts {
        let inv = match machina_core::gather_firewall_inventory(&hostname) {
            Ok(i) => i,
            Err(_) => continue,
        };
        let critical = inv
            .open_ports
            .iter()
            .filter(|p| {
                matches!(
                    p.risk,
                    machina_core::firewall::types::ExposureRisk::Critical
                )
            })
            .count();
        if critical > 0 {
            remediations.push(FirewallRemediation {
                id: format!("fw-exposure-{host_id}"),
                host_id: host_id.to_string(),
                hostname: hostname.clone(),
                label: format!("{critical} critical exposure(s) on {hostname}"),
                review: format!(
                    "Firewall score {}/100 — restrict public database/SSH ports",
                    inv.score.score
                ),
                action: "navigate_firewall".into(),
                risk: "Critical".into(),
                priority: 1,
            });
        } else if inv.score.score < 65 {
            remediations.push(FirewallRemediation {
                id: format!("fw-score-{host_id}"),
                host_id: host_id.to_string(),
                hostname: hostname.clone(),
                label: format!("Low firewall score on {hostname} ({})", inv.score.score),
                review: inv
                    .score
                    .recommendations
                    .first()
                    .map(|r| r.label.clone())
                    .unwrap_or_else(|| "Apply Production Server profile".into()),
                action: "secure_machine".into(),
                risk: "Warning".into(),
                priority: 2,
            });
        }
        if inv.posture.drift_detected {
            remediations.push(FirewallRemediation {
                id: format!("fw-drift-{host_id}"),
                host_id: host_id.to_string(),
                hostname,
                label: "Firewall drift detected".into(),
                review: "Rules changed outside Zyra OS — review or revert to baseline".into(),
                action: "investigate_drift".into(),
                risk: "Warning".into(),
                priority: 2,
            });
        }
    }

    let summary = if remediations.is_empty() {
        "Zeus Firewall — no critical host exposures.".into()
    } else {
        format!(
            "{} Zeus Firewall remediation(s) across fleet",
            remediations.len()
        )
    };

    Ok(FirewallRemediateProposal {
        remediations,
        summary,
    })
}
