// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use machina_core::{
    builtin_profiles, gather_firewall_inventory, FirewallBackend, FirewallInventory,
    FirewallPlanRequest, FirewallPlanResult, FirewallPosture, FirewallScore, OpenPort,
    StealthLevel,
};
use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

use crate::agent_client;
use crate::config::ControllerConfig;

#[derive(Debug, Clone, Serialize)]
pub struct FirewallTargetSummary {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub hostname: String,
    pub enabled: bool,
    pub backend: String,
    pub profile: Option<String>,
    pub risk: String,
    pub score: u32,
    pub open_ports: usize,
    pub blocked_today: u64,
    pub agent_reachable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FirewallOverview {
    pub targets: Vec<FirewallTargetSummary>,
    pub critical_count: usize,
    pub warning_count: usize,
    pub profiles: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FirewallTargetDetail {
    pub target: FirewallTargetSummary,
    pub inventory: FirewallInventory,
}

pub async fn overview(
    pool: &DbPool,
    cfg: &ControllerConfig,
) -> anyhow::Result<FirewallOverview> {
    let hosts: Vec<(Uuid, String, String, String)> = crate::db::query_as(
        "SELECT id, hostname, COALESCE(agent_grpc_addr, ''), state FROM hosts ORDER BY hostname",
    )
    .fetch_all(pool)
    .await?;

    let mut targets = Vec::new();
    let mut critical = 0usize;
    let mut warning = 0usize;

    if hosts.is_empty() {
        let inv = gather_firewall_inventory("localhost")?;
        let risk = risk_label(&inv);
        if risk == "critical" {
            critical += 1;
        } else if risk == "warning" {
            warning += 1;
        }
        targets.push(summary_from_inventory(
            "local".into(),
            "host".into(),
            "localhost".into(),
            "localhost".into(),
            &inv,
            true,
        ));
    } else {
        for (id, hostname, agent_addr, state) in hosts {
            let (inv, reachable) =
                fetch_inventory(cfg, &agent_addr, &hostname, state == "online").await;
            let risk = risk_label(&inv);
            if risk == "critical" {
                critical += 1;
            } else if risk == "warning" {
                warning += 1;
            }
            targets.push(summary_from_inventory(
                id.to_string(),
                "host".into(),
                hostname.clone(),
                hostname,
                &inv,
                reachable,
            ));
        }
    }

    if let Ok(rows) = super::metal::load_all(pool).await {
        for row in rows {
            let inv = machina_core::gather_metal_inventory(&super::metal::row_to_input(&row));
            let risk = risk_label(&inv);
            if risk == "critical" {
                critical += 1;
            } else if risk == "warning" {
                warning += 1;
            }
            targets.push(summary_from_inventory(
                row.id.to_string(),
                "bare_metal".into(),
                row.hostname.clone(),
                row.hostname,
                &inv,
                false,
            ));
        }
    }

    let profiles: Vec<String> = builtin_profiles().into_iter().map(|p| p.name).collect();
    let summary = format!(
        "{} machines scanned · {} critical · {} warnings",
        targets.len(),
        critical,
        warning
    );
    Ok(FirewallOverview {
        targets,
        critical_count: critical,
        warning_count: warning,
        profiles,
        summary,
    })
}

pub async fn target_detail(
    pool: &DbPool,
    cfg: &ControllerConfig,
    target_id: &str,
) -> anyhow::Result<FirewallTargetDetail> {
    if target_id == "local" {
        let inv = gather_firewall_inventory("localhost")?;
        let target = summary_from_inventory(
            "local".into(),
            "host".into(),
            "localhost".into(),
            "localhost".into(),
            &inv,
            true,
        );
        return Ok(FirewallTargetDetail {
            target,
            inventory: inv,
        });
    }

    let id = Uuid::parse_str(target_id)?;
    if let Some(row) = crate::db::query_as::<_, (String, String, String)>(
        "SELECT hostname, COALESCE(agent_grpc_addr, ''), state FROM hosts WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    {
        let (hostname, agent_addr, state) = row;
        let (inv, reachable) =
            fetch_inventory(cfg, &agent_addr, &hostname, state == "online").await;
        let target = summary_from_inventory(
            target_id.into(),
            "host".into(),
            hostname.clone(),
            hostname,
            &inv,
            reachable,
        );
        return Ok(FirewallTargetDetail {
            target,
            inventory: inv,
        });
    }

    let row = super::metal::load_row(pool, id).await?;
    let inv = machina_core::gather_metal_inventory(&super::metal::row_to_input(&row));
    let target = summary_from_inventory(
        target_id.into(),
        "bare_metal".into(),
        row.hostname.clone(),
        row.hostname,
        &inv,
        false,
    );
    Ok(FirewallTargetDetail {
        target,
        inventory: inv,
    })
}

pub async fn target_ports(
    pool: &DbPool,
    cfg: &ControllerConfig,
    target_id: &str,
) -> anyhow::Result<Vec<OpenPort>> {
    let detail = target_detail(pool, cfg, target_id).await?;
    Ok(detail.inventory.open_ports)
}

pub async fn target_services(
    pool: &DbPool,
    cfg: &ControllerConfig,
    target_id: &str,
) -> anyhow::Result<Vec<machina_core::firewall::types::AllowedService>> {
    let detail = target_detail(pool, cfg, target_id).await?;
    Ok(detail.inventory.services)
}

pub async fn target_score(
    pool: &DbPool,
    cfg: &ControllerConfig,
    target_id: &str,
) -> anyhow::Result<machina_core::FirewallScore> {
    let detail = target_detail(pool, cfg, target_id).await?;
    Ok(detail.inventory.score)
}

pub async fn plan_target(
    pool: &DbPool,
    cfg: &ControllerConfig,
    target_id: &str,
    mut req: FirewallPlanRequest,
) -> anyhow::Result<FirewallPlanResult> {
    // Server-owned: the zone→CIDR map is config, not something an API caller
    // supplies — inject it here so every path to compile_profile_plan/apply_plan
    // (local host or remote agent) gets it, regardless of which handler built `req`.
    req.zone_cidrs = cfg.firewall_zones.clone();
    if target_id != "local" {
        if let Ok(id) = Uuid::parse_str(target_id) {
            if crate::db::query_scalar::<_, i64>("SELECT COUNT(*) FROM hosts WHERE id = ?")
                .bind(id)
                .fetch_one(pool)
                .await?
                == 0
                && super::metal::load_row(pool, id).await.is_ok()
            {
                return super::metal::plan_metal(pool, id, req).await;
            }
        }
    }
    let hostname = resolve_hostname(pool, cfg, target_id).await?;
    if let Some(addr) = resolve_agent(pool, cfg, target_id).await? {
        return agent_client::apply_firewall_plan(&addr, &req, req.dry_run).await;
    }
    machina_core::compile_profile_plan(&hostname, &req).map_err(|e| anyhow::anyhow!(e.to_string()))
}

pub async fn apply_target(
    pool: &DbPool,
    cfg: &ControllerConfig,
    target_id: &str,
    mut req: FirewallPlanRequest,
    actor: &str,
) -> anyhow::Result<FirewallPlanResult> {
    req.zone_cidrs = cfg.firewall_zones.clone();
    if target_id != "local" {
        if let Ok(id) = Uuid::parse_str(target_id) {
            if crate::db::query_scalar::<_, i64>("SELECT COUNT(*) FROM hosts WHERE id = ?")
                .bind(id)
                .fetch_one(pool)
                .await?
                == 0
                && super::metal::load_row(pool, id).await.is_ok()
            {
                return super::metal::apply_metal(pool, id, req, actor).await;
            }
        }
    }
    let mut apply_req = req;
    apply_req.dry_run = false;
    if let Ok(host_id) = Uuid::parse_str(target_id) {
        if let Ok(detail) = target_detail(pool, cfg, target_id).await {
            let _ = super::checkpoint::save_checkpoint(
                pool,
                "host",
                host_id,
                "pre-apply",
                &detail.inventory,
                Some(actor),
            )
            .await;
        }
    }
    let result = plan_target(pool, cfg, target_id, apply_req.clone()).await?;
    if let Ok(host_id) = Uuid::parse_str(target_id) {
        if let Ok(detail) = target_detail(pool, cfg, target_id).await {
            let _ = super::drift::save_snapshot(pool, "host", host_id, &detail.inventory).await;
        }
        let _ = crate::db::query(
            "INSERT INTO firewall_timeline (id, target_kind, target_id, kind, summary, detail_json, actor) VALUES (?, 'host', ?, 'apply', ?, ?, ?)",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(host_id)
        .bind(format!("Applied firewall plan ({})", apply_req.profile.as_deref().unwrap_or("custom")))
        .bind(serde_json::json!({ "operations": result.operations }))
        .bind(actor)
        .execute(pool)
        .await;
        let _ = crate::db::query(
            "INSERT INTO audit_logs (id, actor, action, resource_type, resource_id, detail)
             VALUES (?, ?, 'zeus_firewall.apply', 'host', ?, ?)",
        )
        .bind(Uuid::new_v4())
        .bind(actor)
        .bind(host_id)
        .bind(serde_json::json!({ "profile": apply_req.profile }))
        .execute(pool)
        .await;
    }
    Ok(result)
}

async fn fetch_inventory(
    cfg: &ControllerConfig,
    agent_addr: &str,
    hostname: &str,
    online: bool,
) -> (FirewallInventory, bool) {
    let addr = if agent_addr.is_empty() {
        cfg.default_agent_addr.clone()
    } else {
        agent_addr.to_string()
    };
    if online {
        if let Ok(inv) = agent_client::get_firewall_inventory(&addr).await {
            return (inv, true);
        }
    }
    let inv = gather_firewall_inventory(hostname).unwrap_or_else(|_| empty_inventory(hostname));
    (inv, false)
}

fn empty_inventory(hostname: &str) -> FirewallInventory {
    gather_firewall_inventory(hostname).unwrap_or_else(|_| FirewallInventory {
        hostname: hostname.to_string(),
        posture: FirewallPosture {
            enabled: false,
            backend: FirewallBackend::Unknown,
            stealth_level: StealthLevel::Off,
            drift_detected: false,
            profile: None,
            default_inbound: None,
            default_outbound: None,
            backend_zone: None,
            status_line: None,
            last_changed: None,
        },
        rules: vec![],
        open_ports: vec![],
        services: vec![],
        score: FirewallScore {
            score: 0,
            breakdown: vec![],
            recommendations: vec![],
        },
        profiles_available: vec![],
        nftables_summary: None,
        activity: None,
    })
}

fn summary_from_inventory(
    id: String,
    kind: String,
    name: String,
    hostname: String,
    inv: &FirewallInventory,
    agent_reachable: bool,
) -> FirewallTargetSummary {
    FirewallTargetSummary {
        id,
        kind,
        name,
        hostname,
        enabled: inv.posture.enabled,
        backend: inv.posture.backend.as_str().into(),
        profile: inv.posture.profile.clone(),
        risk: risk_label(inv).into(),
        score: inv.score.score,
        open_ports: inv.open_ports.len(),
        blocked_today: inv.activity.as_ref().map(|a| a.blocked_today).unwrap_or(0),
        agent_reachable,
    }
}

pub fn risk_label(inv: &FirewallInventory) -> &'static str {
    if inv.open_ports.iter().any(|p| {
        matches!(
            p.risk,
            machina_core::firewall::types::ExposureRisk::Critical
        )
    }) {
        "critical"
    } else if inv.score.score < 70 {
        "warning"
    } else {
        "low"
    }
}

async fn resolve_hostname(
    pool: &DbPool,
    _cfg: &ControllerConfig,
    target_id: &str,
) -> anyhow::Result<String> {
    if target_id == "local" {
        return Ok("localhost".into());
    }
    let id = Uuid::parse_str(target_id)?;
    if let Some(hostname) =
        crate::db::query_scalar::<_, String>("SELECT hostname FROM hosts WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?
    {
        return Ok(hostname);
    }
    let row = super::metal::load_row(pool, id).await?;
    Ok(row.hostname)
}

async fn resolve_agent(
    pool: &DbPool,
    cfg: &ControllerConfig,
    target_id: &str,
) -> anyhow::Result<Option<String>> {
    if target_id == "local" {
        return Ok(Some(cfg.default_agent_addr.clone()));
    }
    let host_id = Uuid::parse_str(target_id)?;
    let addr: String =
        crate::db::query_scalar("SELECT COALESCE(agent_grpc_addr, '') FROM hosts WHERE id = ?")
            .bind(host_id)
            .fetch_one(pool)
            .await?;
    if addr.is_empty() {
        Ok(None)
    } else {
        Ok(Some(addr))
    }
}

pub async fn apply_profile(
    pool: &DbPool,
    cfg: &ControllerConfig,
    target_id: &str,
    profile: &str,
    actor: &str,
    dry_run: bool,
) -> anyhow::Result<FirewallPlanResult> {
    let req = super::profiles::plan_for_profile(profile, dry_run)?;
    if dry_run {
        plan_target(pool, cfg, target_id, req).await
    } else {
        apply_target(pool, cfg, target_id, req, actor).await
    }
}

pub async fn zeus_firewall_status() -> serde_json::Value {
    serde_json::json!({
        "feature": "zeus-firewall",
        "phase": 3,
        "ai_id": "AI-172",
        "backends": ["firewalld", "ufw", "nftables", "iptables", "k8s_network_policy"],
        "ready": true
    })
}
