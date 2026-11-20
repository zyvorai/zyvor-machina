// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Serialize)]
pub struct FrameworkControl {
    pub id: String,
    pub framework: String,
    pub title: String,
    pub passed: bool,
    pub score: u8,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct FrameworkScore {
    pub framework: String,
    pub score: u8,
    pub grade: String,
    pub control_count: usize,
    pub failed_count: usize,
}

#[derive(Debug, Serialize)]
pub struct ComplianceFrameworksReport {
    pub frameworks: Vec<FrameworkScore>,
    pub controls: Vec<FrameworkControl>,
    pub summary: String,
}

pub async fn scan(pool: &DbPool) -> anyhow::Result<ComplianceFrameworksReport> {
    let base = super::compliance::generate(pool).await?;
    let security = super::security::scan(pool).await?;

    let mut controls = Vec::new();

    for check in &base.checks {
        let frameworks = map_check_to_frameworks(&check.id);
        for fw in frameworks {
            controls.push(FrameworkControl {
                id: format!("{}-{}", fw.to_lowercase(), check.id),
                framework: fw.into(),
                title: check.name.clone(),
                passed: check.passed,
                score: check.score,
                detail: check.detail.clone(),
            });
        }
    }

    controls.push(FrameworkControl {
        id: "cis-sec-sentinel".into(),
        framework: "CIS".into(),
        title: "Security Sentinel risk level".into(),
        passed: security.risk_level != "high",
        score: match security.risk_level.as_str() {
            "low" => 95,
            "medium" => 75,
            _ => 45,
        },
        detail: format!(
            "Zyra Security Sentinel: {} risk ({} findings)",
            security.risk_level,
            security.findings.len()
        ),
    });

    controls.push(FrameworkControl {
        id: "pci-api-keys".into(),
        framework: "PCI".into(),
        title: "API key hygiene".into(),
        passed: security.findings.iter().all(|f| !f.id.contains("api_key")),
        score: 80,
        detail: "Rotate API keys quarterly; restrict scopes.".into(),
    });

    controls.push(FrameworkControl {
        id: "hipaa-backup".into(),
        framework: "HIPAA".into(),
        title: "Production backup coverage".into(),
        passed: base
            .checks
            .iter()
            .any(|c| c.id == "backup_coverage" && c.passed),
        score: base
            .checks
            .iter()
            .find(|c| c.id == "backup_coverage")
            .map(|c| c.score)
            .unwrap_or(0),
        detail: "PHI workloads require documented backup RPO/RTO.".into(),
    });

    controls.push(FrameworkControl {
        id: "soc2-ha".into(),
        framework: "SOC2".into(),
        title: "HA on production workloads".into(),
        passed: base
            .checks
            .iter()
            .any(|c| c.id == "ha_coverage" && c.passed),
        score: base
            .checks
            .iter()
            .find(|c| c.id == "ha_coverage")
            .map(|c| c.score)
            .unwrap_or(0),
        detail: "Availability commitments for production VMs.".into(),
    });

    let framework_names = ["CIS", "PCI", "SOC2", "HIPAA"];
    let mut frameworks = Vec::new();
    for fw in framework_names {
        let fw_controls: Vec<_> = controls.iter().filter(|c| c.framework == fw).collect();
        if fw_controls.is_empty() {
            continue;
        }
        let failed = fw_controls.iter().filter(|c| !c.passed).count();
        let avg: u8 = (fw_controls.iter().map(|c| c.score as u32).sum::<u32>()
            / fw_controls.len().max(1) as u32) as u8;
        frameworks.push(FrameworkScore {
            framework: fw.into(),
            score: avg,
            grade: grade_for(avg),
            control_count: fw_controls.len(),
            failed_count: failed,
        });
    }

    let summary = format!(
        "{} framework(s) mapped · overall compliance grade {} ({}/100)",
        frameworks.len(),
        base.grade,
        base.score
    );

    Ok(ComplianceFrameworksReport {
        frameworks,
        controls,
        summary,
    })
}

fn map_check_to_frameworks(check_id: &str) -> Vec<&'static str> {
    match check_id {
        "backup_coverage" => vec!["SOC2", "HIPAA", "PCI"],
        "guest_tools" => vec!["CIS", "SOC2"],
        "host_availability" => vec!["SOC2", "CIS"],
        "ha_coverage" => vec!["SOC2", "HIPAA"],
        "security_posture" => vec!["CIS", "PCI", "SOC2"],
        _ => vec!["SOC2"],
    }
}

fn grade_for(score: u8) -> String {
    match score {
        90..=100 => "A",
        80..=89 => "B",
        70..=79 => "C",
        60..=69 => "D",
        _ => "F",
    }
    .into()
}
