// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use crate::config::ControllerConfig;
use crate::engine::ai::compliance::simple_text_pdf;
use crate::engine::ai::firewall::compliance_report;
use crate::db::DbPool;

pub async fn export_compliance_pdf(
    pool: &DbPool,
    cfg: &ControllerConfig,
    kind: &str,
) -> anyhow::Result<Vec<u8>> {
    let report = compliance_report(pool, cfg, kind).await?;
    let machines = report
        .get("machines_scanned")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let critical = report.get("critical").and_then(|v| v.as_u64()).unwrap_or(0);
    let lines = vec![
        format!("Report kind: {kind}"),
        format!("Machines scanned: {machines}"),
        format!("Critical: {critical}"),
        format!(
            "Compliant: {}",
            report
                .get("compliant")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
        ),
        String::new(),
        "See Zeus Firewall compliance UI for machine-level detail.".into(),
    ];
    Ok(simple_text_pdf(&format!("Zeus Firewall — {kind}"), &lines))
}
