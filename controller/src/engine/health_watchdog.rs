// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//
// Day-2: autonomous health watchdog. For VMs the operator has opted into, detect a guest
// that has HUNG — the domain is still `running` at the hypervisor but its guest agent, which
// was previously responsive, has gone unreachable — and hard-reset it. This is the gap
// between HA (host failure) and reconcile (desired!=observed power drift): a live-but-wedged
// guest. Every action is bounded (sustained-failure threshold, cooldown, per-hour cap) and
// audited, and the watchdog is disabled by default per VM.

use std::time::Duration;

use uuid::Uuid;

use crate::agent_client;
use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

pub fn spawn(state: AppState) {
    // Global kill-switch so an operator can stop all auto-restarts without editing policies.
    if std::env::var("MACHINA_WATCHDOG_DISABLED").ok().as_deref() == Some("1") {
        tracing::info!("health watchdog disabled via MACHINA_WATCHDOG_DISABLED=1");
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            if let Err(e) = tick(&state).await {
                tracing::warn!("health watchdog: {e:#}");
            }
        }
    });
}

struct WatchTarget {
    vm_id: Uuid,
    name: String,
    host_id: Option<Uuid>,
    observed: String,
    guest_tools_status: String,
    unhealthy_since: Option<String>,
    last_restart_at: Option<String>,
    failure_threshold_secs: i64,
    cooldown_secs: i64,
    max_restarts_per_hour: i64,
    restarts_this_hour: i64,
    hour_window_start: Option<String>,
}

async fn tick(state: &AppState) -> anyhow::Result<()> {
    let rows: Vec<WatchTarget> = crate::db::query_as::<_, (
        Uuid, String, Option<Uuid>, String, String,
        Option<String>, Option<String>, i64, i64, i64, i64, Option<String>,
    )>(
        "SELECT w.vm_id, v.name, v.host_id, v.observed_state,
                COALESCE(v.guest_tools_status, 'unknown'),
                w.unhealthy_since, w.last_restart_at, w.failure_threshold_secs,
                w.cooldown_secs, w.max_restarts_per_hour, w.restarts_this_hour, w.hour_window_start
         FROM vm_watchdog w JOIN vms v ON v.id = w.vm_id
         WHERE w.enabled = TRUE
           AND v.lifecycle_phase NOT IN ('creating','migrating','deleting','snapshotting','backing_up','retired')",
    )
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|r| WatchTarget {
        vm_id: r.0, name: r.1, host_id: r.2, observed: r.3, guest_tools_status: r.4,
        unhealthy_since: r.5, last_restart_at: r.6, failure_threshold_secs: r.7,
        cooldown_secs: r.8, max_restarts_per_hour: r.9, restarts_this_hour: r.10, hour_window_start: r.11,
    })
    .collect();

    for t in rows {
        if let Err(e) = evaluate(state, &t).await {
            tracing::warn!(vm = %t.name, "watchdog evaluate: {e:#}");
        }
    }
    Ok(())
}

