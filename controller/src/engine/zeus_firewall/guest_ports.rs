// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use machina_core::{guest_ports_to_open_ports, simulate_connectivity};
use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

use crate::agent_client;
use crate::config::ControllerConfig;

use super::inventory::target_detail;

#[derive(Debug, Clone, Serialize)]
pub struct GuestPortReport {
    pub vm_id: String,
    pub vm_name: String,
    pub agent_reachable: bool,
    pub ports: Vec<machina_core::OpenPort>,
    pub summary: String,
}

pub async fn vm_guest_ports(
    pool: &DbPool,
    cfg: &ControllerConfig,
    vm_id: &str,
) -> anyhow::Result<GuestPortReport> {
    let vm_uuid = Uuid::parse_str(vm_id).map_err(|e| anyhow::anyhow!("invalid vm id: {e}"))?;
    let row: Option<(String, Uuid)> = crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
        .bind(vm_uuid)
        .fetch_optional(pool)
        .await?;
    let (vm_name, host_id) = row.ok_or_else(|| anyhow::anyhow!("vm not found"))?;
    let agent_addr: String =
        crate::db::query_scalar("SELECT COALESCE(agent_grpc_addr, '') FROM hosts WHERE id = ?")
            .bind(host_id)
            .fetch_one(pool)
            .await?;
    let addr = if agent_addr.is_empty() {
        cfg.default_agent_addr.clone()
    } else {
        agent_addr
    };
    let (agent_reachable, ports) =
        match agent_client::get_guest_firewall_ports(&addr, &vm_name).await {
            Ok(resp) => (
                resp.agent_reachable,
                guest_ports_to_open_ports(&vm_name, &resp.ports),
            ),
            Err(e) => {
                tracing::warn!("guest firewall ports for {vm_name} via {addr}: {e}");
                (false, Vec::new())
            }
        };
    Ok(GuestPortReport {
        vm_id: vm_id.into(),
        vm_name,
        agent_reachable,
        ports: ports.clone(),
        summary: if agent_reachable {
            format!("{} in-guest listening ports via QEMU agent", ports.len())
        } else {
            "Guest agent unreachable — no in-guest port inventory".into()
        },
    })
}

pub async fn connectivity_matrix(
    pool: &DbPool,
    cfg: &ControllerConfig,
    target_id: &str,
    profile: &str,
) -> anyhow::Result<machina_core::ConnectivityMatrix> {
    let detail = target_detail(pool, cfg, target_id).await?;
    let prof = machina_core::profile_by_name(profile)
        .ok_or_else(|| anyhow::anyhow!("unknown profile {profile}"))?;
    let after_rules: Vec<machina_core::ZeusFirewallRule> = prof
        .rules
        .iter()
        .enumerate()
        .map(|(i, r)| machina_core::ZeusFirewallRule {
            id: format!("sim-{i}"),
            direction: r.direction.clone(),
            protocol: r.protocol.clone(),
            ports: r.ports.clone(),
            sources: r.sources.clone(),
            targets: vec![],
            action: r.action.clone(),
            temporary: false,
            expires_at: None,
            description: Some(r.name.clone()),
            scope: "host".into(),
            backend_ref: None,
        })
        .collect();
    let matrix = simulate_connectivity(&detail.inventory, &after_rules);
    if let Ok(host_id) = Uuid::parse_str(target_id) {
        let _ = crate::db::query(
            "INSERT INTO firewall_connectivity_runs (id, target_id, profile, matrix_json)
             VALUES (?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4())
        .bind(host_id)
        .bind(profile)
        .bind(serde_json::to_value(&matrix)?)
        .execute(pool)
        .await;
    }
    Ok(matrix)
}
