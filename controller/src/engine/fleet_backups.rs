// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Fleet Time Machine rollup (Phase 38).

use chrono::{DateTime, Utc};
use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct FleetBackupEvent {
    pub kind: String,
    pub id: String,
    pub vm_id: String,
    pub vm_name: String,
    pub label: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetBackupOverview {
    pub summary: String,
    pub total_events: i64,
    pub backups_completed_24h: i64,
    pub backups_failed_24h: i64,
    pub snapshots_total: i64,
    pub vms_with_backup_7d: i64,
    pub recent: Vec<FleetBackupEvent>,
}

pub async fn overview(pool: &DbPool) -> anyhow::Result<FleetBackupOverview> {
    let backups_completed_24h: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM backup_records WHERE status = 'completed' AND created_at > datetime('now', '-24 hours')",
    )
    .fetch_one(pool)
    .await?;

    let backups_failed_24h: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM backup_records WHERE status = 'failed' AND created_at > datetime('now', '-24 hours')",
    )
    .fetch_one(pool)
    .await?;

    let snapshots_total: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM snapshot_records")
        .fetch_one(pool)
        .await?;

    let vms_with_backup_7d: i64 = crate::db::query_scalar(
        "SELECT COUNT(DISTINCT vm_id) FROM backup_records WHERE status = 'completed' AND created_at > datetime('now', '-7 days')",
    )
    .fetch_one(pool)
    .await?;

    let total_events: i64 = crate::db::query_scalar(
        r#"
        SELECT (
            (SELECT COUNT(*) FROM backup_records) +
            (SELECT COUNT(*) FROM snapshot_records)
        )
        "#,
    )
    .fetch_one(pool)
    .await?;

    let rows: Vec<(String, Uuid, Uuid, String, String, String, DateTime<Utc>)> = crate::db::query_as(
        r#"
        SELECT kind, id, vm_id, vm_name, label, status, created_at FROM (
            SELECT 'backup' AS kind, b.id, b.vm_id, v.name AS vm_name,
                   CASE WHEN b.status = 'completed' THEN 'Backup successful'
                        ELSE 'Backup ' || b.status END AS label,
                   b.status, b.created_at
            FROM backup_records b
            JOIN vms v ON v.id = b.vm_id
            UNION ALL
            SELECT 'snapshot' AS kind, s.id, s.vm_id, v.name AS vm_name,
                   'Snapshot: ' || s.name AS label,
                   s.status, s.created_at
            FROM snapshot_records s
            JOIN vms v ON v.id = s.vm_id
        ) t
        ORDER BY created_at DESC
        LIMIT 40
        "#,
    )
    .fetch_all(pool)
    .await?;

    let recent: Vec<FleetBackupEvent> = rows
        .into_iter()
        .map(
            |(kind, id, vm_id, vm_name, label, status, created_at)| FleetBackupEvent {
                kind,
                id: id.to_string(),
                vm_id: vm_id.to_string(),
                vm_name,
                label,
                status,
                created_at,
            },
        )
        .collect();

    Ok(FleetBackupOverview {
        summary: format!(
            "{total_events} backup/snapshot event(s) · {backups_completed_24h} backup(s) today · {vms_with_backup_7d} VM(s) protected this week"
        ),
        total_events,
        backups_completed_24h,
        backups_failed_24h,
        snapshots_total,
        vms_with_backup_7d,
        recent,
    })
}
