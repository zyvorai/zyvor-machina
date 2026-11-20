// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::time::Duration;

use uuid::Uuid;

use crate::engine::vm_lifecycle;
use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            if !state.leader.is_leader() {
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
            if let Err(e) = reconcile_once(&state).await {
                tracing::warn!("vm reconcile loop: {e:#}");
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

/// Map a desired/observed state pair to the vm.power action that converges the
/// VM, or None when nothing needs doing. A running-but-paused domain must be
/// RESUMED, not started: `start` errors with "domain already running", so the
/// VM would never converge and would re-enqueue a failing task every tick.
/// A pmsuspended domain (guest-initiated S3) counts as up for desired=running:
/// the domain is active, so the agent's `start` is a silent no-op and reconcile
/// would enqueue a useless task every backoff interval forever — and the agent
/// has no pmwakeup op to actually rouse the guest. It IS stoppable (destroy
/// works on an active domain), so desired=stopped must still act.
/// A "shutting down" domain is likewise still active (libvirt VIR_DOMAIN_SHUTDOWN)
/// until the guest actually powers off, so `start` no-ops there too -- without this
/// exclusion reconcile re-enqueues a vm.power task (and its notification) every
/// tick against a guest hung mid-shutdown, forever.
fn reconcile_action(desired: &str, observed: &str) -> Option<&'static str> {
    if desired == "running" && observed == "paused" {
        Some("resume")
    } else if desired == "running"
        && !matches!(
            observed,
            "running" | "blocked" | "pmsuspended" | "shutting down"
        )
    {
        Some("start")
    } else if desired == "stopped"
        && matches!(observed, "running" | "blocked" | "paused" | "pmsuspended")
    {
        Some("stop")
    } else {
        None
    }
}

/// Minutes to wait since the last power attempt before retrying a chronically
/// failing reconcile op: 2^fails minutes, capped at 30. `fails == 0` → no wait.
fn reconcile_backoff_minutes(recent_fails: i64) -> i64 {
    if recent_fails <= 0 {
        return 0;
    }
    (1i64 << recent_fails.min(5)).min(30)
}

async fn reconcile_once(state: &AppState) -> anyhow::Result<()> {
    // Order by staleness, not row order: with more than 20 out-of-sync VMs, an
    // unordered `LIMIT 20` would reconcile the exact same 20 rows every tick
    // forever, starving every VM beyond that cap. `vms.updated_at` isn't usable
    // for this — the host.inventory sync path stamps it on every tick for every
    // VM it sees, converged or not, so it doesn't track "last reconcile attempt".
    // Instead use each VM's most recent 'vm.power' task (any status) as a proxy
    // for "last time we tried to converge this VM": order least-recently-
    // attempted (and never-attempted) first, so a VM skipped this tick sorts
    // ahead of ones just enqueued, guaranteeing rotation across the whole
    // out-of-sync set over successive ticks instead of wedging on the first 20.
    let rows: Vec<(Uuid, String, String, String)> = crate::db::query_as(
        "SELECT v.id, v.name, v.desired_state, v.observed_state
         FROM vms v
         LEFT JOIN (
             SELECT resource_id, MAX(created_at) AS last_attempt
             FROM tasks
             WHERE operation = 'vm.power'
             GROUP BY resource_id
         ) t ON t.resource_id = v.id
         WHERE v.desired_state != v.observed_state
           AND v.observed_state NOT IN ('missing', 'unknown')
           AND (v.inventory_source = 'libvirt' OR (v.inventory_source = 'fluxvm' AND v.managed))
           AND v.lifecycle_phase NOT IN ('creating', 'migrating', 'deleting', 'snapshotting', 'backing_up', 'retired')
         ORDER BY t.last_attempt ASC NULLS FIRST
         LIMIT 20",
    )
    .fetch_all(&state.pool)
    .await?;

    for (vm_id, name, desired, observed) in rows {
        tracing::info!("reconcile VM {name}: desired={desired} observed={observed}");
        let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_optional(&state.pool)
            .await?;

        let Some(host_id) = host_id else {
            continue;
        };

        let Some(action) = reconcile_action(&desired, &observed) else {
            continue;
        };

        // Don't pile up power tasks: if a vm.power for this VM is already pending
        // or running, skip this tick. Otherwise a VM that never converges (a start
        // that keeps failing, or a guest ignoring ACPI shutdown) would accrue a
        // fresh task — and a task_failed webhook — every 60s indefinitely.
        let inflight: i64 = crate::db::query_scalar(
            "SELECT COUNT(*) FROM tasks WHERE resource_id = ? AND operation = 'vm.power' AND status IN ('pending', 'running')",
        )
        .bind(vm_id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
        if inflight > 0 {
            continue;
        }

        // Exponential backoff on a chronically-failing power op. The overlap guard
        // above only prevents *concurrent* tasks; a start/stop that fails fast
        // leaves the pending/running set immediately, so without backoff a VM that
        // never converges would re-enqueue — and fire a task_failed webhook —
        // every 60s forever. Space attempts by 2^fails minutes (capped at 30)
        // measured from the last attempt; recent_fails == 0 → no delay.
        let recent_fails: i64 = crate::db::query_scalar(
            "SELECT COUNT(*) FROM tasks
             WHERE resource_id = ? AND operation = 'vm.power' AND status = 'failed'
               AND created_at > datetime('now', '-1 hour')",
        )
        .bind(vm_id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
        if recent_fails > 0 {
            let backoff_min = reconcile_backoff_minutes(recent_fails);
            let too_soon: bool = crate::db::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM tasks
                 WHERE resource_id = ? AND operation = 'vm.power'
                   AND created_at > datetime('now', ?))",
            )
            .bind(vm_id)
            .bind(format!("-{backoff_min} minutes"))
            .fetch_one(&state.pool)
            .await
            .unwrap_or(false);
            if too_soon {
                continue;
            }
        }

        if let Err(e) = enqueue_task(
            state,
            "vm.power",
            serde_json::json!({
                "vm_id": vm_id.to_string(),
                "action": action,
                "reconcile": true,
            }),
            Some("vm"),
            Some(vm_id),
            Some(host_id),
        )
        .await
        {
            tracing::warn!(vm_id = %vm_id, action, "reconcile enqueue failed: {}", e.message);
        }

        if let Err(e) = vm_lifecycle::sync_phase_from_observed(&state.pool, vm_id).await {
            tracing::warn!(vm_id = %vm_id, "reconcile sync_phase failed: {e:#}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{reconcile_action, reconcile_backoff_minutes, reconcile_once};
    use crate::engine::test_support::{seed_host, test_state};
    use uuid::Uuid;

    #[test]
    fn backoff_schedule() {
        assert_eq!(reconcile_backoff_minutes(0), 0); // healthy → retry immediately
        assert_eq!(reconcile_backoff_minutes(1), 2);
        assert_eq!(reconcile_backoff_minutes(2), 4);
        assert_eq!(reconcile_backoff_minutes(3), 8);
        assert_eq!(reconcile_backoff_minutes(4), 16);
        assert_eq!(reconcile_backoff_minutes(5), 30); // 32 capped to 30
        assert_eq!(reconcile_backoff_minutes(20), 30); // stays capped, no shift overflow
    }

    #[test]
    fn action_mapping() {
        // Paused regression: resume, never start (start = "domain already running").
        assert_eq!(reconcile_action("running", "paused"), Some("resume"));
        assert_eq!(reconcile_action("running", "shutoff"), Some("start"));
        assert_eq!(reconcile_action("running", "crashed"), Some("start"));
        // Already converged (or effectively running) → nothing to do.
        assert_eq!(reconcile_action("running", "running"), None);
        assert_eq!(reconcile_action("running", "blocked"), None);
        // Guest S3: active domain, agent start is a no-op and there's no pmwakeup —
        // must not loop a useless task forever.
        assert_eq!(reconcile_action("running", "pmsuspended"), None);
        // Hung mid-shutdown: domain still active, agent start is a no-op — must not
        // loop a useless (and notification-spamming) task forever either.
        assert_eq!(reconcile_action("running", "shutting down"), None);
        // Stop covers every "still up" observed state, including paused and S3.
        assert_eq!(reconcile_action("stopped", "running"), Some("stop"));
        assert_eq!(reconcile_action("stopped", "blocked"), Some("stop"));
        assert_eq!(reconcile_action("stopped", "paused"), Some("stop"));
        assert_eq!(reconcile_action("stopped", "pmsuspended"), Some("stop"));
        assert_eq!(reconcile_action("stopped", "shutoff"), None);
        // Sleeping (managed-saved) VMs are woken by traffic, never by reconcile.
        assert_eq!(reconcile_action("sleeping", "shutoff"), None);
        assert_eq!(reconcile_action("sleeping", "running"), None);
    }

    async fn seed_vm(pool: &crate::db::DbPool, desired: &str, observed: &str) -> Uuid {
        let host_id = seed_host(pool, Uuid::from_u128(10)).await;
        let vm_id = Uuid::from_u128(11);
        crate::db::query(
            "INSERT INTO vms (id, host_id, name, desired_state, observed_state)
             VALUES (?, ?, 'vm1', ?, ?)",
        )
        .bind(vm_id)
        .bind(host_id)
        .bind(desired)
        .bind(observed)
        .execute(pool)
        .await
        .unwrap();
        vm_id
    }

    async fn power_tasks(pool: &crate::db::DbPool, vm_id: Uuid) -> Vec<serde_json::Value> {
        let rows: Vec<(serde_json::Value,)> = crate::db::query_as(
            "SELECT payload FROM tasks WHERE resource_id = ? AND operation = 'vm.power'",
        )
        .bind(vm_id)
        .fetch_all(pool)
        .await
        .unwrap();
        rows.into_iter().map(|(p,)| p).collect()
    }

    #[tokio::test]
    async fn inflight_power_task_suppresses_duplicate_enqueue() {
        // Dedup regression: a pending vm.power task for the VM must stop the next
        // tick from enqueueing a second, competing task.
        let (state, _rx) = test_state().await;
        let vm_id = seed_vm(&state.pool, "running", "shutoff").await;

        reconcile_once(&state).await.unwrap();
        let tasks = power_tasks(&state.pool, vm_id).await;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["action"], serde_json::json!("start"));
        assert_eq!(tasks[0]["reconcile"], serde_json::json!(true));

        // Task is still 'pending' (no worker in tests) → second pass must not add another.
        reconcile_once(&state).await.unwrap();
        assert_eq!(power_tasks(&state.pool, vm_id).await.len(), 1);
    }

    #[tokio::test]
    async fn paused_vm_is_resumed_not_started() {
        let (state, _rx) = test_state().await;
        let vm_id = seed_vm(&state.pool, "running", "paused").await;

        reconcile_once(&state).await.unwrap();
        let tasks = power_tasks(&state.pool, vm_id).await;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["action"], serde_json::json!("resume"));
    }
}
