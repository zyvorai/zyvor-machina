// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Serialize)]
pub struct CostAnalysis {
    pub estimated_monthly_usd: f64,
    pub predicted_next_month_usd: f64,
    pub vm_count: i64,
    pub idle_vm_count: i64,
    pub oversized_vm_count: i64,
    pub snapshot_heavy_count: i64,
    pub suggestions: Vec<String>,
}

pub async fn analyze(pool: &DbPool) -> anyhow::Result<CostAnalysis> {
    let rates: (f64, f64) = crate::db::query_as(
        "SELECT finops_vcpu_hour_usd, finops_gib_hour_usd FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_one(pool)
    .await?;
    let vm_count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM vms")
        .fetch_one(pool)
        .await?;
    let totals: (i64, i64) =
        crate::db::query_as("SELECT COALESCE(SUM(vcpus), 0), COALESCE(SUM(memory_mib), 0) FROM vms")
            .fetch_one(pool)
            .await?;
    let memory_gib = totals.1 as f64 / 1024.0;
    let hourly = totals.0 as f64 * rates.0 + memory_gib * rates.1;
    let estimated_monthly_usd = hourly * 730.0;
    let idle_vm_count: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM vms WHERE observed_state != 'running'
         AND updated_at < datetime('now', '-30 days')",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let oversized_vm_count: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM vms v
         JOIN vm_metrics m ON m.vm_id = v.id
         WHERE v.observed_state = 'running'
           AND v.memory_mib > 0
           AND m.memory_used_mib > 0
           AND CAST(m.memory_used_mib AS REAL) / CAST(v.memory_mib AS REAL) < 0.35",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let snapshot_heavy_count: i64 = crate::db::query_scalar(
        "SELECT COUNT(DISTINCT vm_id) FROM snapshot_records WHERE status = 'completed'",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let mut suggestions = Vec::new();
    if idle_vm_count > 0 {
        suggestions.push(format!(
            "Archive or remove {idle_vm_count} idle VM(s) stopped 30+ days."
        ));
    }
    if oversized_vm_count > 0 {
        suggestions.push(format!(
            "Right-size {oversized_vm_count} VM(s) using <35% allocated memory."
        ));
    }
    if snapshot_heavy_count > 5 {
        suggestions.push("Consolidate old snapshots to reduce storage cost.".into());
    }

    let growth = if idle_vm_count > 2 {
        1.08
    } else if oversized_vm_count > 3 {
        1.03
    } else {
        1.02
    };
    let predicted_next_month_usd = estimated_monthly_usd * growth;

    Ok(CostAnalysis {
        estimated_monthly_usd,
        predicted_next_month_usd,
        vm_count,
        idle_vm_count,
        oversized_vm_count,
        snapshot_heavy_count,
        suggestions,
    })
}

pub async fn export_csv(pool: &DbPool) -> anyhow::Result<String> {
    let analysis = analyze(pool).await?;
    let rates: (f64, f64) = crate::db::query_as(
        "SELECT finops_vcpu_hour_usd, finops_gib_hour_usd FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_one(pool)
    .await?;

    let vms: Vec<(String, i64, i64, String)> = crate::db::query_as(
        "SELECT name, vcpus, memory_mib, COALESCE(observed_state, 'unknown')
         FROM vms ORDER BY name",
    )
    .fetch_all(pool)
    .await?;

    let mut csv = String::from(
        "Machina Cost Guardian CFO Export\n\
Summary,Value\n\
Estimated monthly USD,",
    );
    csv.push_str(&format!("{:.2}\n", analysis.estimated_monthly_usd));
    csv.push_str(&format!("VM count,{}\n", analysis.vm_count));
    csv.push_str(&format!(
        "Idle VMs (30d+ stopped),{}\n",
        analysis.idle_vm_count
    ));
    csv.push_str(&format!("Oversized VMs,{}\n", analysis.oversized_vm_count));
    csv.push_str(&format!(
        "Snapshot-heavy VMs,{}\n",
        analysis.snapshot_heavy_count
    ));
    csv.push_str(&format!("vCPU rate USD/hr,{}\n", rates.0));
    csv.push_str(&format!("Memory rate USD/GiB/hr,{}\n\n", rates.1));
    csv.push_str("VM,vCPUs,Memory MiB,State,Est monthly USD\n");

    for (name, vcpus, memory_mib, state) in vms {
        let gib = memory_mib as f64 / 1024.0;
        let monthly = (vcpus as f64 * rates.0 + gib * rates.1) * 730.0;
        csv.push_str(&format!(
            "{},{},{},{},{:.2}\n",
            csv_escape(&name),
            vcpus,
            memory_mib,
            csv_escape(&state),
            monthly
        ));
    }
    Ok(csv)
}

fn csv_escape(s: &str) -> String {
    // Neutralize CSV/Excel formula injection: values here can originate from
    // user-controlled data (VM names, states) and this file is served as an
    // attachment for humans to open in Excel/Sheets. A cell starting with
    // =, +, -, or @ is interpreted as a formula by those tools (e.g.
    // `=cmd|'/c calc'!A1`), so prefix a neutralizing apostrophe before
    // applying the existing quote/comma/newline escaping.
    let needs_formula_guard = s
        .chars()
        .next()
        .is_some_and(|c| matches!(c, '=' | '+' | '-' | '@' | '\t' | '\r'));
    let s = if needs_formula_guard {
        std::borrow::Cow::Owned(format!("'{s}"))
    } else {
        std::borrow::Cow::Borrowed(s)
    };
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.into_owned()
    }
}
