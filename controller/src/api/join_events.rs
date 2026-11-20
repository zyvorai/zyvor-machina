// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! The step-by-step story of a host joining the fleet, shown in the web UI as a live terminal.
//! Events are keyed by the enrollment token (known to whoever issued it) and, once the host
//! exists, by its id so later steps (validation) land in the same log.

use axum::extract::{Path, State};
use axum::{Extension, Json};
use chrono::{Duration, Utc};
use serde::Serialize;
use std::sync::atomic::{AtomicI64, Ordering};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

const KEEP_DAYS: i64 = 7;

/// Timestamp for a join event. Events are listed `ORDER BY created_at, id` and `id` is a random UUID, so two events
/// recorded in the same instant would come back in arbitrary order. Microsecond resolution, and never equal to or
/// earlier than the previous value handed out by this process, keeps the order in which steps were recorded.
fn now_text() -> String {
    static LAST_MICROS: AtomicI64 = AtomicI64::new(0);
    let now = Utc::now().timestamp_micros();
    let mut prev = LAST_MICROS.load(Ordering::Relaxed);
    let micros = loop {
        let next = now.max(prev + 1);
        match LAST_MICROS.compare_exchange_weak(prev, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break next,
            Err(seen) => prev = seen,
        }
    };
    chrono::DateTime::from_timestamp_micros(micros)
        .unwrap_or_else(Utc::now)
        .format("%Y-%m-%d %H:%M:%S%.6f")
        .to_string()
}

/// Appends one step. Never fails the caller: a missing log line must not break a join.
pub async fn record(
    pool: &crate::db::DbPool,
    token: &str,
    host_id: Option<Uuid>,
    level: &str,
    step: &str,
    message: &str,
) {
    let res = crate::db::query(
        "INSERT INTO host_join_events (id, token, host_id, level, step, message, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(token)
    .bind(host_id.map(|h| h.to_string()))
    .bind(level)
    .bind(step)
    .bind(message)
    .bind(now_text())
    .execute(pool)
    .await;
    if let Err(e) = res {
        tracing::warn!("join event not recorded: {e}");
    }
}

/// Records a step for a host that is already registered, finding its token from earlier events.
pub async fn record_for_host(
    pool: &crate::db::DbPool,
    host_id: Uuid,
    level: &str,
    step: &str,
    message: &str,
) {
    let token: Option<String> = crate::db::query_scalar(
        "SELECT token FROM host_join_events WHERE host_id = ? ORDER BY created_at DESC LIMIT 1",
    )
    .bind(host_id.to_string())
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    if let Some(token) = token {
        record(pool, &token, Some(host_id), level, step, message).await;
    }
}

/// Drops events older than a week (called when a new token is issued).
pub async fn prune(pool: &crate::db::DbPool) {
    let cutoff = (Utc::now() - Duration::days(KEEP_DAYS))
        .format("%Y-%m-%d %H:%M:%S%.3f")
        .to_string();
    let _ = crate::db::query("DELETE FROM host_join_events WHERE created_at < ?")
        .bind(cutoff)
        .execute(pool)
        .await;
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct JoinEvent {
    pub id: String,
    pub level: String,
    pub step: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct JoinProgress {
    /// `waiting` (token unused), `joined` (a host used it) or `expired`.
    pub token_status: String,
    pub expires_at: Option<String>,
    pub host: Option<super::hosts::HostRow>,
    pub events: Vec<JoinEvent>,
}

pub async fn join_progress(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(token): Path<String>,
) -> Result<Json<JoinProgress>, ApiError> {
    require_operator(&actor)?;
    let row: Option<(Option<String>, Option<String>)> = crate::db::query_as(
        "SELECT CAST(expires_at AS TEXT), CAST(used_at AS TEXT) FROM enrollment_tokens WHERE token = ?",
    )
    .bind(&token)
    .fetch_optional(&state.pool)
    .await?;
    let events: Vec<JoinEvent> = crate::db::query_as(
        "SELECT id, level, step, message, created_at FROM host_join_events
         WHERE token = ? ORDER BY created_at, id LIMIT 500",
    )
    .bind(&token)
    .fetch_all(&state.pool)
    .await?;
    let Some((expires_at, used_at)) = row else {
        if events.is_empty() {
            return Err(ApiError::not_found("unknown enrollment token"));
        }
        return Ok(Json(JoinProgress {
            token_status: "joined".into(),
            expires_at: None,
            host: None,
            events,
        }));
    };
    let host_id: Option<String> = crate::db::query_scalar(
        "SELECT host_id FROM host_join_events WHERE token = ? AND host_id IS NOT NULL
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(&token)
    .fetch_optional(&state.pool)
    .await?
    .flatten();
    let host = match host_id.and_then(|h| Uuid::parse_str(&h).ok()) {
        Some(id) => super::hosts::fetch_host_row(&state, id).await.ok(),
        None => None,
    };
    let expired = expires_at
        .as_deref()
        .and_then(|e| {
            chrono::DateTime::parse_from_rfc3339(e)
                .ok()
                .map(|d| d.with_timezone(&Utc))
        })
        .map(|d| d < Utc::now())
        .unwrap_or(false);
    let token_status = if used_at.is_some() || host.is_some() {
        "joined"
    } else if expired {
        "expired"
    } else {
        "waiting"
    };
    Ok(Json(JoinProgress {
        token_status: token_status.into(),
        expires_at,
        host,
        events,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn events_are_kept_in_order_per_token_and_found_by_host() {
        let pool = crate::engine::test_support::test_pool().await;
        let host = Uuid::new_v4();
        record(&pool, "join-a", None, "info", "token", "issued").await;
        record(
            &pool,
            "join-a",
            Some(host),
            "ok",
            "registered",
            "host record created",
        )
        .await;
        record(&pool, "join-b", None, "info", "token", "other").await;
        record_for_host(&pool, host, "ok", "validate", "checks passed").await;
        let rows: Vec<JoinEvent> = crate::db::query_as(
            "SELECT id, level, step, message, created_at FROM host_join_events
             WHERE token = ? ORDER BY created_at, id",
        )
        .bind("join-a")
        .fetch_all(&pool)
        .await
        .unwrap();
        let steps: Vec<&str> = rows.iter().map(|r| r.step.as_str()).collect();
        assert_eq!(steps, ["token", "registered", "validate"]);
    }
}
