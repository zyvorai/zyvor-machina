// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::time::Duration;

use uuid::Uuid;

use crate::engine::drs;
use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

pub fn spawn_periodic(state: AppState) {
    tokio::spawn(async move {
        loop {
            if !state.leader.is_leader() {
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }

            let interval_secs = drs::get_inventory_sync_interval_secs(&state.pool)
                .await
                .unwrap_or(30);

            if interval_secs > 0 {
                if let Err(e) = sync_all_hosts(&state).await {
                    tracing::warn!("periodic host sync: {e:#}");
                }
                if let Err(e) = sync_kubevirt_inventory(&state).await {
                    tracing::warn!("periodic kubevirt inventory: {e:#}");
                }
                tokio::time::sleep(Duration::from_secs(interval_secs as u64)).await;
            } else {
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        }
    });
}

async fn sync_all_hosts(state: &AppState) -> anyhow::Result<()> {
    // Order by staleness (least-recently-synced first, never-synced first of
    // all) rather than an arbitrary row order: with more than 200 hosts in the
    // fleet, an unordered `LIMIT 200` would sync the exact same 200 rows every
    // tick forever, permanently starving inventory sync for every host beyond
    // that cap. `last_heartbeat_at` is exactly "last successful host.inventory
    // sync" (set by the host_inventory task handler on success), so ordering by
    // it rotates coverage across the whole fleet over time instead of wedging
    // on the same subset.
    let host_ids: Vec<Uuid> = crate::db::query_scalar(
        "SELECT id FROM hosts ORDER BY last_heartbeat_at ASC NULLS FIRST LIMIT 200",
    )
    .fetch_all(&state.pool)
    .await?;

    for host_id in host_ids {
        // Skip if a RECENT host.inventory for this host is already pending/running.
        // Every tick mints a NEW task_id, so the per-id NATS-echo dedup doesn't apply
        // across ticks; without this guard, a worker that can't drain 200 hosts
        // within the interval (each dead host costs up to the connect timeout)
        // accumulates an ever-growing backlog in the task table + channel.
        //
        // The `-5 minutes` bound is essential: host.inventory is a bounded read RPC
        // (~seconds), so anything still 'pending'/'running' after 5 min is orphaned
        // (e.g. dropped by an in-memory-bus restart) — without the bound one such row
        // would block this host's sync FOREVER, so its heartbeat never refreshes and
        // it falls offline, breaking VM placement.
        let inflight: i64 = crate::db::query_scalar(
            "SELECT COUNT(*) FROM tasks WHERE resource_id = ? AND operation = 'host.inventory' \
             AND status IN ('pending', 'running') AND created_at > datetime('now', '-5 minutes')",
        )
        .bind(host_id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
        if inflight > 0 {
            continue;
        }
        if let Err(e) = enqueue_task(
            state,
            "host.inventory",
            serde_json::json!({ "host_id": host_id.to_string() }),
            Some("host"),
            Some(host_id),
            Some(host_id),
        )
        .await
        {
            tracing::warn!(host_id = %host_id, "host.inventory enqueue failed in sync: {}", e.message);
        }
    }
    Ok(())
}

async fn sync_kubevirt_inventory(state: &AppState) -> anyhow::Result<()> {
    let cluster_id: Option<Uuid> =
        crate::db::query_scalar("SELECT id FROM clusters ORDER BY created_at LIMIT 1")
            .fetch_optional(&state.pool)
            .await?;
    let Some(cluster_id) = cluster_id else {
        return Ok(());
    };
    // Same anti-backlog guard as host.inventory, with the same staleness bound so an
    // orphaned pending row can't wedge kubevirt sync forever.
    let inflight: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE resource_id = ? AND operation = 'kubevirt.inventory' \
         AND status IN ('pending', 'running') AND created_at > datetime('now', '-5 minutes')",
    )
    .bind(cluster_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);
    if inflight > 0 {
        return Ok(());
    }
    if let Err(e) = enqueue_task(
        state,
        "kubevirt.inventory",
        serde_json::json!({ "cluster_id": cluster_id.to_string() }),
        Some("cluster"),
        Some(cluster_id),
        None,
    )
    .await
    {
        tracing::warn!(cluster_id = %cluster_id, "kubevirt.inventory enqueue failed in sync: {}", e.message);
    }
    Ok(())
}
