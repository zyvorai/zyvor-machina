// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//
// Day-2: scheduled fleet backups with retention. Backups were manual-trigger only;
// this mirrors fleet_snapshot_scheduler (which had both scheduling and retention) so
// backups get the same automation + a bounded catalog.

use crate::db::DbPool;
use uuid::Uuid;

use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

pub fn spawn(state: AppState) {
    super::backup_verifier::spawn(state.clone());
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            if let Err(e) = tick(&state).await {
                tracing::warn!("fleet backup scheduler: {e:#}");
            }
        }
    });
}

async fn tick(state: &AppState) -> anyhow::Result<()> {
    // Due = never run, or last run older than the schedule's interval. We compare in SQL
    // using each row's interval_hours so schedules with different cadences are honored.
    let rows: Vec<(Uuid, String, String, String, Option<String>, i32)> = crate::db::query_as(
        "SELECT id, project, tag_filter, backup_type, target_id, retain_count
         FROM backup_schedules
         WHERE enabled = TRUE
           AND (last_run_at IS NULL
                OR last_run_at < datetime('now', printf('-%d hours', interval_hours)))",
    )
    .fetch_all(&state.pool)
    .await?;

    for (sched_id, project, tag_filter, backup_type, target_id, retain_count) in rows {
        // A failure here must not abort the whole tick: if it did, schedules later in `rows`
        // would be skipped this tick, and — worse — this schedule's `last_run_at` would stay
        // stale, so the *next* tick would see it as still due and re-enqueue backups for VMs
        // that already got one in this run (duplicate backups). Log and move on instead.
        if let Err(e) = enqueue_backups_for_schedule(
            &state.pool,
            state,
            &project,
            &tag_filter,
            &backup_type,
            target_id.as_deref(),
        )
        .await
        {
            tracing::warn!("fleet backup scheduler: schedule {sched_id} enqueue failed: {e:#}");
            continue;
        }
        crate::db::query("UPDATE backup_schedules SET last_run_at = datetime('now') WHERE id = ?")
            .bind(sched_id)
            .execute(&state.pool)
            .await?;
        if retain_count > 0 {
            prune_retained_backups(state, &project, &tag_filter, retain_count).await?;
        }
    }
    Ok(())
}

/// Select the managed VMs a schedule targets (whole fleet, a project, or a project+tag),
/// mirroring fleet_snapshot_scheduler's filtering.
async fn select_vms(
    pool: &DbPool,
    project: &str,
    tag_filter: &str,
) -> anyhow::Result<Vec<(Uuid, String, Option<Uuid>)>> {
    let base = "SELECT id, name, host_id FROM vms
                WHERE managed = TRUE AND COALESCE(inventory_source, 'libvirt') = 'libvirt'
                  AND lifecycle_phase NOT IN ('retired', 'deleting')";
    let vms = if !project.is_empty() && !tag_filter.is_empty() {
        crate::db::query_as(&format!(
            "{base} AND project = ? AND EXISTS (SELECT 1 FROM json_each(COALESCE(tags,'[]')) WHERE value = ?)"
        ))
        .bind(project)
        .bind(tag_filter)
        .fetch_all(pool)
        .await?
    } else if !project.is_empty() {
        crate::db::query_as(&format!("{base} AND project = ?"))
            .bind(project)
            .fetch_all(pool)
            .await?
    } else {
        crate::db::query_as(base).fetch_all(pool).await?
    };
    Ok(vms)
}

async fn enqueue_backups_for_schedule(
    pool: &DbPool,
    app: &AppState,
    project: &str,
    tag_filter: &str,
    backup_type: &str,
    target_id: Option<&str>,
) -> anyhow::Result<()> {
    let vms = select_vms(pool, project, tag_filter).await?;
    for (vm_id, vm_name, host_id) in vms {
        let backup_id = Uuid::new_v4();
        // Insert the record before enqueue (the worker looks it up by id), compensating
        // with a delete on enqueue failure — same ordering as create_vm_backup.
        crate::db::query(
            "INSERT INTO backup_records (id, vm_id, backup_type, status) VALUES (?, ?, ?, 'pending')",
        )
        .bind(backup_id)
        .bind(vm_id)
        .bind(backup_type)
        .execute(pool)
        .await?;
        if let Err(e) = enqueue_task(
            app,
            "vm.backup",
            serde_json::json!({
                "vm_id": vm_id.to_string(),
                "backup_id": backup_id.to_string(),
                "target_id": target_id,
                "description": format!("Scheduled backup for {vm_name}"),
            }),
            Some("vm"),
            Some(vm_id),
            host_id,
        )
        .await
        {
            let _ = crate::db::query("DELETE FROM backup_records WHERE id = ?")
                .bind(backup_id)
                .execute(pool)
                .await;
            // Log and keep going: aborting here would skip the remaining VMs in this
            // schedule *and* (via the caller) leave last_run_at stale, causing the whole
            // schedule — including VMs already enqueued above — to be retried next tick.
            tracing::warn!(
                "fleet backup scheduler: enqueue vm.backup for {vm_id}: {}",
                e.message
            );
        }
    }
    Ok(())
}

/// Retention: keep only the newest `retain_count` completed backups per VM in the schedule's
/// scope; for older ones, enqueue `vm.backup.delete` which removes the on-disk file via the
/// agent AND the catalog row — so backup storage is actually reclaimed (not just the DB row).
async fn prune_retained_backups(
    app: &AppState,
    project: &str,
    tag_filter: &str,
    retain_count: i32,
) -> anyhow::Result<()> {
    let vms = select_vms(&app.pool, project, tag_filter).await?;
    for (vm_id, _name, host_id) in vms {
        let prunable: Vec<(Uuid,)> = crate::db::query_as(
            "SELECT id FROM backup_records
             WHERE vm_id = ? AND status IN ('completed', 'succeeded')
               AND id NOT IN (
                   SELECT id FROM backup_records
                   WHERE vm_id = ? AND status IN ('completed', 'succeeded')
                   ORDER BY created_at DESC LIMIT ?
               )",
        )
        .bind(vm_id)
        .bind(vm_id)
        .bind(retain_count)
        .fetch_all(&app.pool)
        .await?;
        for (backup_id,) in prunable {
            let _ = enqueue_task(
                app,
                "vm.backup.delete",
                serde_json::json!({ "backup_id": backup_id.to_string() }),
                Some("vm"),
                Some(vm_id),
                host_id,
            )
            .await;
        }
    }
    Ok(())
}
