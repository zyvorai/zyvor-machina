// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//
// Day-2: on-demand TLS cert-expiry status (engine/cert_monitor does the alerting).

use axum::extract::State;
use axum::Extension;
use axum::Json;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::cert_monitor::{cert_status, default_cert_path};
use crate::state::AppState;

pub async fn get_cert_status(
    State(_state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let path = default_cert_path();
    match cert_status(&path).await {
        Some((not_after, days)) => Ok(Json(serde_json::json!({
            "path": path,
            "not_after": not_after,
            "days_remaining": days,
            "expiring_soon": days <= 30,
            "expired": days < 0,
        }))),
        None => Ok(Json(serde_json::json!({
            "path": path,
            "available": false,
            "detail": "certificate not found or unreadable (TLS may be terminated upstream)",
        }))),
    }
}
