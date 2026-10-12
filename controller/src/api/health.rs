// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;

use crate::state::AppState;

pub async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let db_ok = crate::db::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .is_ok();
    // Atlas is optional and never degrades the controller; it is reported when enabled so
    // an operator sees a dead storage gateway next to the controller's own health.
    let atlas = match crate::engine::atlas_bridge::cached_reachable(&state.config).await {
        Some(up) => json!({ "enabled": true, "reachable": up }),
        None => json!({ "enabled": false }),
    };
    Json(json!({
        "atlas": atlas,
        "status": if db_ok { "ok" } else { "degraded" },
        "component": "machina-controller",
        "version": env!("CARGO_PKG_VERSION"),
        "controller_id": state.config.controller_id,
        "leader": state.leader.is_leader(),
        "database": if db_ok { "ok" } else { "unavailable" },
        // which backend this controller was built for, and its connection pool (so an operator can see the choice they made)
        "database_backend": crate::db::BACKEND,
        "database_pool": { "size": state.pool.size(), "idle": state.pool.num_idle() },
    }))
}

pub async fn ready(State(state): State<AppState>) -> Result<Json<serde_json::Value>, StatusCode> {
    crate::db::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(Json(json!({ "ready": true })))
}

const OPENAPI_JSON: &str = include_str!("../../../docs/openapi-controller.json");

pub async fn openapi() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/json")], OPENAPI_JSON)
}
