// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Host Linux OS surfaces — proxy agent RPC for enrolled hypervisors (Phase 33).

use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

use crate::agent_client;
use crate::config::ControllerConfig;

pub async fn resolve_agent_addr(
    pool: &DbPool,
    cfg: &ControllerConfig,
    host_id: Uuid,
) -> anyhow::Result<(String, String)> {
    let row: (String, String) = crate::db::query_as(
        "SELECT hostname, COALESCE(NULLIF(agent_grpc_addr, ''), '') FROM hosts WHERE id = ?",
    )
    .bind(host_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("host not found"))?;
    let addr = if row.1.is_empty() {
        cfg.default_agent_addr.clone()
    } else {
        row.1
    };
    if addr.is_empty() {
        anyhow::bail!("no agent address for host {}", row.0);
    }
    Ok((row.0, addr))
}

pub async fn linux_observability(
    pool: &DbPool,
    cfg: &ControllerConfig,
    host_id: Uuid,
) -> anyhow::Result<serde_json::Value> {
    let (_, addr) = resolve_agent_addr(pool, cfg, host_id).await?;
    agent_client::get_linux_observability(&addr).await
}

pub async fn network_diagnostics(
    pool: &DbPool,
    cfg: &ControllerConfig,
    host_id: Uuid,
) -> anyhow::Result<serde_json::Value> {
    let (hostname, addr) = match resolve_agent_addr(pool, cfg, host_id).await {
        Ok(v) => v,
        Err(e) => {
            return Ok(serde_json::json!({
                "agent_reachable": false,
                "hostname": hostname_from_pool(pool, host_id).await.unwrap_or_default(),
                "summary": format!("Network diagnostics unavailable: {e}"),
                "interfaces": [],
                "routes": [],
            }));
        }
    };
    match agent_client::get_systemd_network_diagnostics(&addr).await {
        Ok(v) => Ok(normalize_network_diagnostics(v)),
        Err(e) => {
            tracing::warn!("network diagnostics for {hostname} via {addr}: {e}");
            Ok(serde_json::json!({
                "agent_reachable": false,
                "hostname": hostname,
                "summary": format!("Network diagnostics unavailable: {e}"),
                "interfaces": [],
                "routes": [],
            }))
        }
    }
}

async fn hostname_from_pool(pool: &DbPool, host_id: Uuid) -> anyhow::Result<String> {
    crate::db::query_scalar("SELECT hostname FROM hosts WHERE id = ?")
        .bind(host_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("host not found"))
}

pub async fn linux_audit(
    pool: &DbPool,
    cfg: &ControllerConfig,
    host_id: Uuid,
) -> anyhow::Result<serde_json::Value> {
    let (_, addr) = resolve_agent_addr(pool, cfg, host_id).await?;
    let raw = agent_client::get_linux_audit(&addr).await?;
    Ok(normalize_linux_audit(raw))
}

pub async fn linux_package_updates(
    pool: &DbPool,
    cfg: &ControllerConfig,
    host_id: Uuid,
) -> anyhow::Result<serde_json::Value> {
    let (_, addr) = resolve_agent_addr(pool, cfg, host_id).await?;
    let raw = agent_client::get_linux_package_updates(&addr).await?;
    Ok(normalize_linux_package_updates(raw))
}

pub async fn linux_filesystems(
    pool: &DbPool,
    cfg: &ControllerConfig,
    host_id: Uuid,
) -> anyhow::Result<serde_json::Value> {
    let (_, addr) = resolve_agent_addr(pool, cfg, host_id).await?;
    let rows = agent_client::get_linux_filesystems(&addr).await?;
    Ok(serde_json::json!({ "filesystems": rows }))
}

pub async fn linux_top_processes(
    pool: &DbPool,
    cfg: &ControllerConfig,
    host_id: Uuid,
    limit: u32,
    order: &str,
) -> anyhow::Result<serde_json::Value> {
    let (_, addr) = resolve_agent_addr(pool, cfg, host_id).await?;
    let rows = agent_client::get_linux_top_processes(&addr, limit, order).await?;
    Ok(serde_json::json!({ "processes": rows }))
}

pub async fn require_maintenance_mode(pool: &DbPool, host_id: Uuid) -> anyhow::Result<()> {
    let maintenance: bool = crate::db::query_scalar("SELECT maintenance_mode FROM hosts WHERE id = ?")
        .bind(host_id)
        .fetch_one(pool)
        .await?;
    if !maintenance {
        anyhow::bail!("host must be in maintenance mode before package apply or reboot");
    }
    Ok(())
}

