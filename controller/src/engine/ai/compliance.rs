// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Serialize)]
pub struct ComplianceCheck {
    pub id: String,
    pub name: String,
    pub passed: bool,
    pub score: u8,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct ComplianceReport {
    pub score: u8,
    pub grade: String,
    pub summary: String,
    pub checks: Vec<ComplianceCheck>,
    pub markdown: String,
    pub finding_count: usize,
}

pub async fn generate(pool: &DbPool) -> anyhow::Result<ComplianceReport> {
    let total_vms: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM vms WHERE COALESCE(managed, TRUE) = TRUE")
            .fetch_one(pool)
            .await
            .unwrap_or(0);

    let prod_vms: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM vms WHERE EXISTS (SELECT 1 FROM json_each(COALESCE(tags,'[]')) WHERE value='prod')
         OR EXISTS (SELECT 1 FROM json_each(COALESCE(tags,'[]')) WHERE value='production')",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let prod_with_backup: i64 = crate::db::query_scalar(
        "SELECT COUNT(DISTINCT v.id) FROM vms v
         WHERE (EXISTS (SELECT 1 FROM json_each(COALESCE(v.tags,'[]')) WHERE value='prod') OR EXISTS (SELECT 1 FROM json_each(COALESCE(v.tags,'[]')) WHERE value='production'))
           AND EXISTS (SELECT 1 FROM backup_records b WHERE b.vm_id = v.id AND b.status = 'completed')",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let backup_pct = if prod_vms > 0 {
        (prod_with_backup * 100 / prod_vms).clamp(0, 100) as u8
    } else {
        100
    };
    let backup_passed = backup_pct >= 80;

    let running: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM vms WHERE observed_state = 'running'")
            .fetch_one(pool)
            .await
            .unwrap_or(0);
    let with_guest: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM vms WHERE observed_state = 'running'
         AND guest_tools_status NOT IN ('unknown', 'not_installed')",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let guest_pct = if running > 0 {
        (with_guest * 100 / running).clamp(0, 100) as u8
    } else {
        100
    };
    let guest_passed = guest_pct >= 70;

    let hosts_total: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM hosts")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let hosts_online: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM hosts WHERE state = 'online'")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let host_pct = if hosts_total > 0 {
        (hosts_online * 100 / hosts_total).clamp(0, 100) as u8
    } else {
        100
    };
    let host_passed = host_pct == 100;

    let prod_ha: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM vms v
         JOIN ha_policies hp ON hp.vm_id = v.id AND hp.enabled = TRUE
         WHERE v.observed_state = 'running'
           AND (EXISTS (SELECT 1 FROM json_each(COALESCE(v.tags,'[]')) WHERE value='prod') OR EXISTS (SELECT 1 FROM json_each(COALESCE(v.tags,'[]')) WHERE value='production'))",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let ha_pct = if prod_vms > 0 {
        (prod_ha * 100 / prod_vms).clamp(0, 100) as u8
    } else {
        100
    };
    let ha_passed = ha_pct >= 50;

    let security = super::security::scan(pool).await?;
    let sec_passed = security.risk_level != "high";
    let sec_score = match security.risk_level.as_str() {
        "low" => 100,
        "medium" => 70,
        _ => 40,
    };

    let checks = vec![
        ComplianceCheck {
            id: "backup_coverage".into(),
            name: "Production backup coverage".into(),
            passed: backup_passed,
            score: backup_pct,
            detail: format!("{prod_with_backup}/{prod_vms} production VMs have completed backups ({backup_pct}%)"),
        },
        ComplianceCheck {
            id: "guest_tools".into(),
            name: "Guest tools on running VMs".into(),
            passed: guest_passed,
            score: guest_pct,
            detail: format!("{with_guest}/{running} running VMs report guest tools ({guest_pct}%)"),
        },
        ComplianceCheck {
            id: "host_availability".into(),
            name: "Host availability".into(),
            passed: host_passed,
            score: host_pct,
            detail: format!("{hosts_online}/{hosts_total} hosts online"),
        },
        ComplianceCheck {
            id: "ha_coverage".into(),
            name: "HA on production VMs".into(),
            passed: ha_passed,
            score: ha_pct,
            detail: format!("{prod_ha}/{prod_vms} production VMs have HA enabled ({ha_pct}%)"),
        },
        ComplianceCheck {
            id: "security_posture".into(),
            name: "Security Sentinel posture".into(),
            passed: sec_passed,
            score: sec_score,
            detail: format!(
                "Risk level: {} — {} finding(s)",
                security.risk_level,
                security.findings.len()
            ),
        },
    ];

    let score = if checks.is_empty() {
        0
    } else {
        (checks.iter().map(|c| c.score as u16).sum::<u16>() / checks.len() as u16) as u8
    };
    let grade = if score >= 90 {
        "A"
    } else if score >= 75 {
        "B"
    } else if score >= 60 {
        "C"
    } else {
        "D"
    };

    let mut markdown = String::from("# Machina Compliance Report\n\n");
    markdown.push_str(&format!(
        "**Overall score:** {score}/100 (Grade {grade})\n\n"
    ));
    markdown.push_str(&format!(
        "**Cluster VMs:** {total_vms} total, {prod_vms} tagged production\n\n"
    ));
    markdown.push_str("## Checks\n\n");
    for c in &checks {
        markdown.push_str(&format!(
            "- [{}] **{}** — {} ({}/100)\n",
            if c.passed { "x" } else { " " },
            c.name,
            c.detail,
            c.score
        ));
    }
    if !security.findings.is_empty() {
        markdown.push_str("\n## Security findings\n\n");
        for f in &security.findings {
            markdown.push_str(&format!(
                "- **{}** ({}) — {}\n",
                f.title, f.severity, f.detail
            ));
        }
    }
    markdown.push_str("\n---\n_Generated by Machina Compliance — deterministic v1_\n");

    let summary = if score >= 80 {
        "Cluster meets baseline compliance targets.".into()
    } else {
        format!("{score}/100 — address failed checks in Doctor and Recommendations.")
    };

    Ok(ComplianceReport {
        score,
        grade: grade.into(),
        summary,
        checks,
        markdown,
        finding_count: security.findings.len(),
    })
}

