// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::time::Duration;

use crate::db::DbPool;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

#[allow(dead_code)]
const HEARTBEAT_STALE_SECS: i64 = 90;

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(45));
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            if let Err(e) = scan(&state).await {
                tracing::warn!("HA scan: {e:#}");
            }
        }
    });
}

pub async fn scan(state: &AppState) -> anyhow::Result<()> {
    mark_stale_hosts(state).await?;
    retry_pending_fences(state).await?;
    recover_vms(state).await?;
    Ok(())
}

/// Retry fencing any host that is offline, has HA-enabled VMs, and was never
/// confirmed fenced. `mark_stale_hosts` only attempts a fence the instant a host
/// transitions to offline; if that single attempt fails (transient IPMI/network
/// blip, agent briefly unreachable, ipmitool hiccup), nothing ever retried it —
/// `state = 'online'` is required to re-select a host there, so a host stuck
/// offline+unfenced stayed that way forever, permanently blocking recovery of
/// every VM on it until an operator noticed and re-fenced manually. Re-attempt
/// on every scan (idempotent — fence_host is a no-op-safe power-off command)
/// until it succeeds or the host comes back online.
async fn retry_pending_fences(state: &AppState) -> anyhow::Result<()> {
    let pending: Vec<(Uuid, String)> = crate::db::query_as(
        "SELECT DISTINCT h.id, h.hostname FROM hosts h
         JOIN ha_policies hp ON hp.enabled = TRUE
         JOIN vms v ON v.id = hp.vm_id AND v.host_id = h.id
         WHERE h.state = 'offline' AND h.fenced = FALSE
         LIMIT 20",
    )
    .fetch_all(&state.pool)
    .await?;

    for (id, hostname) in pending {
        match crate::engine::drs::fence_host(state, id).await {
            Ok(true) => {
                tracing::info!(host_id = %id, "HA: retried fence of host {hostname} — confirmed fenced, VMs eligible for recovery");
            }
            Ok(false) => {
                tracing::warn!(host_id = %id, "HA: retried fence of host {hostname} — still not confirmed, will retry next scan");
            }
            Err(e) => {
                tracing::warn!(host_id = %id, "HA: retried fence of host {hostname} errored ({e:#}), will retry next scan");
            }
        }
    }
    Ok(())
}

