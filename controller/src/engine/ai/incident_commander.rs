// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

use super::knowledge_runbook;
use super::root_cause::{self, AnalyzeIncidentQuery};

#[derive(Debug, Serialize)]
pub struct ActiveIncident {
    pub id: Uuid,
    pub title: String,
    pub summary: String,
    pub severity: String,
    pub status: String,
    pub affected_resources: Vec<String>,
    pub root_cause: Option<String>,
    pub created_at: DateTime<Utc>,
    pub window_start: Option<DateTime<Utc>>,
    pub window_end: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct IncidentRoom {
    pub incident: ActiveIncident,
    pub timeline: Vec<super::root_cause::TimelineEntry>,
    pub runbook_steps: Vec<String>,
    pub pending_approvals: i64,
    pub correlated_count: usize,
}

pub async fn list_active(pool: &DbPool) -> anyhow::Result<Vec<ActiveIncident>> {
    let rows: Vec<(
        Uuid,
        String,
        String,
        String,
        String,
        serde_json::Value,
        Option<String>,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    )> = crate::db::query_as(
        "SELECT id, title, summary, severity, status, affected_resources, root_cause,
                strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at,
                strftime('%Y-%m-%dT%H:%M:%SZ', window_start) AS window_start,
                strftime('%Y-%m-%dT%H:%M:%SZ', window_end) AS window_end
         FROM ai_incidents WHERE status IN ('open', 'investigating')
         ORDER BY created_at DESC LIMIT 20",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                title,
                summary,
                severity,
                status,
                resources,
                root_cause,
                created_at,
                window_start,
                window_end,
            )| {
                let affected_resources: Vec<String> = resources
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                ActiveIncident {
                    id,
                    title,
                    summary,
                    severity,
                    status,
                    affected_resources,
                    root_cause,
                    created_at,
                    window_start,
                    window_end,
                }
            },
        )
        .collect())
}

pub async fn open_room(pool: &DbPool, incident_id: Uuid) -> anyhow::Result<IncidentRoom> {
    let row: Option<(
        String,
        String,
        String,
        String,
        serde_json::Value,
        Option<String>,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    )> = crate::db::query_as(
        "SELECT title, summary, severity, status, affected_resources, root_cause,
                strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at,
                strftime('%Y-%m-%dT%H:%M:%SZ', window_start) AS window_start,
                strftime('%Y-%m-%dT%H:%M:%SZ', window_end) AS window_end
         FROM ai_incidents WHERE id = ?",
    )
    .bind(incident_id)
    .fetch_optional(pool)
    .await?;

    let Some((
        title,
        summary,
        severity,
        status,
        resources,
        root_cause,
        created_at,
        window_start,
        window_end,
    )) = row
    else {
        anyhow::bail!("Incident not found");
    };

    let affected_resources: Vec<String> = resources
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let rca = root_cause::analyze(
        pool,
        &AnalyzeIncidentQuery {
            hours: 4,
            vm_id: None,
            vm_name: None,
        },
    )
    .await?;

    let runbook = match knowledge_runbook::from_query(pool, &title).await {
        Ok(r) => r,
        Err(_) => super::knowledge_runbook::KnowledgeRunbook {
            query: title.clone(),
            diagnosis_summary: String::new(),
            runbook_title: title.clone(),
            steps: vec!["Review timeline".into(), "Verify agent connectivity".into()],
            commands: vec![],
            summary: String::new(),
        },
    };

    let pending: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM ai_actions WHERE status = 'pending'")
            .fetch_one(pool)
            .await
            .unwrap_or(0);

    Ok(IncidentRoom {
        incident: ActiveIncident {
            id: incident_id,
            title,
            summary,
            severity,
            status,
            affected_resources: affected_resources.clone(),
            root_cause,
            created_at,
            window_start,
            window_end,
        },
        timeline: rca.timeline,
        runbook_steps: runbook.steps,
        pending_approvals: pending,
        correlated_count: affected_resources.len().max(rca.contributing_factors.len()),
    })
}

pub async fn ack(pool: &DbPool, incident_id: Uuid) -> anyhow::Result<()> {
    crate::db::query(
        "UPDATE ai_incidents SET status = 'investigating', updated_at = datetime('now') WHERE id = ?",
    )
    .bind(incident_id)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct CreateIncidentRequest {
    pub title: String,
    pub summary: String,
    pub severity: String,
    pub affected_resources: Vec<String>,
    pub root_cause: Option<String>,
    pub window_start: Option<DateTime<Utc>>,
    pub window_end: Option<DateTime<Utc>>,
}

pub async fn create(pool: &DbPool, req: &CreateIncidentRequest) -> anyhow::Result<Uuid> {
    let resources = serde_json::json!(req.affected_resources);
    let id: Uuid = crate::db::query_scalar(
        "INSERT INTO ai_incidents (id, title, summary, severity, status, affected_resources, root_cause, window_start, window_end) VALUES (?, ?, ?, ?, 'open', ?, ?, ?, ?) RETURNING id",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(&req.title)
    .bind(&req.summary)
    .bind(&req.severity)
    .bind(resources)
    .bind(&req.root_cause)
    .bind(req.window_start)
    .bind(req.window_end)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// Correlate recent failures into a new incident if none open.
pub async fn correlate_and_open(pool: &DbPool) -> anyhow::Result<Option<Uuid>> {
    let active = list_active(pool).await?;
    if !active.is_empty() {
        return Ok(None);
    }

    let failed_events: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM events WHERE created_at > datetime('now', '-1 hour')
         AND (kind LIKE '%fail%' OR kind LIKE '%error%')",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let failed_tasks: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE status = 'failed' AND created_at > datetime('now', '-1 hour')",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    if failed_events + failed_tasks < 2 {
        return Ok(None);
    }

    let rca = root_cause::analyze(
        pool,
        &AnalyzeIncidentQuery {
            hours: 1,
            vm_id: None,
            vm_name: None,
        },
    )
    .await?;

    let resources: Vec<String> = rca
        .timeline
        .iter()
        .take(5)
        .map(|e| e.message.clone())
        .collect();

    create(
        pool,
        &CreateIncidentRequest {
            title: "Correlated infrastructure incident".into(),
            summary: rca.root_cause.clone(),
            severity: if rca.confidence >= 0.7 {
                "high".into()
            } else {
                "medium".into()
            },
            affected_resources: resources,
            root_cause: Some(rca.root_cause),
            window_start: Some(Utc::now() - chrono::Duration::hours(1)),
            window_end: Some(Utc::now()),
        },
    )
    .await
    .map(Some)
}
