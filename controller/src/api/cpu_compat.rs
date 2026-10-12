// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::State;
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::api::ApiError;
use crate::auth::{require_admin, AuthUser};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuCompatRule {
    pub source: String,
    pub compatible_with: Vec<String>,
}

pub async fn get_cpu_compat_matrix(
    State(state): State<AppState>,
) -> Result<Json<Vec<CpuCompatRule>>, ApiError> {
    let raw: serde_json::Value =
        crate::db::query_scalar("SELECT cpu_compat_matrix FROM clusters ORDER BY created_at LIMIT 1")
            .fetch_one(&state.pool)
            .await?;
    let rules: Vec<CpuCompatRule> = serde_json::from_value(raw).unwrap_or_default();
    Ok(Json(rules))
}

#[derive(Debug, Deserialize)]
pub struct PatchCpuCompatBody {
    pub rules: Vec<CpuCompatRule>,
}

pub async fn patch_cpu_compat_matrix(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<PatchCpuCompatBody>,
) -> Result<Json<Vec<CpuCompatRule>>, ApiError> {
    require_admin(&actor)?;
    let val = serde_json::to_value(&body.rules).map_err(|e| ApiError::internal(e.to_string()))?;
    crate::db::query("UPDATE clusters SET cpu_compat_matrix = ?")
        .bind(val)
        .execute(&state.pool)
        .await?;
    get_cpu_compat_matrix(State(state)).await
}

pub fn cpu_compatible(matrix: &[CpuCompatRule], source: &str, dest: &str) -> bool {
    if source.is_empty() || dest.is_empty() || source == dest {
        return true;
    }
    matrix
        .iter()
        .find(|r| r.source == source)
        .map(|r| r.compatible_with.iter().any(|d| d == dest))
        .unwrap_or(true)
}
