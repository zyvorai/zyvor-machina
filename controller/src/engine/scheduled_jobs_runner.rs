// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//
// Day-2: general recurring-jobs runner. Fires enabled scheduled_jobs whose interval has
// elapsed, enqueuing a whitelisted, self-contained task operation. Whitelisting keeps this
// from becoming an arbitrary-task-injection surface; only ops that need no pre-inserted
// tracking row are allowed here (snapshot/backup keep their bespoke record-aware schedulers).

use uuid::Uuid;

use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

/// Operations a scheduled job may run. Extend deliberately — each must be safe to run on a
/// cadence with only a target host and an optional payload (no pre-created DB record).
pub const ALLOWED_OPERATIONS: &[&str] = &["host.inventory"];

pub fn is_allowed_operation(op: &str) -> bool {
    ALLOWED_OPERATIONS.contains(&op)
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            if let Err(e) = tick(&state).await {
                tracing::warn!("scheduled jobs runner: {e:#}");
            }
        }
    });
}

async fn tick(state: &AppState) -> anyhow::Result<()> {
    let rows: Vec<(Uuid, String, String, String, Option<Uuid>)> = crate::db::query_as(
        "SELECT id, name, operation, payload, target_host_id
         FROM scheduled_jobs
         WHERE enabled = TRUE
           AND (last_run_at IS NULL
                OR last_run_at < datetime('now', printf('-%d minutes', interval_minutes)))",
    )
    .fetch_all(&state.pool)
    .await?;

    for (job_id, name, operation, payload, target_host_id) in rows {
        if !is_allowed_operation(&operation) {
            tracing::warn!(
                "scheduled job '{name}' has non-whitelisted operation '{operation}' — skipping"
            );
            continue;
        }
        if let Err(e) = run_job(state, &operation, &payload, target_host_id).await {
            tracing::warn!("scheduled job '{name}' ({operation}): {e:#}");
        }
        // Advance last_run_at regardless so a persistently failing job doesn't hot-loop.
        crate::db::query("UPDATE scheduled_jobs SET last_run_at = datetime('now') WHERE id = ?")
            .bind(job_id)
            .execute(&state.pool)
            .await?;
    }
    Ok(())
}

async fn run_job(
    state: &AppState,
    operation: &str,
    payload: &str,
    target_host_id: Option<Uuid>,
) -> anyhow::Result<()> {
    let payload_json: serde_json::Value =
        serde_json::from_str(payload).unwrap_or_else(|_| serde_json::json!({}));

    // Resolve target hosts: a specific host, or all hosts for host-scoped ops.
    let host_ids: Vec<Uuid> = match target_host_id {
        Some(h) => vec![h],
        None => {
            crate::db::query_scalar("SELECT id FROM hosts ORDER BY hostname LIMIT 500")
                .fetch_all(&state.pool)
                .await?
        }
    };

    for host_id in host_ids {
        let mut p = payload_json.clone();
        if let Some(obj) = p.as_object_mut() {
            obj.insert("host_id".into(), serde_json::json!(host_id.to_string()));
        }
        enqueue_task(
            state,
            operation,
            p,
            Some("host"),
            Some(host_id),
            Some(host_id),
        )
        .await
        .map_err(|e| anyhow::anyhow!("enqueue {operation}: {}", e.message))?;
    }
    Ok(())
}
