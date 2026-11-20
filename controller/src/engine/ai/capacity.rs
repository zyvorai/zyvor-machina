// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Serialize)]
pub struct CapacityPlan {
    pub hosts_online: i64,
    pub memory_headroom_mib: i64,
    pub avg_cpu_percent: f32,
    pub storage_used_gib: i64,
    pub storage_capacity_gib: i64,
    pub storage_runway_days: Option<i32>,
    pub cpu_headroom_percent: f32,
    pub estimated_small_vms_addable: i64,
    pub recommendations: Vec<String>,
    pub forecast_30d_vms: i64,
    pub forecast_60d_vms: i64,
    pub forecast_90d_vms: i64,
}

pub async fn plan(pool: &DbPool) -> anyhow::Result<CapacityPlan> {
    let hosts_online: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM hosts WHERE state = 'online'")
        .fetch_one(pool)
        .await?;
    let mem: (i64, i64) = crate::db::query_as(
        "SELECT COALESCE(SUM(memory_total_mib), 0), COALESCE(SUM(memory_used_mib), 0) FROM hosts WHERE state = 'online'",
    )
    .fetch_one(pool)
    .await?;
    let avg_cpu: f32 = crate::db::query_scalar(
        "SELECT COALESCE(AVG(cpu_percent), 0.0) FROM hosts WHERE state = 'online'",
    )
    .fetch_one(pool)
    .await?;
    let memory_headroom_mib = mem.0.saturating_sub(mem.1);

    let (storage_used, storage_cap): (i64, i64) = crate::db::query_as(
        "SELECT COALESCE(SUM(used_gib), 0), COALESCE(SUM(capacity_gib), 0) FROM storage_pools",
    )
    .fetch_one(pool)
    .await
    .unwrap_or((0, 0));

    let storage_runway_days = if storage_used > 0 && storage_cap > storage_used {
        let daily_growth = (storage_used as f64 * 0.02).max(1.0);
        Some(((storage_cap - storage_used) as f64 / daily_growth) as i32)
    } else {
        None
    };

    let cpu_headroom = (100.0 - avg_cpu).max(0.0);
    let mem_per_vm = 4096i64;
    let estimated_small_vms_addable = if mem_per_vm > 0 {
        memory_headroom_mib / mem_per_vm
    } else {
        0
    };

    let vm_count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM vms WHERE managed = TRUE")
        .fetch_one(pool)
        .await?;
    let growth_rate = 0.02_f64;
    let forecast_30d_vms = (vm_count as f64 * (1.0 + growth_rate)).round() as i64;
    let forecast_60d_vms = (vm_count as f64 * (1.0 + growth_rate * 2.0)).round() as i64;
    let forecast_90d_vms = (vm_count as f64 * (1.0 + growth_rate * 3.0)).round() as i64;

    let mut recommendations = Vec::new();
    recommendations.push(format!(
        "30/60/90-day VM forecast (2% monthly): {forecast_30d_vms} / {forecast_60d_vms} / {forecast_90d_vms} (current {vm_count})"
    ));
    if avg_cpu > 75.0 {
        recommendations.push("CPU pressure high — add hosts or migrate workloads.".into());
    }
    if memory_headroom_mib < 8192 {
        recommendations.push("Memory headroom low — defer large VM creates.".into());
    }
    if let Some(days) = storage_runway_days {
        if days < 30 {
            recommendations.push(format!(
                "Storage may reach capacity in ~{days} days at current growth."
            ));
        }
    }

    Ok(CapacityPlan {
        hosts_online,
        memory_headroom_mib,
        avg_cpu_percent: avg_cpu,
        storage_used_gib: storage_used,
        storage_capacity_gib: storage_cap,
        storage_runway_days,
        cpu_headroom_percent: cpu_headroom,
        estimated_small_vms_addable,
        recommendations,
        forecast_30d_vms,
        forecast_60d_vms,
        forecast_90d_vms,
    })
}

pub async fn export_csv(pool: &DbPool) -> anyhow::Result<String> {
    let plan = plan(pool).await?;
    let mut csv = String::from("Machina Capacity Planner Export\nMetric,Value\n");
    csv.push_str(&format!("Hosts online,{}\n", plan.hosts_online));
    csv.push_str(&format!(
        "Memory headroom MiB,{}\n",
        plan.memory_headroom_mib
    ));
    csv.push_str(&format!("Avg CPU %,{:.1}\n", plan.avg_cpu_percent));
    csv.push_str(&format!(
        "CPU headroom %,{:.1}\n",
        plan.cpu_headroom_percent
    ));
    csv.push_str(&format!("Storage used GiB,{}\n", plan.storage_used_gib));
    csv.push_str(&format!(
        "Storage capacity GiB,{}\n",
        plan.storage_capacity_gib
    ));
    if let Some(days) = plan.storage_runway_days {
        csv.push_str(&format!("Storage runway days,{days}\n"));
    }
    csv.push_str(&format!(
        "Est small VMs addable,{}\n\n",
        plan.estimated_small_vms_addable
    ));
    csv.push_str("Recommendation\n");
    for r in &plan.recommendations {
        csv.push_str(&format!("{}\n", csv_escape(r)));
    }
    Ok(csv)
}

fn csv_escape(s: &str) -> String {
    // Neutralize CSV/Excel formula injection: recommendation text can embed
    // user-controlled data (VM names, etc.) and this file is served as an
    // attachment for humans to open in Excel/Sheets. A cell starting with
    // =, +, -, or @ is interpreted as a formula by those tools (e.g.
    // `=cmd|'/c calc'!A1`), so prefix a neutralizing apostrophe before
    // applying the existing quote/comma escaping.
    let needs_formula_guard = s
        .chars()
        .next()
        .is_some_and(|c| matches!(c, '=' | '+' | '-' | '@' | '\t' | '\r'));
    let s = if needs_formula_guard {
        std::borrow::Cow::Owned(format!("'{s}"))
    } else {
        std::borrow::Cow::Borrowed(s)
    };
    if s.contains(',') || s.contains('"') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.into_owned()
    }
}
