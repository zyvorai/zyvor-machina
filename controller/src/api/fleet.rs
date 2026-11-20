// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::State;
use axum::Extension;
use axum::Json;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::fleet_activity;
use crate::engine::fleet_backups;
use crate::engine::fleet_console;
use crate::engine::fleet_desktop;
use crate::engine::fleet_dna;
use crate::engine::fleet_finder;
use crate::engine::fleet_general;
use crate::engine::fleet_gpu;
use crate::engine::fleet_keychain;
use crate::engine::fleet_linux;
use crate::engine::fleet_maintenance_mission;
use crate::engine::fleet_mission;
use crate::engine::fleet_network;
use crate::engine::fleet_shortcuts;
use crate::engine::fleet_spaces;
use crate::engine::fleet_storage;
use crate::engine::fleet_updates;
use crate::engine::fleet_users;
use crate::state::AppState;

pub async fn desktop_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<fleet_desktop::FleetDesktopOverview>, ApiError> {
    require_operator(&actor)?;
    fleet_desktop::overview(&state.pool, &state.config)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn linux_health(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<fleet_linux::FleetLinuxHealthOverview>, ApiError> {
    require_operator(&actor)?;
    fleet_linux::overview(&state.pool, &state.config)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn activity_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<fleet_activity::FleetActivityOverview>, ApiError> {
    require_operator(&actor)?;
    fleet_activity::overview(&state.pool, &state.config)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn backup_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<fleet_backups::FleetBackupOverview>, ApiError> {
    require_operator(&actor)?;
    fleet_backups::overview(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn finder_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<fleet_finder::FleetFinderOverview>, ApiError> {
    require_operator(&actor)?;
    fleet_finder::overview(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn network_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<fleet_network::FleetNetworkOverview>, ApiError> {
    require_operator(&actor)?;
    fleet_network::overview(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn storage_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<fleet_storage::FleetStorageOverview>, ApiError> {
    require_operator(&actor)?;
    fleet_storage::overview(&state.pool, &state.config)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn console_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<fleet_console::FleetConsoleOverview>, ApiError> {
    require_operator(&actor)?;
    fleet_console::overview(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn updates_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<fleet_updates::FleetUpdatesOverview>, ApiError> {
    require_operator(&actor)?;
    fleet_updates::overview(&state.pool, &state.config)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn keychain_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<fleet_keychain::FleetKeychainOverview>, ApiError> {
    require_operator(&actor)?;
    fleet_keychain::overview(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn users_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<fleet_users::FleetUsersOverview>, ApiError> {
    require_operator(&actor)?;
    fleet_users::overview(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn shortcuts_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<fleet_shortcuts::FleetShortcutsOverview>, ApiError> {
    require_operator(&actor)?;
    fleet_shortcuts::overview(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn spaces_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<fleet_spaces::FleetSpacesOverview>, ApiError> {
    require_operator(&actor)?;
    fleet_spaces::overview(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn general_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<fleet_general::FleetGeneralOverview>, ApiError> {
    require_operator(&actor)?;
    fleet_general::overview(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn mission_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<fleet_mission::FleetMissionOverview>, ApiError> {
    require_operator(&actor)?;
    fleet_mission::overview(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn gpu_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<fleet_gpu::FleetGpuOverview>, ApiError> {
    require_operator(&actor)?;
    fleet_gpu::overview(&state.pool)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn maintenance_mission_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<fleet_maintenance_mission::FleetMaintenanceMissionOverview>, ApiError> {
    require_operator(&actor)?;
    fleet_maintenance_mission::overview(&state.pool, &state.config)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn dna_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<fleet_dna::FleetDnaOverview>, ApiError> {
    require_operator(&actor)?;
    fleet_dna::overview(&state.pool, &state.config)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

#[derive(Debug, serde::Deserialize)]
pub struct FleetDiagnoseBody {
    pub query: String,
}

pub async fn fleet_diagnose(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<FleetDiagnoseBody>,
) -> Result<Json<fleet_linux::FleetDiagnoseReport>, ApiError> {
    require_operator(&actor)?;
    fleet_linux::diagnose(&state.pool, &state.config, &body.query)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}