async fn evaluate(state: &AppState, t: &WatchTarget) -> anyhow::Result<()> {
    // Only running VMs are candidates. A stopped VM is reconcile/HA territory; clear state.
    if t.observed != "running" {
        return clear_unhealthy(state, t.vm_id).await;
    }
    // We can only judge "hung" for a VM whose guest agent was known-good. If tools were never
    // installed (not_installed/unknown), an unreachable probe means nothing — never reset it.
    if !matches!(t.guest_tools_status.as_str(), "healthy" | "installed") {
        return clear_unhealthy(state, t.vm_id).await;
    }

    let reachable = probe_guest_reachable(state, t).await;
    if reachable {
        // Recovered (or never actually down) — reset the unhealthy clock.
        return clear_unhealthy(state, t.vm_id).await;
    }

    // Guest agent unreachable while running + tools were installed => likely hung.
    if t.unhealthy_since.is_none() {
        crate::db::query("UPDATE vm_watchdog SET unhealthy_since = datetime('now') WHERE vm_id = ?")
            .bind(t.vm_id)
            .execute(&state.pool)
            .await?;
        tracing::info!(vm = %t.name, "watchdog: guest unreachable, starting failure timer");
        return Ok(());
    }

    // Sustained-failure gate: only act once continuously unhealthy for failure_threshold_secs.
    let sustained: bool = crate::db::query_scalar(
        "SELECT unhealthy_since <= datetime('now', printf('-%d seconds', ?)) FROM vm_watchdog WHERE vm_id = ?",
    )
    .bind(t.failure_threshold_secs)
    .bind(t.vm_id)
    .fetch_optional(&state.pool)
    .await?
    .unwrap_or(false);
    if !sustained {
        return Ok(());
    }

    // Cooldown gate: don't reset again within cooldown_secs of the last reset.
    if t.last_restart_at.is_some() {
        let cooling: bool = crate::db::query_scalar(
            "SELECT last_restart_at > datetime('now', printf('-%d seconds', ?)) FROM vm_watchdog WHERE vm_id = ?",
        )
        .bind(t.cooldown_secs)
        .bind(t.vm_id)
        .fetch_optional(&state.pool)
        .await?
        .unwrap_or(false);
        if cooling {
            return Ok(());
        }
    }

    // Per-hour cap gate: roll the window, then enforce max_restarts_per_hour.
    let window_expired = match &t.hour_window_start {
        None => true,
        Some(_) => crate::db::query_scalar::<_, bool>(
            "SELECT hour_window_start <= datetime('now', '-1 hour') FROM vm_watchdog WHERE vm_id = ?",
        )
        .bind(t.vm_id)
        .fetch_optional(&state.pool)
        .await?
        .unwrap_or(true),
    };
    let restarts_in_window = if window_expired {
        0
    } else {
        t.restarts_this_hour
    };
    if restarts_in_window >= t.max_restarts_per_hour {
        tracing::warn!(vm = %t.name, "watchdog: per-hour restart cap reached ({}), not resetting", t.max_restarts_per_hour);
        return Ok(());
    }

    // Skip if a power task is already in flight for this VM (mirror reconcile's overlap guard).
    // Propagate (rather than default to 0) on a query failure: we don't actually know
    // whether a task is in flight, and defaulting to "none" here risks double-enqueuing
    // a reset. Failing this tick and retrying next tick is the safe fallback.
    let inflight: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE resource_id = ? AND operation = 'vm.power' AND status IN ('pending','running')",
    )
    .bind(t.vm_id)
    .fetch_one(&state.pool)
    .await?;
    if inflight > 0 {
        return Ok(());
    }

    // Act: hard-reset the wedged guest.
    let host_id = match t.host_id {
        Some(h) => h,
        None => return Ok(()),
    };
    enqueue_task(
        state,
        "vm.power",
        serde_json::json!({ "vm_id": t.vm_id.to_string(), "action": "reset", "watchdog": true }),
        Some("vm"),
        Some(t.vm_id),
        Some(host_id),
    )
    .await
    .map_err(|e| anyhow::anyhow!("enqueue watchdog reset: {}", e.message))?;

    // Record the action: advance the window/counter, stamp last_restart_at, clear the timer.
    let new_count = restarts_in_window + 1;
    crate::db::query(
        "UPDATE vm_watchdog
         SET last_restart_at = datetime('now'),
             unhealthy_since = NULL,
             restarts_this_hour = ?,
             hour_window_start = CASE WHEN ? THEN datetime('now') ELSE hour_window_start END
         WHERE vm_id = ?",
    )
    .bind(new_count)
    .bind(window_expired)
    .bind(t.vm_id)
    .execute(&state.pool)
    .await?;

    let _ = crate::tasks::enqueue::write_audit(
        state,
        "system:watchdog",
        "vm.watchdog.reset",
        "vm",
        Some(t.vm_id),
        serde_json::json!({
            "vm": t.name,
            "reason": "guest agent unreachable while running (hung guest)",
            "failure_threshold_secs": t.failure_threshold_secs,
        }),
    )
    .await;
    state.emit_event(
        "vm.watchdog.reset",
        format!(
            "Watchdog reset hung VM '{}' ({}/{} this hour)",
            t.name, new_count, t.max_restarts_per_hour
        ),
    );
    tracing::warn!(vm = %t.name, "watchdog: hard-reset hung guest ({}/{} this hour)", new_count, t.max_restarts_per_hour);
    Ok(())
}

async fn clear_unhealthy(state: &AppState, vm_id: Uuid) -> anyhow::Result<()> {
    crate::db::query("UPDATE vm_watchdog SET unhealthy_since = NULL WHERE vm_id = ? AND unhealthy_since IS NOT NULL")
        .bind(vm_id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

/// True if the guest agent responds. A connect/RPC failure is treated as unreachable.
async fn probe_guest_reachable(state: &AppState, t: &WatchTarget) -> bool {
    let Some(host_id) = t.host_id else {
        return false;
    };
    let Ok(addr) = host_agent_addr(&state.pool, host_id).await else {
        return false;
    };
    let Ok(mut client) = agent_client::connect(&addr).await else {
        return false;
    };
    match agent_client::get_guest_health(&mut client, &t.name).await {
        Ok(gh) => gh.agent_reachable,
        Err(_) => false,
    }
}

async fn host_agent_addr(pool: &crate::db::DbPool, host_id: Uuid) -> anyhow::Result<String> {
    let addr: String = crate::db::query_scalar("SELECT agent_grpc_addr FROM hosts WHERE id = ?")
        .bind(host_id)
        .fetch_one(pool)
        .await?;
    Ok(addr)
}
