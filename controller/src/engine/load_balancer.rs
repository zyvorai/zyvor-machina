// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Native L4 load balancer reconciliation. Pushes a load balancer's current
//! (protocol, host_port, enabled members) to the owning host's agent, which installs a
//! weighted round-robin iptables rule set (see
//! `machina_core::libvirt::host_network::set_load_balancer_rules`). Called synchronously
//! from the API handlers on every create/member-change/delete -- there's no periodic
//! reconcile loop here because, unlike a VM's libvirt state, nothing external can drift
//! this rule set out from under us between calls.

use crate::db::DbPool;
use uuid::Uuid;

use crate::agent_client::LbMemberDto;
use crate::config::ControllerConfig;
use crate::engine::host_os::resolve_agent_addr;

struct LoadBalancerRow {
    host_id: Uuid,
    protocol: String,
    listener_port: i64,
}

/// Push the current enabled-member set for `lb_id` to its host's agent, then persist the
/// resulting status. Returns the failure so the caller can decide whether to surface it
/// (e.g. reject the mutation that triggered this) or just let the row show `status=error`.
pub async fn apply(pool: &DbPool, cfg: &ControllerConfig, lb_id: Uuid) -> anyhow::Result<()> {
    let (host_id, protocol, listener_port): (Uuid, String, i64) =
        crate::db::query_as("SELECT host_id, protocol, listener_port FROM load_balancers WHERE id = ?")
            .bind(lb_id)
            .fetch_one(pool)
            .await?;
    let lb = LoadBalancerRow {
        host_id,
        protocol,
        listener_port,
    };

    let members: Vec<(String, i64, i64)> = crate::db::query_as(
        "SELECT v.guest_ip, m.port, m.weight
         FROM lb_members m
         JOIN vms v ON v.id = m.vm_id
         WHERE m.load_balancer_id = ? AND m.enabled = TRUE AND m.health <> 'unhealthy'
         ORDER BY m.created_at",
    )
    .bind(lb_id)
    .fetch_all(pool)
    .await?;

    let mut dtos = Vec::with_capacity(members.len());
    for (guest_ip, port, weight) in members {
        if guest_ip.trim().is_empty() {
            continue; // no guest IP yet (agent not reported in) -- skip, don't fail the whole LB
        }
        dtos.push(LbMemberDto {
            vm_ip: guest_ip,
            port: port as u16,
            weight: weight.max(1) as u32,
        });
    }

    let result = async {
        let (_, agent_addr) = resolve_agent_addr(pool, cfg, lb.host_id).await?;
        crate::agent_client::set_load_balancer(
            &agent_addr,
            &lb_id.to_string(),
            &lb.protocol,
            lb.listener_port as u16,
            &dtos,
        )
        .await
    }
    .await;

    match &result {
        Ok(()) => {
            crate::db::query(
                "UPDATE load_balancers SET status = 'active', status_message = '' WHERE id = ?",
            )
            .bind(lb_id)
            .execute(pool)
            .await?;
        }
        Err(e) => {
            crate::db::query(
                "UPDATE load_balancers SET status = 'error', status_message = ? WHERE id = ?",
            )
            .bind(e.to_string())
            .bind(lb_id)
            .execute(pool)
            .await?;
        }
    }
    result
}

/// Tear down the iptables rule set for a load balancer that's about to be deleted from the DB.
/// Best-effort by design (mirrors `host_network::delete_load_balancer_rules`) -- an
/// unreachable host shouldn't block deleting the DB row.
pub async fn teardown(pool: &DbPool, cfg: &ControllerConfig, lb_id: Uuid) {
    let row: Result<(Uuid, String, i64), _> =
        crate::db::query_as("SELECT host_id, protocol, listener_port FROM load_balancers WHERE id = ?")
            .bind(lb_id)
            .fetch_one(pool)
            .await;
    let Ok((host_id, protocol, listener_port)) = row else {
        return;
    };
    let lb = LoadBalancerRow {
        host_id,
        protocol,
        listener_port,
    };
    let Ok((_, agent_addr)) = resolve_agent_addr(pool, cfg, lb.host_id).await else {
        return;
    };
    let _ = crate::agent_client::delete_load_balancer(
        &agent_addr,
        &lb_id.to_string(),
        &lb.protocol,
        lb.listener_port as u16,
    )
    .await;
}
