// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySettings {
    pub enabled: bool,
    pub team_scope: bool,
    pub project_scope: bool,
    pub retention_days: i32,
}

pub async fn get_settings(pool: &DbPool) -> anyhow::Result<MemorySettings> {
    let row: Option<(bool, bool, bool, i32)> = crate::db::query_as(
        "SELECT COALESCE(zeus_memory_enabled, TRUE), COALESCE(zeus_memory_team_scope, FALSE),
                COALESCE(zeus_memory_project_scope, TRUE), COALESCE(zeus_memory_retention_days, 90)
         FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    let row = row.unwrap_or((true, false, true, 90));
    Ok(MemorySettings {
        enabled: row.0,
        team_scope: row.1,
        project_scope: row.2,
        retention_days: row.3,
    })
}

#[derive(Debug, Deserialize)]
pub struct MemorySettingsPatch {
    pub enabled: Option<bool>,
    pub team_scope: Option<bool>,
    pub project_scope: Option<bool>,
    pub retention_days: Option<i32>,
}

pub async fn patch_settings(
    pool: &DbPool,
    patch: &MemorySettingsPatch,
) -> anyhow::Result<MemorySettings> {
    let mut tx = pool.begin().await?;
    if let Some(v) = patch.enabled {
        crate::db::query("UPDATE clusters SET zeus_memory_enabled = ?")
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = patch.team_scope {
        crate::db::query("UPDATE clusters SET zeus_memory_team_scope = ?")
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = patch.project_scope {
        crate::db::query("UPDATE clusters SET zeus_memory_project_scope = ?")
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = patch.retention_days {
        crate::db::query("UPDATE clusters SET zeus_memory_retention_days = ?")
            .bind(v.clamp(1, 3650))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    get_settings(pool).await
}

pub async fn recall_for_user(
    pool: &DbPool,
    user_id: Option<&str>,
    limit: i64,
) -> anyhow::Result<Vec<String>> {
    let settings = get_settings(pool).await?;
    if !settings.enabled {
        return Ok(vec![]);
    }
    let cap = limit.clamp(1, 20);
    let uid = user_id.unwrap_or("");
    // Memory entries are per-owner private data (chat history summaries, etc).
    // With no identified caller there is no owner to scope by — return nothing
    // rather than falling through to an unscoped query that would hand back
    // every user's memories as "recalled" context for whoever is asking.
    if uid.is_empty() {
        return Ok(vec![]);
    }
    let rows: Vec<String> = crate::db::query_scalar(
        "SELECT summary FROM ai_memory_entries
         WHERE owner_id = ? AND (expires_at IS NULL OR expires_at > datetime('now'))
         ORDER BY created_at DESC LIMIT ?",
    )
    .bind(uid)
    .bind(cap)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn remember(
    pool: &DbPool,
    owner_id: &str,
    subject_kind: &str,
    subject_id: &str,
    summary: &str,
    project_id: Option<&str>,
) -> anyhow::Result<Uuid> {
    let settings = get_settings(pool).await?;
    if !settings.enabled {
        return Ok(Uuid::nil());
    }
    let retention = settings.retention_days.max(1) as i64;
    let id: Uuid = crate::db::query_scalar(
        "INSERT INTO ai_memory_entries (id, scope, owner_id, project_id, subject_kind, subject_id, summary, expires_at) VALUES (?, 'user', ?, ?, ?, ?, ?, datetime('now', ? || ' days'))
         RETURNING id",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(owner_id)
    .bind(project_id.unwrap_or(""))
    .bind(subject_kind)
    .bind(subject_id)
    .bind(summary)
    .bind(retention.to_string())
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn purge(pool: &DbPool, scope: &str, owner_id: Option<&str>) -> anyhow::Result<u64> {
    let deleted = if scope == "all" {
        crate::db::query("DELETE FROM ai_memory_entries")
            .execute(pool)
            .await?
            .rows_affected()
    } else if let Some(uid) = owner_id {
        crate::db::query("DELETE FROM ai_memory_entries WHERE owner_id = ?")
            .bind(uid)
            .execute(pool)
            .await?
            .rows_affected()
    } else {
        0
    };
    Ok(deleted)
}

#[derive(Debug, Serialize)]
pub struct ConversationRow {
    pub id: Uuid,
    pub agent_id: String,
    pub summary: String,
    pub updated_at: DateTime<Utc>,
}

pub async fn list_conversations(
    pool: &DbPool,
    user_id: &str,
) -> anyhow::Result<Vec<ConversationRow>> {
    let rows: Vec<(Uuid, String, String, DateTime<Utc>)> = crate::db::query_as(
        "SELECT id, agent_id, summary, strftime('%Y-%m-%dT%H:%M:%SZ', updated_at) AS updated_at FROM ai_conversations WHERE user_id = ? ORDER BY updated_at DESC LIMIT 50",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, agent_id, summary, updated_at)| ConversationRow {
            id,
            agent_id,
            summary,
            updated_at,
        })
        .collect())
}

pub async fn upsert_conversation_summary(
    pool: &DbPool,
    user_id: &str,
    agent_id: &str,
    summary: &str,
) -> anyhow::Result<()> {
    crate::db::query(
        "INSERT INTO ai_conversations (id, user_id, agent_id, summary, updated_at)
         VALUES (?, ?, ?, ?, datetime('now'))
         ON CONFLICT (user_id, agent_id) DO UPDATE SET
             summary = excluded.summary,
             updated_at = excluded.updated_at",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(agent_id)
    .bind(summary)
    .execute(pool)
    .await?;
    Ok(())
}
