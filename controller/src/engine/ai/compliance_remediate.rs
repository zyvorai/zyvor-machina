// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Serialize)]
pub struct ComplianceRemediation {
    pub id: String,
    pub framework: String,
    pub control: String,
    pub action: String,
    pub label: String,
    pub review: String,
    pub risk: String,
}

#[derive(Debug, Serialize)]
pub struct ComplianceRemediationReport {
    pub remediations: Vec<ComplianceRemediation>,
    pub summary: String,
}

pub async fn propose(pool: &DbPool) -> anyhow::Result<ComplianceRemediationReport> {
    let report = super::compliance_frameworks::scan(pool).await?;
    let mut remediations = Vec::new();

    for ctrl in report.controls.iter().filter(|c| !c.passed) {
        let action = match ctrl.id.as_str() {
            id if id.contains("backup") => "bulk_backup",
            id if id.contains("guest") => "install_guest_tools",
            id if id.contains("ha") => "enable_ha",
            id if id.contains("sec") || id.contains("api") => "open_security",
            _ => "open_compliance",
        };
        remediations.push(ComplianceRemediation {
            id: format!("compliance-fix-{}", ctrl.id),
            framework: ctrl.framework.clone(),
            control: ctrl.title.clone(),
            action: action.into(),
            label: format!("Fix {} ({})", ctrl.title, ctrl.framework),
            review: ctrl.detail.clone(),
            risk: "Low".into(),
        });
    }

    let summary = if remediations.is_empty() {
        "All mapped framework controls passing — no compliance remediations.".into()
    } else {
        format!(
            "{} compliance remediation(s) from CIS/PCI/SOC2/HIPAA mapping.",
            remediations.len()
        )
    };

    Ok(ComplianceRemediationReport {
        remediations,
        summary,
    })
}
