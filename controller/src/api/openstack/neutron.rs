// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::api::cloud::network::{CreateSubnet, CreateVpc};
use crate::api::elastic_ips::{Allocate, Associate};
use crate::api::networking::{
    CreateSecurityGroupBody, CreateSecurityGroupRuleBody, ListPortsQuery,
};
use crate::state::AppState;
use axum::extract::Query;

use super::auth::{require_operator, OsActor};
use super::ids::parse_uuid;
use super::OsError;

pub async fn list_networks(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let Json(vpcs) = crate::api::cloud::network::list_vpcs(
        State(state),
        Extension(actor.user.clone()),
        Path(actor.project_id),
    )
    .await?;
    let networks: Vec<Value> = vpcs
        .into_iter()
        .map(|v| {
            json!({
                "id": v.id.to_string(),
                "name": v.name,
                "tenant_id": v.project_id.to_string(),
                "project_id": v.project_id.to_string(),
                "status": "ACTIVE",
                "admin_state_up": true,
                "shared": false,
                "router:external": false,
                "provider:network_type": "local",
                "mtu": 1500,
                "subnets": [],
            })
        })
        .collect();
    Ok(Json(json!({ "networks": networks })))
}

pub async fn get_network(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    let Json(v) = crate::api::cloud::network::get_vpc(
        State(state),
        Extension(actor.user.clone()),
        Path(id),
    )
    .await?;
    Ok(Json(json!({
        "network": {
            "id": v.id.to_string(),
            "name": v.name,
            "tenant_id": v.project_id.to_string(),
            "project_id": v.project_id.to_string(),
            "status": "ACTIVE",
            "admin_state_up": true,
            "shared": false,
            "router:external": false,
            "provider:network_type": "local",
            "mtu": 1500,
            "subnets": [],
        }
    })))
}

pub async fn create_network(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Json(body): Json<Value>,
) -> Result<(axum::http::StatusCode, Json<Value>), OsError> {
    require_operator(&actor)?;
    let net = body
        .get("network")
        .cloned()
        .ok_or_else(|| OsError::bad_request("network object required"))?;
    let name = net
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| OsError::bad_request("name required"))?
        .to_string();
    let cidr = net
        .get("machina:cidr")
        .or_else(|| net.pointer("/provider:segmentation_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("10.42.0.0/16")
        .to_string();
    let host_id = match net.get("machina:host_id").and_then(|v| v.as_str()) {
        Some(h) => parse_uuid(h)?,
        None => {
            crate::db::query_scalar("SELECT id FROM hosts WHERE state = 'online' ORDER BY hostname LIMIT 1")
                .fetch_optional(&state.pool)
                .await?
                .ok_or_else(|| OsError::bad_request("no online host; set machina:host_id"))?
        }
    };
    let create: CreateVpc = serde_json::from_value(json!({
        "name": name,
        "cidr": cidr,
        "host_id": host_id,
    }))
    .map_err(|e| OsError::bad_request(e.to_string()))?;
    let Json(v) = crate::api::cloud::network::create_vpc(
        State(state),
        Extension(actor.user.clone()),
        Path(actor.project_id),
        Json(create),
    )
    .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({
            "network": {
                "id": v.id.to_string(),
                "name": v.name,
                "tenant_id": v.project_id.to_string(),
                "project_id": v.project_id.to_string(),
                "status": "ACTIVE",
                "admin_state_up": true,
                "shared": false,
                "router:external": false,
                "provider:network_type": "local",
                "mtu": 1500,
                "subnets": [],
            }
        })),
    ))
}

