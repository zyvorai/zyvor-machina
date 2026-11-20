// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Serialize)]
pub struct SiemFirewallExport {
    pub exported_at: String,
    pub event_count: usize,
    pub events: Vec<SiemEvent>,
}

#[derive(Debug, Serialize)]
pub struct SiemEvent {
    pub target_kind: String,
    pub target_id: String,
    pub kind: String,
    pub summary: String,
    pub actor: Option<String>,
    pub created_at: String,
    pub detail: serde_json::Value,
}

pub async fn export_timeline(pool: &DbPool, hours: i32) -> anyhow::Result<SiemFirewallExport> {
    // Clamp the caller-supplied window. `hours` is spliced into a SQLite
    // datetime modifier as `'-' || hours || ' hours'`; a negative value (e.g.
    // -5) produces the malformed modifier "--5 hours", which datetime()
    // silently resolves to NULL. That makes `created_at >= NULL` false for
    // every row, so the export "succeeds" with 0 events instead of erroring
    // or returning the intended window — a caller/typo could think a site
    // has no firewall activity when the query simply never matched anything.
    // Also cap the upper bound so a huge value can't be used to pull an
    // unbounded time range (LIMIT 5000 below still bounds row count, but not
    // how far back SQLite has to scan).
    let hours = hours.clamp(1, 24 * 365);
    let rows: Vec<(
        String,
        uuid::Uuid,
        String,
        String,
        Option<String>,
        chrono::DateTime<chrono::Utc>,
        serde_json::Value,
    )> = crate::db::query_as(
        "SELECT target_kind, target_id, kind, summary, actor, created_at, detail_json
             FROM firewall_timeline
             WHERE created_at >= datetime('now', '-' || ? || ' hours')
             ORDER BY created_at DESC
             LIMIT 5000",
    )
    .bind(hours)
    .fetch_all(pool)
    .await?;

    let events: Vec<SiemEvent> = rows
        .into_iter()
        .map(
            |(target_kind, target_id, kind, summary, actor, created_at, detail)| {
                let mut detail = detail;
                if target_kind == "bare_metal" {
                    if let Some(obj) = detail.as_object_mut() {
                        obj.entry("tag".to_string())
                            .or_insert(serde_json::json!("metal"));
                    } else {
                        detail = serde_json::json!({ "tag": "metal", "raw": detail });
                    }
                }
                SiemEvent {
                    target_kind,
                    target_id: target_id.to_string(),
                    kind,
                    summary,
                    actor,
                    created_at: created_at.to_rfc3339(),
                    detail,
                }
            },
        )
        .collect();
    let event_count = events.len();
    Ok(SiemFirewallExport {
        exported_at: chrono::Utc::now().to_rfc3339(),
        event_count,
        events,
    })
}