pub async fn apply_linux_package_upgrade(
    pool: &DbPool,
    cfg: &ControllerConfig,
    host_id: Uuid,
    dry_run: bool,
) -> anyhow::Result<serde_json::Value> {
    if !dry_run {
        require_maintenance_mode(pool, host_id).await?;
    }
    let (_, addr) = resolve_agent_addr(pool, cfg, host_id).await?;
    agent_client::apply_linux_package_upgrade(&addr, dry_run).await
}

pub async fn reboot_linux_host(
    pool: &DbPool,
    cfg: &ControllerConfig,
    host_id: Uuid,
) -> anyhow::Result<()> {
    require_maintenance_mode(pool, host_id).await?;
    let (_, addr) = resolve_agent_addr(pool, cfg, host_id).await?;
    agent_client::host_linux_reboot(&addr).await
}

pub fn normalize_linux_audit(raw: serde_json::Value) -> serde_json::Value {
    let events = raw
        .get("events")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let avc = raw.get("avc_count").and_then(|v| v.as_u64()).unwrap_or(0);
    let available = raw
        .get("available")
        .and_then(|v| v.as_bool())
        .unwrap_or(!events.is_empty());
    serde_json::json!({
        "available": available,
        "auditd_active": available,
        "source": raw.get("source").cloned().unwrap_or(serde_json::Value::Null),
        "recent_events": events.len(),
        "avc_count": avc,
        "summary": format!("{} recent event(s) · {} AVC", events.len(), avc),
        "events": events,
    })
}

pub fn normalize_linux_package_updates(raw: serde_json::Value) -> serde_json::Value {
    let packages = raw
        .get("packages")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let pending = raw.get("pending_count").and_then(|v| v.as_u64());
    serde_json::json!({
        "backend": raw.get("backend"),
        "probed": raw.get("probed"),
        "pending_count": pending,
        "summary": raw.get("summary").and_then(|v| v.as_str()).unwrap_or(""),
        "hint": raw.get("hint"),
        "error": raw.get("error"),
        "reboot_required": raw.get("reboot_required").and_then(|v| v.as_bool()).unwrap_or(false),
        "packages": packages,
    })
}

pub fn normalize_network_diagnostics(raw: serde_json::Value) -> serde_json::Value {
    let networkd_active = raw
        .get("systemd_networkd_active")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let nm_active = raw
        .get("network_manager_active")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let resolvectl = raw
        .get("resolvectl_status")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let resolved_active =
        resolvectl.contains("Active: active") || resolvectl.contains("DNS Servers");
    let mut interfaces = Vec::new();
    if let Some(list) = raw.get("networkctl_list").and_then(|v| v.as_str()) {
        for line in list.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                interfaces.push(serde_json::json!({
                    "name": parts[0],
                    "state": parts.get(1).unwrap_or(&""),
                    "kind": parts.get(2).unwrap_or(&""),
                }));
            }
        }
    }
    let mut out = raw;
    if let Some(obj) = out.as_object_mut() {
        obj.insert("networkd_active".into(), serde_json::json!(networkd_active));
        obj.insert("resolved_active".into(), serde_json::json!(resolved_active));
        obj.insert(
            "network_manager_active".into(),
            serde_json::json!(nm_active),
        );
        obj.insert("interfaces".into(), serde_json::Value::Array(interfaces));
        obj.insert(
            "summary".into(),
            serde_json::json!(format!(
                "networkd {} · NetworkManager {} · resolved {}",
                if networkd_active {
                    "active"
                } else {
                    "inactive"
                },
                if nm_active { "active" } else { "inactive" },
                if resolved_active {
                    "active"
                } else {
                    "inactive"
                }
            )),
        );
    }
    out
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct GuestAgentCheckRow {
    pub id: String,
    pub label: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, serde::Serialize)]
pub struct VmGuestHealthReport {
    pub vm_id: String,
    pub vm_name: String,
    pub agent_reachable: bool,
    pub healthy: bool,
    pub os_pretty_name: String,
    pub guest_ip: String,
    pub guest_hostname: String,
    pub issues: Vec<String>,
    pub summary: String,
    pub install_state: String,
    pub channel_attached: bool,
    pub channel_connected: bool,
    pub agent_ping: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub agent_version: String,
    pub checks: Vec<GuestAgentCheckRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guest_observability: Option<serde_json::Value>,
}

