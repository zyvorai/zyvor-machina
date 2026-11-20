// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, Query, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct WebhookRow {
    pub id: Uuid,
    pub url: String,
    pub events: crate::db::Json<Vec<String>>,
    pub enabled: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateWebhookBody {
    pub url: String,
    pub events: Vec<String>,
    #[serde(default)]
    pub secret: String,
}

pub async fn list_webhooks(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<WebhookRow>>, ApiError> {
    require_admin(&actor)?;
    let rows = crate::db::query_as::<_, WebhookRow>(
        "SELECT id, url, events, enabled, created_at FROM webhooks ORDER BY created_at DESC",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

/// Reject webhook targets that resolve (by literal IP or obvious alias) to
/// private/loopback/link-local network space. Shared with notification_channels.rs
/// (Slack/webhook channel targets go through the same egress as webhook deliveries
/// — see engine/channel_worker.rs — so they need the same SSRF guard).
pub(crate) fn validate_webhook_url(url: &str) -> Result<(), ApiError> {
    let parsed = url::Url::parse(url)
        .map_err(|_| ApiError::bad_request("webhook url is not a valid URL"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return Err(ApiError::bad_request("webhook url must use http or https")),
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| ApiError::bad_request("webhook url has no host"))?;

    // If the host is an IP literal, classify it properly so alternate encodings
    // (decimal 2130706433, hex 0x7f000001, short 127.1, IPv6, IPv4-mapped) can't
    // slip a private/loopback target past a string-prefix check. url::Url already
    // canonicalizes bracketed IPv6; try to parse the host as an IpAddr.
    if let Ok(ip) = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<std::net::IpAddr>()
    {
        let blocked = match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.is_unspecified()
                    || v4.is_broadcast()
                    || v4.is_multicast()
                    // carrier-grade NAT 100.64.0.0/10
                    || (v4.octets()[0] == 100 && (64..=127).contains(&v4.octets()[1]))
            }
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback()
                    || v6.is_unspecified()
                    || v6.is_multicast()
                    // unique-local fc00::/7 and link-local fe80::/10
                    || (v6.segments()[0] & 0xfe00) == 0xfc00
                    || (v6.segments()[0] & 0xffc0) == 0xfe80
                    // IPv4-mapped/compat — re-check the embedded v4
                    || v6.to_ipv4().is_some_and(|v4| v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified())
            }
        };
        if blocked {
            return Err(ApiError::bad_request(
                "webhook url must not target private, loopback, or link-local addresses",
            ));
        }
        return Ok(());
    }

    // Non-IP host. Reject obvious localhost aliases and all-numeric hosts (which
    // hyper may interpret as an integer-encoded IPv4). DNS names that resolve to
    // internal IPs (rebinding) are additionally mitigated by redirect(none) in the
    // delivery worker and should be firewalled at the egress.
    let lower = host.to_ascii_lowercase();
    let blocked = lower == "localhost"
        || lower.ends_with(".localhost")
        || lower.starts_with("0x")
        || host.chars().all(|c| c.is_ascii_digit());
    if blocked {
        return Err(ApiError::bad_request(
            "webhook url must not target private or loopback addresses",
        ));
    }
    Ok(())
}

pub async fn create_webhook(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateWebhookBody>,
) -> Result<Json<WebhookRow>, ApiError> {
    require_admin(&actor)?;
    validate_webhook_url(&body.url)?;
    let id = Uuid::new_v4();
    crate::db::query("INSERT INTO webhooks (id, url, events, secret) VALUES (?, ?, ?, ?)")
        .bind(id)
        .bind(&body.url)
        .bind(serde_json::to_string(&body.events).unwrap_or_else(|_| "[]".into()))
        .bind(&body.secret)
        .execute(&state.pool)
        .await?;
    let row = crate::db::query_as::<_, WebhookRow>(
        "SELECT id, url, events, enabled, created_at FROM webhooks WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

pub async fn delete_webhook(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    let mut tx = state.pool.begin().await?;
    crate::db::query("DELETE FROM webhook_deliveries WHERE webhook_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    crate::db::query("DELETE FROM webhooks WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

pub async fn toggle_webhook(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<WebhookRow>, ApiError> {
    require_admin(&actor)?;
    crate::db::query("UPDATE webhooks SET enabled = NOT enabled WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    let row = crate::db::query_as::<_, WebhookRow>(
        "SELECT id, url, events, enabled, created_at FROM webhooks WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct WebhookDeliveryRow {
    pub id: Uuid,
    pub webhook_id: Option<Uuid>,
    pub url: String,
    pub event_kind: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub status: String,
    pub last_error: String,
    pub next_retry_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct ListWebhookDeliveriesQuery {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    50
}

pub async fn list_webhook_deliveries(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<ListWebhookDeliveriesQuery>,
) -> Result<Json<Vec<WebhookDeliveryRow>>, ApiError> {
    require_admin(&actor)?;
    let limit = q.limit.clamp(1, 200);
    let rows = if let Some(status) = &q.status {
        crate::db::query_as::<_, WebhookDeliveryRow>(
            "SELECT id, webhook_id, url, event_kind, attempts, max_attempts, status, last_error,
                    next_retry_at, created_at
             FROM webhook_deliveries WHERE status = ?
             ORDER BY created_at DESC LIMIT ?",
        )
        .bind(status)
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    } else {
        crate::db::query_as::<_, WebhookDeliveryRow>(
            "SELECT id, webhook_id, url, event_kind, attempts, max_attempts, status, last_error,
                    next_retry_at, created_at
             FROM webhook_deliveries ORDER BY created_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    };
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct PurgeWebhookDeliveriesBody {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub url_contains: Option<String>,
}

pub async fn purge_webhook_deliveries(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<PurgeWebhookDeliveriesBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    let status = body.status.as_deref();
    let url_pat = body.url_contains.as_deref().map(|s| format!("%{s}%"));
    let deleted = if let (Some(st), Some(url)) = (status, url_pat.as_deref()) {
        crate::db::query("DELETE FROM webhook_deliveries WHERE status = ? AND url LIKE ?")
            .bind(st)
            .bind(url)
            .execute(&state.pool)
            .await?
            .rows_affected()
    } else if let Some(st) = status {
        crate::db::query("DELETE FROM webhook_deliveries WHERE status = ?")
            .bind(st)
            .execute(&state.pool)
            .await?
            .rows_affected()
    } else if let Some(url) = url_pat.as_deref() {
        crate::db::query("DELETE FROM webhook_deliveries WHERE url LIKE ?")
            .bind(url)
            .execute(&state.pool)
            .await?
            .rows_affected()
    } else {
        return Err(ApiError::bad_request(
            "provide status and/or url_contains to purge deliveries",
        ));
    };
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

pub async fn retry_webhook_delivery(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<WebhookDeliveryRow>, ApiError> {
    require_admin(&actor)?;
    crate::db::query(
        "UPDATE webhook_deliveries SET status = 'pending', attempts = 0, last_error = '',
         next_retry_at = datetime('now') WHERE id = ?",
    )
    .bind(id)
    .execute(&state.pool)
    .await?;
    let row = crate::db::query_as::<_, WebhookDeliveryRow>(
        "SELECT id, webhook_id, url, event_kind, attempts, max_attempts, status, last_error,
                next_retry_at, created_at FROM webhook_deliveries WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}
