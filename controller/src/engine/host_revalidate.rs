// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Re-checks hosts whose last validation failed. A host that failed once (agent not up yet, a
//! token mismatch while it was being set up) otherwise keeps showing "failed" until someone
//! presses Validate, long after the cause is gone. Every `MACHINA_HOST_REVALIDATE_SECS` (default
//! 600, `0` = off) the leader queues a quiet `host.validate` for each failed host that has none
//! pending; the task writes nothing to the join log (`quiet`).

use std::time::Duration;

use uuid::Uuid;

use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

const DEFAULT_SECS: u64 = 600;

fn interval_secs() -> u64 {
    std::env::var("MACHINA_HOST_REVALIDATE_SECS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(DEFAULT_SECS)
}

pub fn spawn(state: AppState) {
    let secs = interval_secs();
    if secs == 0 {
        tracing::info!("failed-host re-validation off (MACHINA_HOST_REVALIDATE_SECS=0)");
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(secs));
        interval.tick().await; // the first tick fires at once; give the agents time to come up
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            if let Err(e) = tick(&state).await {
                tracing::warn!("host re-validation: {e:#}");
            }
        }
    });
}

/// Failed hosts with no validation already waiting or running.
async fn due(pool: &crate::db::DbPool) -> Result<Vec<Uuid>, sqlx::Error> {
    crate::db::query_scalar::<_, Uuid>(
        "SELECT h.id FROM hosts h
         WHERE h.validation_status = 'failed'
           AND NOT EXISTS (SELECT 1 FROM tasks t
                           WHERE t.operation = 'host.validate' AND t.host_id = h.id
                             AND t.status IN ('pending', 'running'))",
    )
    .fetch_all(pool)
    .await
}

pub(crate) async fn tick(state: &AppState) -> anyhow::Result<usize> {
    let ids = due(&state.pool).await?;
    let mut queued = 0;
    for id in ids {
        match enqueue_task(
            state,
            "host.validate",
            serde_json::json!({ "host_id": id.to_string(), "quiet": true }),
            Some("host"),
            Some(id),
            Some(id),
        )
        .await
        {
            Ok(_) => queued += 1,
            Err(e) => tracing::warn!(host_id = %id, "re-validation enqueue failed: {}", e.message),
        }
    }
    Ok(queued)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::test_support::{seed_host, test_state};

    async fn set_status(pool: &crate::db::DbPool, id: Uuid, status: &str) {
        crate::db::query("UPDATE hosts SET validation_status = ? WHERE id = ?")
            .bind(status)
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn only_failed_hosts_are_queued_and_only_once() {
        let (state, mut rx) = test_state().await;
        let failed = seed_host(&state.pool, Uuid::new_v4()).await;
        set_status(&state.pool, failed, "failed").await;
        let fine = Uuid::new_v4();
        crate::db::query("INSERT INTO hosts (id, hostname, state, validation_status) VALUES (?, 'h2', 'online', 'passed')")
            .bind(fine)
            .execute(&state.pool)
            .await
            .unwrap();

        assert_eq!(tick(&state).await.unwrap(), 1, "the failed host is queued");
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.operation, "host.validate");
        assert_eq!(msg.payload["host_id"], failed.to_string());
        assert_eq!(msg.payload["quiet"], true);

        assert_eq!(tick(&state).await.unwrap(), 0, "a pending validation is not queued twice");
    }

    #[tokio::test]
    async fn nothing_to_do_when_every_host_passed() {
        let (state, _rx) = test_state().await;
        let id = seed_host(&state.pool, Uuid::new_v4()).await;
        set_status(&state.pool, id, "passed").await;
        assert_eq!(tick(&state).await.unwrap(), 0);
    }
}
