// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use crate::db::DbPool;
use uuid::Uuid;

pub const PHASE_IDLE: &str = "idle";
pub const PHASE_CREATING: &str = "creating";
pub const PHASE_STARTING: &str = "starting";
pub const PHASE_RUNNING: &str = "running";
pub const PHASE_STOPPING: &str = "stopping";
pub const PHASE_STOPPED: &str = "stopped";
pub const PHASE_MIGRATING: &str = "migrating";
pub const PHASE_SNAPSHOTTING: &str = "snapshotting";
pub const PHASE_BACKING_UP: &str = "backing_up";
pub const PHASE_DELETING: &str = "deleting";
pub const PHASE_ERROR: &str = "error";
pub const PHASE_RETIRED: &str = "retired";
pub const PHASE_SLEEPING: &str = "sleeping";

pub fn phase_for_operation(op: &str) -> &'static str {
    match op {
        "vm.apply" => PHASE_CREATING,
        "vm.power" => PHASE_STARTING,
        "vm.migrate" => PHASE_MIGRATING,
        "vm.snapshot" | "vm.snapshot.revert" | "vm.snapshot.clone" => PHASE_SNAPSHOTTING,
        "vm.backup" | "vm.backup.restore" => PHASE_BACKING_UP,
        "vm.delete" => PHASE_DELETING,
        _ => PHASE_IDLE,
    }
}

pub async fn set_vm_phase(pool: &DbPool, vm_id: Uuid, phase: &str) -> anyhow::Result<()> {
    crate::db::query("UPDATE vms SET lifecycle_phase = ?, updated_at = datetime('now') WHERE id = ?")
        .bind(phase)
        .bind(vm_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_vm_phase_clear_error(
    pool: &DbPool,
    vm_id: Uuid,
    phase: &str,
) -> anyhow::Result<()> {
    crate::db::query(
        "UPDATE vms SET lifecycle_phase = ?, last_error = '', updated_at = datetime('now') WHERE id = ?",
    )
    .bind(phase)
    .bind(vm_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_vm_error(pool: &DbPool, vm_id: Uuid, message: &str) -> anyhow::Result<()> {
    crate::db::query(
        "UPDATE vms SET lifecycle_phase = ?, last_error = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(PHASE_ERROR)
    .bind(message)
    .bind(vm_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn sync_phase_from_observed(pool: &DbPool, vm_id: Uuid) -> anyhow::Result<()> {
    let row: Option<(String, String, String, String)> = crate::db::query_as(
        "SELECT desired_state, observed_state, lifecycle_phase, COALESCE(last_error, '') FROM vms WHERE id = ?",
    )
    .bind(vm_id)
    .fetch_optional(pool)
    .await?;
    let Some((desired, observed, current_phase, last_error)) = row else {
        return Ok(());
    };
    // RETIRED is a sticky, operator-set phase that blocks future starts. It must
    // survive the stop task (and any later snapshot/backup task) that reconciles a
    // retired VM's observed state — otherwise the start-guard in `power_action`
    // (which only checks `lifecycle_phase == PHASE_RETIRED`) is silently defeated
    // the moment the VM finishes stopping. Only an explicit "restore" clears it.
    if current_phase == PHASE_RETIRED {
        return Ok(());
    }
    let phase = match (desired.as_str(), observed.as_str()) {
        ("running", "running") | ("running", "blocked") => PHASE_RUNNING,
        ("running", _) if !matches!(observed.as_str(), "running" | "blocked") => PHASE_STARTING,
        ("stopped", "shutoff") | ("stopped", "stopped") => PHASE_STOPPED,
        ("stopped", _) => PHASE_STOPPING,
        ("sleeping", "shutoff") | ("sleeping", "stopped") => PHASE_SLEEPING,
        (_, "running") | (_, "blocked") => PHASE_RUNNING,
        _ => PHASE_IDLE,
    };
    // Drop stale "already running" resume/start failures once inventory agrees the guest is up.
    let stale_already_running = matches!(observed.as_str(), "running" | "blocked")
        && last_error.to_ascii_lowercase().contains("already running");
    if phase == PHASE_RUNNING && (current_phase == PHASE_ERROR || stale_already_running) {
        set_vm_phase_clear_error(pool, vm_id, phase).await
    } else {
        set_vm_phase(pool, vm_id, phase).await
    }
}
