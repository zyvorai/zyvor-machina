// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use crate::db::DbPool;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::engine::soc::siem::{
    elastic_bulk, forward_replay, qradar_rest, splunk_hec, webhook_json,
};
use crate::engine::soc::{asm, detection, run_cycle};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SocEventRow {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub source: String,
    pub category: String,
    pub severity: String,
    pub host_id: Option<Uuid>,
    pub vm_id: Option<Uuid>,
    pub actor: Option<String>,
    pub summary: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SocAlertRow {
    pub id: Uuid,
    pub rule_id: Option<Uuid>,
    pub title: String,
    pub severity: String,
    pub status: String,
    pub assigned_to: Option<String>,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub event_count: i32,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SocRuleRow {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub severity: String,
    pub query_json: Value,
    pub throttle_minutes: i32,
    pub builtin: bool,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub limit: Option<i64>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PatchAlertBody {
    pub status: Option<String>,
    pub assigned_to: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpsertRuleBody {
    pub name: String,
    pub description: Option<String>,
    pub enabled: Option<bool>,
    pub severity: Option<String>,
    pub query_json: Value,
    pub throttle_minutes: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct PatchRuleBody {
    pub enabled: Option<bool>,
    pub severity: Option<String>,
    pub query_json: Option<Value>,
    pub throttle_minutes: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct SplunkConfigBody {
    pub url: String,
    pub token: String,
    pub index: Option<String>,
    pub sourcetype_events: Option<String>,
    pub sourcetype_alerts: Option<String>,
    pub host: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct IntegrationConfigBody {
    pub enabled: Option<bool>,
    pub config_json: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct ReplayQuery {
    pub hours: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct TestRuleQuery {
    pub hours: Option<i32>,
}

pub async fn list_events(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<SocEventRow>>, ApiError> {
    require_operator(&actor)?;
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let rows = crate::db::query_as::<_, SocEventRow>(
        "SELECT id, occurred_at, source, category, severity, host_id, vm_id, actor, summary
         FROM soc_events ORDER BY occurred_at DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn list_alerts(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<SocAlertRow>>, ApiError> {
    require_operator(&actor)?;
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let rows = if let Some(status) = q.status.filter(|s| !s.is_empty()) {
        crate::db::query_as::<_, SocAlertRow>(
            "SELECT id, rule_id, title, severity, status, assigned_to,
                    strftime('%Y-%m-%dT%H:%M:%SZ', first_seen) AS first_seen,
                    strftime('%Y-%m-%dT%H:%M:%SZ', last_seen) AS last_seen, event_count
             FROM soc_alerts WHERE status = ? ORDER BY last_seen DESC LIMIT ?",
        )
        .bind(status)
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    } else {
        crate::db::query_as::<_, SocAlertRow>(
            "SELECT id, rule_id, title, severity, status, assigned_to,
                    strftime('%Y-%m-%dT%H:%M:%SZ', first_seen) AS first_seen,
                    strftime('%Y-%m-%dT%H:%M:%SZ', last_seen) AS last_seen, event_count
             FROM soc_alerts ORDER BY last_seen DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    };
    Ok(Json(rows))
}

pub async fn get_alert(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<SocAlertDetail>, ApiError> {
    require_operator(&actor)?;
    build_alert_detail(&state.pool, id).await
}

pub async fn patch_alert(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchAlertBody>,
) -> Result<Json<SocAlertRow>, ApiError> {
    require_operator(&actor)?;
    let mut tx = state.pool.begin().await?;
    if let Some(status) = &body.status {
        crate::db::query(
            "UPDATE soc_alerts SET status = ?, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(status)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    if let Some(assignee) = &body.assigned_to {
        crate::db::query(
            "UPDATE soc_alerts SET assigned_to = ?, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(assignee)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    fetch_alert(&state.pool, id).await
}

async fn fetch_alert(pool: &DbPool, id: Uuid) -> Result<Json<SocAlertRow>, ApiError> {
    let row = crate::db::query_as::<_, SocAlertRow>(
        "SELECT id, rule_id, title, severity, status, assigned_to,
                strftime('%Y-%m-%dT%H:%M:%SZ', first_seen) AS first_seen,
                strftime('%Y-%m-%dT%H:%M:%SZ', last_seen) AS last_seen, event_count
         FROM soc_alerts WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(|_| ApiError::not_found("alert not found"))?;
    Ok(Json(row))
}

pub async fn delete_alert(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<axum::http::StatusCode, ApiError> {
    require_admin(&actor)?;
    let deleted = crate::db::query("DELETE FROM soc_alerts WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if deleted == 0 {
        return Err(ApiError::not_found("alert not found"));
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn list_rules(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<SocRuleRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, SocRuleRow>(
        "SELECT id, name, description, enabled, severity, query_json, throttle_minutes, builtin
         FROM soc_detection_rules ORDER BY name LIMIT 500",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn create_rule(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<UpsertRuleBody>,
) -> Result<Json<SocRuleRow>, ApiError> {
    require_admin(&actor)?;
    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO soc_detection_rules (id, name, description, enabled, severity, query_json, throttle_minutes)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(&body.name)
    .bind(body.description.as_deref().unwrap_or(""))
    .bind(body.enabled.unwrap_or(true))
    .bind(body.severity.as_deref().unwrap_or("medium"))
    .bind(&body.query_json)
    .bind(body.throttle_minutes.unwrap_or(60))
    .execute(&state.pool)
    .await?;
    fetch_rule(&state.pool, id).await
}

pub async fn patch_rule(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchRuleBody>,
) -> Result<Json<SocRuleRow>, ApiError> {
    require_admin(&actor)?;
    let mut tx = state.pool.begin().await?;
    if let Some(enabled) = body.enabled {
        crate::db::query(
            "UPDATE soc_detection_rules SET enabled = ?, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(enabled)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    if let Some(sev) = &body.severity {
        crate::db::query("UPDATE soc_detection_rules SET severity = ?, updated_at = datetime('now') WHERE id = ? AND builtin = FALSE")
            .bind(sev)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(q) = &body.query_json {
        crate::db::query("UPDATE soc_detection_rules SET query_json = ?, updated_at = datetime('now') WHERE id = ? AND builtin = FALSE")
            .bind(q)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(t) = body.throttle_minutes {
        crate::db::query(
            "UPDATE soc_detection_rules SET throttle_minutes = ?, updated_at = datetime('now') WHERE id = ? AND builtin = FALSE",
        )
        .bind(t)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    fetch_rule(&state.pool, id).await
}

async fn fetch_rule(pool: &DbPool, id: Uuid) -> Result<Json<SocRuleRow>, ApiError> {
    let row = crate::db::query_as::<_, SocRuleRow>(
        "SELECT id, name, description, enabled, severity, query_json, throttle_minutes, builtin
         FROM soc_detection_rules WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(|_| ApiError::not_found("rule not found"))?;
    Ok(Json(row))
}

pub async fn delete_rule(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    require_admin(&actor)?;
    let deleted =
        crate::db::query("DELETE FROM soc_detection_rules WHERE id = ? AND builtin = FALSE")
            .bind(id)
            .execute(&state.pool)
            .await?
            .rows_affected();
    if deleted == 0 {
        return Err(ApiError::not_found("rule not found or is built-in"));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn test_rule(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<TestRuleQuery>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    detection::test_rule(&state.pool, id, q.hours.unwrap_or(24))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn asm_summary(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<asm::AsmSummary>, ApiError> {
    require_operator(&actor)?;
    asm::build_asm_summary(&state.pool, &state.config)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn get_splunk_integration(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<IntegrationPublic>, ApiError> {
    require_admin(&actor)?;
    let row = fetch_integration_db(&state.pool, "splunk_hec").await?;
    Ok(Json(integration_public_db(&row)))
}

pub async fn put_splunk_integration(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<SplunkConfigBody>,
) -> Result<Json<IntegrationPublic>, ApiError> {
    require_admin(&actor)?;
    let id = splunk_integration_id(&state.pool).await?;
    let mut cfg = serde_json::json!({
        "url": body.url,
        "token": body.token,
        "index": body.index.unwrap_or_else(|| "machina".into()),
        "sourcetype_events": body.sourcetype_events.unwrap_or_else(|| "machina:soc:ecs".into()),
        "sourcetype_alerts": body.sourcetype_alerts.unwrap_or_else(|| "machina:soc:alert".into()),
        "host": body.host.unwrap_or_default(),
    });
    if body.token.is_empty() {
        let existing: Value =
            crate::db::query_scalar("SELECT config_json FROM soc_integrations WHERE id = ?")
                .bind(id)
                .fetch_one(&state.pool)
                .await?;
        if let Some(t) = existing.get("token") {
            cfg["token"] = t.clone();
        }
    }
    crate::db::query(
        "UPDATE soc_integrations SET config_json = ?, enabled = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(cfg)
    .bind(body.enabled.unwrap_or(true))
    .bind(id)
    .execute(&state.pool)
    .await?;
    let row = fetch_integration_db(&state.pool, "splunk_hec").await?;
    Ok(Json(integration_public_db(&row)))
}

pub async fn test_splunk_integration(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let row = fetch_integration_db(&state.pool, "splunk_hec").await?;
    splunk_hec::test_connection(&row.config_json, &state.config.controller_id)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(|msg| Json(serde_json::json!({ "ok": true, "message": msg })))
}

pub async fn list_integrations(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<IntegrationPublic>>, ApiError> {
    require_admin(&actor)?;
    let rows: Vec<IntegrationDbRow> = crate::db::query_as(
        "SELECT id, integration_type, name, enabled, config_json,
                strftime('%Y-%m-%dT%H:%M:%SZ', last_success_at) AS last_success_at, last_error
         FROM soc_integrations ORDER BY integration_type LIMIT 100",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows.iter().map(integration_public_db).collect()))
}

pub async fn patch_integration(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(integration_type): Path<String>,
    Json(body): Json<IntegrationConfigBody>,
) -> Result<Json<IntegrationPublic>, ApiError> {
    require_admin(&actor)?;
    let mut tx = state.pool.begin().await?;
    if let Some(cfg) = body.config_json {
        let existing: Value = crate::db::query_scalar(
            "SELECT config_json FROM soc_integrations WHERE integration_type = ? AND name = 'default'",
        )
        .bind(&integration_type)
        .fetch_one(&mut *tx)
        .await
        .unwrap_or(Value::Null);
        let merged = merge_integration_config(&existing, &cfg);
        crate::db::query(
            "UPDATE soc_integrations SET config_json = ?, updated_at = datetime('now') WHERE integration_type = ? AND name = 'default'",
        )
        .bind(merged)
        .bind(&integration_type)
        .execute(&mut *tx)
        .await?;
    }
    if let Some(enabled) = body.enabled {
        crate::db::query(
            "UPDATE soc_integrations SET enabled = ?, updated_at = datetime('now') WHERE integration_type = ? AND name = 'default'",
        )
        .bind(enabled)
        .bind(&integration_type)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    let row: IntegrationDbRow = fetch_integration_db(&state.pool, &integration_type).await?;
    Ok(Json(integration_public_db(&row)))
}

pub async fn test_integration(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(integration_type): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let row: IntegrationDbRow = fetch_integration_db(&state.pool, &integration_type).await?;
    let msg = match integration_type.as_str() {
        "splunk_hec" => {
            splunk_hec::test_connection(&row.config_json, &state.config.controller_id).await?
        }
        "elastic_bulk" => elastic_bulk::test_connection(&row.config_json).await?,
        "webhook_json" => webhook_json::test_connection(&row.config_json).await?,
        "qradar_rest" => qradar_rest::test_connection(&row.config_json).await?,
        other => {
            return Err(ApiError::bad_request(format!(
                "unknown integration: {other}"
            )))
        }
    };
    Ok(Json(serde_json::json!({ "ok": true, "message": msg })))
}

pub async fn forward_replay_handler(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<ReplayQuery>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let n = forward_replay(
        &state.pool,
        q.hours.unwrap_or(24),
        &state.config.controller_id,
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "forwarded": n, "hours": q.hours.unwrap_or(24) }),
    ))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct IntegrationDbRow {
    id: Uuid,
    integration_type: String,
    name: String,
    enabled: bool,
    config_json: Value,
    last_success_at: Option<DateTime<Utc>>,
    last_error: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PlaybookRow {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub trigger_json: Value,
    pub steps_json: Value,
}

#[derive(Debug, Deserialize)]
pub struct UpsertPlaybookBody {
    pub name: String,
    pub description: Option<String>,
    pub enabled: Option<bool>,
    pub trigger_json: Option<Value>,
    pub steps_json: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct PatchPlaybookBody {
    pub description: Option<String>,
    pub enabled: Option<bool>,
    pub trigger_json: Option<Value>,
    pub steps_json: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct SocSettingsPublic {
    pub webhook_url: String,
}

#[derive(Debug, Deserialize)]
pub struct PatchSocSettingsBody {
    pub webhook_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MitreTag {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SocEventDetailRow {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub source: String,
    pub category: String,
    pub severity: String,
    pub summary: String,
    pub ecs_json: Value,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct AlertDetailDbRow {
    id: Uuid,
    rule_id: Option<Uuid>,
    title: String,
    severity: String,
    status: String,
    assigned_to: Option<String>,
    first_seen: DateTime<Utc>,
    last_seen: DateTime<Utc>,
    event_count: i32,
    dedupe_key: String,
    event_ids: Value,
    detail_json: Value,
    rule_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SocAlertDetail {
    pub id: Uuid,
    pub rule_id: Option<Uuid>,
    pub rule_name: Option<String>,
    pub title: String,
    pub severity: String,
    pub status: String,
    pub assigned_to: Option<String>,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub event_count: i32,
    pub dedupe_key: String,
    pub detail_json: Value,
    pub mitre_tags: Vec<MitreTag>,
    pub linked_events: Vec<SocEventDetailRow>,
    pub playbook_runs: Vec<PlaybookRunDetailRow>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PlaybookRunDetailRow {
    pub id: Uuid,
    pub playbook_id: Uuid,
    pub playbook_name: Option<String>,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub step_results: Value,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PlaybookRunRow {
    pub id: Uuid,
    pub playbook_id: Uuid,
    pub alert_id: Option<Uuid>,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

pub async fn run_ingest_cycle(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<crate::engine::soc::CycleStats>, ApiError> {
    require_admin(&actor)?;
    run_cycle(&state.pool, &state.config, &state.config.controller_id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn list_playbooks(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<PlaybookRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, PlaybookRow>(
        "SELECT id, name, description, enabled, trigger_json, steps_json FROM soc_playbooks ORDER BY name LIMIT 500",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn get_playbook(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<PlaybookRow>, ApiError> {
    require_operator(&actor)?;
    fetch_playbook(&state.pool, id).await
}

pub async fn create_playbook(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<UpsertPlaybookBody>,
) -> Result<Json<PlaybookRow>, ApiError> {
    require_admin(&actor)?;
    let id = Uuid::new_v4();
    let steps = body.steps_json.unwrap_or_else(|| serde_json::json!([]));
    let trigger = body
        .trigger_json
        .unwrap_or_else(|| serde_json::json!({ "min_severity": "medium", "rule_names": [] }));
    crate::db::query(
        "INSERT INTO soc_playbooks (id, name, description, enabled, trigger_json, steps_json)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(&body.name)
    .bind(body.description.as_deref().unwrap_or(""))
    .bind(body.enabled.unwrap_or(true))
    .bind(trigger)
    .bind(steps)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("unique") {
            ApiError::bad_request("playbook name already exists")
        } else {
            ApiError::internal(e.to_string())
        }
    })?;
    fetch_playbook(&state.pool, id).await
}

pub async fn patch_playbook(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchPlaybookBody>,
) -> Result<Json<PlaybookRow>, ApiError> {
    require_admin(&actor)?;
    let mut tx = state.pool.begin().await?;
    if let Some(desc) = &body.description {
        crate::db::query(
            "UPDATE soc_playbooks SET description = ?, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(desc)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    if let Some(enabled) = body.enabled {
        crate::db::query(
            "UPDATE soc_playbooks SET enabled = ?, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(enabled)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    if let Some(trigger) = &body.trigger_json {
        crate::db::query(
            "UPDATE soc_playbooks SET trigger_json = ?, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(trigger)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    if let Some(steps) = &body.steps_json {
        crate::db::query(
            "UPDATE soc_playbooks SET steps_json = ?, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(steps)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    fetch_playbook(&state.pool, id).await
}

pub async fn delete_playbook(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let name: String = crate::db::query_scalar("SELECT name FROM soc_playbooks WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|_| ApiError::not_found("playbook not found"))?;
    if name == "notify_on_critical" {
        return Err(ApiError::bad_request("built-in playbook cannot be deleted"));
    }
    crate::db::query("DELETE FROM soc_playbooks WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

pub async fn get_soc_settings(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<SocSettingsPublic>, ApiError> {
    require_operator(&actor)?;
    let url = load_soc_webhook_url(&state.pool).await;
    Ok(Json(SocSettingsPublic { webhook_url: url }))
}

pub async fn patch_soc_settings(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<PatchSocSettingsBody>,
) -> Result<Json<SocSettingsPublic>, ApiError> {
    require_admin(&actor)?;
    if let Some(url) = body.webhook_url {
        crate::db::query(
            "INSERT INTO soc_settings (id, webhook_url, updated_at) VALUES (1, ?, datetime('now'))
             ON CONFLICT (id) DO UPDATE SET webhook_url = EXCLUDED.webhook_url, updated_at = datetime('now')",
        )
        .bind(url.trim())
        .execute(&state.pool)
        .await?;
    }
    let url = load_soc_webhook_url(&state.pool).await;
    Ok(Json(SocSettingsPublic { webhook_url: url }))
}

pub async fn list_playbook_runs(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<PlaybookRunRow>>, ApiError> {
    require_operator(&actor)?;
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let rows = crate::db::query_as::<_, PlaybookRunRow>(
        "SELECT id, playbook_id, alert_id, status,
                strftime('%Y-%m-%dT%H:%M:%SZ', started_at) AS started_at,
                strftime('%Y-%m-%dT%H:%M:%SZ', finished_at) AS finished_at
         FROM soc_playbook_runs ORDER BY started_at DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let open_alerts: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM soc_alerts WHERE status IN ('open', 'acknowledged')",
    )
    .fetch_one(&state.pool)
    .await?;
    let events_24h: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM soc_events WHERE occurred_at > datetime('now', '-24 hours')",
    )
    .fetch_one(&state.pool)
    .await?;
    let critical: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM soc_alerts WHERE status IN ('open', 'acknowledged') AND severity IN ('critical', 'high')",
    )
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(serde_json::json!({
        "open_alerts": open_alerts,
        "events_24h": events_24h,
        "critical_alerts": critical,
    })))
}

#[derive(Debug, Serialize)]
pub struct IntegrationPublic {
    pub id: Uuid,
    pub integration_type: String,
    pub name: String,
    pub enabled: bool,
    pub config: Value,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

fn merge_integration_config(existing: &Value, patch: &Value) -> Value {
    let mut out = patch.clone();
    let Some(patch_obj) = out.as_object_mut() else {
        return out;
    };
    let Some(existing_obj) = existing.as_object() else {
        return out;
    };
    for key in [
        "token",
        "api_key",
        "client_secret",
        "api_token",
        "bearer_token",
    ] {
        let keep = patch_obj
            .get(key)
            .and_then(|v| v.as_str())
            .is_none_or(|s| s.is_empty() || s.contains('•'));
        if keep {
            if let Some(v) = existing_obj.get(key) {
                patch_obj.insert(key.to_string(), v.clone());
            }
        }
    }
    out
}

async fn fetch_integration_db(
    pool: &DbPool,
    integration_type: &str,
) -> Result<IntegrationDbRow, ApiError> {
    crate::db::query_as(
        "SELECT id, integration_type, name, enabled, config_json,
                strftime('%Y-%m-%dT%H:%M:%SZ', last_success_at) AS last_success_at, last_error
         FROM soc_integrations WHERE integration_type = ? AND name = 'default'",
    )
    .bind(integration_type)
    .fetch_one(pool)
    .await
    .map_err(|_| ApiError::not_found("integration not found"))
}

fn integration_public_db(row: &IntegrationDbRow) -> IntegrationPublic {
    IntegrationPublic {
        id: row.id,
        integration_type: row.integration_type.clone(),
        name: row.name.clone(),
        enabled: row.enabled,
        config: redact_integration_config(&row.config_json),
        last_success_at: row.last_success_at,
        last_error: row.last_error.clone(),
    }
}

fn redact_integration_config(cfg: &Value) -> Value {
    let mut cfg = cfg.clone();
    if let Some(obj) = cfg.as_object_mut() {
        for key in [
            "token",
            "api_key",
            "client_secret",
            "api_token",
            "bearer_token",
        ] {
            if obj
                .get(key)
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.is_empty())
            {
                obj.insert(key.to_string(), serde_json::json!("••••••••"));
            }
        }
    }
    cfg
}

async fn fetch_playbook(pool: &DbPool, id: Uuid) -> Result<Json<PlaybookRow>, ApiError> {
    crate::db::query_as::<_, PlaybookRow>(
        "SELECT id, name, description, enabled, trigger_json, steps_json FROM soc_playbooks WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .map(Json)
    .map_err(|_| ApiError::not_found("playbook not found"))
}

async fn load_soc_webhook_url(pool: &DbPool) -> String {
    if let Ok(url) =
        crate::db::query_scalar::<_, String>("SELECT webhook_url FROM soc_settings WHERE id = 1")
            .fetch_one(pool)
            .await
    {
        let url = url.trim().to_string();
        if !url.is_empty() {
            return url;
        }
    }
    std::env::var("MACHINA_SOC_WEBHOOK_URL").unwrap_or_default()
}

async fn build_alert_detail(pool: &DbPool, id: Uuid) -> Result<Json<SocAlertDetail>, ApiError> {
    let row: AlertDetailDbRow = crate::db::query_as(
        "SELECT a.id, a.rule_id, a.title, a.severity, a.status, a.assigned_to,
                strftime('%Y-%m-%dT%H:%M:%SZ', a.first_seen) AS first_seen,
                strftime('%Y-%m-%dT%H:%M:%SZ', a.last_seen) AS last_seen,
                a.event_count, a.dedupe_key, a.event_ids, a.detail_json, r.name AS rule_name
         FROM soc_alerts a
         LEFT JOIN soc_detection_rules r ON r.id = a.rule_id
         WHERE a.id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(|_| ApiError::not_found("alert not found"))?;

    let event_ids: Vec<Uuid> = serde_json::from_value(row.event_ids.clone()).unwrap_or_default();
    let linked_events = if event_ids.is_empty() {
        vec![]
    } else {
        let placeholders = std::iter::repeat_n("?", event_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT id, occurred_at, source, category, severity, summary, ecs_json
             FROM soc_events WHERE id IN ({placeholders}) ORDER BY occurred_at DESC LIMIT 50"
        );
        let mut q = crate::db::query_as::<_, SocEventDetailRow>(&sql);
        for id in &event_ids {
            q = q.bind(id);
        }
        q.fetch_all(pool).await?
    };

    let mut mitre_tags = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for ev in &linked_events {
        for tag in extract_mitre_tags(&ev.ecs_json) {
            let key = format!("{}:{}", tag.id, tag.name);
            if seen.insert(key) {
                mitre_tags.push(tag);
            }
        }
    }

    let playbook_runs: Vec<PlaybookRunDetailRow> = crate::db::query_as(
        "SELECT r.id, r.playbook_id, p.name AS playbook_name, r.status,
                strftime('%Y-%m-%dT%H:%M:%SZ', r.started_at) AS started_at,
                strftime('%Y-%m-%dT%H:%M:%SZ', r.finished_at) AS finished_at,
                r.step_results, r.error
         FROM soc_playbook_runs r
         LEFT JOIN soc_playbooks p ON p.id = r.playbook_id
         WHERE r.alert_id = ?
         ORDER BY r.started_at DESC LIMIT 20",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    Ok(Json(SocAlertDetail {
        id: row.id,
        rule_id: row.rule_id,
        rule_name: row.rule_name,
        title: row.title,
        severity: row.severity,
        status: row.status,
        assigned_to: row.assigned_to,
        first_seen: row.first_seen,
        last_seen: row.last_seen,
        event_count: row.event_count,
        dedupe_key: row.dedupe_key,
        detail_json: row.detail_json,
        mitre_tags,
        linked_events,
        playbook_runs,
    }))
}

fn extract_mitre_tags(ecs: &Value) -> Vec<MitreTag> {
    let mut out = Vec::new();
    if let Some(id) = ecs
        .pointer("/threat/technique/id")
        .or_else(|| ecs.pointer("/threat/technique/0/id"))
        .and_then(|v| v.as_str())
    {
        let name = ecs
            .pointer("/threat/technique/name")
            .or_else(|| ecs.pointer("/threat/technique/0/name"))
            .and_then(|v| v.as_str())
            .unwrap_or(id);
        out.push(MitreTag {
            id: id.to_string(),
            name: name.to_string(),
        });
    }
    if let Some(arr) = ecs
        .get("machina")
        .and_then(|m| m.get("mitre"))
        .and_then(|v| v.as_array())
    {
        for item in arr {
            if let Some(s) = item.as_str() {
                out.push(MitreTag {
                    id: s.to_string(),
                    name: s.to_string(),
                });
            } else if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                let name = item.get("name").and_then(|v| v.as_str()).unwrap_or(id);
                out.push(MitreTag {
                    id: id.to_string(),
                    name: name.to_string(),
                });
            }
        }
    }
    if let Some(tags) = ecs.pointer("/rule/tags").and_then(|v| v.as_array()) {
        for tag in tags {
            if let Some(s) = tag.as_str() {
                if s.starts_with("attack.") || s.contains("T") {
                    out.push(MitreTag {
                        id: s.to_string(),
                        name: s.to_string(),
                    });
                }
            }
        }
    }
    out
}

async fn splunk_integration_id(pool: &DbPool) -> Result<Uuid, ApiError> {
    crate::db::query_scalar("SELECT id FROM soc_integrations WHERE integration_type = 'splunk_hec' AND name = 'default'")
        .fetch_optional(pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Splunk integration not configured — add it via the SOC integrations page"))
}
