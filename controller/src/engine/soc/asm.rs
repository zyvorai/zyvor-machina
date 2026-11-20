// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

use crate::config::ControllerConfig;
use crate::engine::ai::security_graph;
use crate::engine::zeus_firewall;

#[derive(Debug, Serialize)]
pub struct AsmSummary {
    pub exposure_score: f32,
    pub firewall_targets: usize,
    pub high_risk_nodes: usize,
    pub open_port_findings: Vec<AsmFinding>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AsmFinding {
    pub kind: String,
    pub resource: String,
    pub detail: String,
    pub severity: String,
}

pub async fn build_asm_summary(
    pool: &DbPool,
    cfg: &ControllerConfig,
) -> anyhow::Result<AsmSummary> {
    let overview = zeus_firewall::overview(pool, cfg).await?;
    let graph = security_graph::build_graph(pool).await?;

    let high_risk = graph
        .nodes
        .iter()
        .filter(|n| n.risk.as_deref() == Some("high"))
        .count();

    let mut findings = Vec::new();
    for t in overview.targets.iter().take(20) {
        let score = t.score as f64;
        if score >= 60.0 || t.risk == "critical" || t.risk == "high" {
            findings.push(AsmFinding {
                kind: "firewall_target".into(),
                resource: t.name.clone(),
                detail: format!(
                    "Risk {} score {score:.0} · {} open ports",
                    t.risk, t.open_ports
                ),
                severity: if score >= 80.0 || t.risk == "critical" {
                    "high"
                } else {
                    "medium"
                }
                .into(),
            });
        }
    }

    for n in graph
        .nodes
        .iter()
        .filter(|n| n.risk.as_deref() == Some("high"))
        .take(10)
    {
        findings.push(AsmFinding {
            kind: n.kind.clone(),
            resource: n.label.clone(),
            detail: "High risk node in security graph".into(),
            severity: "high".into(),
        });
    }

    let exposure_score = (100.0_f32 - (findings.len() as f32 * 4.0)).clamp(20.0, 100.0);

    let mut recommendations = vec![
        "Review Zeus Firewall open ports and temporary rules.".into(),
        "Run connectivity simulation for critical VM pairs.".into(),
    ];
    if high_risk > 0 {
        recommendations.push(format!("Remediate {high_risk} high-risk graph node(s)."));
    }

    Ok(AsmSummary {
        exposure_score,
        firewall_targets: overview.targets.len(),
        high_risk_nodes: high_risk,
        open_port_findings: findings,
        recommendations,
    })
}
