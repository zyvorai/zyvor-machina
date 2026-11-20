// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//
// Day-2: CRUD for threshold alert rules (evaluated by engine/alert_evaluator.rs).

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AlertRuleRow {
    pub id: Uuid,
    pub name: String,
    pub metric: String,
    pub comparator: String,
    pub threshold: f64,
    pub severity: String,
    pub scope_project: String,
    pub scope_tag: String,
    pub cooldown_minutes: i64,
    pub enabled: bool,
    pub last_fired_at: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateAlertRuleBody {
    pub name: String,
    #[serde(default = "default_metric")]
    pub metric: String,
    #[serde(default = "default_comparator")]
    pub comparator: String,
    pub threshold: f64,
    #[serde(default = "default_severity")]
    pub severity: String,
    #[serde(default)]
    pub scope_project: String,
    #[serde(default)]
    pub scope_tag: String,
    #[serde(default = "default_cooldown")]
    pub cooldown_minutes: i64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_metric() -> String {
    "cpu_percent".into()
}
fn default_comparator() -> String {
    "gt".into()
}
fn default_severity() -> String {
    "warning".into()
}
fn default_cooldown() -> i64 {
    30
}
fn default_enabled() -> bool {
    true
}

pub async fn list_alert_rules(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<AlertRuleRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, AlertRuleRow>(
        "SELECT id, name, metric, comparator, threshold, severity,
                scope_project, scope_tag, cooldown_minutes, enabled,
                last_fired_at, created_at
         FROM alert_rules ORDER BY created_at DESC LIMIT 500",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn create_alert_rule(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateAlertRuleBody>,
) -> Result<Json<AlertRuleRow>, ApiError> {
    require_operator(&actor)?;
    if body.name.trim().is_empty() || body.name.len() > 128 {
        return Err(ApiError::bad_request("rule name must be 1–128 characters"));
    }
    if !matches!(body.metric.as_str(), "cpu_percent" | "mem_percent")
        && !crate::engine::alert_evaluator::is_fleet_metric(&body.metric)
    {
        return Err(ApiError::bad_request(
            "metric must be cpu_percent, mem_percent, host_offline, storage_pool_percent, backup_failed_24h or failed_task_burst",
        ));
    }
    if !matches!(body.comparator.as_str(), "gt" | "lt") {
        return Err(ApiError::bad_request("comparator must be gt or lt"));
    }
    if !matches!(body.severity.as_str(), "info" | "warning" | "critical") {
        return Err(ApiError::bad_request(
            "severity must be info, warning, or critical",
        ));
    }
    if !body.threshold.is_finite() {
        return Err(ApiError::bad_request("threshold must be a finite number"));
    }
    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO alert_rules
           (id, name, metric, comparator, threshold, severity, scope_project, scope_tag, cooldown_minutes, enabled)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(body.name.trim())
    .bind(&body.metric)
    .bind(&body.comparator)
    .bind(body.threshold)
    .bind(&body.severity)
    .bind(&body.scope_project)
    .bind(&body.scope_tag)
    .bind(body.cooldown_minutes.max(1))
    .bind(body.enabled)
    .execute(&state.pool)
    .await?;
    let row = crate::db::query_as::<_, AlertRuleRow>(
        "SELECT id, name, metric, comparator, threshold, severity,
                scope_project, scope_tag, cooldown_minutes, enabled, last_fired_at, created_at
         FROM alert_rules WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct PatchAlertRuleBody {
    pub enabled: bool,
}

pub async fn patch_alert_rule(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchAlertRuleBody>,
) -> Result<Json<AlertRuleRow>, ApiError> {
    require_operator(&actor)?;
    let updated = crate::db::query("UPDATE alert_rules SET enabled = ? WHERE id = ?")
        .bind(body.enabled)
        .bind(id)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if updated == 0 {
        return Err(ApiError::not_found("alert rule not found"));
    }
    let row = crate::db::query_as::<_, AlertRuleRow>(
        "SELECT id, name, metric, comparator, threshold, severity,
                scope_project, scope_tag, cooldown_minutes, enabled, last_fired_at, created_at
         FROM alert_rules WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

pub async fn delete_alert_rule(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let deleted = crate::db::query("DELETE FROM alert_rules WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if deleted == 0 {
        return Err(ApiError::not_found("alert rule not found"));
    }
    Ok(Json(serde_json::json!({ "deleted": true })))
}
