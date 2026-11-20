// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Fleet Stage Manager / workspace spaces rollup (Phase 47).

use serde::Serialize;
use crate::db::DbPool;

use crate::engine::enterprise_security;

#[derive(Debug, Clone, Serialize)]
pub struct FleetSpaceItem {
    pub name: String,
    pub vm_count: i64,
    pub running_count: i64,
    pub stopped_count: i64,
    pub host_count: i64,
    pub network_isolation: String,
    pub enforce_quotas: bool,
    pub quota_status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetSpacesOverview {
    pub summary: String,
    pub space_count: usize,
    pub total_vms: i64,
    pub running_vms: i64,
    pub spaces: Vec<FleetSpaceItem>,
}

pub async fn overview(pool: &DbPool) -> anyhow::Result<FleetSpacesOverview> {
    let vm_rows: Vec<(String, i64, i64, i64, i64)> = crate::db::query_as(
        "SELECT COALESCE(NULLIF(project, ''), 'default') AS name,
                COUNT(*),
                COUNT(*) FILTER (WHERE observed_state = 'running'),
                COUNT(*) FILTER (WHERE observed_state IS DISTINCT FROM 'running'),
                COUNT(DISTINCT host_id)
         FROM vms
         GROUP BY 1
         ORDER BY 2 DESC, 1",
    )
    .fetch_all(pool)
    .await?;

    let tenants = enterprise_security::tenant_isolation_overview(pool).await?;

    let mut total_vms = 0i64;
    let mut running_vms = 0i64;
    let mut spaces = Vec::new();

    for (name, vm_count, running_count, stopped_count, host_count) in vm_rows {
        total_vms += vm_count;
        running_vms += running_count;
        let tenant = tenants.projects.iter().find(|p| p.project_name == name);
        let (network_isolation, enforce_quotas, quota_status) = tenant
            .map(|t| {
                (
                    t.network_isolation.clone(),
                    t.enforce_quotas,
                    t.quota_status.clone(),
                )
            })
            .unwrap_or(("shared".into(), false, "not enforced".into()));
        spaces.push(FleetSpaceItem {
            name,
            vm_count,
            running_count,
            stopped_count,
            host_count,
            network_isolation,
            enforce_quotas,
            quota_status,
        });
    }

    let space_count = spaces.len();
    Ok(FleetSpacesOverview {
        summary: format!(
            "{space_count} workspace space(s) · {running_vms}/{total_vms} VM(s) running"
        ),
        space_count,
        total_vms,
        running_vms,
        spaces,
    })
}