pub fn report_to_html(report: &ComplianceReport) -> String {
    let mut rows = String::new();
    for c in &report.checks {
        let status = if c.passed { "PASS" } else { "FAIL" };
        let color = if c.passed { "#22c55e" } else { "#f59e0b" };
        rows.push_str(&format!(
            "<tr><td style=\"color:{color};font-weight:600\">{status}</td><td>{name}</td><td>{detail}</td><td>{score}/100</td></tr>",
            name = html_escape(&c.name),
            detail = html_escape(&c.detail),
            score = c.score,
        ));
    }
    format!(
        r#"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"/><title>Machina Compliance Report</title>
<style>
body{{font-family:system-ui,sans-serif;max-width:800px;margin:2rem auto;padding:0 1rem;color:#1e293b}}
h1{{font-size:1.5rem}} .score{{font-size:2.5rem;font-weight:700;color:#0f172a}}
table{{width:100%;border-collapse:collapse;margin-top:1.5rem;font-size:0.875rem}}
th,td{{border:1px solid #e2e8f0;padding:0.5rem 0.75rem;text-align:left}}
th{{background:#f8fafc}} footer{{margin-top:2rem;font-size:0.75rem;color:#64748b}}
@media print{{body{{margin:0}}}}
</style></head><body>
<h1>Machina Compliance Report</h1>
<p class="score">{score}/100 <span style="font-size:1rem;font-weight:500">(Grade {grade})</span></p>
<p>{summary}</p>
<table><thead><tr><th>Status</th><th>Check</th><th>Detail</th><th>Score</th></tr></thead>
<tbody>{rows}</tbody></table>
<footer>Generated by Zyra · {findings} security finding(s) · Print this page to save as PDF</footer>
</body></html>"#,
        score = report.score,
        grade = report.grade,
        summary = html_escape(&report.summary),
        findings = report.finding_count,
        rows = rows,
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn report_to_pdf(report: &ComplianceReport) -> Vec<u8> {
    let mut lines = vec![
        format!("Score: {}/100 (Grade {})", report.score, report.grade),
        report.summary.clone(),
        String::new(),
    ];
    for c in &report.checks {
        let status = if c.passed { "PASS" } else { "FAIL" };
        lines.push(format!(
            "[{status}] {} — {} ({}/100)",
            c.name, c.detail, c.score
        ));
    }
    if report.finding_count > 0 {
        lines.push(String::new());
        lines.push(format!(
            "Security findings: {} (see Security Sentinel for details)",
            report.finding_count
        ));
    }
    simple_text_pdf("Machina Compliance Report", &lines)
}

fn pdf_escape(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii() && (*c >= ' ' || *c == '\n'))
        .map(|c| match c {
            '\\' => "\\\\".to_string(),
            '(' => "\\(".to_string(),
            ')' => "\\)".to_string(),
            _ => c.to_string(),
        })
        .collect()
}

pub fn simple_text_pdf(title: &str, body_lines: &[String]) -> Vec<u8> {
    let mut stream = String::from("BT\n/F1 16 Tf\n72 750 Td\n");
    stream.push_str(&format!("({}) Tj\n", pdf_escape(title)));
    stream.push_str("0 -22 Td\n/F1 10 Tf\n");
    for line in body_lines {
        if line.is_empty() {
            stream.push_str("0 -10 Td\n");
            continue;
        }
        stream.push_str(&format!("({}) Tj\n", pdf_escape(line)));
        stream.push_str("0 -14 Td\n");
    }
    stream.push_str("ET\n");
    build_pdf_with_xref(&stream)
}

fn build_pdf_with_xref(stream: &str) -> Vec<u8> {
    let parts = [
        "%PDF-1.4\n",
        "1 0 obj<< /Type /Catalog /Pages 2 0 R >>endobj\n",
        "2 0 obj<< /Type /Pages /Kids [3 0 R] /Count 1 >>endobj\n",
        "3 0 obj<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>endobj\n",
        &format!(
            "4 0 obj<< /Length {} >>stream\n{}endstream\nendobj\n",
            stream.len(),
            stream
        ),
        "5 0 obj<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>endobj\n",
    ];

    let mut body = String::new();
    let mut offsets = vec![0usize];
    for part in &parts {
        offsets.push(body.len());
        body.push_str(part);
    }

    let xref_start = body.len();
    body.push_str("xref\n0 6\n");
    body.push_str("0000000000 65535 f \n");
    for off in offsets.iter().skip(1) {
        body.push_str(&format!("{:010} 00000 n \n", off));
    }
    body.push_str("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n");
    body.push_str(&format!("{xref_start}\n%%EOF\n"));
    body.into_bytes()
}
