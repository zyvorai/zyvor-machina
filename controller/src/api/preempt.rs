// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Preemptible instances: settings, per-VM flag, and what is preempted now.

use axum::{
    extract::{Path, State},
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::preempt::{self, Settings, MAX_PRIORITY, MAX_RESERVE_PCT, RESUME_MARGIN_PCT};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PreemptibleVm {
    pub id: Uuid,
    pub name: String,
    pub host_id: Option<Uuid>,
    pub host: Option<String>,
    pub project: Option<String>,
    pub memory_mib: i64,
    pub priority: i64,
    pub desired_state: String,
    pub observed_state: String,
    pub preempted_at: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PreemptEvent {
    pub vm: String,
    pub kind: String,
    pub reason: String,
    pub at: String,
}

pub async fn overview(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let s = preempt::settings(&state.pool).await;
    let vms: Vec<PreemptibleVm> = crate::db::query_as(
        "SELECT v.id, v.name, v.host_id, h.hostname AS host, v.project, v.memory_mib,
                v.preempt_priority AS priority, v.desired_state, v.observed_state, v.preempted_at
         FROM vms v LEFT JOIN hosts h ON h.id = v.host_id
         WHERE v.preemptible = TRUE OR v.preempted_at IS NOT NULL
         ORDER BY v.preempted_at IS NULL, v.preempt_priority, v.name",
    )
    .fetch_all(&state.pool)
    .await?;
    let hosts = preempt::hosts(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let hosts: Vec<Value> = hosts
        .iter()
        .map(|h| {
            let running: i64 = vms
                .iter()
                .filter(|v| {
                    v.host_id == Some(h.id)
                        && v.preempted_at.is_none()
                        && v.observed_state == "running"
                })
                .map(|v| v.memory_mib)
                .sum();
            json!({
                "id": h.id,
                "name": h.name,
                "total_mib": h.total_mib,
                "used_mib": h.used_mib,
                "free_mib": (h.total_mib - h.used_mib).max(0),
                "reserve_mib": h.reserve_mib(s.reserve_pct),
                "preemptible_running_mib": running,
                "fresh": h.fresh,
            })
        })
        .collect();
    let events: Vec<PreemptEvent> = crate::db::query_as(
        "SELECT v.name AS vm, e.kind, e.reason, e.at FROM vm_sleep_events e
         JOIN vms v ON v.id = e.vm_id
         WHERE e.reason LIKE 'preempted%' OR e.reason = 'capacity freed'
         ORDER BY e.id DESC LIMIT 50",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(json!({
        "settings": s,
        "resume_margin_pct": RESUME_MARGIN_PCT,
        "hosts": hosts,
        "vms": vms,
        "events": events,
    })))
}

pub async fn update_settings(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(b): Json<Settings>,
) -> Result<Json<Settings>, ApiError> {
    require_operator(&actor)?;
    if !(0..=MAX_RESERVE_PCT).contains(&b.reserve_pct) {
        return Err(ApiError::bad_request(format!(
            "reserve_pct must be 0..={MAX_RESERVE_PCT}"
        )));
    }
    crate::db::query(
        "UPDATE preempt_settings SET enabled = ?, reserve_pct = ?, updated_at = CURRENT_TIMESTAMP WHERE id = 1",
    )
    .bind(b.enabled)
    .bind(b.reserve_pct)
    .execute(&state.pool)
    .await?;
    state.emit_event(
        "preempt.settings",
        format!(
            "Preemption {} with {}% free memory reserved by {}",
            if b.enabled { "on" } else { "off" },
            b.reserve_pct,
            actor.username
        ),
    );
    Ok(Json(preempt::settings(&state.pool).await))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VmPreemptBody {
    preemptible: bool,
    #[serde(default)]
    priority: i64,
}

pub async fn set_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(b): Json<VmPreemptBody>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    if !(0..=MAX_PRIORITY).contains(&b.priority) {
        return Err(ApiError::bad_request(format!(
            "priority must be 0..={MAX_PRIORITY}"
        )));
    }
    let row: Option<(Option<Uuid>, bool)> =
        crate::db::query_as("SELECT host_id, preempted_at IS NOT NULL FROM vms WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((host, was_preempted)) = row else {
        return Err(ApiError::not_found("vm not found"));
    };
    crate::db::query(
        "UPDATE vms SET preemptible = ?, preempt_priority = ?,
         preempted_at = CASE WHEN ? THEN preempted_at ELSE NULL END WHERE id = ?",
    )
    .bind(b.preemptible)
    .bind(b.priority)
    .bind(b.preemptible)
    .bind(id)
    .execute(&state.pool)
    .await?;
    if was_preempted && !b.preemptible {
        if let Some(h) = host {
            let _ = crate::engine::vm_sleep::sync_host(&state, h).await;
        }
    }
    Ok(Json(
        json!({ "preemptible": b.preemptible, "priority": b.priority }),
    ))
}
