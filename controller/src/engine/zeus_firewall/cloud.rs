// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use machina_core::gather_cloud_inventory;
use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct CloudOverview {
    pub inventory: machina_core::CloudFirewallInventory,
    pub last_snapshot_at: Option<String>,
}

pub async fn overview(pool: &DbPool) -> anyhow::Result<CloudOverview> {
    let inv = gather_cloud_inventory();
    let last: Option<(chrono::DateTime<chrono::Utc>,)> = crate::db::query_as(
        "SELECT captured_at FROM firewall_cloud_snapshots ORDER BY captured_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .unwrap_or(None);

    let _ = crate::db::query(
        "INSERT INTO firewall_cloud_snapshots (id, provider, summary, inventory_json)
         VALUES (?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4())
    .bind(inv.provider.as_str())
    .bind(&inv.summary)
    .bind(serde_json::to_value(&inv)?)
    .execute(pool)
    .await;

    Ok(CloudOverview {
        inventory: inv,
        last_snapshot_at: last.map(|(t,)| t.to_rfc3339()),
    })
}
