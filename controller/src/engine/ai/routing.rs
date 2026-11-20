// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

use super::providers::{self, ResolvedProvider};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskClass {
    Infrastructure,
    CodeGeneration,
    SecurityAnalysis,
    Research,
    LongContext,
    FastLocal,
}

impl TaskClass {
    pub fn as_db_key(self) -> &'static str {
        match self {
            Self::Infrastructure => "infrastructure",
            Self::CodeGeneration => "code_generation",
            Self::SecurityAnalysis => "security_analysis",
            Self::Research => "research",
            Self::LongContext => "long_context",
            Self::FastLocal => "fast_local",
        }
    }

    pub fn from_agent(agent_id: &str) -> Self {
        match agent_id {
            "architect" => Self::Infrastructure,
            "devops" => Self::CodeGeneration,
            "kubernetes" => Self::Infrastructure,
            "security" => Self::SecurityAnalysis,
            "cost" => Self::Research,
            "observability" | "sre" => Self::LongContext,
            "ai_engineer" => Self::CodeGeneration,
            "database" => Self::LongContext,
            _ => Self::Infrastructure,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RoutingRequest {
    pub task_class: TaskClass,
    pub agent_id: Option<String>,
    pub user_id: Option<String>,
}

pub async fn resolve(
    pool: &DbPool,
    req: &RoutingRequest,
) -> anyhow::Result<Option<ResolvedProvider>> {
    let air_gap: bool = crate::db::query_scalar(
        "SELECT COALESCE(zeus_air_gap_llm, FALSE) FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false);

    let task_key = req.task_class.as_db_key();
    let rule: Option<(Option<Uuid>, Option<Uuid>)> = crate::db::query_as(
        "SELECT provider_id, model_id FROM ai_routing_rules WHERE task_class = ? AND enabled = TRUE",
    )
    .bind(task_key)
    .fetch_optional(pool)
    .await?;

    let mut resolved = if let Some((Some(pid), model_uuid)) = rule {
        let model_id = if let Some(mid) = model_uuid {
            crate::db::query_scalar::<_, String>("SELECT model_id FROM ai_models WHERE id = ?")
                .bind(mid)
                .fetch_optional(pool)
                .await?
        } else {
            None
        };
        providers::resolve_for_provider(pool, Some(pid), model_id.as_deref()).await?
    } else {
        providers::resolve_default(pool).await?
    };

    if air_gap {
        if let Some(ref mut r) = resolved {
            if !is_local_kind(&r.kind) {
                resolved = providers::resolve_local(pool).await?;
            }
        }
    }

    if req.task_class == TaskClass::FastLocal {
        if let Some(local) = providers::resolve_local(pool).await? {
            return Ok(Some(local));
        }
    }

    Ok(resolved)
}

fn is_local_kind(kind: &str) -> bool {
    matches!(kind, "ollama" | "vllm" | "openai_compatible")
}

#[derive(Debug, Clone, Serialize)]
pub struct RoutingRuleRow {
    pub task_class: String,
    pub provider_id: Option<Uuid>,
    pub model_id: Option<Uuid>,
    pub enabled: bool,
}

pub async fn list_rules(pool: &DbPool) -> anyhow::Result<Vec<RoutingRuleRow>> {
    let rows: Vec<(String, Option<Uuid>, Option<Uuid>, bool)> = crate::db::query_as(
        "SELECT task_class, provider_id, model_id, enabled FROM ai_routing_rules ORDER BY priority",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(task_class, provider_id, model_id, enabled)| RoutingRuleRow {
                task_class,
                provider_id,
                model_id,
                enabled,
            },
        )
        .collect())
}

#[derive(Debug, Deserialize)]
pub struct PatchRoutingRuleBody {
    pub provider_id: Option<Uuid>,
    pub model_id: Option<Uuid>,
    pub enabled: bool,
}

pub async fn patch_rule(
    pool: &DbPool,
    task_class: &str,
    body: &PatchRoutingRuleBody,
) -> anyhow::Result<RoutingRuleRow> {
    let row: (String, Option<Uuid>, Option<Uuid>, bool) = crate::db::query_as(
        "INSERT INTO ai_routing_rules (id, task_class, provider_id, model_id, enabled) VALUES (?, ?, ?, ?, ?)
         ON CONFLICT (task_class) DO UPDATE SET
            provider_id = EXCLUDED.provider_id,
            model_id = EXCLUDED.model_id,
            enabled = EXCLUDED.enabled
         RETURNING task_class, provider_id, model_id, enabled",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(task_class)
    .bind(body.provider_id)
    .bind(body.model_id)
    .bind(body.enabled)
    .fetch_one(pool)
    .await?;
    Ok(RoutingRuleRow {
        task_class: row.0,
        provider_id: row.1,
        model_id: row.2,
        enabled: row.3,
    })
}
