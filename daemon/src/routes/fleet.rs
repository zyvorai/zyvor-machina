// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Multi-daemon fleet overview: peer health, aggregated VM lists, proxied lifecycle.

use axum::extract::{Extension, Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use machina_core::config::{FleetConfig, MachinaConfig};
use machina_core::libvirt::automation::{self, Alert};
use machina_core::{LibvirtManager, VmInfo};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tracing::warn;

use crate::auth::{require_api_scope, RequestActor};
use crate::error::AppError;

#[derive(serde::Serialize)]
struct FleetPeerStatus {
    name: String,
    url: String,
    reachable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vm_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    host_cpu_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    host_memory_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vms_running: Option<u32>,
}

#[derive(serde::Serialize)]
struct FleetVmRow {
    name: String,
    state: String,
    peer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    libvirt_connection: Option<String>,
}

fn fleet_cfg() -> FleetConfig {
    MachinaConfig::load().fleet
}

fn peer_client(peer: &machina_core::config::FleetPeer) -> Client {
    let mut b = Client::builder().timeout(Duration::from_secs(12));
    if peer.insecure_tls {
        b = b.danger_accept_invalid_certs(true);
    }
    b.build().unwrap_or_else(|_| Client::new())
}

async fn fetch_peer_text(
    peer: &machina_core::config::FleetPeer,
    path: &str,
) -> Result<String, String> {
    let base = peer.url.trim().trim_end_matches('/');
    let url = format!("{base}/api/v1{path}");
    let client = peer_client(peer);
    let mut req = client.get(&url);
    let token = peer.api_token.trim();
    if !token.is_empty() {
        req = req.bearer_auth(token);
    }
    let res = req.send().await.map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()));
    }
    res.text().await.map_err(|e| e.to_string())
}

async fn fetch_peer_json(
    peer: &machina_core::config::FleetPeer,
    path: &str,
) -> Result<Value, String> {
    let base = peer.url.trim().trim_end_matches('/');
    let url = format!("{base}/api/v1{path}");
    let client = peer_client(peer);
    let mut req = client.get(&url);
    let token = peer.api_token.trim();
    if !token.is_empty() {
        req = req.bearer_auth(token);
    }
    let res = req.send().await.map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()));
    }
    res.json().await.map_err(|e| e.to_string())
}

async fn fleet_status() -> Json<Value> {
    let cfg = fleet_cfg();
    if !cfg.is_enabled() {
        return Json(json!({
            "enabled": false,
            "peers": [],
            "primary_peer": cfg.primary_peer,
            "standby_peer": cfg.standby_peer,
        }));
    }
    let mut peers = Vec::new();
    for p in &cfg.peers {
        let mut row = FleetPeerStatus {
            name: p.name.clone(),
            url: p.url.clone(),
            reachable: false,
            error: None,
            version: None,
            vm_count: None,
            host_cpu_percent: None,
            host_memory_percent: None,
            vms_running: None,
        };
        match fetch_peer_json(p, "/system/platform-info").await {
            Ok(info) => {
                row.reachable = true;
                row.version = info
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                match fetch_peer_json(p, "/vms").await {
                    Ok(vms) => {
                        if let Some(arr) = vms.as_array() {
                            row.vm_count = Some(arr.len());
                            row.vms_running = Some(
                                arr.iter()
                                    .filter(|v| {
                                        v.get("state")
                                            .and_then(|s| s.as_str())
                                            .map(|s| s.eq_ignore_ascii_case("running"))
                                            .unwrap_or(false)
                                    })
                                    .count() as u32,
                            );
                        }
                    }
                    Err(e) => row.error = Some(format!("vms: {e}")),
                }
                if let Ok(stats) = fetch_peer_json(p, "/host/stats").await {
                    row.host_cpu_percent = stats.get("cpu_percent").and_then(|v| v.as_f64());
                    row.host_memory_percent = stats.get("memory_percent").and_then(|v| v.as_f64());
                }
            }
            Err(e) => row.error = Some(e),
        }
        peers.push(row);
    }
    Json(json!({
        "enabled": true,
        "primary_peer": cfg.primary_peer,
        "standby_peer": cfg.standby_peer,
        "peers": peers
    }))
}

