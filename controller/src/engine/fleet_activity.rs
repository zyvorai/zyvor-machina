// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Fleet Activity Monitor aggregator (Phase 37).

use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

use crate::config::ControllerConfig;
use crate::engine::host_os;

#[derive(Debug, Clone, Serialize)]
pub struct VmActivityItem {
    pub vm_id: String,
    pub vm_name: String,
    pub host_id: Option<String>,
    pub observed_state: String,
    pub cpu_percent: f32,
    pub memory_used_mib: i64,
    pub memory_mib: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostActivityItem {
    pub host_id: String,
    pub hostname: String,
    pub state: String,
    pub cpu_percent: f32,
    pub memory_percent: f32,
    pub vm_count: i32,
    pub io_pressure_pct: f64,
    pub cpu_pressure_pct: f64,
    pub memory_pressure_pct: f64,
    pub thermal_max_c: f64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetActivityOverview {
    pub summary: String,
    pub top_vms: Vec<VmActivityItem>,
    pub hosts: Vec<HostActivityItem>,
    pub pressure_hosts: usize,
    pub running_vms: i64,
}

pub async fn overview(
    pool: &DbPool,
    cfg: &ControllerConfig,
) -> anyhow::Result<FleetActivityOverview> {
    let running_vms: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM vms WHERE observed_state = 'running'")
            .fetch_one(pool)
            .await?;

    let vm_rows: Vec<(Uuid, String, Option<Uuid>, String, f32, i64, i64)> = crate::db::query_as(
        "SELECT v.id, v.name, v.host_id, v.observed_state,
                COALESCE(m.cpu_percent, 0.0), COALESCE(m.memory_used_mib, 0), v.memory_mib
         FROM vms v
         LEFT JOIN vm_metrics m ON m.vm_id = v.id
         WHERE v.observed_state = 'running'
         ORDER BY COALESCE(m.cpu_percent, 0) DESC, v.name
         LIMIT 25",
    )
    .fetch_all(pool)
    .await?;

    let top_vms: Vec<VmActivityItem> = vm_rows
        .into_iter()
        .map(
            |(id, name, host_id, observed_state, cpu_percent, memory_used_mib, memory_mib)| {
                VmActivityItem {
                    vm_id: id.to_string(),
                    vm_name: name,
                    host_id: host_id.map(|h| h.to_string()),
                    observed_state,
                    cpu_percent,
                    memory_used_mib,
                    memory_mib,
                }
            },
        )
        .collect();

    let host_rows: Vec<(Uuid, String, String, f32, i64, i64, i32)> = crate::db::query_as(
        "SELECT id, hostname, state, cpu_percent, memory_used_mib, memory_total_mib, vm_count
         FROM hosts
         ORDER BY cpu_percent DESC, hostname
         LIMIT 20",
    )
    .fetch_all(pool)
    .await?;

    // `/proc/pressure/*` reports `avg10` already as a 0-100 percentage (kernel
    // PSI docs), not a 0-1 fraction — multiplying by 100 here inflated every
    // reading 100x (e.g. a real 17.67% IO stall showed as "1767%"), which in
    // turn tripped the `io > 50.0` "under pressure" threshold on almost any
    // nonzero pressure.
    fn psi_pct(obs: &serde_json::Value, key: &str) -> f64 {
        obs.get("pressure")
            .and_then(|p| p.get(key))
            .and_then(|i| i.get("some"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
    }

    let mut hosts = Vec::new();
    let mut pressure_hosts = 0usize;

    for (id, hostname, state, cpu_percent, mem_used, mem_total, vm_count) in host_rows {
        let mem_pct = if mem_total > 0 {
            (mem_used as f32 / mem_total as f32) * 100.0
        } else {
            0.0
        };

        if state != "online" {
            hosts.push(HostActivityItem {
                host_id: id.to_string(),
                hostname,
                state,
                cpu_percent,
                memory_percent: mem_pct,
                vm_count,
                io_pressure_pct: 0.0,
                cpu_pressure_pct: 0.0,
                memory_pressure_pct: 0.0,
                thermal_max_c: 0.0,
                status: "offline".into(),
            });
            continue;
        }

        let (io_pressure_pct, cpu_pressure_pct, memory_pressure_pct, thermal_max_c, status) =
            if let Ok(obs) = host_os::linux_observability(pool, cfg, id).await {
                let io = psi_pct(&obs, "io");
                let cpu_psi = psi_pct(&obs, "cpu");
                let mem_psi = psi_pct(&obs, "memory");
                let thermal_max = obs
                    .get("thermal")
                    .and_then(|t| t.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.get("temp_celsius").and_then(|v| v.as_f64()))
                            .fold(0.0_f64, f64::max)
                    })
                    .unwrap_or(0.0);
                let st = if io > 50.0 || cpu_psi > 50.0 || mem_psi > 50.0 {
                    "pressure"
                } else if thermal_max > 80.0 {
                    "thermal"
                } else if cpu_percent > 85.0 || mem_pct > 85.0 {
                    "hot"
                } else {
                    "ok"
                };
                (io, cpu_psi, mem_psi, thermal_max, st)
            } else if cpu_percent > 85.0 || mem_pct > 85.0 {
                (0.0, 0.0, 0.0, 0.0, "hot")
            } else {
                (0.0, 0.0, 0.0, 0.0, "ok")
            };

        if status == "pressure" || status == "thermal" {
            pressure_hosts += 1;
        }

        hosts.push(HostActivityItem {
            host_id: id.to_string(),
            hostname,
            state,
            cpu_percent,
            memory_percent: mem_pct,
            vm_count,
            io_pressure_pct,
            cpu_pressure_pct,
            memory_pressure_pct,
            thermal_max_c,
            status: status.into(),
        });
    }

    Ok(FleetActivityOverview {
        summary: format!(
            "{running_vms} running VM(s) · {} host(s) · {pressure_hosts} under Linux pressure",
            hosts.len()
        ),
        top_vms,
        hosts,
        pressure_hosts,
        running_vms,
    })
}
