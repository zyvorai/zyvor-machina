// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use axum::Extension;

use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct LibvirtActionBody {
    pub action: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Deserialize)]
pub struct LibvirtQueryParams {
    pub action: String,
    #[serde(default)]
    pub disk: Option<String>,
    #[serde(default)]
    pub bandwidth_bytes: Option<bool>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub disk_only: Option<bool>,
    #[serde(default)]
    pub quiesce: Option<bool>,
    #[serde(default)]
    pub storage_mode: Option<String>,
    #[serde(default)]
    pub snapshot: Option<String>,
    #[serde(default)]
    pub snapshot_action: Option<String>,
}

async fn vm_agent_row(state: &AppState, vm_id: Uuid) -> Result<(String, Uuid), ApiError> {
    super::vm_row::vm_agent_row_libvirt(state, vm_id).await
}

pub async fn query_vm_libvirt(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<LibvirtQueryParams>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let (name, host_id) = vm_agent_row(&state, id).await?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut payload = serde_json::json!({});
    if let Some(disk) = q.disk {
        payload["disk"] = serde_json::Value::String(disk);
    }
    if let Some(bb) = q.bandwidth_bytes {
        payload["bandwidth_bytes"] = serde_json::Value::Bool(bb);
    }
    if let Some(name) = q.name.filter(|s| !s.trim().is_empty()) {
        payload["name"] = serde_json::Value::String(name);
    }
    if let Some(disk_only) = q.disk_only {
        payload["disk_only"] = serde_json::Value::Bool(disk_only);
    }
    if let Some(quiesce) = q.quiesce {
        payload["quiesce"] = serde_json::Value::Bool(quiesce);
    }
    if let Some(mode) = q.storage_mode.filter(|s| !s.trim().is_empty()) {
        payload["storage_mode"] = serde_json::Value::String(mode);
    }
    if let Some(snap) = q.snapshot.filter(|s| !s.trim().is_empty()) {
        payload["snapshot"] = serde_json::Value::String(snap);
    }
    if let Some(action) = q.snapshot_action.filter(|s| !s.trim().is_empty()) {
        payload["action"] = serde_json::Value::String(action);
    }
    let result = crate::agent_client::vm_libvirt_query(&mut client, &name, &q.action, &payload)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(result))
}

pub async fn invoke_vm_libvirt(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<LibvirtActionBody>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let (name, host_id) = vm_agent_row(&state, id).await?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let result =
        crate::agent_client::vm_libvirt_invoke(&mut client, &name, &body.action, &body.payload)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    state.emit_event(
        "vm.libvirt",
        format!("VM {name} libvirt action {}", body.action),
    );
    Ok(Json(result))
}

#[derive(Debug, Deserialize)]
pub struct PutDomainXmlBody {
    pub xml: String,
}

pub async fn put_vm_domain_xml(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<PutDomainXmlBody>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let (name, host_id) = vm_agent_row(&state, id).await?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let result = crate::agent_client::vm_libvirt_invoke(
        &mut client,
        &name,
        "domain.xml.update",
        &serde_json::json!({ "xml": body.xml }),
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    state.emit_event("vm.xml", format!("Updated domain XML for {name}"));
    Ok(Json(result))
}

#[derive(Debug, Deserialize)]
pub struct HostLibvirtQueryParams {
    pub action: String,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HostLibvirtActionBody {
    pub action: String,
    #[serde(default)]
    pub payload: Value,
}

pub async fn invoke_host_libvirt(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(host_id): Path<Uuid>,
    Json(body): Json<HostLibvirtActionBody>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let result = crate::agent_client::host_libvirt_invoke(&mut client, &body.action, &body.payload)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    state.emit_event(
        "host.libvirt",
        format!("Host {host_id} libvirt action {}", body.action),
    );
    Ok(Json(result))
}

pub async fn query_host_libvirt(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(host_id): Path<Uuid>,
    Query(q): Query<HostLibvirtQueryParams>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut payload = serde_json::json!({});
    if let Some(url) = q.url.filter(|s| !s.trim().is_empty()) {
        payload["url"] = serde_json::Value::String(url);
    }
    let result = crate::agent_client::host_libvirt_query(&mut client, &q.action, &payload)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(result))
}
