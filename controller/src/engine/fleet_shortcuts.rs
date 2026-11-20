// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Fleet Shortcuts / blueprint Launchpad rollup (Phase 46).

use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct FleetShortcutItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub actions: Vec<String>,
    pub vm_count: usize,
    pub action_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetShortcutsOverview {
    pub summary: String,
    pub blueprint_count: usize,
    pub total_vms_covered: usize,
    pub runbook_count: usize,
    pub executions_24h: i64,
    pub shortcuts: Vec<FleetShortcutItem>,
}

pub async fn overview(pool: &DbPool) -> anyhow::Result<FleetShortcutsOverview> {
    let rows: Vec<(
        Uuid,
        String,
        String,
        serde_json::Value,
        crate::db::Json<Vec<Uuid>>,
    )> = crate::db::query_as(
        "SELECT id, name, description, actions, vm_ids FROM blueprints ORDER BY name",
    )
    .fetch_all(pool)
    .await?;

    let runbook_count: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM ops_runbook_catalog WHERE enabled = true")
            .fetch_one(pool)
            .await
            .unwrap_or(0);

    let executions_24h: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM ops_runbook_executions WHERE created_at > datetime('now', '-24 hours')",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let mut total_vms_covered = 0usize;
    let mut shortcuts = Vec::new();

    for (id, name, description, actions_json, vm_ids) in rows {
        let actions: Vec<String> = serde_json::from_value(actions_json).unwrap_or_default();
        let vm_count = vm_ids.len();
        total_vms_covered += vm_count;
        shortcuts.push(FleetShortcutItem {
            id: id.to_string(),
            name,
            description,
            action_count: actions.len(),
            actions,
            vm_count,
        });
    }

    let blueprint_count = shortcuts.len();
    Ok(FleetShortcutsOverview {
        summary: format!(
            "{blueprint_count} shortcut(s) · {total_vms_covered} VM(s) covered · {runbook_count} runbook(s) · {executions_24h} run(s) in 24h"
        ),
        blueprint_count,
        total_vms_covered,
        runbook_count: runbook_count as usize,
        executions_24h,
        shortcuts,
    })
}
