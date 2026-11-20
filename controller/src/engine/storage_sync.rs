// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use crate::db::DbPool;
use uuid::Uuid;

use crate::agent_client;

/// Import libvirt storage pools from a host into the platform `storage_pools` table.
pub async fn sync_host_storage(
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
    let list = agent_client::list_storage_pools(&mut client).await?;
    let mut imported = 0usize;

    for sp in list.pools {
        if sp.name.is_empty() {
            continue;
        }
        let path = if sp.path.is_empty() {
            None
        } else {
            Some(sp.path)
        };
        let capacity_gib = sp.capacity_gib.round() as i64;
        let used_gib = sp.used_gib.round() as i64;
        let backend = if sp.backend.is_empty() {
            "directory"
        } else {
            sp.backend.as_str()
        };
        let result = crate::db::query(
            "INSERT INTO storage_pools (id, cluster_id, name, storage_class, backend, path, capacity_gib, used_gib)
             VALUES (?, ?, ?, 'silver', ?, ?, ?, ?)
             ON CONFLICT (cluster_id, name) DO UPDATE SET
               backend = EXCLUDED.backend,
               path = COALESCE(EXCLUDED.path, storage_pools.path),
               -- Gate both on a valid capacity read (>0). Guarding used_gib on
               -- `used > 0` meant a pool that legitimately dropped to 0 usage
               -- never recorded it; a fully-failed read (all zeros) still keeps
               -- the old values via the capacity guard.
               capacity_gib = CASE WHEN EXCLUDED.capacity_gib > 0 THEN EXCLUDED.capacity_gib ELSE storage_pools.capacity_gib END,
               used_gib = CASE WHEN EXCLUDED.capacity_gib > 0 THEN EXCLUDED.used_gib ELSE storage_pools.used_gib END",
        )
        .bind(Uuid::new_v4())
        .bind(cluster_id)
        .bind(&sp.name)
        .bind(backend)
        .bind(&path)
        .bind(capacity_gib)
        .bind(used_gib)
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
        match sync_host_storage(pool, host_id, &addr).await {
            Ok(n) => total += n,
            Err(e) => tracing::warn!(%host_id, "storage discover failed: {e:#}"),
        }
    }
    Ok(total)
}
