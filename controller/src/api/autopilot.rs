// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Autopilot capacity: history-based rightsizing and host consolidation, both
//! proposed as approval actions.

use axum::{extract::State, Extension, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::ai::actions::{create_action, CreateActionBody, ZyraActionRow};
use crate::engine::{consolidation, rightsizing};
use crate::state::AppState;

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::internal(e.to_string())
}

pub async fn rightsizing(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let recs = rightsizing::recommend(&state.pool)
        .await
        .map_err(internal)?;
    let delta: f64 = recs.iter().map(|r| r.monthly_delta_usd).sum();
    Ok(Json(json!({
        "recommendations": recs,
        "monthly_delta_usd": (delta * 100.0).round() / 100.0,
        "min_hours": rightsizing::MIN_HOURS,
    })))
}

#[derive(Debug, Deserialize)]
pub struct ResizeBody {
    pub vm_id: Uuid,
    #[serde(default)]
    pub vcpus: Option<u32>,
    #[serde(default)]
    pub memory_mib: Option<i64>,
}

/// Files a `vm.resize` action; without sizes, the current recommendation.
pub async fn propose_resize(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ResizeBody>,
) -> Result<Json<ZyraActionRow>, ApiError> {
    require_operator(&actor)?;
    let (vcpus, memory_mib, why) = match (body.vcpus, body.memory_mib) {
        (None, None) => {
            let recs = rightsizing::recommend(&state.pool)
                .await
                .map_err(internal)?;
            let r = recs
                .into_iter()
                .find(|r| r.vm_id == body.vm_id)
                .ok_or_else(|| ApiError::not_found("no recommendation for this VM"))?;
            if let Some(id) = r.pending_action {
                return Err(ApiError::conflict(
                    "a resize for this VM is already waiting for approval",
                    format!("approve or reject action {id}"),
                ));
            }
            (
                r.suggested_vcpus,
                r.suggested_memory_mib,
                r.reasons.join("; "),
            )
        }
        (v, m) => {
            let (_, _, cv, cm): (String, Option<Uuid>, i64, i64) =
                crate::db::query_as("SELECT name, host_id, vcpus, memory_mib FROM vms WHERE id = ?")
                    .bind(body.vm_id)
                    .fetch_optional(&state.pool)
                    .await?
                    .ok_or_else(|| ApiError::not_found("vm not found"))?;
            (
                v.unwrap_or(cv.max(1) as u32),
                m.unwrap_or(cm),
                "requested".to_string(),
            )
        }
    };
    let r = rightsizing::propose_ref(&state, body.vm_id, vcpus, memory_mib).await?;
    let label = format!(
        "Resize {}: {} → {} vCPUs, {} → {} MiB",
        r.name, r.from_vcpus, r.vcpus, r.from_memory_mib, r.memory_mib
    );
    let object_ref = serde_json::to_value(&r).map_err(internal)?;
    let row = create_action(
        &state.pool,
        &CreateActionBody {
            action_type: rightsizing::RESIZE_ACTION.into(),
            label,
            review: why,
            risk: "Restarts may be needed for a smaller size".into(),
            object_ref,
            source: "rightsizing".into(),
        },
        &actor.username,
    )
    .await
    .map_err(internal)?;
    Ok(Json(row))
}

pub async fn consolidation(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<consolidation::Plan>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(
        consolidation::current(&state.pool)
            .await
            .map_err(internal)?,
    ))
}

/// Files a `drs.consolidate` action with the current plan.
pub async fn propose_consolidation(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ZyraActionRow>, ApiError> {
    require_operator(&actor)?;
    let plan = consolidation::current(&state.pool)
        .await
        .map_err(internal)?;
    if plan.moves.is_empty() {
        return Err(
            ApiError::bad_request("nothing to consolidate").with_code("nothing_to_consolidate")
        );
    }
    let hosts: Vec<&str> = plan.emptied.iter().map(|h| h.name.as_str()).collect();
    let row = create_action(
        &state.pool,
        &CreateActionBody {
            action_type: consolidation::CONSOLIDATE_ACTION.into(),
            label: format!(
                "Consolidate: {} live migrations empty {}",
                plan.moves.len(),
                hosts.join(", ")
            ),
            review: format!(
                "{} can be powered down afterwards. Each move is prechecked when it runs.",
                hosts.join(", ")
            ),
            risk: "Live migrations".into(),
            object_ref: json!({ "moves": plan.moves, "emptied": hosts }),
            source: "consolidation".into(),
        },
        &actor.username,
    )
    .await
    .map_err(internal)?;
    Ok(Json(row))
}
