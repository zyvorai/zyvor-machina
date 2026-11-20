// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use crate::db::DbPool;
use uuid::Uuid;

use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            if let Err(e) = tick(&state).await {
                tracing::warn!("fleet snapshot scheduler: {e:#}");
            }
        }
    });
}

async fn tick(state: &AppState) -> anyhow::Result<()> {
    let rows: Vec<(Uuid, String, String, String, bool, bool, i32)> = crate::db::query_as(
        "SELECT id, name, project, tag_filter, disk_only, quiesce, retain_count
         FROM fleet_snapshot_schedules
         WHERE enabled = TRUE
           AND (last_run_at IS NULL OR last_run_at < datetime('now', '-23 hours'))",
    )
    .fetch_all(&state.pool)
    .await?;

    for (sched_id, _name, project, tag_filter, disk_only, quiesce, _retain) in rows {
        // A failure here must not abort the whole tick: if it did, schedules later in `rows`
        // would be skipped this tick, and — worse — this schedule's `last_run_at` would stay
        // stale, so the *next* tick would see it as still due and re-enqueue snapshots for
        // VMs that already got one in this run (duplicate snapshots). Log and move on instead.
        if let Err(e) = enqueue_snapshots_for_schedule(
            &state.pool,
            state,
            sched_id,
            &project,
            &tag_filter,
            disk_only,
            quiesce,
        )
        .await
        {
            tracing::warn!("fleet snapshot scheduler: schedule {sched_id} enqueue failed: {e:#}");
            continue;
        }
        crate::db::query(
            "UPDATE fleet_snapshot_schedules SET last_run_at = datetime('now') WHERE id = ?",
        )
        .bind(sched_id)
        .execute(&state.pool)
        .await?;
    }
    Ok(())
}

async fn enqueue_snapshots_for_schedule(
    pool: &DbPool,
    app: &AppState,
    _sched_id: Uuid,
    project: &str,
    tag_filter: &str,
    disk_only: bool,
    quiesce: bool,
) -> anyhow::Result<()> {
    let vms: Vec<(Uuid, String, Option<Uuid>)> = if !project.is_empty() && !tag_filter.is_empty() {
        crate::db::query_as(
            "SELECT id, name, host_id FROM vms
             WHERE managed = TRUE AND COALESCE(inventory_source, 'libvirt') = 'libvirt'
               AND lifecycle_phase NOT IN ('retired', 'deleting')
               AND project = ? AND EXISTS (SELECT 1 FROM json_each(COALESCE(tags,'[]')) WHERE value = ?)",
        )
        .bind(project)
        .bind(tag_filter)
        .fetch_all(pool)
        .await?
    } else if !project.is_empty() {
        crate::db::query_as(
            "SELECT id, name, host_id FROM vms
             WHERE managed = TRUE AND COALESCE(inventory_source, 'libvirt') = 'libvirt'
               AND lifecycle_phase NOT IN ('retired', 'deleting')
               AND project = ?",
        )
        .bind(project)
        .fetch_all(pool)
        .await?
    } else {
        crate::db::query_as(
            "SELECT id, name, host_id FROM vms
             WHERE managed = TRUE AND COALESCE(inventory_source, 'libvirt') = 'libvirt'
               AND lifecycle_phase NOT IN ('retired', 'deleting')",
        )
        .fetch_all(pool)
        .await?
    };

    let stamp = chrono::Utc::now().format("%Y%m%d");
    for (vm_id, vm_name, host_id) in vms {
        let snap_name = format!("fleet-{stamp}");
        let record_id = Uuid::new_v4();
        crate::db::query(
            "INSERT INTO snapshot_records (id, vm_id, name, status) VALUES (?, ?, ?, 'pending')",
        )
        .bind(record_id)
        .bind(vm_id)
        .bind(&snap_name)
        .execute(pool)
        .await?;
        if let Err(e) = enqueue_task(
            app,
            "vm.snapshot",
            serde_json::json!({
                "vm_id": vm_id.to_string(),
                "snapshot_id": record_id.to_string(),
                "name": snap_name,
                "description": format!("Fleet scheduled snapshot for {vm_name}"),
                "disk_only": disk_only,
                "quiesce": quiesce,
                "storage_mode": "",
            }),
            Some("vm"),
            Some(vm_id),
            host_id,
        )
        .await
        {
            let _ = crate::db::query("DELETE FROM snapshot_records WHERE id = ?")
                .bind(record_id)
                .execute(pool)
                .await;
            // Log and keep going: aborting here would skip the remaining VMs in this
            // schedule *and* (via the caller) leave last_run_at stale, causing the whole
            // schedule — including VMs already enqueued above — to be retried next tick.
            tracing::warn!(
                "fleet snapshot scheduler: enqueue vm.snapshot for {vm_id}: {}",
                e.message
            );
        }
    }
    Ok(())
}