pub async fn delete_network(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    let _ = crate::api::cloud::network::delete_vpc(
        State(state),
        Extension(actor.user.clone()),
        Path(id),
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn list_subnets(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let Json(vpcs) = crate::api::cloud::network::list_vpcs(
        State(state.clone()),
        Extension(actor.user.clone()),
        Path(actor.project_id),
    )
    .await?;
    let mut subnets = Vec::new();
    for v in vpcs {
        let rows: Vec<(Uuid, String, String, Uuid)> = crate::db::query_as(
            "SELECT id, name, cidr, vpc_id FROM cloud_subnets WHERE vpc_id = ? ORDER BY name",
        )
        .bind(v.id)
        .fetch_all(&state.pool)
        .await?;
        for (id, name, cidr, vpc_id) in rows {
            subnets.push(json!({
                "id": id.to_string(),
                "name": name,
                "network_id": vpc_id.to_string(),
                "tenant_id": actor.project_id.to_string(),
                "project_id": actor.project_id.to_string(),
                "ip_version": 4,
                "cidr": cidr,
                "gateway_ip": null,
                "enable_dhcp": true,
                "allocation_pools": [],
                "dns_nameservers": [],
                "host_routes": [],
            }));
        }
    }
    Ok(Json(json!({ "subnets": subnets })))
}

pub async fn get_subnet(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    let row: Option<(Uuid, String, String, Uuid)> =
        crate::db::query_as("SELECT id, name, cidr, vpc_id FROM cloud_subnets WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let (id, name, cidr, vpc_id) =
        row.ok_or_else(|| OsError::not_found("subnet not found"))?;
    Ok(Json(json!({
        "subnet": {
            "id": id.to_string(),
            "name": name,
            "network_id": vpc_id.to_string(),
            "tenant_id": actor.project_id.to_string(),
            "project_id": actor.project_id.to_string(),
            "ip_version": 4,
            "cidr": cidr,
            "gateway_ip": null,
            "enable_dhcp": true,
            "allocation_pools": [],
            "dns_nameservers": [],
            "host_routes": [],
        }
    })))
}

pub async fn create_subnet(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Json(body): Json<Value>,
) -> Result<(axum::http::StatusCode, Json<Value>), OsError> {
    require_operator(&actor)?;
    let sub = body
        .get("subnet")
        .cloned()
        .ok_or_else(|| OsError::bad_request("subnet object required"))?;
    let network_id = sub
        .get("network_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| OsError::bad_request("network_id required"))?;
    let network_id = parse_uuid(network_id)?;
    let name = sub
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("subnet")
        .to_string();
    let cidr = sub
        .get("cidr")
        .and_then(|v| v.as_str())
        .ok_or_else(|| OsError::bad_request("cidr required"))?
        .to_string();
    let create: CreateSubnet = serde_json::from_value(json!({
        "name": name,
        "cidr": cidr,
    }))
    .map_err(|e| OsError::bad_request(e.to_string()))?;
    let Json(val) = crate::api::cloud::network::create_subnet(
        State(state),
        Extension(actor.user.clone()),
        Path(network_id),
        Json(create),
    )
    .await?;
    let id = val
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({
            "subnet": {
                "id": id,
                "name": name,
                "network_id": network_id.to_string(),
                "tenant_id": actor.project_id.to_string(),
                "project_id": actor.project_id.to_string(),
                "ip_version": 4,
                "cidr": cidr,
                "gateway_ip": null,
                "enable_dhcp": true,
                "allocation_pools": [],
                "dns_nameservers": [],
                "host_routes": [],
            }
        })),
    ))
}

pub async fn delete_subnet(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    let _ = crate::api::cloud::network::delete_subnet(
        State(state),
        Extension(actor.user.clone()),
        Path(id),
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn list_ports(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let Json(rows) = crate::api::networking::list_ports(
        State(state),
        Extension(actor.user.clone()),
        Query(ListPortsQuery {
            network_id: None,
            vm_id: None,
        }),
    )
    .await?;
    let ports: Vec<Value> = rows
        .into_iter()
        .map(|p| {
            json!({
                "id": p.id.to_string(),
                "name": p.description,
                "status": p.status,
                "admin_state_up": true,
                "network_id": p.network_id.to_string(),
                "tenant_id": actor.project_id.to_string(),
                "project_id": actor.project_id.to_string(),
                "mac_address": p.mac_address.clone().unwrap_or_default(),
                "fixed_ips": p.private_ip.as_ref().map(|ip| vec![json!({"ip_address": ip, "subnet_id": p.subnet_id})]).unwrap_or_default(),
                "device_id": p.vm_id.map(|v| v.to_string()).unwrap_or_default(),
                "device_owner": "",
                "security_groups": p.security_group_id.map(|g| vec![g.to_string()]).unwrap_or_default(),
            })
        })
        .collect();
    Ok(Json(json!({ "ports": ports })))
}

pub async fn get_port(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    let Json(p) =
        crate::api::networking::get_port(State(state), Extension(actor.user.clone()), Path(id))
            .await?;
    Ok(Json(json!({
        "port": {
            "id": p.id.to_string(),
            "name": p.description,
            "status": p.status,
            "admin_state_up": true,
            "network_id": p.network_id.to_string(),
            "tenant_id": actor.project_id.to_string(),
            "project_id": actor.project_id.to_string(),
            "mac_address": p.mac_address.clone().unwrap_or_default(),
            "fixed_ips": p.private_ip.as_ref().map(|ip| vec![json!({"ip_address": ip, "subnet_id": p.subnet_id})]).unwrap_or_default(),
            "device_id": p.vm_id.map(|v| v.to_string()).unwrap_or_default(),
            "device_owner": "",
            "security_groups": p.security_group_id.map(|g| vec![g.to_string()]).unwrap_or_default(),
        }
    })))
}

pub async fn list_security_groups(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let Json(rows) = crate::api::networking::list_security_groups(
        State(state.clone()),
        Extension(actor.user.clone()),
    )
    .await?;
    let mut groups = Vec::new();
    for g in rows {
        let Json(rules) = crate::api::networking::list_security_group_rules(
            State(state.clone()),
            Extension(actor.user.clone()),
            Path(g.id),
        )
        .await?;
        let security_group_rules: Vec<Value> = rules
            .into_iter()
            .map(|r| {
                json!({
                    "id": r.id.to_string(),
                    "security_group_id": r.security_group_id.to_string(),
                    "direction": r.direction,
                    "ethertype": "IPv4",
                    "protocol": r.protocol,
                    "port_range_min": r.port_min,
                    "port_range_max": r.port_max,
                    "remote_ip_prefix": r.remote_cidr,
                    "remote_group_id": null,
                    "tenant_id": actor.project_id.to_string(),
                    "project_id": actor.project_id.to_string(),
                    "description": r.description,
                })
            })
            .collect();
        groups.push(json!({
            "id": g.id.to_string(),
            "name": g.name,
            "description": g.description,
            "tenant_id": actor.project_id.to_string(),
            "project_id": actor.project_id.to_string(),
            "security_group_rules": security_group_rules,
        }));
    }
    Ok(Json(json!({ "security_groups": groups })))
}

pub async fn get_security_group(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    let Json(g) = crate::api::networking::get_security_group(
        State(state.clone()),
        Extension(actor.user.clone()),
        Path(id),
    )
    .await?;
    let Json(rules) = crate::api::networking::list_security_group_rules(
        State(state),
        Extension(actor.user.clone()),
        Path(g.id),
    )
    .await?;
    let security_group_rules: Vec<Value> = rules
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id.to_string(),
                "security_group_id": r.security_group_id.to_string(),
                "direction": r.direction,
                "ethertype": "IPv4",
                "protocol": r.protocol,
                "port_range_min": r.port_min,
                "port_range_max": r.port_max,
                "remote_ip_prefix": r.remote_cidr,
                "remote_group_id": null,
                "description": r.description,
            })
        })
        .collect();
    Ok(Json(json!({
        "security_group": {
            "id": g.id.to_string(),
            "name": g.name,
            "description": g.description,
            "tenant_id": actor.project_id.to_string(),
            "project_id": actor.project_id.to_string(),
            "security_group_rules": security_group_rules,
        }
    })))
}

