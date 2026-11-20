// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//
// Day-2: CRUD + test for notification channels (Slack/email/webhook). Delivered by
// engine/channel_worker.rs; fed by engine/webhooks::dispatch_channels.

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ChannelRow {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub target: String,
    pub events: String,
    pub enabled: bool,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateChannelBody {
    pub name: String,
    pub kind: String,
    pub target: String,
    #[serde(default)]
    pub events: Option<Vec<String>>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

pub async fn list_channels(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<ChannelRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, ChannelRow>(
        "SELECT id, name, kind, target, events, enabled, created_at
         FROM notification_channels ORDER BY created_at DESC LIMIT 500",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

fn validate_channel(kind: &str, target: &str) -> Result<(), ApiError> {
    match kind {
        // Slack/webhook targets are delivered by engine/channel_worker.rs over the
        // same outbound HTTP path as webhooks.rs — apply the same SSRF guard
        // (reject loopback/private/link-local literals and localhost aliases) so a
        // channel can't be used to make the controller call internal-only services.
        "slack" | "webhook" => super::webhooks::validate_webhook_url(target)?,
        "email" => {
            if !target.contains('@') || target.len() < 3 {
                return Err(ApiError::bad_request("target must be an email address"));
            }
        }
        _ => {
            return Err(ApiError::bad_request(
                "kind must be slack, email, or webhook",
            ))
        }
    }
    Ok(())
}

pub async fn create_channel(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateChannelBody>,
) -> Result<Json<ChannelRow>, ApiError> {
    require_operator(&actor)?;
    if body.name.trim().is_empty() || body.name.len() > 128 {
        return Err(ApiError::bad_request("name must be 1–128 characters"));
    }
    validate_channel(&body.kind, &body.target)?;
    let events = body.events.unwrap_or_else(|| vec!["alert.*".to_string()]);
    let events_json = serde_json::to_string(&events).unwrap_or_else(|_| "[\"alert.*\"]".into());
    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO notification_channels (id, name, kind, target, events, enabled)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(body.name.trim())
    .bind(&body.kind)
    .bind(&body.target)
    .bind(&events_json)
    .bind(body.enabled)
    .execute(&state.pool)
    .await?;
    let row = crate::db::query_as::<_, ChannelRow>(
        "SELECT id, name, kind, target, events, enabled, created_at FROM notification_channels WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

pub async fn delete_channel(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let deleted = crate::db::query("DELETE FROM notification_channels WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if deleted == 0 {
        return Err(ApiError::not_found("channel not found"));
    }
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// Enqueue a test notification to a single channel so operators can confirm wiring.
pub async fn test_channel(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let ch = crate::db::query_as::<_, (String, String)>(
        "SELECT kind, target FROM notification_channels WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let Some((kind, target)) = ch else {
        return Err(ApiError::not_found("channel not found"));
    };
    crate::db::query(
        "INSERT INTO channel_deliveries (id, channel_id, kind, target, subject, body, event_kind)
         VALUES (?, ?, ?, ?, ?, ?, 'test')",
    )
    .bind(Uuid::new_v4())
    .bind(id)
    .bind(&kind)
    .bind(&target)
    .bind("[machina] Test notification")
    .bind("This is a test notification from Machina — your channel is wired correctly.")
    .execute(&state.pool)
    .await?;
    Ok(Json(serde_json::json!({ "queued": true })))
}
