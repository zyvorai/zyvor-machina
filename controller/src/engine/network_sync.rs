// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use crate::db::DbPool;
use uuid::Uuid;

use crate::agent_client;

/// Import libvirt networks from a host into the platform `networks` table.
pub async fn sync_host_networks(
    pool: &DbPool,
    host_id: Uuid,
    agent_addr: &str,
) -> anyhow::Result<usize> {
    // cluster_id is nullable — a host not yet assigned to a cluster decodes to
    // None; skip rather than erroring the sync loop (decoding NULL into a
    // non-Option Uuid would raise a ColumnDecode error).
    let cluster_id: Option<Uuid> =
        crate::db::query_scalar::<_, Option<Uuid>>("SELECT cluster_id FROM hosts WHERE id = ?")
            .bind(host_id)
            .fetch_optional(pool)
            .await?
            .flatten();
    let Some(cluster_id) = cluster_id else {
        return Ok(0);
    };

    let mut client = agent_client::connect(agent_addr).await?;
    let list = agent_client::list_networks(&mut client).await?;
    let mut imported = 0usize;

    for net in list.networks {
        if net.name.is_empty() {
            continue;
        }
        let bridge = if net.bridge.is_empty() {
            None
        } else {
            Some(net.bridge)
        };
        let result = crate::db::query(
            "INSERT INTO networks (id, cluster_id, name, backend, bridge)
             VALUES (?, ?, ?, 'linux-bridge', ?)
             ON CONFLICT (cluster_id, name) DO UPDATE SET
               bridge = COALESCE(EXCLUDED.bridge, networks.bridge)",
        )
        .bind(Uuid::new_v4())
        .bind(cluster_id)
        .bind(&net.name)
        .bind(&bridge)
        .execute(pool)
        .await?;
        if result.rows_affected() > 0 {
            imported += 1;
        }
    }
    Ok(imported)
}

pub async fn discover_all_online(pool: &DbPool) -> anyhow::Result<usize> {
    let hosts: Vec<(Uuid, String)> = crate::db::query_as(
        "SELECT id, agent_grpc_addr FROM hosts WHERE state = 'online' ORDER BY hostname LIMIT 200",
    )
    .fetch_all(pool)
    .await?;

    let mut total = 0usize;
    for (host_id, addr) in hosts {
        match sync_host_networks(pool, host_id, &addr).await {
            Ok(n) => total += n,
            Err(e) => tracing::warn!(%host_id, "network discover failed: {e:#}"),
        }
    }
    Ok(total)
}
