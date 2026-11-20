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
            if let Err(e) = run_due(&state).await {
                tracing::warn!("maintenance scheduler: {e:#}");
            }
        }
    });
}

async fn run_due(state: &AppState) -> anyhow::Result<()> {
    let due: Vec<(Uuid, Uuid, String, bool)> = crate::db::query_as(
        "SELECT id, host_id, action, evacuate FROM maintenance_schedules
         WHERE status = 'pending' AND run_at <= datetime('now') LIMIT 100",
    )
    .fetch_all(&state.pool)
    .await?;

    for (id, host_id, action, evacuate) in due {
        match enqueue_task(
            state,
            "host.maintenance",
            serde_json::json!({
                "host_id": host_id.to_string(),
                "action": action,
                "evacuate": evacuate,
            }),
            Some("host"),
            Some(host_id),
            Some(host_id),
        )
        .await
        {
            Ok(_) => {
                crate::db::query("UPDATE maintenance_schedules SET status = 'queued' WHERE id = ?")
                    .bind(id)
                    .execute(&state.pool)
                    .await?;
            }
            Err(e) => {
                tracing::warn!(schedule_id = %id, "maintenance task enqueue failed, will retry next tick: {}", e.message);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::run_due;
    use crate::engine::test_support::{seed_host, test_state};
    use crate::state::AppState;
    use uuid::Uuid;

    async fn seed_schedule(state: &AppState, run_at_offset: &str) -> (Uuid, Uuid) {
        let host_id = seed_host(&state.pool, Uuid::from_u128(20)).await;
        let sched_id = Uuid::from_u128(21);
        crate::db::query(
            "INSERT INTO maintenance_schedules (id, host_id, action, evacuate, run_at, status)
             VALUES (?, ?, 'enter', TRUE, datetime('now', ?), 'pending')",
        )
        .bind(sched_id)
        .bind(host_id)
        .bind(run_at_offset)
        .execute(&state.pool)
        .await
        .unwrap();
        (sched_id, host_id)
    }

    async fn schedule_status(state: &AppState, id: Uuid) -> String {
        crate::db::query_scalar("SELECT status FROM maintenance_schedules WHERE id = ?")
            .bind(id)
            .fetch_one(&state.pool)
            .await
            .unwrap()
    }

    async fn maintenance_task_count(state: &AppState, host_id: Uuid) -> i64 {
        crate::db::query_scalar(
            "SELECT COUNT(*) FROM tasks WHERE resource_id = ? AND operation = 'host.maintenance'",
        )
        .bind(host_id)
        .fetch_one(&state.pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn due_schedule_enqueues_task_and_marks_queued() {
        let (state, _rx) = test_state().await;
        let (sched_id, host_id) = seed_schedule(&state, "-1 minute").await;

        run_due(&state).await.unwrap();
        assert_eq!(schedule_status(&state, sched_id).await, "queued");
        assert_eq!(maintenance_task_count(&state, host_id).await, 1);

        // Queued schedules are no longer 'pending' → a second tick must not
        // enqueue a duplicate maintenance task.
        run_due(&state).await.unwrap();
        assert_eq!(maintenance_task_count(&state, host_id).await, 1);
    }

    #[tokio::test]
    async fn future_schedule_is_left_pending() {
        let (state, _rx) = test_state().await;
        let (sched_id, host_id) = seed_schedule(&state, "+1 hour").await;

        run_due(&state).await.unwrap();
        assert_eq!(schedule_status(&state, sched_id).await, "pending");
        assert_eq!(maintenance_task_count(&state, host_id).await, 0);
    }

    #[tokio::test]
    async fn failed_enqueue_leaves_schedule_pending_for_retry() {
        // Regression (5fd9ac4c): a failed enqueue used to advance the schedule to
        // 'queued' anyway, permanently losing the maintenance action. Dropping the
        // bus receiver makes every publish — and thus enqueue_task — fail.
        let (state, rx) = test_state().await;
        drop(rx);
        let (sched_id, _host_id) = seed_schedule(&state, "-1 minute").await;

        run_due(&state).await.unwrap();
        assert_eq!(schedule_status(&state, sched_id).await, "pending");
    }
}
