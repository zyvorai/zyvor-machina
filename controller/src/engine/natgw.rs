// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Pushes the NAT-enabled subnets of each host to its agent (`nat.sync`) on every change and every 30 s.

use std::time::Duration;

use serde_json::json;
use crate::db::DbPool;
use uuid::Uuid;

use crate::state::AppState;

/// CIDRs of the NAT-enabled, ready subnets on `host`.
pub async fn cidrs_for_host(pool: &DbPool, host: Uuid) -> anyhow::Result<Vec<String>> {
    Ok(crate::db::query_scalar(
        "SELECT s.cidr FROM cloud_subnets s JOIN cloud_vpcs v ON v.id = s.vpc_id WHERE v.host_id = ? AND s.nat_enabled = 1 AND s.status = 'ready' ORDER BY s.cidr",
    )
    .bind(host)
    .fetch_all(pool)
    .await?)
}

pub async fn push_host(pool: &DbPool, host: Uuid) -> anyhow::Result<()> {
    let addr: Option<String> = crate::db::query_scalar("SELECT agent_grpc_addr FROM hosts WHERE id = ? AND state = 'online'").bind(host).fetch_optional(pool).await?;
    let addr = addr.ok_or_else(|| anyhow::anyhow!("the host is not online"))?;
    let entries: Vec<_> = cidrs_for_host(pool, host).await?.into_iter().map(|c| json!({ "cidr": c })).collect();
    let mut client = crate::agent_client::connect(&addr).await?;
    crate::agent_client::host_libvirt_invoke(&mut client, "nat.sync", &json!({ "entries": entries })).await?;
    Ok(())
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            // Hosts that have ever had NAT get a push (an empty one clears the rules after the last subnet is switched off).
            let hosts: Vec<Uuid> = crate::db::query_scalar("SELECT DISTINCT v.host_id FROM cloud_subnets s JOIN cloud_vpcs v ON v.id = s.vpc_id WHERE s.nat_enabled = 1")
                .fetch_all(&state.pool)
                .await
                .unwrap_or_default();
            for h in hosts {
                if let Err(e) = push_host(&state.pool, h).await {
                    tracing::debug!(host = %h, "nat sync: {e:#}");
                }
            }
        }
    });
}