pub async fn vm_guest_health(
    pool: &DbPool,
    cfg: &ControllerConfig,
    vm_id: Uuid,
) -> anyhow::Result<VmGuestHealthReport> {
    let row: (String, Uuid) = crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("vm not found"))?;
    let (vm_name, host_id) = row;
    let (_, addr) = resolve_agent_addr(pool, cfg, host_id).await?;
    let mut client = agent_client::connect(&addr).await?;
    let gh = agent_client::get_guest_health(&mut client, &vm_name).await?;
    let summary = match gh.install_state.as_str() {
        "running" if gh.healthy => format!("Guest agent running · {}", gh.os_pretty_name),
        "running" => format!("Guest agent running · {} issue(s)", gh.issues.len()),
        "channel_only" => "Guest agent channel attached — start guestkit-agent in VM".into(),
        "none" => "Guest agent not configured — attach channel and install guestkit-agent".into(),
        _ if !gh.agent_reachable => "Guest agent unreachable".into(),
        _ => format!("{} issue(s) reported", gh.issues.len()),
    };
    let checks: Vec<GuestAgentCheckRow> = if gh.diagnostics_json.is_empty() {
        Vec::new()
    } else {
        serde_json::from_str::<serde_json::Value>(&gh.diagnostics_json)
            .ok()
            .and_then(|v| v.get("checks").and_then(|c| c.as_array()).cloned())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        Some(GuestAgentCheckRow {
                            id: item.get("id")?.as_str()?.to_string(),
                            label: item.get("label")?.as_str()?.to_string(),
                            passed: item.get("passed")?.as_bool()?,
                            detail: item.get("detail")?.as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let guest_observability = if gh.agent_ping {
        agent_client::get_guest_observability(&mut client, &vm_name)
            .await
            .ok()
    } else {
        None
    };
    if !gh.guest_ip.is_empty() {
        if let Err(e) =
            crate::db::query("UPDATE vms SET guest_ip = ?, updated_at = datetime('now') WHERE id = ?")
                .bind(&gh.guest_ip)
                .bind(vm_id)
                .execute(pool)
                .await
        {
            tracing::warn!(vm_id = %vm_id, "host_os: failed to persist guest_ip: {e:#}");
        }
    }

    Ok(VmGuestHealthReport {
        vm_id: vm_id.to_string(),
        vm_name,
        agent_reachable: gh.agent_reachable,
        healthy: gh.healthy,
        os_pretty_name: gh.os_pretty_name,
        guest_ip: gh.guest_ip,
        guest_hostname: gh.guest_hostname,
        issues: gh.issues,
        summary,
        install_state: gh.install_state,
        channel_attached: gh.channel_attached,
        channel_connected: gh.channel_connected,
        agent_ping: gh.agent_ping,
        agent_version: gh.agent_version,
        checks,
        guest_observability,
    })
}

pub async fn vm_guest_agent_action(
    pool: &DbPool,
    cfg: &ControllerConfig,
    vm_id: Uuid,
    action: &str,
) -> anyhow::Result<serde_json::Value> {
    let row: (String, Uuid) = crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("vm not found"))?;
    let (_, addr) = resolve_agent_addr(pool, cfg, row.1).await?;
    let mut client = agent_client::connect(&addr).await?;
    agent_client::guest_agent_action(&mut client, &row.0, action).await
}

pub async fn vm_guest_observability(
    pool: &DbPool,
    cfg: &ControllerConfig,
    vm_id: Uuid,
) -> anyhow::Result<serde_json::Value> {
    let row: (String, Uuid) = crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("vm not found"))?;
    let (_, addr) = resolve_agent_addr(pool, cfg, row.1).await?;
    let mut client = agent_client::connect(&addr).await?;
    agent_client::get_guest_observability(&mut client, &row.0).await
}

