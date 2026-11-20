// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Scheduled runbook triggers from catalog auto_trigger hints.

use crate::db::DbPool;

use crate::config::ControllerConfig;
use crate::state::AppState;

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(600));
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            if let Err(e) = tick_triggers(&state.pool, &state.config).await {
                tracing::warn!("runbook scheduler: {e:#}");
            }
        }
    });
}

async fn tick_triggers(pool: &DbPool, cfg: &ControllerConfig) -> anyhow::Result<()> {
    let rows: Vec<(String, Option<String>, Option<chrono::DateTime<chrono::Utc>>)> = crate::db::query_as(
        "SELECT incident, auto_trigger, last_triggered_at FROM ops_runbook_catalog WHERE enabled = true AND auto_trigger IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    for (incident, trigger, last) in rows {
        if let Some(last) = last {
            if (chrono::Utc::now() - last).num_minutes() < 60 {
                continue;
            }
        }
        let Some(trigger) = trigger else { continue };
        if !trigger_fired(pool, cfg, &trigger).await? {
            continue;
        }
        if let Err(e) = crate::engine::operations::execute_runbook(
            pool,
            &incident,
            "runbook-scheduler",
            &serde_json::json!({ "auto_trigger": trigger }),
        )
        .await
        {
            tracing::error!(incident = %incident, "runbook scheduler: auto-triggered runbook failed: {e:#}");
        }
        crate::db::query(
            "UPDATE ops_runbook_catalog SET last_triggered_at = datetime('now') WHERE incident = ?",
        )
        .bind(&incident)
        .execute(pool)
        .await?;
        tracing::info!("auto-triggered runbook {incident} ({trigger})");
    }
    Ok(())
}

async fn trigger_fired(
    pool: &DbPool,
    _cfg: &ControllerConfig,
    trigger: &str,
) -> anyhow::Result<bool> {
    if trigger == "host.state=offline" {
        let n: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM hosts WHERE state = 'offline'")
            .fetch_one(pool)
            .await?;
        return Ok(n > 0);
    }
    if trigger == "task.failed:backup" {
        let n: i64 = crate::db::query_scalar(
            "SELECT COUNT(*) FROM tasks WHERE status = 'failed' AND operation LIKE '%backup%' AND created_at > datetime('now', '-1 hours')",
        )
        .fetch_one(pool)
        .await?;
        return Ok(n > 0);
    }
    if trigger == "zeus.drift_detected" {
        let n: i64 = crate::db::query_scalar(
            "SELECT COUNT(*) FROM firewall_timeline WHERE kind = 'drift' AND created_at > datetime('now', '-24 hours')",
        )
        .fetch_one(pool)
        .await?;
        return Ok(n > 0);
    }
    if trigger == "storage.used_pct>85" {
        let n: i64 = crate::db::query_scalar(
            "SELECT COUNT(*) FROM storage_pools WHERE capacity_gib > 0 AND (used_gib * 1.0 / capacity_gib) > 0.85",
        )
        .fetch_one(pool)
        .await?;
        return Ok(n > 0);
    }
    Ok(false)
}