pub async fn create_security_group(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Json(body): Json<Value>,
) -> Result<(axum::http::StatusCode, Json<Value>), OsError> {
    require_operator(&actor)?;
    let sg = body
        .get("security_group")
        .cloned()
        .ok_or_else(|| OsError::bad_request("security_group object required"))?;
    let name = sg
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| OsError::bad_request("name required"))?
        .to_string();
    let Json(g) = crate::api::networking::create_security_group(
        State(state),
        Extension(actor.user.clone()),
        Json(CreateSecurityGroupBody {
            name,
            description: sg
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .into(),
            project_id: Some(actor.project_id),
        }),
    )
    .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({
            "security_group": {
                "id": g.id.to_string(),
                "name": g.name,
                "description": g.description,
                "tenant_id": actor.project_id.to_string(),
                "project_id": actor.project_id.to_string(),
                "security_group_rules": [],
            }
        })),
    ))
}

pub async fn delete_security_group(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    let _ = crate::api::networking::delete_security_group(
        State(state),
        Extension(actor.user.clone()),
        Path(id),
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn list_security_group_rules(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let Json(groups) = crate::api::networking::list_security_groups(
        State(state.clone()),
        Extension(actor.user.clone()),
    )
    .await?;
    let mut rules_out = Vec::new();
    for g in groups {
        let Json(rules) = crate::api::networking::list_security_group_rules(
            State(state.clone()),
            Extension(actor.user.clone()),
            Path(g.id),
        )
        .await?;
        for r in rules {
            rules_out.push(json!({
                "id": r.id.to_string(),
                "security_group_id": r.security_group_id.to_string(),
                "direction": r.direction,
                "ethertype": "IPv4",
                "protocol": r.protocol,
                "port_range_min": r.port_min,
                "port_range_max": r.port_max,
                "remote_ip_prefix": r.remote_cidr,
                "remote_group_id": null,
                "description": r.description,
            }));
        }
    }
    Ok(Json(json!({ "security_group_rules": rules_out })))
}

pub async fn create_security_group_rule(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Json(body): Json<Value>,
) -> Result<(axum::http::StatusCode, Json<Value>), OsError> {
    require_operator(&actor)?;
    let rule = body
        .get("security_group_rule")
        .cloned()
        .ok_or_else(|| OsError::bad_request("security_group_rule object required"))?;
    let sg = rule
        .get("security_group_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| OsError::bad_request("security_group_id required"))?;
    let sg = parse_uuid(sg)?;
    let Json(r) = crate::api::networking::create_security_group_rule(
        State(state),
        Extension(actor.user.clone()),
        Path(sg),
        Json(CreateSecurityGroupRuleBody {
            direction: rule
                .get("direction")
                .and_then(|v| v.as_str())
                .unwrap_or("ingress")
                .into(),
            protocol: rule
                .get("protocol")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            port_min: rule.get("port_range_min").and_then(|v| v.as_i64()).map(|n| n as i32),
            port_max: rule.get("port_range_max").and_then(|v| v.as_i64()).map(|n| n as i32),
            remote_cidr: rule
                .get("remote_ip_prefix")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            remote_sg_id: rule
                .get("remote_group_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            description: rule
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .into(),
        }),
    )
    .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({
            "security_group_rule": {
                "id": r.id.to_string(),
                "security_group_id": r.security_group_id.to_string(),
                "direction": r.direction,
                "ethertype": "IPv4",
                "protocol": r.protocol,
                "port_range_min": r.port_min,
                "port_range_max": r.port_max,
                "remote_ip_prefix": r.remote_cidr,
                "remote_group_id": null,
                "description": r.description,
            }
        })),
    ))
}

pub async fn delete_security_group_rule(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    let _ = crate::api::networking::delete_security_group_rule(
        State(state),
        Extension(actor.user.clone()),
        Path(id),
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

fn fip_json(id: Uuid, address: &str, vm_id: Option<Uuid>, project: Uuid) -> Value {
    json!({
        "id": id.to_string(),
        "floating_ip_address": address,
        "floating_network_id": null,
        "router_id": null,
        "port_id": null,
        "fixed_ip_address": null,
        "status": if vm_id.is_some() { "ACTIVE" } else { "DOWN" },
        "project_id": project.to_string(),
        "tenant_id": project.to_string(),
        "description": "",
        "dns_domain": null,
        "dns_name": null,
        "port_details": null,
        "tags": [],
        "created_at": "",
        "updated_at": "",
        "revision_number": 0,
        "machina_vm_id": vm_id.map(|v| v.to_string()),
    })
}

pub async fn list_floating_ips(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let Json(rows) =
        crate::api::elastic_ips::list(State(state), Extension(actor.user.clone())).await?;
    let floatingips: Vec<Value> = rows
        .into_iter()
        .map(|e| fip_json(e.id, &e.address, e.vm_id, actor.project_id))
        .collect();
    Ok(Json(json!({ "floatingips": floatingips })))
}

pub async fn get_floating_ip(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let eip = crate::api::elastic_ips::find(&state.pool, &id).await?;
    Ok(Json(json!({
        "floatingip": fip_json(eip.id, &eip.address, eip.vm_id, actor.project_id)
    })))
}

pub async fn create_floating_ip(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Json(body): Json<Value>,
) -> Result<(axum::http::StatusCode, Json<Value>), OsError> {
    require_operator(&actor)?;
    let _fip = body.get("floatingip");
    let Json(eip) = crate::api::elastic_ips::allocate(
        State(state),
        Extension(actor.user.clone()),
        Json(Allocate { pool: None }),
    )
    .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({
            "floatingip": fip_json(eip.id, &eip.address, eip.vm_id, actor.project_id)
        })),
    ))
}

