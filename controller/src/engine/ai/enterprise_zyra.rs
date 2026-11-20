// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use crate::db::DbPool;

#[derive(Debug, Clone, Serialize)]
pub struct ZyraEnterpriseOverview {
    pub zyra_admin_role: bool,
    pub zyra_execute_role: bool,
    pub zyra_read_role: bool,
    pub air_gap_llm: bool,
    pub audit_events_24h: i64,
    pub scim_enabled: bool,
    pub sso_configured: bool,
}

pub async fn overview(pool: &DbPool, username: &str) -> anyhow::Result<ZyraEnterpriseOverview> {
    let air_gap: bool = crate::db::query_scalar(
        "SELECT COALESCE(zeus_air_gap_llm, FALSE) FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false);
    let audit_events_24h: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM audit_logs WHERE action LIKE 'zeus.%' AND created_at > datetime('now', '-24 hours')",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let is_admin = crate::db::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE username = ? AND role = 'admin'",
    )
    .bind(username)
    .fetch_one(pool)
    .await
    .unwrap_or(0)
        > 0;
    Ok(ZyraEnterpriseOverview {
        zyra_admin_role: is_admin,
        zyra_execute_role: is_admin,
        zyra_read_role: true,
        air_gap_llm: air_gap,
        audit_events_24h,
        scim_enabled: false,
        sso_configured: false,
    })
}

#[derive(Debug, Deserialize)]
pub struct ZyraEnterprisePatch {
    pub air_gap_llm: Option<bool>,
}

pub async fn patch(pool: &DbPool, patch: &ZyraEnterprisePatch) -> anyhow::Result<()> {
    if let Some(v) = patch.air_gap_llm {
        crate::db::query("UPDATE clusters SET zeus_air_gap_llm = ?")
            .bind(v)
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub fn require_zyra_execute(actor: &crate::auth::AuthUser) -> Result<(), crate::api::ApiError> {
    crate::auth::require_operator(actor)
}

pub fn require_zyra_admin(actor: &crate::auth::AuthUser) -> Result<(), crate::api::ApiError> {
    crate::auth::require_admin(actor)
}

pub async fn audit_llm_call(
    pool: &DbPool,
    actor: &str,
    provider_kind: &str,
    model: &str,
    task_class: &str,
    agent_id: Option<&str>,
) -> anyhow::Result<()> {
    crate::db::query(
        "INSERT INTO audit_logs (id, actor, action, resource_type, detail)
         VALUES (?, ?, 'zeus.llm.complete', 'zeus', ?)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(actor)
    .bind(serde_json::json!({
        "provider_kind": provider_kind,
        "model": model,
        "task_class": task_class,
        "agent_id": agent_id
    }))
    .execute(pool)
    .await?;
    Ok(())
}
