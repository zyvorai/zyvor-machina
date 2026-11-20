// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use crate::db::DbPool;

#[derive(Debug, Deserialize)]
pub struct ServiceImpactQuery {
    pub service: String,
}

#[derive(Debug, Serialize)]
pub struct ServiceImpactResult {
    pub service: String,
    pub severity: String,
    pub affected_vms: Vec<String>,
    pub affected_hosts: Vec<String>,
    pub blast_radius: usize,
    pub summary: String,
    pub recommendations: Vec<String>,
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub async fn simulate(
    pool: &DbPool,
    q: &ServiceImpactQuery,
) -> anyhow::Result<ServiceImpactResult> {
    let name = q.service.trim();
    let pattern = format!("%{}%", escape_like(name));

    let group_id: Option<uuid::Uuid> = crate::db::query_scalar(
        "SELECT id FROM application_groups WHERE name LIKE ? ESCAPE '\\' LIMIT 1",
    )
    .bind(&pattern)
    .fetch_optional(pool)
    .await?;

    let mut affected_vms = Vec::new();
    let mut affected_hosts = Vec::new();

    if let Some(gid) = group_id {
        let rows: Vec<(String, Option<String>)> = crate::db::query_as(
            "SELECT v.name, h.hostname FROM application_group_vms agv
             JOIN vms v ON v.id = agv.vm_id
             LEFT JOIN hosts h ON h.id = v.host_id
             WHERE agv.group_id = ?",
        )
        .bind(gid)
        .fetch_all(pool)
        .await?;

        for (vm, host) in rows {
            affected_vms.push(vm);
            if let Some(h) = host {
                if !affected_hosts.contains(&h) {
                    affected_hosts.push(h);
                }
            }
        }
    } else {
        let vms: Vec<String> =
            crate::db::query_scalar("SELECT name FROM vms WHERE name LIKE ? ESCAPE '\\' LIMIT 12")
                .bind(&pattern)
                .fetch_all(pool)
                .await
                .unwrap_or_default();
        affected_vms = vms;
    }

    let blast_radius = affected_vms.len() + affected_hosts.len();
    let severity = if affected_vms.len() >= 5 {
        "critical"
    } else if affected_vms.is_empty() {
        "low"
    } else {
        "high"
    };

    let summary = if affected_vms.is_empty() {
        format!("No VMs found for service '{name}' — check application group name.")
    } else {
        format!(
            "If service '{name}' fails, {} VM(s) on {} host(s) are in blast radius.",
            affected_vms.len(),
            affected_hosts.len().max(1)
        )
    };

    Ok(ServiceImpactResult {
        service: name.into(),
        severity: severity.into(),
        affected_vms,
        affected_hosts,
        blast_radius,
        summary,
        recommendations: vec![
            "Verify HA policies on production VMs in this service.".into(),
            "Run backup coverage check in Compliance frameworks.".into(),
            "Use Digital Twin to simulate host evacuation for this tier.".into(),
        ],
    })
}
