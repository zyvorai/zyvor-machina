// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Fleet Console.app unified log tail (Phase 42).

use chrono::{DateTime, Utc};
use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Clone, Serialize)]
pub struct FleetConsoleEntry {
    pub source: String,
    pub id: String,
    pub severity: String,
    pub actor: Option<String>,
    pub action: String,
    pub message: String,
    pub resource_type: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetConsoleOverview {
    pub summary: String,
    pub total_24h: i64,
    pub audit_24h: i64,
    pub events_24h: i64,
    pub tasks_failed_24h: i64,
    pub audit_count: i64,
    pub event_count: i64,
    pub task_count: i64,
    pub entries: Vec<FleetConsoleEntry>,
}

pub async fn overview(pool: &DbPool) -> anyhow::Result<FleetConsoleOverview> {
    let audit_24h: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM audit_logs WHERE created_at > datetime('now', '-24 hours')",
    )
    .fetch_one(pool)
    .await?;

    let events_24h: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM events WHERE created_at > datetime('now', '-24 hours')",
    )
    .fetch_one(pool)
    .await?;

    let tasks_failed_24h: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE status = 'failed' AND created_at > datetime('now', '-24 hours')",
    )
    .fetch_one(pool)
    .await?;

    let total_24h = audit_24h + events_24h + tasks_failed_24h;

    let audit_count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM audit_logs")
        .fetch_one(pool)
        .await?;
    let event_count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(pool)
        .await?;
    let task_count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM tasks")
        .fetch_one(pool)
        .await?;

    let rows: Vec<(String, String, String, Option<String>, String, String, Option<String>, DateTime<Utc>)> =
        crate::db::query_as(
            r#"
            SELECT source, id, severity, actor, action, message, resource_type, created_at FROM (
                SELECT
                    'audit' AS source,
                    lower(hex(id)) AS id,
                    CASE
                        WHEN action LIKE '%fail%' OR action LIKE '%delete%' OR action LIKE '%fence%' THEN 'warn'
                        ELSE 'info'
                    END AS severity,
                    actor,
                    action,
                    COALESCE(action || COALESCE(' · ' || resource_type, ''), action) AS message,
                    resource_type,
                    strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
                FROM audit_logs
                UNION ALL
                SELECT
                    'event',
                    lower(hex(id)),
                    CASE
                        WHEN kind LIKE '%fail%' OR kind LIKE '%error%' THEN 'error'
                        WHEN kind LIKE '%warn%' THEN 'warn'
                        ELSE 'info'
                    END,
                    NULL,
                    kind,
                    message,
                    resource_type,
                    strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
                FROM events
                UNION ALL
                SELECT
                    'task',
                    lower(hex(id)),
                    CASE WHEN status = 'failed' THEN 'error' ELSE 'info' END,
                    NULL,
                    operation,
                    COALESCE(NULLIF(message, ''), operation || ' — ' || status),
                    resource_type,
                    strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
                FROM tasks
            ) merged
            ORDER BY created_at DESC
            LIMIT 120
            "#,
        )
        .fetch_all(pool)
        .await?;

    let entries: Vec<FleetConsoleEntry> = rows
        .into_iter()
        .map(
            |(source, id, severity, actor, action, message, resource_type, created_at)| {
                FleetConsoleEntry {
                    source,
                    id,
                    severity,
                    actor,
                    action,
                    message,
                    resource_type,
                    created_at,
                }
            },
        )
        .collect();

    Ok(FleetConsoleOverview {
        summary: format!(
            "{total_24h} event(s) in 24h · {audit_24h} audit · {events_24h} platform · {tasks_failed_24h} failed task(s)"
        ),
        total_24h,
        audit_24h,
        events_24h,
        tasks_failed_24h,
        audit_count,
        event_count,
        task_count,
        entries,
    })
}
