// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CreateVmPortForwardBody {
    pub protocol: String,
    pub host_port: u16,
    pub vm_port: u16,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct DeleteVmPortForwardBody {
    pub protocol: String,
    pub host_port: u16,
    pub vm_port: u16,
}

fn validate_vm_port_forward_fields(
    protocol: &str,
    host_port: u16,
    vm_port: u16,
) -> Result<(), ApiError> {
    use machina_core::libvirt::host_network::{
        validate_port_forward_host_port, validate_port_forward_protocol,
        validate_port_forward_vm_port,
    };
    validate_port_forward_protocol(protocol).map_err(|e| ApiError::bad_request(e.to_string()))?;
    validate_port_forward_host_port(host_port).map_err(|e| ApiError::bad_request(e.to_string()))?;
    validate_port_forward_vm_port(vm_port).map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(())
}

async fn vm_host_agent(state: &AppState, vm_id: Uuid) -> Result<(String, String), ApiError> {
    let row: (String, Option<Uuid>, String, Option<String>) = crate::db::query_as(
        "SELECT name, host_id, COALESCE(inventory_source, 'libvirt'), guest_ip FROM vms WHERE id = ?",
    )
    .bind(vm_id)
    .fetch_one(&state.pool)
    .await?;
    if row.2 == "kubevirt" {
        return Err(ApiError::bad_request(
            "Port forwarding is only available for libvirt-managed VMs",
        ));
    }
    let host_id = row
        .1
        .ok_or_else(|| ApiError::bad_request("VM has no host assigned"))?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok((row.3.unwrap_or_default(), agent_addr))
}

pub async fn list_vm_port_forwards(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<crate::agent_client::PortForwardRuleDto>>, ApiError> {
    require_operator(&actor)?;
    let (guest_ip, agent_addr) = vm_host_agent(&state, id).await?;
    let guest_ip = guest_ip.trim();
    if guest_ip.is_empty() {
        return Ok(Json(vec![]));
    }
    let rules = crate::agent_client::list_port_forwards(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let filtered: Vec<_> = rules.into_iter().filter(|r| r.vm_ip == guest_ip).collect();
    Ok(Json(filtered))
}

pub async fn create_vm_port_forward(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateVmPortForwardBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let (guest_ip, agent_addr) = vm_host_agent(&state, id).await?;
    let guest_ip = guest_ip.trim().to_string();
    if guest_ip.is_empty() {
        return Err(ApiError::bad_request(
            "Guest IP is not known yet — wait for DHCP or install guest tools",
        ));
    }
    let proto = body.protocol.to_lowercase();
    validate_vm_port_forward_fields(&proto, body.host_port, body.vm_port)?;
    crate::agent_client::create_port_forward(
        &agent_addr,
        &proto,
        body.host_port,
        &guest_ip,
        body.vm_port,
        &body.description,
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_vm_port_forward(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<DeleteVmPortForwardBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let (guest_ip, agent_addr) = vm_host_agent(&state, id).await?;
    let guest_ip = guest_ip.trim();
    if guest_ip.is_empty() {
        return Err(ApiError::bad_request("Guest IP is not known yet"));
    }
    let proto = body.protocol.to_lowercase();
    validate_vm_port_forward_fields(&proto, body.host_port, body.vm_port)?;
    crate::agent_client::delete_port_forward(
        &agent_addr,
        &proto,
        body.host_port,
        guest_ip,
        body.vm_port,
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PortForwardTemplateDto {
    pub id: String,
    pub name: String,
    pub vm_port: i32,
    pub host_port: i32,
    pub access: String,
}

fn read_port_forward_templates(spec: &serde_json::Value) -> Vec<PortForwardTemplateDto> {
    spec.get("machina")
        .and_then(|m| m.get("port_forward_templates"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

pub async fn list_vm_port_forward_templates(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<PortForwardTemplateDto>>, ApiError> {
    require_operator(&actor)?;
    let spec: serde_json::Value = crate::db::query_scalar("SELECT spec_json FROM vms WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|_| ApiError::not_found("VM not found"))?;
    Ok(Json(read_port_forward_templates(&spec)))
}

pub async fn upsert_vm_port_forward_template(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<PortForwardTemplateDto>,
) -> Result<Json<Vec<PortForwardTemplateDto>>, ApiError> {
    require_operator(&actor)?;
    if body.name.trim().is_empty() || body.id.trim().is_empty() {
        return Err(ApiError::bad_request("Template id and name are required"));
    }
    if body.vm_port <= 0 || body.host_port <= 0 {
        return Err(ApiError::bad_request("Ports must be positive"));
    }
    let mut spec: serde_json::Value = crate::db::query_scalar("SELECT spec_json FROM vms WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|_| ApiError::not_found("VM not found"))?;
    let mut templates = read_port_forward_templates(&spec);
    templates.retain(|t| t.id != body.id);
    templates.push(body);
    let machina = spec
        .as_object_mut()
        .and_then(|o| {
            if !o.contains_key("machina") {
                o.insert("machina".into(), serde_json::json!({}));
            }
            o.get_mut("machina").and_then(|v| v.as_object_mut())
        })
        .ok_or_else(|| ApiError::internal("Invalid VM spec"))?;
    machina.insert(
        "port_forward_templates".into(),
        serde_json::to_value(&templates).map_err(|e| ApiError::internal(e.to_string()))?,
    );
    crate::db::query("UPDATE vms SET spec_json = ?, updated_at = datetime('now') WHERE id = ?")
        .bind(&spec)
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(templates))
}
