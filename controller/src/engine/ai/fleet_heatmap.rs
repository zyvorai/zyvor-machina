// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Serialize)]
pub struct HostHeatCell {
    pub host_id: String,
    pub hostname: String,
    pub cpu_percent: f32,
    pub memory_percent: f32,
    pub vm_count: i32,
    pub classification: String,
}

#[derive(Debug, Serialize)]
pub struct FleetHeatmap {
    pub hosts: Vec<HostHeatCell>,
    pub hotspots: Vec<String>,
    pub cold_hosts: Vec<String>,
    pub power_waste_hosts: Vec<String>,
}

pub async fn heatmap(pool: &DbPool) -> anyhow::Result<FleetHeatmap> {
    let rows: Vec<(uuid::Uuid, String, f32, i64, i64, i32, String)> = crate::db::query_as(
        "SELECT id, hostname, cpu_percent, memory_used_mib, memory_total_mib, vm_count, state
         FROM hosts ORDER BY hostname",
    )
    .fetch_all(pool)
    .await?;

    let mut hosts = Vec::new();
    let mut hotspots = Vec::new();
    let mut cold_hosts = Vec::new();
    let mut power_waste_hosts = Vec::new();

    for (id, hostname, cpu, mem_used, mem_total, vm_count, state) in rows {
        let mem_pct = if mem_total > 0 {
            (mem_used as f32 / mem_total as f32) * 100.0
        } else {
            0.0
        };
        let classification = if state != "online" {
            "offline"
        } else if cpu >= 85.0 || mem_pct >= 85.0 {
            "hot"
        } else if cpu < 15.0 && mem_pct < 20.0 && vm_count <= 1 {
            "cold"
        } else {
            "balanced"
        };

        if classification == "hot" {
            hotspots.push(hostname.clone());
        }
        if classification == "cold" {
            cold_hosts.push(hostname.clone());
        }
        if classification == "cold" && vm_count == 0 {
            power_waste_hosts.push(hostname.clone());
        }

        hosts.push(HostHeatCell {
            host_id: id.to_string(),
            hostname,
            cpu_percent: cpu,
            memory_percent: mem_pct,
            vm_count,
            classification: classification.into(),
        });
    }

    Ok(FleetHeatmap {
        hosts,
        hotspots,
        cold_hosts,
        power_waste_hosts,
    })
}