#[derive(Debug, serde::Serialize)]
pub struct VmGuestServiceRow {
    pub name: String,
    pub status: String,
    pub detail: String,
    #[serde(default)]
    pub controllable: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct VmGuestServicesReport {
    pub vm_id: String,
    pub vm_name: String,
    pub agent_reachable: bool,
    pub services: Vec<VmGuestServiceRow>,
    pub summary: String,
}

pub async fn vm_guest_services(
    pool: &DbPool,
    cfg: &ControllerConfig,
    vm_id: Uuid,
) -> anyhow::Result<VmGuestServicesReport> {
    let health = vm_guest_health(pool, cfg, vm_id).await?;
    let mut services = Vec::new();
    if health.agent_reachable {
        services.push(VmGuestServiceRow {
            name: "guestkit-agent".into(),
            status: if health.healthy {
                "running"
            } else {
                "degraded"
            }
            .into(),
            detail: health.os_pretty_name.clone(),
            controllable: false,
        });
        let row: (String, Uuid) = crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("vm not found"))?;
        let (_, addr) = resolve_agent_addr(pool, cfg, row.1).await?;
        let mut client = agent_client::connect(&addr).await?;
        if let Ok(val) =
            agent_client::guest_agent_action(&mut client, &row.0, "list_services").await
        {
            if let Some(arr) = val.get("services").and_then(|s| s.as_array()) {
                for item in arr {
                    let name = item
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    if name.is_empty() {
                        continue;
                    }
                    services.push(VmGuestServiceRow {
                        name,
                        status: item
                            .get("status")
                            .and_then(|s| s.as_str())
                            .unwrap_or("unknown")
                            .into(),
                        detail: item
                            .get("detail")
                            .and_then(|d| d.as_str())
                            .unwrap_or("")
                            .into(),
                        controllable: item
                            .get("controllable")
                            .and_then(|c| c.as_bool())
                            .unwrap_or(false),
                    });
                }
            }
        }
    }
    // Listening ports belong on Security / guest-ports — do not clutter Guest services.
    Ok(VmGuestServicesReport {
        vm_id: health.vm_id,
        vm_name: health.vm_name,
        agent_reachable: health.agent_reachable,
        summary: format!("{} service row(s) from guest agent", services.len()),
        services,
    })
}

pub async fn vm_guest_service_action(
    pool: &DbPool,
    cfg: &ControllerConfig,
    vm_id: Uuid,
    unit: &str,
    action: &str,
) -> anyhow::Result<serde_json::Value> {
    let row: (String, Uuid) = crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("vm not found"))?;
    let (_, addr) = resolve_agent_addr(pool, cfg, row.1).await?;
    let mut client = agent_client::connect(&addr).await?;
    let keyed = match action.trim().to_ascii_lowercase().as_str() {
        "start" => format!("service_start:{unit}"),
        "stop" => format!("service_stop:{unit}"),
        "restart" => format!("service_restart:{unit}"),
        other => anyhow::bail!("unsupported guest service action: {other}"),
    };
    agent_client::guest_agent_action(&mut client, &row.0, &keyed).await
}

pub async fn vm_guest_network_get(
    pool: &DbPool,
    cfg: &ControllerConfig,
    vm_id: Uuid,
) -> anyhow::Result<serde_json::Value> {
    let row: (String, Uuid) = crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("vm not found"))?;
    let (_, addr) = resolve_agent_addr(pool, cfg, row.1).await?;
    let mut client = agent_client::connect(&addr).await?;
    let val = agent_client::guest_agent_action(&mut client, &row.0, "get_network").await?;
    Ok(val.get("network").cloned().unwrap_or_else(|| {
        serde_json::json!({
            "interfaces": [],
            "routes": [],
            "default_gateway": null
        })
    }))
}

pub async fn vm_guest_network_apply(
    pool: &DbPool,
    cfg: &ControllerConfig,
    vm_id: Uuid,
    req: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let row: (String, Uuid) = crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("vm not found"))?;
    let (_, addr) = resolve_agent_addr(pool, cfg, row.1).await?;
    let mut client = agent_client::connect(&addr).await?;
    let keyed = format!("network_apply:{req}");
    agent_client::guest_agent_action(&mut client, &row.0, &keyed).await
}

