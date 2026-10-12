// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

pub mod elastic_bulk;
pub mod qradar_rest;
pub mod splunk_hec;
pub mod webhook_json;

use crate::db::DbPool;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow)]
pub struct IntegrationRow {
    pub id: Uuid,
    pub integration_type: String,
    pub name: String,
    pub enabled: bool,
    pub config_json: Value,
}

pub async fn forward_all_integrations(pool: &DbPool, controller_id: &str) -> anyhow::Result<usize> {
    let rows: Vec<IntegrationRow> = crate::db::query_as(
        "SELECT id, integration_type, name, enabled, config_json FROM soc_integrations WHERE enabled = TRUE",
    )
    .fetch_all(pool)
    .await?;

    let mut total = 0usize;
    for row in rows {
        let n = match row.integration_type.as_str() {
            "splunk_hec" => splunk_hec::forward(pool, &row, controller_id).await?,
            "elastic_bulk" => elastic_bulk::forward(pool, &row, controller_id).await?,
            "webhook_json" => webhook_json::forward(pool, &row, controller_id).await?,
            "qradar_rest" => qradar_rest::forward(pool, &row, controller_id).await?,
            "sentinel_dcr" => {
                tracing::warn!(
                    "soc integration `sentinel_dcr` was removed (local-first); \
                     point a `webhook_json` or `elastic_bulk` integration at a local collector instead"
                );
                0
            }
            other => {
                tracing::warn!("unknown soc integration type: {other}");
                0
            }
        };
        total += n;
    }
    Ok(total)
}

pub async fn forward_replay(
    pool: &DbPool,
    hours: i32,
    controller_id: &str,
) -> anyhow::Result<usize> {
    let since = Utc::now() - chrono::Duration::hours(hours as i64);
    let _ = crate::db::query("DELETE FROM soc_event_exports WHERE exported_at < ?")
        .bind(since)
        .execute(pool)
        .await;
    forward_all_integrations(pool, controller_id).await
}

pub(crate) async fn fetch_unexported_events(
    pool: &DbPool,
    integration_id: Uuid,
    limit: i64,
) -> anyhow::Result<Vec<EventRow>> {
    let rows: Vec<EventRow> = crate::db::query_as(
        "SELECT e.id, e.occurred_at, e.source, e.severity, e.summary, e.ecs_json
         FROM soc_events e
         WHERE NOT EXISTS (
           SELECT 1 FROM soc_event_exports x
           WHERE x.resource_id = e.id AND x.integration_id = ? AND x.resource_type = 'event'
         )
         ORDER BY e.occurred_at ASC
         LIMIT ?",
    )
    .bind(integration_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub(crate) async fn mark_exported(
    pool: &DbPool,
    integration_id: Uuid,
    resource_type: &str,
    resource_ids: &[Uuid],
) -> anyhow::Result<()> {
    for id in resource_ids {
        crate::db::query(
            "INSERT INTO soc_event_exports (integration_id, resource_type, resource_id) VALUES (?, ?, ?) ON CONFLICT DO NOTHING",
        )
        .bind(integration_id)
        .bind(resource_type)
        .bind(id)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub(crate) async fn integration_ok(pool: &DbPool, id: Uuid) -> anyhow::Result<()> {
    crate::db::query(
        "UPDATE soc_integrations SET last_success_at = datetime('now'), last_error = NULL, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn integration_err(pool: &DbPool, id: Uuid, err: &str) -> anyhow::Result<()> {
    crate::db::query(
        "UPDATE soc_integrations SET last_error = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(err.chars().take(2000).collect::<String>())
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct EventRow {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub source: String,
    pub severity: String,
    pub summary: String,
    pub ecs_json: Value,
}

pub(crate) async fn fetch_unexported_alerts(
    pool: &DbPool,
    integration_id: Uuid,
    limit: i64,
) -> anyhow::Result<Vec<AlertRow>> {
    let rows: Vec<AlertRow> = crate::db::query_as(
        "SELECT a.id, a.title, a.severity, a.status, strftime('%Y-%m-%dT%H:%M:%SZ', a.first_seen) AS first_seen, strftime('%Y-%m-%dT%H:%M:%SZ', a.last_seen) AS last_seen, a.detail_json
         FROM soc_alerts a
         WHERE a.last_seen > datetime('now', '-7 days')
         AND NOT EXISTS (
           SELECT 1 FROM soc_event_exports x
           WHERE x.resource_id = a.id AND x.integration_id = ? AND x.resource_type = 'alert'
         )
         ORDER BY a.last_seen ASC
         LIMIT ?",
    )
    .bind(integration_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)]
pub(crate) struct AlertRow {
    pub id: Uuid,
    pub title: String,
    pub severity: String,
    pub status: String,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub detail_json: Value,
}
