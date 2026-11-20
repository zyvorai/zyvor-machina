// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct PromptRow {
    pub id: Uuid,
    pub scope: String,
    pub owner_id: String,
    pub team_id: String,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub agent_id: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePromptBody {
    pub scope: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub team_id: String,
}

#[derive(Debug, Deserialize)]
pub struct PatchPromptBody {
    pub title: Option<String>,
    pub body: Option<String>,
    pub tags: Option<Vec<String>>,
    pub agent_id: Option<String>,
}

pub async fn list_prompts(pool: &DbPool, user_id: &str) -> anyhow::Result<Vec<PromptRow>> {
    let rows: Vec<(
        Uuid,
        String,
        String,
        String,
        String,
        String,
        serde_json::Value,
        String,
        DateTime<Utc>,
    )> = crate::db::query_as(
        "SELECT id, scope, owner_id, team_id, title, body, tags, agent_id,
                strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
             FROM ai_prompts
             WHERE scope = 'org' OR owner_id = ? OR (scope = 'team' AND team_id <> '')
             ORDER BY created_at DESC LIMIT 200",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(map_row).collect())
}

fn map_row(
    (id, scope, owner_id, team_id, title, body, tags, agent_id, created_at): (
        Uuid,
        String,
        String,
        String,
        String,
        String,
        serde_json::Value,
        String,
        DateTime<Utc>,
    ),
) -> PromptRow {
    PromptRow {
        id,
        scope,
        owner_id,
        team_id,
        title,
        body,
        tags: serde_json::from_value(tags).unwrap_or_default(),
        agent_id,
        created_at,
    }
}

pub async fn create_prompt(
    pool: &DbPool,
    user_id: &str,
    body: &CreatePromptBody,
) -> anyhow::Result<PromptRow> {
    let scope = if body.scope.trim().is_empty() {
        "personal"
    } else {
        body.scope.trim()
    };
    let agent_id = if body.agent_id.trim().is_empty() {
        "auto"
    } else {
        body.agent_id.trim()
    };
    let tags = serde_json::to_value(&body.tags)?;
    let id: Uuid = crate::db::query_scalar(
        "INSERT INTO ai_prompts (id, scope, owner_id, team_id, title, body, tags, agent_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(scope)
    .bind(user_id)
    .bind(body.team_id.trim())
    .bind(body.title.trim())
    .bind(body.body.trim())
    .bind(tags)
    .bind(agent_id)
    .fetch_one(pool)
    .await?;
    get_prompt(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("prompt missing"))
}

pub async fn get_prompt(pool: &DbPool, id: Uuid) -> anyhow::Result<Option<PromptRow>> {
    let row: Option<(Uuid, String, String, String, String, String, serde_json::Value, String, DateTime<Utc>)> =
        crate::db::query_as(
            "SELECT id, scope, owner_id, team_id, title, body, tags, agent_id, strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at FROM ai_prompts WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_row))
}

/// Returns true if `actor` (username, role) may modify/delete the given prompt:
/// the owner, or an admin. Org/team-shared prompts are still owner-scoped for
/// writes — sharing only affects visibility via `list_prompts`.
async fn can_modify(pool: &DbPool, id: Uuid, actor: &str, role: &str) -> anyhow::Result<bool> {
    if role == "admin" {
        return Ok(true);
    }
    let owner_id: Option<String> =
        crate::db::query_scalar("SELECT owner_id FROM ai_prompts WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(owner_id.as_deref() == Some(actor))
}

pub async fn patch_prompt(
    pool: &DbPool,
    id: Uuid,
    actor: &str,
    role: &str,
    body: &PatchPromptBody,
) -> anyhow::Result<PromptRow> {
    if !can_modify(pool, id, actor, role).await? {
        anyhow::bail!("prompt not found or not owned by you");
    }
    let mut tx = pool.begin().await?;
    if let Some(v) = &body.title {
        crate::db::query("UPDATE ai_prompts SET title = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(v)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &body.body {
        crate::db::query("UPDATE ai_prompts SET body = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(v)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &body.tags {
        crate::db::query("UPDATE ai_prompts SET tags = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(serde_json::to_value(v)?)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &body.agent_id {
        crate::db::query(
            "UPDATE ai_prompts SET agent_id = ?, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(v)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    get_prompt(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("prompt not found"))
}

pub async fn delete_prompt(
    pool: &DbPool,
    id: Uuid,
    actor: &str,
    role: &str,
) -> anyhow::Result<bool> {
    if !can_modify(pool, id, actor, role).await? {
        return Ok(false);
    }
    let r = crate::db::query("DELETE FROM ai_prompts WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}
