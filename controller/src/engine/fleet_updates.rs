// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Fleet Software Update rollup (Phase 43).

use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

use crate::config::ControllerConfig;
use crate::engine::host_os;

#[derive(Debug, Clone, Serialize)]
pub struct FleetHostUpdateItem {
    pub host_id: String,
    pub hostname: String,
    pub agent_version: String,
    pub agent_update_available: bool,
    pub backend: String,
    pub pending_count: Option<u32>,
    pub summary: Option<String>,
    pub reboot_required: bool,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetUpdatesOverview {
    pub summary: String,
    pub recommended_agent: String,
    pub hosts_scanned: usize,
    pub hosts_with_updates: usize,
    pub hosts_reboot_required: usize,
    pub agent_drift_count: usize,
    pub total_pending_packages: u64,
    pub hosts: Vec<FleetHostUpdateItem>,
}

fn has_pending_updates(pending: Option<u32>, summary: &Option<String>) -> bool {
    if pending.unwrap_or(0) > 0 {
        return true;
    }
    summary
        .as_deref()
        .map(|s| {
            let lower = s.to_ascii_lowercase();
            lower.contains("would be upgraded")
                || lower.contains("upgradable")
                || lower.contains("updates available")
                || lower.contains("patches needed")
                || lower.contains("pending")
        })
        .unwrap_or(false)
}

pub async fn overview(
    pool: &DbPool,
    cfg: &ControllerConfig,
) -> anyhow::Result<FleetUpdatesOverview> {
    let recommended_agent = env!("CARGO_PKG_VERSION").to_string();
    let rows: Vec<(Uuid, String, String)> = crate::db::query_as(
        "SELECT id, hostname, COALESCE(agent_version, '') FROM hosts WHERE state = 'online' ORDER BY hostname LIMIT 8",
    )
    .fetch_all(pool)
    .await?;

    let mut hosts = Vec::new();
    let mut hosts_with_updates = 0usize;
    let mut hosts_reboot_required = 0usize;
    let mut agent_drift_count = 0usize;
    let mut total_pending_packages = 0_u64;

    for (host_id, hostname, agent_version) in rows {
        let agent_update_available =
            !agent_version.is_empty() && agent_version != recommended_agent;
        if agent_update_available {
            agent_drift_count += 1;
        }

        let Ok(check) = host_os::linux_package_updates(pool, cfg, host_id).await else {
            hosts.push(FleetHostUpdateItem {
                host_id: host_id.to_string(),
                hostname,
                agent_version,
                agent_update_available,
                backend: "unknown".into(),
                pending_count: None,
                summary: Some("Agent unreachable".into()),
                reboot_required: false,
                status: "unreachable".into(),
            });
            continue;
        };

        let backend = check
            .get("backend")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let pending_count = check
            .get("pending_count")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32);
        let summary = check
            .get("summary")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let reboot_required = check
            .get("reboot_required")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let probe_error = check.get("error").and_then(|v| v.as_str());

        if pending_count.unwrap_or(0) > 0 {
            total_pending_packages += pending_count.unwrap_or(0) as u64;
        }

        let updates = has_pending_updates(pending_count, &summary);
        if updates {
            hosts_with_updates += 1;
        }
        if reboot_required {
            hosts_reboot_required += 1;
        }

        let status = if probe_error.is_some() && !updates {
            "error"
        } else if reboot_required && !updates {
            "reboot"
        } else if updates {
            "updates"
        } else {
            "ok"
        };

        hosts.push(FleetHostUpdateItem {
            host_id: host_id.to_string(),
            hostname,
            agent_version,
            agent_update_available,
            backend,
            pending_count,
            summary,
            reboot_required,
            status: status.into(),
        });
    }

    let hosts_scanned = hosts.len();
    Ok(FleetUpdatesOverview {
        summary: format!(
            "{hosts_scanned} host(s) · {hosts_with_updates} with OS updates · {hosts_reboot_required} reboot pending · {agent_drift_count} agent drift"
        ),
        recommended_agent,
        hosts_scanned,
        hosts_with_updates,
        hosts_reboot_required,
        agent_drift_count,
        total_pending_packages,
        hosts,
    })
}
