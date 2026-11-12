// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Thin Octavia L4 façade over native `/api/v1/load-balancers` (iptables WRR, no amphora).

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::api::load_balancers::{AddLbMemberBody, CreateLoadBalancerBody};
use crate::state::AppState;

use super::auth::{require_operator, OsActor};
use super::ids::parse_uuid;
use super::OsError;

fn lb_json(id: Uuid, name: &str, protocol: &str, port: i64, project: Uuid, host: Uuid) -> Value {
    json!({
        "id": id.to_string(),
        "name": name,
        "description": "",
        "provisioning_status": "ACTIVE",
        "operating_status": "ONLINE",
        "admin_state_up": true,
        "project_id": project.to_string(),
        "vip_address": null,
        "vip_port_id": null,
        "vip_subnet_id": null,
        "provider": "machina",
        "listeners": [{
            "id": format!("{id}-listener"),
            "name": format!("{name}-listener"),
            "protocol": protocol.to_uppercase(),
            "protocol_port": port,
            "admin_state_up": true,
        }],
        "pools": [{
            "id": format!("{id}-pool"),
            "name": format!("{name}-pool"),
            "protocol": protocol.to_uppercase(),
            "lb_algorithm": "ROUND_ROBIN",
            "admin_state_up": true,
        }],
        "machina_host_id": host.to_string(),
    })
}

pub async fn list_loadbalancers(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let Json(rows) = crate::api::load_balancers::list_load_balancers(
        State(state),
        Extension(actor.user.clone()),
    )
    .await?;
    let loadbalancers: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            lb_json(
                r.id,
                &r.name,
                &r.protocol,
                r.listener_port,
                r.project_id.unwrap_or(actor.project_id),
                r.host_id,
            )
        })
        .collect();
    Ok(Json(json!({ "loadbalancers": loadbalancers })))
}

pub async fn get_loadbalancer(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    let Json(r) = crate::api::load_balancers::get_load_balancer(
        State(state),
        Extension(actor.user.clone()),
        Path(id),
    )
    .await?;
    Ok(Json(json!({
        "loadbalancer": lb_json(
            r.id,
            &r.name,
            &r.protocol,
            r.listener_port,
            r.project_id.unwrap_or(actor.project_id),
            r.host_id,
        )
    })))
}

pub async fn create_loadbalancer(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Json(body): Json<Value>,
) -> Result<(axum::http::StatusCode, Json<Value>), OsError> {
    require_operator(&actor)?;
    let lb = body
        .get("loadbalancer")
        .cloned()
        .ok_or_else(|| OsError::bad_request("loadbalancer object required"))?;
    let name = lb
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| OsError::bad_request("name required"))?
        .to_string();
    let host = lb
        .get("machina_host_id")
        .or_else(|| lb.get("vip_network_id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| OsError::bad_request("machina_host_id (host UUID) required"))?;
    let host_id = parse_uuid(host)?;
    let port = lb
        .get("vip_port_id")
        .and_then(|v| v.as_i64())
        .or_else(|| {
            lb.pointer("/listeners/0/protocol_port")
                .and_then(|v| v.as_i64())
        })
        .unwrap_or(80) as u16;
    let protocol = lb
        .pointer("/listeners/0/protocol")
        .and_then(|v| v.as_str())
        .unwrap_or("TCP")
        .to_lowercase();
    let Json(r) = crate::api::load_balancers::create_load_balancer(
        State(state),
        Extension(actor.user.clone()),
        Json(CreateLoadBalancerBody {
            name,
            host_id,
            listener_port: port,
            protocol,
            project_id: Some(actor.project_id),
        }),
    )
    .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({
            "loadbalancer": lb_json(
                r.id,
                &r.name,
                &r.protocol,
                r.listener_port,
                r.project_id.unwrap_or(actor.project_id),
                r.host_id,
            )
        })),
    ))
}

pub async fn delete_loadbalancer(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    let _ = crate::api::load_balancers::delete_load_balancer(
        State(state),
        Extension(actor.user.clone()),
        Path(id),
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn list_members(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(pool_id): Path<String>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    // Pool id is `{lb_id}-pool` from lb_json.
    let lb_id = pool_id
        .strip_suffix("-pool")
        .unwrap_or(&pool_id)
        .to_string();
    let lb = parse_uuid(&lb_id)?;
    let Json(rows) = crate::api::load_balancers::list_lb_members(
        State(state),
        Extension(actor.user.clone()),
        Path(lb),
    )
    .await?;
    let members: Vec<Value> = rows
        .into_iter()
        .map(|m| {
            json!({
                "id": m.id.to_string(),
                "address": m.vm_ip.clone().unwrap_or_default(),
                "protocol_port": m.port,
                "weight": m.weight,
                "admin_state_up": m.enabled,
                "operating_status": if m.health == "healthy" { "ONLINE" } else { "OFFLINE" },
                "machina_vm_id": m.vm_id.to_string(),
            })
        })
        .collect();
    Ok(Json(json!({ "members": members })))
}

pub async fn create_member(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(pool_id): Path<String>,
    Json(body): Json<Value>,
) -> Result<(axum::http::StatusCode, Json<Value>), OsError> {
    require_operator(&actor)?;
    let lb_id = pool_id
        .strip_suffix("-pool")
        .unwrap_or(&pool_id)
        .to_string();
    let lb = parse_uuid(&lb_id)?;
    let mem = body
        .get("member")
        .cloned()
        .ok_or_else(|| OsError::bad_request("member object required"))?;
    let vm = mem
        .get("machina_vm_id")
        .or_else(|| mem.get("address"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| OsError::bad_request("machina_vm_id (VM UUID) required"))?;
    let vm_id = parse_uuid(vm)?;
    let port = mem
        .get("protocol_port")
        .and_then(|v| v.as_u64())
        .unwrap_or(80) as u16;
    let weight = mem.get("weight").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
    let Json(m) = crate::api::load_balancers::add_lb_member(
        State(state),
        Extension(actor.user.clone()),
        Path(lb),
        Json(AddLbMemberBody {
            vm_id,
            port,
            weight,
        }),
    )
    .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({
            "member": {
                "id": m.id.to_string(),
                "address": m.vm_ip.clone().unwrap_or_default(),
                "protocol_port": m.port,
                "weight": m.weight,
                "admin_state_up": m.enabled,
                "machina_vm_id": m.vm_id.to_string(),
            }
        })),
    ))
}

pub async fn delete_member(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path((pool_id, member_id)): Path<(String, String)>,
) -> Result<axum::http::StatusCode, OsError> {
    require_operator(&actor)?;
    let lb_id = pool_id
        .strip_suffix("-pool")
        .unwrap_or(&pool_id)
        .to_string();
    let lb = parse_uuid(&lb_id)?;
    let mid = parse_uuid(&member_id)?;
    let _ = crate::api::load_balancers::delete_lb_member(
        State(state),
        Extension(actor.user.clone()),
        Path((lb, mid)),
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