pub async fn update_floating_ip(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let fip = body
        .get("floatingip")
        .cloned()
        .ok_or_else(|| OsError::bad_request("floatingip object required"))?;
    if let Some(port) = fip.get("port_id") {
        if port.is_null() {
            let Json(eip) = crate::api::elastic_ips::disassociate(
                State(state),
                Extension(actor.user.clone()),
                Path(id),
            )
            .await?;
            return Ok(Json(json!({
                "floatingip": fip_json(eip.id, &eip.address, eip.vm_id, actor.project_id)
            })));
        }
        // Machina associates by VM id via machina_vm_id or port_id-as-vm-uuid
        let vm = fip
            .get("machina_vm_id")
            .or(Some(port))
            .and_then(|v| v.as_str())
            .ok_or_else(|| OsError::bad_request("machina_vm_id or port_id (vm uuid) required"))?;
        let vm = parse_uuid(vm)?;
        let Json(eip) = crate::api::elastic_ips::associate(
            State(state),
            Extension(actor.user.clone()),
            Path(id),
            Json(Associate { vm_id: vm }),
        )
        .await?;
        return Ok(Json(json!({
            "floatingip": fip_json(eip.id, &eip.address, eip.vm_id, actor.project_id)
        })));
    }
    Err(OsError::bad_request("port_id update required"))
}

pub async fn delete_floating_ip(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, OsError> {
    require_operator(&actor)?;
    let _ = crate::api::elastic_ips::release(
        State(state),
        Extension(actor.user.clone()),
        Path(id),
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
