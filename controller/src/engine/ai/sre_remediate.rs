// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Serialize)]
pub struct SreRemediation {
    pub id: String,
    pub vm_id: Option<String>,
    pub vm_name: String,
    pub action: String,
    pub label: String,
    pub review: String,
    pub risk: String,
    pub priority: u8,
}

#[derive(Debug, Serialize)]
pub struct SreRemediationReport {
    pub remediations: Vec<SreRemediation>,
    pub summary: String,
}

pub async fn propose(pool: &DbPool) -> anyhow::Result<SreRemediationReport> {
    let forecast = super::sre_predict::forecast(pool).await?;
    let mut remediations = Vec::new();

    for (i, f) in forecast.forecasts.iter().enumerate() {
        let (action, label, review, risk) = match f.resource.as_str() {
            "memory" => (
                "rightsize_vm",
                format!("Right-size memory for {}", f.vm_name),
                format!("{} — add RAM or reduce workload before OOM.", f.message),
                if f.severity == "critical" {
                    "Review required"
                } else {
                    "Low"
                },
            ),
            "cpu" => (
                "scale_out",
                format!("Reduce CPU pressure on {}", f.vm_name),
                format!("{} — rebalance or add vCPU capacity.", f.message),
                "Medium",
            ),
            "storage" => (
                "expand_storage",
                "Expand storage pool capacity".into(),
                f.message.clone(),
                "Review required",
            ),
            _ => continue,
        };

        remediations.push(SreRemediation {
            id: format!("sre-remediate-{i}"),
            vm_id: if f.vm_id == "cluster" {
                None
            } else {
                Some(f.vm_id.clone())
            },
            vm_name: f.vm_name.clone(),
            action: action.into(),
            label,
            review,
            risk: risk.into(),
            priority: if f.severity == "critical" { 1 } else { 2 },
        });
    }

    let summary = if remediations.is_empty() {
        "No proactive SRE remediations — cluster forecasts are within guardrails.".into()
    } else {
        format!(
            "{} AI SRE remediation(s) proposed from exhaustion forecasts.",
            remediations.len()
        )
    };

    Ok(SreRemediationReport {
        remediations,
        summary,
    })
}