async fn fleet_vms(State(manager): State<LibvirtManager>) -> Result<Json<Value>, AppError> {
    let cfg = fleet_cfg();
    let mut rows: Vec<FleetVmRow> = Vec::new();

    let local = manager.list_all_vms()?;
    for vm in local {
        rows.push(FleetVmRow {
            name: vm.name.clone(),
            state: vm.state.clone(),
            peer: "local".to_string(),
            libvirt_connection: vm.libvirt_connection.clone(),
        });
    }

    if cfg.is_enabled() {
        for p in &cfg.peers {
            match fetch_peer_json(p, "/vms").await {
                Ok(vms) => {
                    if let Some(arr) = vms.as_array() {
                        for item in arr {
                            if let Ok(vm) = serde_json::from_value::<VmInfo>(item.clone()) {
                                rows.push(FleetVmRow {
                                    name: vm.name,
                                    state: vm.state,
                                    peer: p.name.clone(),
                                    libvirt_connection: vm.libvirt_connection,
                                });
                            }
                        }
                    }
                }
                Err(e) => warn!("fleet peer {} vms: {}", p.name, e),
            }
        }
    }

    Ok(Json(json!({ "enabled": cfg.is_enabled(), "vms": rows })))
}

fn fleet_capacity(cpu: f64, mem: f64, disk: f64) -> Value {
    let (score, label) = machina_core::fleet_capacity_score(cpu, mem, disk);
    json!({
        "score": score,
        "label": label,
    })
}

