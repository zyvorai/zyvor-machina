// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Backups nobody has ever read back are a hope, not a backup. The leader re-checks stored backups on a schedule:
//! the owning host's agent runs `qemu-img check` on every image (a multi-disk backup checks every disk) and the result
//! is recorded on the backup, shown in the UI, and written to the audit log when it fails. Atlas-backed backups
//! are verified by Atlas itself and are skipped here.

use crate::db::DbPool;
use uuid::Uuid;

use crate::agent_client;
use crate::state::AppState;
use crate::tasks::enqueue::write_audit;

const TICK_SECS: u64 = 30 * 60;
const BATCH: i64 = 5;

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(TICK_SECS));
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            if let Err(e) = tick(&state).await {
                tracing::warn!("backup verifier: {e:#}");
            }
        }
    });
}

fn recheck_days() -> i64 {
    std::env::var("MACHINA_BACKUP_VERIFY_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|d| *d >= 1)
        .unwrap_or(7)
}

/// Completed local backups never verified, or last verified more than `recheck_days` ago. Oldest check first.
async fn due(pool: &DbPool) -> anyhow::Result<Vec<Uuid>> {
    Ok(crate::db::query_scalar(
        "SELECT id FROM backup_records
         WHERE status = 'completed' AND backup_path != '' AND backup_path NOT LIKE 'atlas%'
           AND (verified_at IS NULL OR verified_at < datetime('now', printf('-%d days', ?)))
         ORDER BY COALESCE(verified_at, created_at) LIMIT ?",
    )
    .bind(recheck_days())
    .bind(BATCH)
    .fetch_all(pool)
    .await?)
}

async fn tick(state: &AppState) -> anyhow::Result<()> {
    for id in due(&state.pool).await? {
        if let Err(e) = verify_one(state, id, "backup-verifier").await {
            tracing::warn!("backup verifier: {id}: {e:#}");
        }
    }
    Ok(())
}

/// Verify one backup now and record the outcome. Returns (ok, message).
pub async fn verify_one(
    state: &AppState,
    backup_id: Uuid,
    actor: &str,
) -> anyhow::Result<(bool, String)> {
    let row: Option<(String, Option<Uuid>, Uuid, String)> = crate::db::query_as(
        "SELECT COALESCE(br.backup_path, ''), v.host_id, br.vm_id, v.name
         FROM backup_records br JOIN vms v ON v.id = br.vm_id
         WHERE br.id = ? AND br.status = 'completed'",
    )
    .bind(backup_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some((path, host_id, vm_id, vm_name)) = row else {
        anyhow::bail!("backup not found or not completed");
    };
    let Some(host_id) = host_id else {
        anyhow::bail!("the machine has no host");
    };
    let addr: String = crate::db::query_scalar("SELECT agent_grpc_addr FROM hosts WHERE id = ?")
        .bind(host_id)
        .fetch_one(&state.pool)
        .await?;
    let (ok, message) = match agent_client::connect(&addr).await {
        Ok(mut c) => match agent_client::verify_backup(&mut c, &path).await {
            Ok(r) => (r.ok, r.message),
            Err(e) => (false, format!("could not reach the host agent: {e}")),
        },
        Err(e) => (false, format!("could not reach the host agent: {e}")),
    };
    // An unreachable agent says nothing about the backup, so do not mark it failed — leave it due for next time.
    let unreachable = message.starts_with("could not reach the host agent");
    if !unreachable {
        crate::db::query(
            "UPDATE backup_records SET verified_at = datetime('now'), verify_status = ?, verify_message = ? WHERE id = ?",
        )
        .bind(if ok { "ok" } else { "failed" })
        .bind(&message)
        .bind(backup_id)
        .execute(&state.pool)
        .await?;
        if !ok {
            let _ = write_audit(
                state,
                actor,
                "backup.verify_failed",
                "vm",
                Some(vm_id),
                serde_json::json!({ "vm": vm_name, "backup_id": backup_id.to_string(), "detail": message }),
            )
            .await;
        }
    }
    Ok((ok, message))
}
