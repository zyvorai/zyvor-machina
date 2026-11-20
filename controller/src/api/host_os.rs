// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, Query, State};
use axum::Extension;
use axum::Json;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::engine::host_os;
use crate::state::AppState;

pub async fn host_linux_observability(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    host_os::linux_observability(&state.pool, &state.config, id)
        .await
        .map(Json)
        .map_err(ApiError::from_upstream)
}

pub async fn host_network_diagnostics(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    host_os::network_diagnostics(&state.pool, &state.config, id)
        .await
        .map(Json)
        .map_err(ApiError::from_upstream)
}

pub async fn host_linux_audit(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    host_os::linux_audit(&state.pool, &state.config, id)
        .await
        .map(Json)
        .map_err(ApiError::from_upstream)
}

pub async fn host_linux_package_updates(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    host_os::linux_package_updates(&state.pool, &state.config, id)
        .await
        .map(Json)
        .map_err(ApiError::from_upstream)
}

#[derive(Debug, serde::Deserialize)]
pub struct LinuxProcessQuery {
    pub limit: Option<u32>,
    pub order: Option<String>,
}

pub async fn host_linux_filesystems(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    host_os::linux_filesystems(&state.pool, &state.config, id)
        .await
        .map(Json)
        .map_err(ApiError::from_upstream)
}

pub async fn host_linux_processes(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<LinuxProcessQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    host_os::linux_top_processes(
        &state.pool,
        &state.config,
        id,
        q.limit.unwrap_or(20).min(64),
        q.order.as_deref().unwrap_or("memory"),
    )
    .await
    .map(Json)
    .map_err(ApiError::from_upstream)
}

#[derive(Debug, serde::Deserialize)]
pub struct LinuxPackageUpgradeBody {
    #[serde(default)]
    pub dry_run: bool,
}

pub async fn host_linux_package_upgrade(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<LinuxPackageUpgradeBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    use crate::tasks::enqueue::enqueue_task;

    if body.dry_run {
        let result = host_os::apply_linux_package_upgrade(&state.pool, &state.config, id, true)
            .await
            .map_err(ApiError::from_upstream)?;
        return Ok(Json(serde_json::json!({
            "dry_run": true,
            "result": result,
            "summary": "Package upgrade preview completed"
        })));
    }
    host_os::require_maintenance_mode(&state.pool, id)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let task_id = enqueue_task(
        &state,
        "host.linux.package_upgrade",
        serde_json::json!({ "host_id": id.to_string() }),
        Some("host"),
        Some(id),
        Some(id),
    )
    .await?;
    Ok(Json(serde_json::json!({
        "task_id": task_id.to_string(),
        "summary": "Linux package upgrade queued"
    })))
}

pub async fn host_linux_reboot(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    use crate::tasks::enqueue::enqueue_task;

    host_os::require_maintenance_mode(&state.pool, id)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let task_id = enqueue_task(
        &state,
        "host.linux.reboot",
        serde_json::json!({ "host_id": id.to_string() }),
        Some("host"),
        Some(id),
        Some(id),
    )
    .await?;
    Ok(Json(serde_json::json!({
        "task_id": task_id.to_string(),
        "summary": "Host reboot queued"
    })))
}

pub async fn vm_guest_health(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<host_os::VmGuestHealthReport>, ApiError> {
    require_operator(&actor)?;
    host_os::vm_guest_health(&state.pool, &state.config, id)
        .await
        .map(Json)
        .map_err(ApiError::from_upstream)
}

pub async fn vm_guest_observability(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    host_os::vm_guest_observability(&state.pool, &state.config, id)
        .await
        .map(Json)
        .map_err(ApiError::from_upstream)
}

pub async fn vm_guest_services(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<host_os::VmGuestServicesReport>, ApiError> {
    require_operator(&actor)?;
    host_os::vm_guest_services(&state.pool, &state.config, id)
        .await
        .map(Json)
        .map_err(ApiError::from_upstream)
}

pub async fn vm_guest_service_action(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((id, unit, action)): Path<(Uuid, String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    host_os::vm_guest_service_action(&state.pool, &state.config, id, &unit, &action)
        .await
        .map(Json)
        .map_err(ApiError::from_upstream)
}

pub async fn vm_guest_network_get(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    host_os::vm_guest_network_get(&state.pool, &state.config, id)
        .await
        .map(Json)
        .map_err(ApiError::from_upstream)
}

#[derive(Debug, serde::Deserialize)]
pub struct GuestNetworkApplyBody {
    pub iface: String,
    pub address_cidr: String,
    #[serde(default)]
    pub gateway: Option<String>,
    #[serde(default)]
    pub replace: bool,
    #[serde(default)]
    pub dns: Vec<String>,
    #[serde(default)]
    pub routes: Vec<serde_json::Value>,
}

pub async fn vm_guest_network_apply(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<GuestNetworkApplyBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let req = serde_json::json!({
        "iface": body.iface,
        "address_cidr": body.address_cidr,
        "gateway": body.gateway,
        "replace": body.replace,
        "dns": body.dns,
        "routes": body.routes,
    });
    host_os::vm_guest_network_apply(&state.pool, &state.config, id, &req)
        .await
        .map(Json)
        .map_err(ApiError::from_upstream)
}

pub async fn vm_guest_sync_time(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    host_os::vm_guest_agent_action(&state.pool, &state.config, id, "sync_time")
        .await
        .map(Json)
        .map_err(ApiError::from_upstream)
}

pub async fn vm_guest_fstrim(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    host_os::vm_guest_agent_action(&state.pool, &state.config, id, "fstrim")
        .await
        .map(Json)
        .map_err(ApiError::from_upstream)
}

pub async fn vm_guest_fs_freeze_status(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    host_os::vm_guest_agent_action(&state.pool, &state.config, id, "fs_freeze_status")
        .await
        .map(Json)
        .map_err(ApiError::from_upstream)
}

#[derive(Debug, serde::Deserialize)]
pub struct GuestAiInsightsQuery {
    #[serde(default)]
    pub refresh: bool,
    pub focus: Option<String>,
}

pub async fn vm_guest_ai_insights(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<GuestAiInsightsQuery>,
) -> Result<Json<crate::engine::ai::guest_insights::GuestAiInsightsReport>, ApiError> {
    require_operator(&actor)?;
    crate::engine::ai::guest_insights::generate_insights(
        &state.pool,
        &state.config,
        id,
        q.refresh,
        q.focus.as_deref(),
    )
    .await
    .map(Json)
    .map_err(ApiError::from_upstream)
}

#[derive(Debug, serde::Deserialize)]
pub struct OsDiagnoseBody {
    pub query: Option<String>,
}

pub async fn diagnose_host(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<OsDiagnoseBody>,
) -> Result<Json<host_os::HostOsDiagnoseReport>, ApiError> {
    require_operator(&actor)?;
    host_os::diagnose_host(&state.pool, &state.config, id, body.query.as_deref())
        .await
        .map(Json)
        .map_err(ApiError::from_upstream)
}

pub async fn diagnose_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<OsDiagnoseBody>,
) -> Result<Json<host_os::VmOsDiagnoseReport>, ApiError> {
    require_operator(&actor)?;
    host_os::diagnose_vm(&state.pool, &state.config, id, body.query.as_deref())
        .await
        .map(Json)
        .map_err(ApiError::from_upstream)
}
