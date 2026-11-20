// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::time::Duration;

use uuid::Uuid;

use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            if let Err(e) = tick(&state).await {
                tracing::warn!("vm schedule runner: {e:#}");
            }
        }
    });
}

async fn tick(state: &AppState) -> anyhow::Result<()> {
    let due: Vec<(Uuid, Uuid, String, i64, Option<i64>)> = crate::db::query_as(
        "SELECT id, vm_id, action, interval_minutes, retention
         FROM vm_schedules
         WHERE enabled = TRUE AND next_run_at <= datetime('now')
         LIMIT 50",
    )
    .fetch_all(&state.pool)
    .await?;

    for (sched_id, vm_id, action, _interval_minutes, retention) in due {
        let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_optional(&state.pool)
            .await?
            .flatten();

        let task_result = match action.as_str() {
            "start" | "shutdown" | "stop" => {
                enqueue_task(
                    state,
                    "vm.power",
                    serde_json::json!({
                        "vm_id": vm_id.to_string(),
                        "action": action,
                        "mode": "",
                    }),
                    Some("vm"),
                    Some(vm_id),
                    host_id,
                )
                .await
            }
            "snapshot" => {
                let snap_name = format!("sched-{}", chrono::Utc::now().format("%Y%m%d-%H%M"));
                let record_id = Uuid::new_v4();
                if let Err(e) = crate::db::query(
                    "INSERT INTO snapshot_records (id, vm_id, name, status) VALUES (?, ?, ?, 'pending')",
                )
                .bind(record_id)
                .bind(vm_id)
                .bind(&snap_name)
                .execute(&state.pool)
                .await
                {
                    tracing::warn!(schedule_id = %sched_id, vm_id = %vm_id, "failed to insert snapshot_record: {e:#}");
                    continue;
                }

                enqueue_task(
                    state,
                    "vm.snapshot",
                    serde_json::json!({
                        "vm_id": vm_id.to_string(),
                        "snapshot_id": record_id.to_string(),
                        "name": snap_name,
                        "description": "Scheduled snapshot",
                        "disk_only": true,
                        "quiesce": false,
                        "storage_mode": "",
                        "retention": retention,
                    }),
                    Some("vm"),
                    Some(vm_id),
                    host_id,
                )
                .await
            }
            _ => continue,
        };

        if let Err(e) = task_result {
            tracing::warn!(schedule_id = %sched_id, vm_id = %vm_id, action, "failed to enqueue scheduled task: {}", e.message);
            continue;
        }

        if let Err(e) = crate::db::query(
            // Advance from max(next_run_at, now): if the schedule fell far behind
            // (controller downtime), advancing from the stale next_run_at leaves
            // it in the past and it re-fires every tick until it catches up — a
            // backfill storm (e.g. a day idle at 5-min interval = ~288 runs).
            "UPDATE vm_schedules
             SET last_run_at = datetime('now'),
                 next_run_at = datetime(max(next_run_at, datetime('now')), '+' || interval_minutes || ' minutes')
             WHERE id = ?",
        )
        .bind(sched_id)
        .execute(&state.pool)
        .await
        {
            tracing::warn!(schedule_id = %sched_id, "failed to advance next_run_at: {e:#}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn snapshot_name_format_matches_expected_pattern() {
        let name = format!("sched-{}", chrono::Utc::now().format("%Y%m%d-%H%M"));
        // Must start with "sched-" followed by YYYYMMDD-HHMM (14 digits + dash)
        assert!(name.starts_with("sched-"), "name = {name}");
        let suffix = &name["sched-".len()..];
        assert_eq!(suffix.len(), 13, "expected YYYYMMDD-HHMM, got: {suffix}");
        assert!(
            suffix.chars().all(|c| c.is_ascii_digit() || c == '-'),
            "unexpected chars: {suffix}"
        );
    }
}