fn synthetic_peer_unreachable_alert(peer_name: &str, peer_url: &str) -> Alert {
    Alert {
        id: format!("fleet-peer-{peer_name}-unreachable"),
        rule_name: "Fleet peer unreachable".into(),
        message: format!("Peer '{peer_name}' at {peer_url} is not reachable"),
        severity: "critical".into(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        acknowledged: false,
    }
}

async fn fleet_metrics(State(manager): State<LibvirtManager>) -> Result<Json<Value>, AppError> {
    let cfg = fleet_cfg();
    let local_stats = machina_core::libvirt::extras::get_host_stats();
    let local_vms = manager.list_all_vms()?;
    let local_running = local_vms
        .iter()
        .filter(|v| v.state.eq_ignore_ascii_case("running"))
        .count();
    let local_capacity = fleet_capacity(
        local_stats.cpu_percent,
        local_stats.memory_percent,
        local_stats.disk_percent,
    );
    let mut peers: Vec<Value> = Vec::new();
    if cfg.is_enabled() {
        for p in &cfg.peers {
            let mut row = json!({
                "name": p.name,
                "url": p.url,
                "reachable": false,
            });
            if let Ok(stats) = fetch_peer_json(p, "/host/stats").await {
                row["reachable"] = json!(true);
                let cpu = stats
                    .get("cpu_percent")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let mem = stats
                    .get("memory_percent")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let disk = stats
                    .get("disk_percent")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                row["host_cpu_percent"] = json!(cpu);
                row["host_memory_percent"] = json!(mem);
                row["host_disk_percent"] = json!(disk);
                row["load_1"] = stats.get("load_1").cloned().unwrap_or(json!(null));
                row["capacity"] = fleet_capacity(cpu, mem, disk);
            }
            if let Ok(vms) = fetch_peer_json(p, "/vms").await {
                row["reachable"] = json!(true);
                if let Some(arr) = vms.as_array() {
                    row["vm_count"] = json!(arr.len());
                    row["vms_running"] = json!(arr
                        .iter()
                        .filter(|v| {
                            v.get("state")
                                .and_then(|s| s.as_str())
                                .map(|s| s.eq_ignore_ascii_case("running"))
                                .unwrap_or(false)
                        })
                        .count());
                }
            }
            peers.push(row);
        }
    }
    Ok(Json(json!({
        "enabled": cfg.is_enabled(),
        "local": {
            "host_cpu_percent": local_stats.cpu_percent,
            "host_memory_percent": local_stats.memory_percent,
            "host_disk_percent": local_stats.disk_percent,
            "load_1": local_stats.load_1,
            "vm_count": local_vms.len(),
            "vms_running": local_running,
            "capacity": local_capacity,
        },
        "peers": peers,
    })))
}

#[derive(serde::Serialize)]
struct FleetAlertRow {
    peer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    alerts: Vec<Alert>,
    unacknowledged: usize,
}

#[derive(Deserialize)]
struct FleetPlacementRequest {
    #[serde(default = "default_placement_vcpus")]
    vcpus: u32,
    #[serde(default = "default_placement_memory_mb")]
    memory_mb: u64,
}

fn default_placement_vcpus() -> u32 {
    2
}

fn default_placement_memory_mb() -> u64 {
    2048
}

#[derive(Serialize)]
struct PlacementCandidate {
    peer: String,
    reachable: bool,
    recommended: bool,
    capacity: Value,
    host_cpu_percent: Option<f64>,
    host_memory_percent: Option<f64>,
    host_disk_percent: Option<f64>,
    vm_count: Option<usize>,
    vms_running: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn compute_placement_candidates(
    manager: &LibvirtManager,
    vcpus: u32,
    memory_mb: u64,
) -> Result<(bool, Vec<PlacementCandidate>), AppError> {
    let cfg = fleet_cfg();
    let local_stats = machina_core::libvirt::extras::get_host_stats();
    let local_vms = manager.list_all_vms()?;
    let (base_score, _) = machina_core::fleet_capacity_score(
        local_stats.cpu_percent,
        local_stats.memory_percent,
        local_stats.disk_percent,
    );
    let adjusted = machina_core::placement_adjusted_score(base_score, vcpus, memory_mb);
    let mut local_cap = fleet_capacity(
        local_stats.cpu_percent,
        local_stats.memory_percent,
        local_stats.disk_percent,
    );
    if let Some(obj) = local_cap.as_object_mut() {
        obj.insert("adjusted_score".into(), json!(adjusted));
    }
    let mut candidates = vec![PlacementCandidate {
        peer: "local".into(),
        reachable: true,
        recommended: false,
        capacity: local_cap,
        host_cpu_percent: Some(local_stats.cpu_percent),
        host_memory_percent: Some(local_stats.memory_percent),
        host_disk_percent: Some(local_stats.disk_percent),
        vm_count: Some(local_vms.len()),
        vms_running: Some(
            local_vms
                .iter()
                .filter(|v| v.state.eq_ignore_ascii_case("running"))
                .count() as u32,
        ),
        error: None,
    }];

    if cfg.is_enabled() {
        for p in &cfg.peers {
            let mut row = PlacementCandidate {
                peer: p.name.clone(),
                reachable: false,
                recommended: false,
                capacity: json!({}),
                host_cpu_percent: None,
                host_memory_percent: None,
                host_disk_percent: None,
                vm_count: None,
                vms_running: None,
                error: None,
            };
            if let Ok(stats) = fetch_peer_json(p, "/host/stats").await {
                row.reachable = true;
                let cpu = stats
                    .get("cpu_percent")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let mem = stats
                    .get("memory_percent")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let disk = stats
                    .get("disk_percent")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                row.host_cpu_percent = Some(cpu);
                row.host_memory_percent = Some(mem);
                row.host_disk_percent = Some(disk);
                let (base, _) = machina_core::fleet_capacity_score(cpu, mem, disk);
                let adj = machina_core::placement_adjusted_score(base, vcpus, memory_mb);
                let mut cap = fleet_capacity(cpu, mem, disk);
                if let Some(obj) = cap.as_object_mut() {
                    obj.insert("adjusted_score".into(), json!(adj));
                }
                row.capacity = cap;
            } else {
                row.error = match fetch_peer_json(p, "/system/platform-info").await {
                    Err(e) => Some(e),
                    Ok(_) => Some("host stats unavailable".into()),
                };
            }
            if row.reachable {
                if let Ok(vms) = fetch_peer_json(p, "/vms").await {
                    if let Some(arr) = vms.as_array() {
                        row.vm_count = Some(arr.len());
                        row.vms_running = Some(
                            arr.iter()
                                .filter(|v| {
                                    v.get("state")
                                        .and_then(|s| s.as_str())
                                        .map(|s| s.eq_ignore_ascii_case("running"))
                                        .unwrap_or(false)
                                })
                                .count() as u32,
                        );
                    }
                }
            }
            candidates.push(row);
        }
    }

    candidates.sort_by(|a, b| {
        let sa = a
            .capacity
            .get("adjusted_score")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let sb = b
            .capacity
            .get("adjusted_score")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    if let Some(best) = candidates.iter_mut().find(|c| c.reachable) {
        best.recommended = true;
    }

    Ok((cfg.is_enabled(), candidates))
}

async fn fleet_placement(
    State(manager): State<LibvirtManager>,
    Json(req): Json<FleetPlacementRequest>,
) -> Result<Json<Value>, AppError> {
    let (enabled, candidates) =
        compute_placement_candidates(&manager, req.vcpus, req.memory_mb).await?;
    Ok(Json(json!({
        "enabled": enabled,
        "request": { "vcpus": req.vcpus, "memory_mb": req.memory_mb },
        "candidates": candidates,
    })))
}

#[derive(Deserialize)]
struct FleetCreateVmRequest {
    #[serde(default)]
    peer: Option<String>,
    #[serde(default)]
    auto_place: bool,
    #[serde(default = "default_placement_vcpus")]
    placement_vcpus: u32,
    #[serde(default = "default_placement_memory_mb")]
    placement_memory_mb: u64,
    create: Value,
}

async fn fleet_create_vm(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Json(req): Json<FleetCreateVmRequest>,
) -> Result<Json<Value>, AppError> {
    require_api_scope(&actor, "fleet:proxy").map_err(AppError::from)?;
    if actor.role == machina_core::libvirt::automation::Role::ReadOnly {
        return Err(AppError::from(machina_core::LibvirtError::Forbidden(
            "Read-only role cannot create VMs on fleet peers".into(),
        )));
    }
    let cfg = fleet_cfg();
    if !cfg.is_enabled() && req.peer.as_deref() != Some("local") {
        return Err(AppError::from(machina_core::LibvirtError::NotFound(
            "Fleet is not enabled".into(),
        )));
    }

    let peer_name = match req.peer {
        Some(p) => p,
        None if req.auto_place => {
            let (_, candidates) = compute_placement_candidates(
                &manager,
                req.placement_vcpus,
                req.placement_memory_mb,
            )
            .await?;
            candidates
                .into_iter()
                .find(|c| c.recommended)
                .map(|c| c.peer)
                .ok_or_else(|| {
                    AppError::from(machina_core::LibvirtError::Operation(
                        "No reachable fleet peer for placement".into(),
                    ))
                })?
        }
        None => {
            return Err(AppError::from(machina_core::LibvirtError::Invalid(
                "Specify peer or set auto_place=true".into(),
            )));
        }
    };

    if peer_name == "local" {
        return Ok(Json(json!({
            "peer": "local",
            "proxied": false,
            "action": "create_local",
            "message": "Use POST /api/v1/vms on this daemon to create the VM locally.",
        })));
    }

    let peer = cfg
        .peers
        .iter()
        .find(|p| p.name == peer_name)
        .ok_or_else(|| {
            AppError::from(machina_core::LibvirtError::NotFound(format!(
                "Unknown fleet peer '{peer_name}'"
            )))
        })?;

    let base = peer.url.trim().trim_end_matches('/');
    let url = format!("{base}/api/v1/vms");
    let client = peer_client(peer);
    let mut rb = client.post(&url);
    let token = peer.api_token.trim();
    if !token.is_empty() {
        rb = rb.bearer_auth(token);
    }
    let res = rb
        .json(&req.create)
        .send()
        .await
        .map_err(|e| AppError::from(machina_core::LibvirtError::Operation(e.to_string())))?;
    let status = res.status().as_u16();
    let body: Value = res.json().await.unwrap_or(json!({}));
    Ok(Json(json!({
        "peer": peer_name,
        "proxied": true,
        "status": status,
        "body": body,
    })))
}

async fn fleet_alerts() -> Json<Value> {
    let cfg = fleet_cfg();
    let mut rows: Vec<FleetAlertRow> = Vec::new();
    let local = automation::load_alerts();
    let local_unacked = local.iter().filter(|a| !a.acknowledged).count();
    rows.push(FleetAlertRow {
        peer: "local".into(),
        error: None,
        unacknowledged: local_unacked,
        alerts: local,
    });

    if cfg.is_enabled() {
        for p in &cfg.peers {
            let mut row = FleetAlertRow {
                peer: p.name.clone(),
                error: None,
                alerts: Vec::new(),
                unacknowledged: 0,
            };
            let reachable = fetch_peer_json(p, "/system/platform-info").await.is_ok();
            if !reachable {
                let alert = synthetic_peer_unreachable_alert(&p.name, &p.url);
                row.unacknowledged = 1;
                row.alerts = vec![alert];
                row.error = Some("peer unreachable".into());
            } else {
                match fetch_peer_json(p, "/alerts").await {
                    Ok(body) => {
                        if let Ok(alerts) = serde_json::from_value::<Vec<Alert>>(body) {
                            row.unacknowledged = alerts.iter().filter(|a| !a.acknowledged).count();
                            row.alerts = alerts;
                        } else {
                            row.error = Some("invalid alerts JSON".into());
                        }
                    }
                    Err(e) => row.error = Some(e),
                }
            }
            rows.push(row);
        }
    }

    let total_unacked: usize = rows.iter().map(|r| r.unacknowledged).sum();
    Json(json!({
        "enabled": cfg.is_enabled(),
        "total_unacknowledged": total_unacked,
        "peers": rows,
    }))
}

fn prom_scrape_target(url: &str) -> (String, &'static str) {
    let u = url.trim().trim_end_matches('/');
    if let Some(rest) = u.strip_prefix("https://") {
        (rest.to_string(), "https")
    } else if let Some(rest) = u.strip_prefix("http://") {
        (rest.to_string(), "http")
    } else {
        (u.to_string(), "http")
    }
}

async fn fleet_prometheus_aggregate(
    State(manager): State<LibvirtManager>,
    Extension(stats): Extension<std::sync::Arc<crate::daemon_stats::DaemonStats>>,
    Extension(http_metrics): Extension<std::sync::Arc<crate::http_metrics::HttpMetrics>>,
) -> impl axum::response::IntoResponse {
    use axum::http::header;
    use machina_core::inject_peer_label;

    let cfg = fleet_cfg();
    let mut out = String::new();
    let local = crate::routes::prometheus::collect_prometheus_exposition(
        manager.clone(),
        stats.clone(),
        http_metrics.clone(),
    )
    .await;
    out.push_str(&format!(
        "# machina fleet prometheus aggregate (local + {} peers)\n",
        cfg.peers.len()
    ));
    out.push_str(&inject_peer_label(&local, "local"));
    if cfg.is_enabled() {
        for p in &cfg.peers {
            match fetch_peer_text(p, "/prometheus").await {
                Ok(text) => out.push_str(&inject_peer_label(&text, &p.name)),
                Err(e) => {
                    out.push_str(&format!(
                        "# peer {} unreachable for /prometheus: {e}\n",
                        p.name
                    ));
                }
            }
        }
    }
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        out,
    )
}

async fn fleet_prometheus_targets() -> Json<Value> {
    let cfg = fleet_cfg();
    let machina = MachinaConfig::load();
    let metrics_path = "/api/v1/prometheus";
    let (local_target, local_scheme) = prom_scrape_target(&machina.daemon_url());
    let local_target = if local_target.starts_with("0.0.0.0:") {
        local_target.replacen("0.0.0.0", "127.0.0.1", 1)
    } else {
        local_target
    };

    let mut static_configs: Vec<Value> = Vec::new();
    static_configs.push(json!({
        "targets": [local_target],
        "labels": { "machina_peer": "local" }
    }));

    if cfg.is_enabled() {
        for p in &cfg.peers {
            let (target, _) = prom_scrape_target(&p.url);
            static_configs.push(json!({
                "targets": [target],
                "labels": { "machina_peer": p.name }
            }));
        }
    }

    Json(json!({
        "enabled": cfg.is_enabled(),
        "metrics_path": metrics_path,
        "note": "Prometheus scrape requires Bearer API token; add authorization to scrape_config or use a dedicated read-only token.",
        "scrape_configs": [{
            "job_name": "machina-fleet",
            "metrics_path": metrics_path,
            "scheme": local_scheme,
            "static_configs": static_configs,
        }]
    }))
}

#[derive(Deserialize)]
struct FleetProxyBody {
    method: String,
    path: String,
    #[serde(default)]
    body: Option<Value>,
}

async fn fleet_proxy_action(
    Extension(actor): Extension<RequestActor>,
    Path(peer_name): Path<String>,
    Json(req): Json<FleetProxyBody>,
) -> Result<Json<Value>, AppError> {
    require_api_scope(&actor, "fleet:proxy").map_err(AppError::from)?;
    if actor.role == machina_core::libvirt::automation::Role::ReadOnly {
        return Err(AppError::from(machina_core::LibvirtError::Forbidden(
            "Read-only role cannot proxy fleet actions".into(),
        )));
    }
    let cfg = fleet_cfg();
    if !cfg.is_enabled() {
        return Err(AppError::from(machina_core::LibvirtError::NotFound(
            "Fleet is not enabled".into(),
        )));
    }
    let peer = cfg
        .peers
        .iter()
        .find(|p| p.name == peer_name)
        .ok_or_else(|| {
            AppError::from(machina_core::LibvirtError::NotFound(format!(
                "Unknown fleet peer '{peer_name}'"
            )))
        })?;

    let method = req.method.to_uppercase();
    if !matches!(method.as_str(), "GET" | "POST" | "DELETE") {
        return Err(AppError::from(machina_core::LibvirtError::Invalid(
            "method must be GET, POST, or DELETE".into(),
        )));
    }
    let path = req.path.trim();
    if !path.starts_with('/') || path.contains("..") {
        return Err(AppError::from(machina_core::LibvirtError::Invalid(
            "path must start with / and must not contain ..".into(),
        )));
    }

    let base = peer.url.trim().trim_end_matches('/');
    let url = format!("{base}/api/v1{path}");
    let client = peer_client(peer);
    let mut rb = match method.as_str() {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "DELETE" => client.delete(&url),
        _ => unreachable!(),
    };
    let token = peer.api_token.trim();
    if !token.is_empty() {
        rb = rb.bearer_auth(token);
    }
    if let Some(body) = req.body {
        rb = rb.json(&body);
    }
    let res = rb
        .send()
        .await
        .map_err(|e| AppError::from(machina_core::LibvirtError::Operation(e.to_string())))?;
    let status = res.status().as_u16();
    let body: Value = res.json().await.unwrap_or(json!({}));
    Ok(Json(
        json!({ "peer": peer_name, "status": status, "body": body }),
    ))
}

pub fn fleet_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/fleet/status", get(fleet_status))
        .route("/fleet/metrics", get(fleet_metrics))
        .route("/fleet/alerts", get(fleet_alerts))
        .route("/fleet/placement", post(fleet_placement))
        .route("/fleet/create-vm", post(fleet_create_vm))
        .route("/fleet/prometheus-targets", get(fleet_prometheus_targets))
        .route("/fleet/prometheus", get(fleet_prometheus_aggregate))
        .route("/fleet/vms", get(fleet_vms))
        .route("/fleet/peers/{peer}/proxy", post(fleet_proxy_action))
}
