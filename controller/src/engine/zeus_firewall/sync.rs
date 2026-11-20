// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Auto-sync firewall posture during host.inventory.

use crate::db::DbPool;
use uuid::Uuid;

use crate::agent_client;
use crate::config::ControllerConfig;

use super::drift;

pub async fn sync_host_posture(
    pool: &DbPool,
    _cfg: &ControllerConfig,
    host_id: Uuid,
    agent_addr: &str,
) -> anyhow::Result<()> {
    let inv = match agent_client::get_firewall_inventory(agent_addr).await {
        Ok(inv) => inv,
        Err(e) => {
            tracing::debug!(%host_id, "firewall inventory sync skipped: {e:#}");
            return Ok(());
        }
    };

    // Older drift events carried only severity, so a volatile inventory field
    // produced hundreds of indistinguishable rows. Keep the newest legacy row
    // per host and remove only those pre-fingerprint duplicates. New events
    // include `actual` and retain their full history.
    let _ = crate::db::query(
        "DELETE FROM events
         WHERE kind = 'firewall.drift' AND resource_type = 'host' AND resource_id = ?
           AND CAST(payload AS TEXT) NOT LIKE '%\"actual\"%'
           AND id NOT IN (
             SELECT id FROM events
             WHERE kind = 'firewall.drift' AND resource_type = 'host' AND resource_id = ?
               AND CAST(payload AS TEXT) NOT LIKE '%\"actual\"%'
             ORDER BY created_at DESC LIMIT 1
           )",
    )
    .bind(host_id)
    .bind(host_id)
    .execute(pool)
    .await;

    let report = drift::detect_drift(pool, "host", host_id, &inv).await?;
    if report.drift_detected {
        let recent_detail: Option<serde_json::Value> = crate::db::query_scalar(
            "SELECT detail_json FROM firewall_timeline
             WHERE target_kind = 'host' AND target_id = ? AND kind = 'drift'
               AND created_at > datetime('now', '-1 hour')
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(host_id)
        .fetch_optional(pool)
        .await
        .unwrap_or(None);
        let duplicate = recent_detail
            .as_ref()
            .and_then(|v| v.get("actual"))
            .and_then(|v| v.as_str())
            == Some(report.actual.as_str());

        if !duplicate {
            let _ = crate::db::query(
                "INSERT INTO firewall_timeline (id, target_kind, target_id, kind, summary, detail_json, actor) VALUES (?, 'host', ?, 'drift', ?, ?, 'system')",
            )
            .bind(uuid::Uuid::new_v4())
            .bind(host_id)
            .bind(&report.summary)
            .bind(serde_json::json!({
                "expected": &report.expected,
                "actual": &report.actual,
            }))
            .execute(pool)
            .await;

            let hostname: String = crate::db::query_scalar("SELECT hostname FROM hosts WHERE id = ?")
                .bind(host_id)
                .fetch_one(pool)
                .await
                .unwrap_or_else(|_| host_id.to_string());
            let _ = crate::db::query(
                "INSERT INTO events (id, kind, message, resource_type, resource_id, payload) VALUES (?, 'firewall.drift', ?, 'host', ?, ?)",
            )
            .bind(uuid::Uuid::new_v4())
            .bind(format!("Firewall drift on {hostname}: {}", report.summary))
            .bind(host_id)
            .bind(serde_json::json!({
                "severity": "warning",
                "actual": &report.actual,
            }))
            .execute(pool)
            .await;
        }
    }

    drift::save_snapshot(pool, "host", host_id, &inv).await?;
    Ok(())
}
