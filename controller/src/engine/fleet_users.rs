// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Fleet Users & Groups rollup (Phase 45).

use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

use crate::engine::enterprise_security;

#[derive(Debug, Clone, Serialize)]
pub struct FleetUserItem {
    pub id: String,
    pub username: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetWorkspaceItem {
    pub name: String,
    pub vm_count: i64,
    pub network_isolation: String,
    pub enforce_quotas: bool,
    pub quota_status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetUsersOverview {
    pub summary: String,
    pub user_count: usize,
    pub admin_count: usize,
    pub operator_count: usize,
    pub viewer_count: usize,
    pub workspace_count: usize,
    pub workspaces_enforced: usize,
    pub users: Vec<FleetUserItem>,
    pub workspaces: Vec<FleetWorkspaceItem>,
}

pub async fn overview(pool: &DbPool) -> anyhow::Result<FleetUsersOverview> {
    let user_rows: Vec<(Uuid, String, String)> =
        crate::db::query_as("SELECT id, username, role FROM users ORDER BY username LIMIT 32")
            .fetch_all(pool)
            .await?;

    let role_counts: Vec<(String, i64)> =
        crate::db::query_as("SELECT role, COUNT(*) FROM users GROUP BY role ORDER BY role")
            .fetch_all(pool)
            .await?;

    let mut admin_count = 0usize;
    let mut operator_count = 0usize;
    let mut viewer_count = 0usize;
    for (role, count) in &role_counts {
        match role.as_str() {
            "admin" => admin_count = *count as usize,
            "operator" => operator_count = *count as usize,
            "viewer" => viewer_count = *count as usize,
            _ => {}
        }
    }

    let tenants = enterprise_security::tenant_isolation_overview(pool).await?;
    let workspaces: Vec<FleetWorkspaceItem> = tenants
        .projects
        .into_iter()
        .map(|p| FleetWorkspaceItem {
            name: p.project_name,
            vm_count: p.vm_count,
            network_isolation: p.network_isolation,
            enforce_quotas: p.enforce_quotas,
            quota_status: p.quota_status,
        })
        .collect();

    let users: Vec<FleetUserItem> = user_rows
        .into_iter()
        .map(|(id, username, role)| FleetUserItem {
            id: id.to_string(),
            username,
            role,
        })
        .collect();

    let user_count = users.len();
    let workspace_count = workspaces.len();
    let workspaces_enforced = tenants.enforced_count;

    Ok(FleetUsersOverview {
        summary: format!(
            "{user_count} user(s) · {admin_count} admin · {workspace_count} workspace(s) · {workspaces_enforced} enforced"
        ),
        user_count,
        admin_count,
        operator_count,
        viewer_count,
        workspace_count,
        workspaces_enforced,
        users,
        workspaces,
    })
}
