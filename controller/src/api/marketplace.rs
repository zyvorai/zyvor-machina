// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::platform_plugins::{self, PluginPublishRequest, PluginRow};
use crate::state::AppState;

pub async fn plugins_overview(
    State(state): State<AppState>,
) -> Result<Json<platform_plugins::MarketplaceOverview>, ApiError> {
    platform_plugins::marketplace_overview(&state.pool, &state.config)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn install_plugin(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(slug): Path<String>,
) -> Result<Json<platform_plugins::PluginInstallResult>, ApiError> {
    require_operator(&actor)?;
    platform_plugins::install_plugin(&state.pool, &slug)
        .await
        .map(Json)
        .map_err(|e| ApiError::bad_request(e.to_string()))
}

pub async fn uninstall_plugin(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(slug): Path<String>,
) -> Result<Json<platform_plugins::PluginInstallResult>, ApiError> {
    require_operator(&actor)?;
    platform_plugins::uninstall_plugin(&state.pool, &slug)
        .await
        .map(Json)
        .map_err(|e| ApiError::bad_request(e.to_string()))
}

pub async fn publish_plugin(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<PluginPublishRequest>,
) -> Result<Json<PluginRow>, ApiError> {
    require_operator(&actor)?;
    platform_plugins::publish_plugin(&state.pool, &body)
        .await
        .map(Json)
        .map_err(|e| ApiError::bad_request(e.to_string()))
}
