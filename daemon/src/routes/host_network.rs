// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Extension, Path, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use machina_core::libvirt::{host_network, host_sysctl};
use machina_core::{audit, AuditEvent, LibvirtManager};
use serde::Deserialize;

use crate::auth::{require_browser_session_for_host_insight, require_write, RequestActor};
use crate::error::AppError;

// ── Host Interfaces ────────────────────────────────────────────────

async fn list_interfaces(
    State(_manager): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let interfaces = host_network::list_host_interfaces()?;
    Ok(Json(serde_json::json!(interfaces)))
}

async fn get_network_backends(
    State(_manager): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (net_backend, fw_backend) = host_network::get_detected_backends();
    Ok(Json(serde_json::json!({
        "network_backend": net_backend,
        "firewall_backend": fw_backend,
    })))
}

async fn get_sysctl_tuning(
    State(_manager): State<LibvirtManager>,
) -> Json<host_sysctl::SysctlTuningResponse> {
    Json(host_sysctl::sysctl_tuning_report())
}

async fn get_systemd_network_diagnostics(
    State(_manager): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let out = host_network::get_systemd_network_diagnostics()?;
    Ok(Json(serde_json::json!(out)))
}

async fn get_lldp_neighbors(
    State(_manager): State<LibvirtManager>,
) -> Json<host_network::LldpInventory> {
    Json(host_network::gather_lldp_neighbors())
}

async fn get_systemd_interface_status(
    State(_manager): State<LibvirtManager>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let out = host_network::get_systemd_interface_status(&name)?;
    Ok(Json(
        serde_json::json!({ "interface": name, "status": out }),
    ))
}

async fn get_host_routing_tables(
    State(_manager): State<LibvirtManager>,
) -> Result<Json<host_network::HostRoutingTables>, AppError> {
    let out = host_network::get_host_routing_tables()?;
    Ok(Json(out))
}

fn log_route_audit(req: &host_network::KernelRouteChangeRequest, result: &str) {
    let target = format!("{} {} {}", req.family, req.operation, req.destination);
    let event = AuditEvent {
        timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        action: "host-kernel-route".to_string(),
        target,
        result: result.to_string(),
        actor: String::new(),
    };
    audit::write_audit_event(&event);
}

async fn post_kernel_route_change(
    State(_manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<host_network::KernelRouteChangeRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browser_session_for_host_insight(&actor).map_err(AppError::from)?;
    // Adding/removing host kernel routes is a mutation (traffic redirection / DoS risk),
    // so gate on write role like every sibling handler in this module.
    require_write(&actor, "vms:write")?;
    host_network::modify_kernel_route(&req).map_err(AppError::from)?;
    log_route_audit(&req, "ok");
    Ok(Json(serde_json::json!({ "status": "ok" })))
}

// ── Bridges ────────────────────────────────────────────────────────

async fn create_bridge_handler(
    State(_manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<host_network::CreateBridgeRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_write() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Creating host bridges requires the operator or admin role.".into(),
        )
        .into());
    }
    host_network::create_bridge(&req)?;
    Ok(Json(
        serde_json::json!({ "status": "created", "name": req.name }),
    ))
}

async fn delete_bridge_handler(
    State(_manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_write() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Deleting host bridges requires the operator or admin role.".into(),
        )
        .into());
    }
    host_network::delete_bridge(&name)?;
    Ok(Json(
        serde_json::json!({ "status": "deleted", "name": name }),
    ))
}

// ── Port Forwarding ────────────────────────────────────────────────

async fn list_port_forwards(
    State(_manager): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let rules = host_network::list_port_forwards()?;
    Ok(Json(serde_json::json!(rules)))
}

async fn create_port_forward(
    State(_manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<host_network::CreatePortForwardRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_write() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Creating port forwards requires the operator or admin role.".into(),
        )
        .into());
    }
    host_network::create_port_forward(&req)?;
    Ok(Json(
        serde_json::json!({ "status": "created", "host_port": req.host_port, "vm_ip": req.vm_ip, "vm_port": req.vm_port }),
    ))
}

#[derive(Deserialize)]
struct DeletePortForwardRequest {
    protocol: String,
    host_port: u16,
    vm_ip: String,
    vm_port: u16,
}

async fn delete_port_forward(
    State(_manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<DeletePortForwardRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_write() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Deleting port forwards requires the operator or admin role.".into(),
        )
        .into());
    }
    host_network::delete_port_forward(&req.protocol, req.host_port, &req.vm_ip, req.vm_port)?;
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

// ── Firewall Rules ─────────────────────────────────────────────────

async fn list_firewall_rules(
    State(_manager): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let rules = host_network::list_firewall_rules()?;
    Ok(Json(serde_json::json!(rules)))
}

async fn create_firewall_rule(
    State(_manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<host_network::CreateFirewallRuleRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_write() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Creating firewall rules requires the operator or admin role.".into(),
        )
        .into());
    }
    host_network::create_firewall_rule(&req)?;
    Ok(Json(
        serde_json::json!({ "status": "created", "vm_ip": req.vm_ip, "action": req.action }),
    ))
}

async fn delete_firewall_rule(
    State(_manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<host_network::CreateFirewallRuleRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_write() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Deleting firewall rules requires the operator or admin role.".into(),
        )
        .into());
    }
    host_network::delete_firewall_rule(&req)?;
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

// ── Router ─────────────────────────────────────────────────────────

pub fn host_network_routes() -> Router<LibvirtManager> {
    Router::new()
        // Host interfaces + detected backends
        .route("/host/interfaces", get(list_interfaces))
        .route("/host/backends", get(get_network_backends))
        .route("/host/sysctl-tuning", get(get_sysctl_tuning))
        .route("/host/network-diag", get(get_systemd_network_diagnostics))
        .route("/host/lldp", get(get_lldp_neighbors))
        .route(
            "/host/network-diag/interface/{name}",
            get(get_systemd_interface_status),
        )
        .route("/host/routing-tables", get(get_host_routing_tables))
        .route("/host/routing", post(post_kernel_route_change))
        // Bridges
        .route("/host/bridges", post(create_bridge_handler))
        .route("/host/bridges/{name}", delete(delete_bridge_handler))
        // Port forwarding
        .route(
            "/portforward",
            get(list_port_forwards).post(create_port_forward),
        )
        .route("/portforward/delete", post(delete_port_forward))
        // Firewall
        .route(
            "/firewall",
            get(list_firewall_rules).post(create_firewall_rule),
        )
        .route("/firewall/delete", post(delete_firewall_rule))
}
