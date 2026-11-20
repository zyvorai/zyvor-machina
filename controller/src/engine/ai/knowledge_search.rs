// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Serialize)]
pub struct KnowledgeHit {
    pub kind: String,
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub navigate: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct KnowledgeSearchResult {
    pub query: String,
    pub hits: Vec<KnowledgeHit>,
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub async fn search(pool: &DbPool, query: &str) -> anyhow::Result<KnowledgeSearchResult> {
    let q = query.trim();
    let mut hits = Vec::new();
    if q.is_empty() {
        return Ok(KnowledgeSearchResult {
            query: q.into(),
            hits,
        });
    }

    let pattern = format!("%{}%", escape_like(q));

    let vms: Vec<(uuid::Uuid, String, String)> = crate::db::query_as(
        "SELECT id, name, observed_state FROM vms WHERE name LIKE ? ESCAPE '\\' OR EXISTS (SELECT 1 FROM json_each(COALESCE(tags,'[]')) WHERE value = ?) LIMIT 12",
    )
    .bind(&pattern)
    .bind(q)
    .fetch_all(pool)
    .await?;
    for (id, name, state) in vms {
        hits.push(KnowledgeHit {
            kind: "vm".into(),
            id: id.to_string(),
            title: name.clone(),
            snippet: format!("VM · {state}"),
            score: 1.0,
            navigate: Some(format!("/platform/vms/{id}")),
        });
    }

    let hosts: Vec<(uuid::Uuid, String, String)> = crate::db::query_as(
        "SELECT id, hostname, state FROM hosts WHERE hostname LIKE ? ESCAPE '\\' LIMIT 8",
    )
    .bind(&pattern)
    .fetch_all(pool)
    .await?;
    for (id, name, state) in hosts {
        hits.push(KnowledgeHit {
            kind: "host".into(),
            id: id.to_string(),
            title: name,
            snippet: format!("Host · {state}"),
            score: 0.95,
            navigate: Some(format!("/platform/hosts/{id}")),
        });
    }

    let apps: Vec<(uuid::Uuid, String)> = crate::db::query_as(
        "SELECT id, name FROM application_groups WHERE name LIKE ? ESCAPE '\\' OR description LIKE ? ESCAPE '\\' LIMIT 8",
    )
    .bind(&pattern)
    .bind(&pattern)
    .fetch_all(pool)
    .await?;
    for (id, name) in apps {
        hits.push(KnowledgeHit {
            kind: "application".into(),
            id: id.to_string(),
            title: name,
            snippet: "Application group".into(),
            score: 0.9,
            navigate: Some("/platform/applications".into()),
        });
    }

    let tasks: Vec<(uuid::Uuid, String, String)> = crate::db::query_as(
        "SELECT id, operation, status FROM tasks WHERE operation LIKE ? ESCAPE '\\' OR status LIKE ? ESCAPE '\\' ORDER BY created_at DESC LIMIT 8",
    )
    .bind(&pattern)
    .bind(&pattern)
    .fetch_all(pool)
    .await?;
    for (id, op, status) in tasks {
        hits.push(KnowledgeHit {
            kind: "task".into(),
            id: id.to_string(),
            title: op,
            snippet: format!("Task · {status}"),
            score: 0.85,
            navigate: Some("/platform/tasks".into()),
        });
    }

    let events: Vec<(String, String)> = crate::db::query_as(
        "SELECT kind, message FROM events WHERE kind LIKE ? ESCAPE '\\' OR message LIKE ? ESCAPE '\\' ORDER BY created_at DESC LIMIT 8",
    )
    .bind(&pattern)
    .bind(&pattern)
    .fetch_all(pool)
    .await?;
    for (kind, message) in events {
        hits.push(KnowledgeHit {
            kind: "event".into(),
            id: kind.clone(),
            title: kind,
            snippet: message.chars().take(120).collect(),
            score: 0.8,
            navigate: Some("/platform/events".into()),
        });
    }

    let audits: Vec<(String, String)> = crate::db::query_as(
        "SELECT action, actor FROM audit_logs WHERE action LIKE ? ESCAPE '\\' OR actor LIKE ? ESCAPE '\\' ORDER BY created_at DESC LIMIT 6",
    )
    .bind(&pattern)
    .bind(&pattern)
    .fetch_all(pool)
    .await?;
    for (action, actor) in audits {
        hits.push(KnowledgeHit {
            kind: "audit".into(),
            id: action.clone(),
            title: action,
            snippet: format!("by {actor}"),
            score: 0.75,
            navigate: Some("/platform/events".into()),
        });
    }

    if q.to_lowercase().contains("payment") || q.to_lowercase().contains("billing") {
        hits.push(KnowledgeHit {
            kind: "insight".into(),
            id: "billing-slow".into(),
            title: "Billing latency correlation".into(),
            snippet: "Check VM metrics, failed tasks, and network path to billing DB.".into(),
            score: 0.7,
            navigate: Some("/platform/reports".into()),
        });
    }

    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.truncate(30);

    Ok(KnowledgeSearchResult {
        query: q.into(),
        hits,
    })
}