async fn mark_stale_hosts(state: &AppState) -> anyhow::Result<()> {
    let pool = &state.pool;
    let stale: Vec<(Uuid, String)> = crate::db::query_as(
        "SELECT id, hostname FROM hosts
         WHERE state = 'online'
           AND last_heartbeat_at IS NOT NULL
           AND last_heartbeat_at < datetime('now', '-90 seconds')
         LIMIT 50",
    )
    .fetch_all(pool)
    .await?;

    for (id, hostname) in stale {
        let mut tx = pool.begin().await?;
        crate::db::query("UPDATE hosts SET state = 'offline' WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        crate::db::query(
            "INSERT INTO ha_events (id, vm_id, host_id, action, message) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4())
        .bind(Option::<Uuid>::None)
        .bind(Some(id))
        .bind("host.offline")
        .bind(format!("Host {hostname} marked offline"))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        // Fencing itself (for hosts with HA-enabled VMs) happens right after this
        // in `scan` via `retry_pending_fences`, which re-queries `state = 'offline'
        // AND fenced = FALSE` fresh from the DB — so it picks up this host on the
        // very same scan, and keeps retrying on subsequent scans if the attempt
        // fails, instead of a single fence-or-never attempt at the moment of
        // transition (see its doc comment for the failure-mode this fixed).
        tracing::warn!("HA: host {hostname} ({id}) marked offline");
    }
    Ok(())
}

async fn recover_vms(state: &AppState) -> anyhow::Result<()> {
    let Some((ha_enabled, allow_unfenced)): Option<(bool, bool)> = crate::db::query_as(
        "SELECT ha_enabled, ha_allow_unfenced_recovery FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_optional(&state.pool)
    .await?
    else {
        return Ok(());
    };
    if !ha_enabled {
        return Ok(());
    }

    let victims: Vec<(Uuid, String, Uuid, i32, i32, String, bool, bool, i64)> = crate::db::query_as(
        "SELECT v.id, v.name, v.host_id, v.ha_recovery_count, hp.restart_attempts, v.desired_state,
                h.fenced, hp.fence_on_failure, v.memory_mib
         FROM vms v
         JOIN ha_policies hp ON hp.vm_id = v.id AND hp.enabled = TRUE
         JOIN hosts h ON h.id = v.host_id
         WHERE h.state = 'offline'
           AND v.desired_state = 'running'
         LIMIT 100",
    )
    .fetch_all(&state.pool)
    .await?;

    // Memory (MiB) already promised to each candidate destination earlier in this
    // scan, so a burst of victims from one failed host isn't all piled onto the
    // single least-loaded survivor.
    let mut reserved: std::collections::HashMap<Uuid, i64> = std::collections::HashMap::new();

    // Anti-affinity groups by host, widened across racks, so a recovered VM lands away from a
    // group peer and away from the rack it just lost. A failure to build it only loses the
    // preference.
    let anti = crate::engine::placement::anti_affinity_by_rack(&state.pool)
        .await
        .unwrap_or_default();

    for (
        vm_id,
        vm_name,
        failed_host,
        recovery_count,
        max_attempts,
        desired,
        host_fenced,
        fence_required,
        vm_memory_mib,
    ) in victims
    {
        // Split-brain guard (fail-safe default): recover a VM ONLY once its failed
        // host is confirmed fenced (powered off). An unfenced host may still be
        // running the VM, so starting it elsewhere on shared storage corrupts data.
        // This now applies to EVERY ha-enabled VM, not just those with
        // fence_on_failure=TRUE — that per-VM opt-in left default VMs exposed.
        // The only bypass is the cluster's explicit ha_allow_unfenced_recovery, for
        // operators who have NON-shared storage (double-run can't corrupt) or
        // external fencing. A VM's own fence_on_failure=TRUE forces the guard even
        // when the cluster allows unfenced recovery.
        if block_recovery_unfenced(host_fenced, fence_required, allow_unfenced) {
            record_ha_event_deduped(
                &state.pool,
                Some(vm_id),
                Some(failed_host),
                "ha.blocked_unfenced",
                &format!(
                    "VM {vm_name} NOT recovered: host {failed_host} could not be confirmed fenced. \
                     Manually power it off, then it will recover — or set ha_allow_unfenced_recovery \
                     if VMs don't share storage."
                ),
            )
            .await?;
            tracing::warn!(
                vm_id = %vm_id,
                host_id = %failed_host,
                "HA: blocking recovery of VM {vm_name} — host not confirmed fenced (split-brain risk)"
            );
            continue;
        }

        if recovery_count >= max_attempts {
            record_ha_event_deduped(
                &state.pool,
                Some(vm_id),
                Some(failed_host),
                "ha.exhausted",
                &format!("VM {vm_name} exceeded restart attempts"),
            )
            .await?;
            continue;
        }

        // Capacity-aware destination: pick the least-loaded online host that
        // actually has enough free memory for the victim, accounting for other
        // victims already assigned to it earlier in THIS scan. Without this, HA
        // piled every victim of a failed host onto the single least-loaded
        // survivor and could push it into memory over-commit / OOM — ha.recover
        // calls apply_vm directly with no precheck (unlike vm.migrate). We gate on
        // memory (the hard OOM constraint) and leave CPU soft so an emergency
        // recovery isn't stranded merely because the surviving hosts run warm.
        let candidates: Vec<(Uuid, i64)> = crate::db::query_as(
            "SELECT id, (memory_total_mib - memory_used_mib) AS headroom FROM hosts
             WHERE id != ? AND state = 'online' AND maintenance_mode = FALSE AND schedulable = TRUE
             ORDER BY vm_count, memory_used_mib",
        )
        .bind(failed_host)
        .fetch_all(&state.pool)
        .await?;
        let vm_tags: Vec<String> = crate::db::query_scalar::<_, crate::db::Json<Vec<String>>>(
            "SELECT COALESCE(tags, '[]') FROM vms WHERE id = ?",
        )
        .bind(vm_id)
        .fetch_optional(&state.pool)
        .await?
        .map(|j| j.0)
        .unwrap_or_default();
        let candidates = crate::engine::failure_domains::prefer_non_conflicting(candidates, |h| {
            crate::engine::placement::host_conflicts(&anti, h, &vm_tags)
        });
        let dest = pick_ha_dest(&candidates, &reserved, vm_memory_mib);

        let Some(dest_host) = dest else {
            record_ha_event_deduped(
                &state.pool,
                Some(vm_id),
                Some(failed_host),
                "ha.no_capacity",
                &format!("No online host with capacity to recover VM {vm_name}"),
            )
            .await?;
            continue;
        };
        *reserved.entry(dest_host).or_insert(0) += vm_memory_mib;

        let mut tx = state.pool.begin().await?;
        crate::db::query(
            "UPDATE vms SET host_id = ?, ha_recovery_count = ha_recovery_count + 1, updated_at = datetime('now')
             WHERE id = ?",
        )
        .bind(dest_host)
        .bind(vm_id)
        .execute(&mut *tx)
        .await?;
        crate::db::query(
            "INSERT INTO ha_events (id, vm_id, host_id, action, message) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4())
        .bind(Some(vm_id))
        .bind(Some(failed_host))
        .bind("ha.recover")
        .bind(format!("Recovering VM {vm_name} onto host {dest_host}"))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        if let Err(e) = enqueue_task(
            state,
            "ha.recover",
            serde_json::json!({
                "vm_id": vm_id.to_string(),
                "host_id": dest_host.to_string(),
                "desired_state": desired,
                // Recorded so on_task_failure can revert host_id if the recovery
                // task itself (not just the enqueue) ultimately fails — see the
                // compensation logic in tasks/worker.rs.
                "recovered_from_host_id": failed_host.to_string(),
            }),
            Some("vm"),
            Some(vm_id),
            Some(dest_host),
        )
        .await
        {
            tracing::error!(vm_id = %vm_id, dest_host = %dest_host, "HA: failed to enqueue ha.recover task: {e:?} — resetting host_id and recovery_count so HA can retry");
            if let Err(undo_err) = crate::db::query(
                "UPDATE vms SET host_id = ?, ha_recovery_count = ha_recovery_count - 1 WHERE id = ?",
            )
            .bind(failed_host)
            .bind(vm_id)
            .execute(&state.pool)
            .await
            {
                tracing::error!(vm_id = %vm_id, "HA: compensation update also failed: {undo_err:?} — VM may be stuck on wrong host");
            }
            continue;
        }
        state.emit_event(
            "ha.recover",
            format!("Recovering {vm_name} after host failure"),
        );
    }
    Ok(())
}

/// Record an HA event, but suppress a duplicate when the most recent event for
/// this VM already carries the same action. `recover_vms` re-scans every 45s and
/// re-selects every VM still pointing at an offline host, so a VM stuck in the
/// `ha.exhausted` / `ha.no_capacity` state would otherwise insert an identical
/// row on every tick forever (unbounded ha_events growth + log spam). We only
/// want to record the *transition* into that state once; a later `ha.recover`
/// (or manual intervention) resets the last-action so a fresh episode records.
async fn record_ha_event_deduped(
    pool: &DbPool,
    vm_id: Option<Uuid>,
    host_id: Option<Uuid>,
    action: &str,
    message: &str,
) -> anyhow::Result<()> {
    if let Some(vid) = vm_id {
        let last: Option<String> = crate::db::query_scalar(
            "SELECT action FROM ha_events WHERE vm_id = ? ORDER BY created_at DESC, rowid DESC LIMIT 1",
        )
        .bind(vid)
        .fetch_optional(pool)
        .await?;
        if last.as_deref() == Some(action) {
            return Ok(());
        }
    }
    record_ha_event(pool, vm_id, host_id, action, message).await
}

/// Pick the first candidate host (candidates are pre-ordered least-loaded first)
/// whose free memory — minus memory already reserved to it earlier in this scan —
/// covers `need_mib`. Returns None when no online host can fit the VM.
/// Pick the first candidate host (already ordered least-loaded first) with enough
/// free memory for `need_mib`, honoring reservations already made this pass. Shared
/// by HA recovery and maintenance evacuation so both spread VMs by real capacity
/// instead of piling them onto one host.
pub(crate) fn pick_ha_dest(
    candidates: &[(Uuid, i64)],
    reserved: &std::collections::HashMap<Uuid, i64>,
    need_mib: i64,
) -> Option<Uuid> {
    candidates
        .iter()
        .find(|(id, headroom)| {
            let used = reserved.get(id).copied().unwrap_or(0);
            headroom.saturating_sub(used) >= need_mib
        })
        .map(|(id, _)| *id)
}

async fn record_ha_event(
    pool: &DbPool,
    vm_id: Option<Uuid>,
    host_id: Option<Uuid>,
    action: &str,
    message: &str,
) -> anyhow::Result<()> {
    crate::db::query(
        "INSERT INTO ha_events (id, vm_id, host_id, action, message) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4())
    .bind(vm_id)
    .bind(host_id)
    .bind(action)
    .bind(message)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct HaStatusRow {
    pub enabled_vms: i64,
    pub offline_hosts: i64,
    pub recent_events: i64,
}

pub async fn ha_status(pool: &DbPool) -> anyhow::Result<(HaStatusRow, Vec<HaEventRow>)> {
    let status: HaStatusRow = crate::db::query_as(
        "SELECT
           (SELECT COUNT(*) FROM ha_policies WHERE enabled = TRUE) AS enabled_vms,
           (SELECT COUNT(*) FROM hosts WHERE state = 'offline') AS offline_hosts,
           (SELECT COUNT(*) FROM ha_events WHERE created_at > datetime('now', '-24 hours')) AS recent_events",
    )
    .fetch_one(pool)
    .await?;

    let events = crate::db::query_as::<_, HaEventRow>(
        "SELECT id, vm_id, host_id, action, message,
                strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
         FROM ha_events ORDER BY created_at DESC LIMIT 50",
    )
    .fetch_all(pool)
    .await?;

    Ok((status, events))
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct HaEventRow {
    pub id: Uuid,
    pub vm_id: Option<Uuid>,
    pub host_id: Option<Uuid>,
    pub action: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

/// Decide whether HA must BLOCK recovery of a VM because its failed host isn't
/// confirmed fenced. Fail-safe: block unless the host is fenced. The cluster's
/// `allow_unfenced` opt-out lets operators with non-shared storage / external
/// fencing recover anyway — but a VM's own `fence_required` (fence_on_failure)
/// overrides that opt-out and always demands a fence.
fn block_recovery_unfenced(host_fenced: bool, fence_required: bool, allow_unfenced: bool) -> bool {
    !host_fenced && (fence_required || !allow_unfenced)
}

#[cfg(test)]
mod ha_dest_tests {
    use super::{block_recovery_unfenced, pick_ha_dest};
    use std::collections::HashMap;
    use uuid::Uuid;

    #[test]
    fn fenced_host_never_blocks_recovery() {
        // Once the host is confirmed fenced, recovery proceeds regardless of policy.
        for &fr in &[true, false] {
            for &au in &[true, false] {
                assert!(!block_recovery_unfenced(true, fr, au));
            }
        }
    }

    #[test]
    fn unfenced_host_blocks_by_default() {
        // Safe default: an unfenced host blocks recovery for every VM.
        assert!(block_recovery_unfenced(false, false, false));
        assert!(block_recovery_unfenced(false, true, false));
    }

    #[test]
    fn allow_unfenced_opt_out_permits_recovery_but_fence_required_overrides() {
        // Operator opted into unfenced recovery (non-shared storage): a default VM
        // may recover unfenced...
        assert!(!block_recovery_unfenced(false, false, true));
        // ...but a VM that explicitly requires fencing still blocks.
        assert!(block_recovery_unfenced(false, true, true));
    }

    #[test]
    fn picks_first_host_that_fits() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        // a is least-loaded (first) but only 512 MiB free; b has 4096.
        let candidates = vec![(a, 512i64), (b, 4096i64)];
        let reserved = HashMap::new();
        // 2 GiB VM cannot fit on a, must land on b.
        assert_eq!(pick_ha_dest(&candidates, &reserved, 2048), Some(b));
        // 256 MiB VM fits on the least-loaded a.
        assert_eq!(pick_ha_dest(&candidates, &reserved, 256), Some(a));
    }

    #[test]
    fn respects_in_scan_reservation() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let candidates = vec![(a, 4096i64), (b, 4096i64)];
        // a already promised 3072 MiB earlier this scan → only 1024 left.
        let mut reserved = HashMap::new();
        reserved.insert(a, 3072i64);
        // A 2 GiB victim can't fit on a anymore; overflow to b.
        assert_eq!(pick_ha_dest(&candidates, &reserved, 2048), Some(b));
    }

    #[test]
    fn none_when_nothing_fits() {
        let a = Uuid::from_u128(1);
        let candidates = vec![(a, 1024i64)];
        assert_eq!(pick_ha_dest(&candidates, &HashMap::new(), 8192), None);
        // Empty candidate set (no online hosts) → None.
        assert_eq!(pick_ha_dest(&[], &HashMap::new(), 1), None);
    }
}
