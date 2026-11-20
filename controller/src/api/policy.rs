// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::State;
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::engine::policy;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct PolicyRuleRow {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub rule_json: serde_json::Value,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ProjectQuotaRow {
    pub project: String,
    pub max_vms: i32,
    pub max_vcpu: i32,
    pub max_memory_mib: i64,
    pub max_storage_gib: i64,
}

pub async fn list_policy_rules(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<PolicyRuleRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = policy::list_policy_rules(&state.pool).await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, name, enabled, rule_json)| PolicyRuleRow {
                id,
                name,
                enabled,
                rule_json,
            })
            .collect(),
    ))
}

pub async fn list_project_quotas(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<ProjectQuotaRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, ProjectQuotaRow>(
        "SELECT project, max_vms, max_vcpu, max_memory_mib, max_storage_gib FROM project_quotas ORDER BY project LIMIT 500",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct UpsertQuotaBody {
    pub project: String,
    pub max_vms: i32,
    pub max_vcpu: i32,
    pub max_memory_mib: i64,
    pub max_storage_gib: i64,
}

pub async fn upsert_project_quota(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<UpsertQuotaBody>,
) -> Result<Json<ProjectQuotaRow>, ApiError> {
    require_admin(&actor)?;
    policy::upsert_project_quota(
        &state.pool,
        &body.project,
        body.max_vms,
        body.max_vcpu,
        body.max_memory_mib,
        body.max_storage_gib,
    )
    .await?;
    Ok(Json(ProjectQuotaRow {
        project: body.project,
        max_vms: body.max_vms,
        max_vcpu: body.max_vcpu,
        max_memory_mib: body.max_memory_mib,
        max_storage_gib: body.max_storage_gib,
    }))
}
