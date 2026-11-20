// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Serialize)]
pub struct StackVmStatus {
    pub name: String,
    pub observed_state: String,
    pub host: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct MissionStackStatus {
    pub gpu_vms: Vec<StackVmStatus>,
    pub environment_vms: Vec<StackVmStatus>,
    pub total_stack_vms: usize,
    pub running: usize,
    pub summary: String,
}

pub async fn status(pool: &DbPool) -> anyhow::Result<MissionStackStatus> {
    let raw_rows: Vec<(String, String, Option<String>, String)> = crate::db::query_as(
        "SELECT v.name, v.observed_state, h.hostname, COALESCE(v.tags, '[]')
         FROM vms v
         LEFT JOIN hosts h ON h.id = v.host_id
         WHERE EXISTS (SELECT 1 FROM json_each(COALESCE(v.tags,'[]')) WHERE value IN ('mission-stack','gpu','environment'))
         ORDER BY v.name",
    )
    .fetch_all(pool)
    .await?;

    let mut gpu_vms = Vec::new();
    let mut environment_vms = Vec::new();
    let mut running = 0usize;

    for (name, state, host, tags_json) in raw_rows {
        let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
        if state == "running" {
            running += 1;
        }
        let row = StackVmStatus {
            name: name.clone(),
            observed_state: state,
            host,
            tags: tags.clone(),
        };
        if tags.iter().any(|t| t == "mission-stack" || t == "gpu") {
            gpu_vms.push(row);
        } else {
            environment_vms.push(row);
        }
    }

    let total_stack_vms = gpu_vms.len() + environment_vms.len();
    let summary = if total_stack_vms == 0 {
        "No mission-stack or environment VMs yet — run stack or environment execute.".into()
    } else {
        format!(
            "{total_stack_vms} stack VM(s) · {running} running · {} GPU · {} environment",
            gpu_vms.len(),
            environment_vms.len()
        )
    };

    Ok(MissionStackStatus {
        gpu_vms,
        environment_vms,
        total_stack_vms,
        running,
        summary,
    })
}
