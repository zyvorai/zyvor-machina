// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Launchpad catalog — a quick-launch app/template shortcut catalog that was
// superseded by the Blueprints feature (`api/blueprints.rs`) before it shipped.
// The route is kept as a typed "unavailable" stub, matching the Atlas
// disabled-feature pattern, so clients get a graceful 503 instead of a bare
// 404 or router "not found" page.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

pub async fn launchpad_catalog(
    State(_state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    Err(ApiError {
        status: StatusCode::SERVICE_UNAVAILABLE,
        message: "Launchpad is not available on this platform".into(),
        error_code: Some("launchpad_unavailable".into()),
        remediation: Some("Use Platform → Blueprints to create quick-launch shortcuts.".into()),
        object_ref: None,
    })
}
