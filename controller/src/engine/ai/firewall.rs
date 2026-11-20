// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

use crate::config::ControllerConfig;
use crate::engine::zeus_firewall::inventory::{plan_target, target_detail};
use machina_core::FirewallPlanRequest;

#[derive(Debug, Clone, Serialize)]
pub struct FirewallExplainReport {
    pub target: String,
    pub risk: String,
    pub evidence: Vec<String>,
    pub recommendation: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SecurePlanStep {
    pub step: u32,
    pub action: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SecurePlanReport {
    pub target: String,
    pub steps: Vec<SecurePlanStep>,
    pub risk_after: String,
    pub rollback: bool,
    pub summary: String,
}

pub async fn explain_exposure(
    pool: &DbPool,
    cfg: &ControllerConfig,
    target_id: &str,
    question: Option<&str>,
) -> anyhow::Result<FirewallExplainReport> {
    let detail = target_detail(pool, cfg, target_id).await?;
    let mut evidence = Vec::new();
    for port in &detail.inventory.open_ports {
        if matches!(
            port.risk,
            machina_core::firewall::types::ExposureRisk::Critical
        ) {
            evidence.push(format!(
                "{} port {} open on {} — allowed from {:?}",
                port.service_name, port.port, port.bind_address, port.allowed_from
            ));
        }
    }
    for rule in &detail.inventory.rules {
        if rule.action == "allow"
            && rule.sources.iter().any(|s| s == "any" || s == "0.0.0.0/0")
            && (rule.ports == "22" || rule.ports == "5432" || rule.ports == "3306")
        {
            evidence.push(format!(
                "Rule {} allows {} from any source",
                rule.id, rule.ports
            ));
        }
    }
    let risk = detail.target.risk.clone();
    let recommendation = if evidence.is_empty() {
        "No critical exposures detected".into()
    } else if evidence
        .iter()
        .any(|e| e.contains("5432") || e.contains("3306"))
    {
        "Restrict database to application subnet only".into()
    } else {
        "Restrict SSH to admin subnet only".into()
    };
    let summary = question
        .map(|q| format!("{q}: {}", detail.target.name))
        .unwrap_or_else(|| format!("Firewall posture for {}", detail.target.name));
    Ok(FirewallExplainReport {
        target: detail.target.name,
        risk,
        evidence,
        recommendation,
        summary,
    })
}

pub async fn secure_machine_plan(
    pool: &DbPool,
    cfg: &ControllerConfig,
    target_id: &str,
) -> anyhow::Result<SecurePlanReport> {
    let detail = target_detail(pool, cfg, target_id).await?;
    let mut steps = vec![
        SecurePlanStep {
            step: 1,
            action: "Keep SSH allowed only from admin subnet".into(),
        },
        SecurePlanStep {
            step: 2,
            action: "Restrict database ports to application servers".into(),
        },
        SecurePlanStep {
            step: 3,
            action: "Allow monitoring from monitoring namespace".into(),
        },
        SecurePlanStep {
            step: 4,
            action: "Block all other inbound".into(),
        },
        SecurePlanStep {
            step: 5,
            action: "Enable Stealth Mode (Standard)".into(),
        },
        SecurePlanStep {
            step: 6,
            action: "Create rollback checkpoint".into(),
        },
    ];
    if detail
        .inventory
        .open_ports
        .iter()
        .any(|p| p.port == 80 || p.port == 443)
    {
        steps.insert(
            1,
            SecurePlanStep {
                step: 2,
                action: "Keep HTTP/HTTPS for web traffic".into(),
            },
        );
    }
    Ok(SecurePlanReport {
        target: detail.target.name,
        steps,
        risk_after: "low".into(),
        rollback: true,
        summary: format!("Safe Firewall Plan for {}", detail.target.hostname),
    })
}

pub async fn simulate_plan(
    pool: &DbPool,
    cfg: &ControllerConfig,
    target_id: &str,
    profile: &str,
) -> anyhow::Result<serde_json::Value> {
    let detail = target_detail(pool, cfg, target_id).await?;
    let plan = plan_target(
        pool,
        cfg,
        target_id,
        FirewallPlanRequest {
            profile: Some(profile.into()),
            enable: Some(true),
            stealth_level: None,
            preset: None,
            dry_run: true,
            zone_cidrs: Default::default(),
        },
    )
    .await?;
    let after_rules: Vec<machina_core::ZeusFirewallRule> = machina_core::profile_by_name(profile)
        .map(|p| {
            p.rules
                .iter()
                .enumerate()
                .map(|(i, r)| machina_core::ZeusFirewallRule {
                    id: format!("sim-{i}"),
                    direction: r.direction.clone(),
                    protocol: r.protocol.clone(),
                    ports: r.ports.clone(),
                    sources: r.sources.clone(),
                    targets: vec![],
                    action: r.action.clone(),
                    temporary: false,
                    expires_at: None,
                    description: Some(r.name.clone()),
                    scope: "host".into(),
                    backend_ref: None,
                })
                .collect()
        })
        .unwrap_or_else(|| detail.inventory.rules.clone());
    let matrix = machina_core::simulate_connectivity(&detail.inventory, &after_rules);
    Ok(serde_json::json!({
        "diff": plan.diff,
        "warnings": plan.diff.warnings,
        "simulation": {
            "allows": matrix.allows,
            "blocks": matrix.blocks,
        },
        "connectivity_matrix": matrix,
        "connectivity_notes": plan.diff.warnings
    }))
}

pub async fn compliance_report(
    pool: &DbPool,
    cfg: &ControllerConfig,
    report_kind: &str,
) -> anyhow::Result<serde_json::Value> {
    let overview = crate::engine::zeus_firewall::overview(pool, cfg).await?;
    let targets: Vec<_> = if report_kind == "metal" {
        overview
            .targets
            .iter()
            .filter(|t| t.kind == "bare_metal")
            .collect()
    } else {
        overview.targets.iter().collect()
    };
    let critical: Vec<_> = targets
        .iter()
        .filter(|t| t.risk == "critical")
        .map(|t| serde_json::json!({ "name": t.name, "kind": t.kind, "score": t.score, "open_ports": t.open_ports }))
        .collect();
    Ok(serde_json::json!({
        "report": report_kind,
        "machines_scanned": targets.len(),
        "compliant": targets.iter().filter(|t| t.risk == "low").count(),
        "warnings": targets.iter().filter(|t| t.risk == "warning").count(),
        "critical": critical.len(),
        "critical_machines": critical,
        "export_formats": ["pdf", "csv", "json"]
    }))
}
