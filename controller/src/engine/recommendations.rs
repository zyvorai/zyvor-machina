// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Clone, Serialize)]
pub struct Recommendation {
    pub id: String,
    pub impact: String,
    pub title: String,
    pub why: String,
    pub risk: String,
    pub action: String,
    pub fix_action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_ref: Option<serde_json::Value>,
}

pub async fn generate_recommendations(pool: &DbPool) -> anyhow::Result<Vec<Recommendation>> {
    let mut out = Vec::new();

    let no_backup: Vec<(uuid::Uuid, String)> = crate::db::query_as(
        "SELECT v.id, v.name FROM vms v
         WHERE COALESCE(v.managed, TRUE) = TRUE
           AND (EXISTS (SELECT 1 FROM json_each(COALESCE(v.tags,'[]')) WHERE value='prod') OR EXISTS (SELECT 1 FROM json_each(COALESCE(v.tags,'[]')) WHERE value='production'))
           AND NOT EXISTS (SELECT 1 FROM backup_records b WHERE b.vm_id = v.id AND b.status = 'completed')
         LIMIT 10",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    if !no_backup.is_empty() {
        out.push(Recommendation {
            id: "enable-backup-prod".into(),
            impact: "High Impact".into(),
            title: format!("Enable backup on {} production VM(s)", no_backup.len()),
            why: "Production VMs without a completed backup are at risk during host or storage failures.".into(),
            risk: "Low — backup tasks are non-destructive".into(),
            action: "Apply daily backup policy".into(),
            fix_action: "bulk_backup".into(),
            object_ref: Some(serde_json::json!({ "vm_ids": no_backup.iter().map(|(id, _)| id).collect::<Vec<_>>() })),
        });
    }

    let no_ha: Vec<(uuid::Uuid, String)> = crate::db::query_as(
        "SELECT v.id, v.name FROM vms v
         LEFT JOIN ha_policies hp ON hp.vm_id = v.id
         WHERE COALESCE(hp.enabled, FALSE) = FALSE
           AND v.observed_state = 'running'
           AND (EXISTS (SELECT 1 FROM json_each(COALESCE(v.tags,'[]')) WHERE value='prod') OR EXISTS (SELECT 1 FROM json_each(COALESCE(v.tags,'[]')) WHERE value='production'))
         LIMIT 10",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    if !no_ha.is_empty() {
        out.push(Recommendation {
            id: "enable-ha-critical".into(),
            impact: "Reliability".into(),
            title: format!("Enable HA on {} critical VM(s)", no_ha.len()),
            why: "Running production VMs without HA will not auto-restart after host failure."
                .into(),
            risk: "Low".into(),
            action: "Configure HA restart policy".into(),
            fix_action: "bulk_ha".into(),
            object_ref: Some(
                serde_json::json!({ "vm_ids": no_ha.iter().map(|(id, _)| id).collect::<Vec<_>>() }),
            ),
        });
    }

    let no_guest: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM vms WHERE observed_state = 'running'
         AND guest_tools_status IN ('unknown', 'not_installed')",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    if no_guest > 0 {
        out.push(Recommendation {
            id: "install-guest-tools".into(),
            impact: "High Impact".into(),
            title: format!("Install guest tools on {no_guest} running VM(s)"),
            why: "Guest tools enable graceful shutdown, IP reporting, and app-consistent backups."
                .into(),
            risk: "Low — install via cloud-init or package manager".into(),
            action: "Review VMs and install Zyvor Guest Tools".into(),
            fix_action: "open_vms".into(),
            object_ref: None,
        });
    }

    let offline: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM hosts WHERE state = 'offline'")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    if offline > 0 {
        out.push(Recommendation {
            id: "hosts-offline".into(),
            impact: "Reliability".into(),
            title: format!("{offline} host(s) offline — investigate connectivity"),
            why: "Offline hosts reduce capacity and may indicate agent or network issues.".into(),
            risk: "Medium".into(),
            action: "Open hosts dashboard".into(),
            fix_action: "open_hosts".into(),
            object_ref: None,
        });
    }

    Ok(out)
}
