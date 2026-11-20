// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Mission Control geography aggregator (site → rack → host).

use serde::Serialize;
use crate::db::DbPool;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct MissionHostRow {
    pub id: Uuid,
    pub hostname: String,
    pub address: String,
    pub state: String,
    pub maintenance_mode: bool,
    pub vm_count: i32,
    pub cpu_percent: f32,
    pub memory_used_mib: i64,
    pub memory_total_mib: i64,
    pub site: String,
    pub rack: String,
    pub rack_u: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MissionHost {
    pub id: String,
    pub hostname: String,
    pub address: String,
    pub state: String,
    pub maintenance_mode: bool,
    pub vm_count: i32,
    pub cpu_percent: f32,
    pub memory_used_mib: i64,
    pub memory_total_mib: i64,
    pub site: String,
    pub rack: String,
    pub rack_u: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MissionRack {
    pub name: String,
    pub hosts: Vec<MissionHost>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MissionSite {
    pub name: String,
    pub racks: Vec<MissionRack>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MissionSummary {
    pub hosts: i64,
    pub vms: i64,
    pub hosts_online: i64,
    pub health_pct: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetMissionOverview {
    pub sites: Vec<MissionSite>,
    pub unassigned_hosts: Vec<MissionHost>,
    pub summary: MissionSummary,
}

const MISSION_HOST_SQL: &str = "SELECT id, hostname, address, state, maintenance_mode, vm_count,
         cpu_percent, memory_used_mib, memory_total_mib,
         COALESCE(site, '') AS site, COALESCE(rack, '') AS rack, rack_u
         FROM hosts ORDER BY site, rack, rack_u NULLS LAST, hostname";

fn map_host(row: MissionHostRow) -> MissionHost {
    MissionHost {
        id: row.id.to_string(),
        hostname: row.hostname,
        address: row.address,
        state: row.state,
        maintenance_mode: row.maintenance_mode,
        vm_count: row.vm_count,
        cpu_percent: row.cpu_percent,
        memory_used_mib: row.memory_used_mib,
        memory_total_mib: row.memory_total_mib,
        site: row.site,
        rack: row.rack,
        rack_u: row.rack_u,
    }
}

pub async fn overview(pool: &DbPool) -> anyhow::Result<FleetMissionOverview> {
    let rows = crate::db::query_as::<_, MissionHostRow>(MISSION_HOST_SQL)
        .fetch_all(pool)
        .await?;

    let hosts_total = rows.len() as i64;
    let hosts_online = rows
        .iter()
        .filter(|h| h.state == "online" && !h.maintenance_mode)
        .count() as i64;
    let vm_count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM vms")
        .fetch_one(pool)
        .await?;
    let health_pct = if hosts_total > 0 {
        ((hosts_online as f64 / hosts_total as f64) * 100.0).round() as i32
    } else {
        100
    };

    let mut unassigned = Vec::new();
    let mut site_map: BTreeMap<String, BTreeMap<String, Vec<MissionHost>>> = BTreeMap::new();

    for row in rows {
        let host = map_host(row);
        let site_key = host.site.trim();
        let rack_key = host.rack.trim();
        if site_key.is_empty() {
            unassigned.push(host);
            continue;
        }
        let rack_name = if rack_key.is_empty() {
            "Unassigned rack".into()
        } else {
            rack_key.to_string()
        };
        site_map
            .entry(site_key.to_string())
            .or_default()
            .entry(rack_name)
            .or_default()
            .push(host);
    }

    let sites = site_map
        .into_iter()
        .map(|(name, racks_map)| MissionSite {
            name,
            racks: racks_map
                .into_iter()
                .map(|(name, hosts)| MissionRack { name, hosts })
                .collect(),
        })
        .collect();

    Ok(FleetMissionOverview {
        sites,
        unassigned_hosts: unassigned,
        summary: MissionSummary {
            hosts: hosts_total,
            vms: vm_count,
            hosts_online,
            health_pct,
        },
    })
}