#[derive(Debug, Clone, Serialize)]
pub struct OsDiagnoseAction {
    pub label: String,
    pub action: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostOsDiagnoseReport {
    pub host_id: String,
    pub hostname: String,
    pub query: String,
    pub summary: String,
    pub hypotheses: Vec<super::ai::knowledge_diagnose::DiagnoseHypothesis>,
    pub fix_actions: Vec<OsDiagnoseAction>,
}

pub async fn diagnose_host(
    pool: &DbPool,
    cfg: &ControllerConfig,
    host_id: Uuid,
    query: Option<&str>,
) -> anyhow::Result<HostOsDiagnoseReport> {
    let hostname: String = crate::db::query_scalar("SELECT hostname FROM hosts WHERE id = ?")
        .bind(host_id)
        .fetch_one(pool)
        .await?;
    let q = query
        .unwrap_or("why is this host under pressure")
        .to_string();
    let mut diag =
        super::ai::knowledge_diagnose::diagnose(pool, &format!("{q} {hostname}")).await?;
    let mut fix_actions = vec![
        OsDiagnoseAction {
            label: "Sync host inventory".into(),
            action: "host.sync".into(),
            detail: format!("POST /api/v1/hosts/{host_id}/sync"),
        },
        OsDiagnoseAction {
            label: "Validate join checklist".into(),
            action: "host.validate".into(),
            detail: format!("GET /api/v1/hosts/{host_id}/validate"),
        },
        OsDiagnoseAction {
            label: "Open Machine Security".into(),
            action: "firewall.open".into(),
            detail: format!("/platform/zeus/security/firewall/{host_id}"),
        },
    ];
    if let Ok(obs) = linux_observability(pool, cfg, host_id).await {
        // `avg10` from `/proc/pressure/io` is already a 0-100 percentage, not
        // a 0-1 fraction — `io > 0.3` fired on almost any nonzero pressure,
        // and `io * 100.0` in the evidence string double-inflated the display
        // (e.g. a real 17.67% read as "1767%").
        let io = obs
            .get("pressure")
            .and_then(|p| p.get("io"))
            .and_then(|i| i.get("some"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        if io > 30.0 {
            diag.hypotheses
                .push(super::ai::knowledge_diagnose::DiagnoseHypothesis {
                    title: "IO pressure on hypervisor".into(),
                    confidence: 0.82,
                    evidence: format!("PSI io some {:.0}%", io),
                    action: "Check storage pool latency and running VM disk IOPS.".into(),
                });
            fix_actions.push(OsDiagnoseAction {
                label: "Review storage pools".into(),
                action: "nav.storage".into(),
                detail: "/platform/storage".into(),
            });
        }
    }
    Ok(HostOsDiagnoseReport {
        host_id: host_id.to_string(),
        hostname: hostname.clone(),
        query: q.clone(),
        summary: format!("{} — {}", hostname, diag.summary),
        hypotheses: diag.hypotheses,
        fix_actions,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct VmOsDiagnoseReport {
    pub vm_id: String,
    pub vm_name: String,
    pub query: String,
    pub summary: String,
    pub guest_healthy: bool,
    pub hypotheses: Vec<super::ai::knowledge_diagnose::DiagnoseHypothesis>,
    pub fix_actions: Vec<OsDiagnoseAction>,
}

pub async fn diagnose_vm(
    pool: &DbPool,
    cfg: &ControllerConfig,
    vm_id: Uuid,
    query: Option<&str>,
) -> anyhow::Result<VmOsDiagnoseReport> {
    let health = vm_guest_health(pool, cfg, vm_id).await?;
    let q = query
        .unwrap_or("guest health and exposed ports")
        .to_string();
    let mut diag =
        super::ai::knowledge_diagnose::diagnose(pool, &format!("{} {}", q, health.vm_name)).await?;
    if !health.agent_reachable {
        diag.hypotheses.insert(
            0,
            super::ai::knowledge_diagnose::DiagnoseHypothesis {
                title: "QEMU guest agent offline".into(),
                confidence: 0.9,
                evidence: health.summary.clone(),
                action: "Install Guest Tools and ensure VM is running.".into(),
            },
        );
    }
    Ok(VmOsDiagnoseReport {
        vm_id: vm_id.to_string(),
        vm_name: health.vm_name.clone(),
        query: q,
        summary: health.summary.clone(),
        guest_healthy: health.healthy,
        hypotheses: diag.hypotheses,
        fix_actions: vec![
            OsDiagnoseAction {
                label: "Install Guest Tools".into(),
                action: "vm.guest_tools".into(),
                detail: format!("POST /api/v1/vms/{vm_id}/guest-tools/install"),
            },
            OsDiagnoseAction {
                label: "Run VM health check".into(),
                action: "vm.health_check".into(),
                detail: format!("POST /api/v1/vms/{vm_id}/health-check"),
            },
        ],
    })
}
