// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Fleet Keychain secrets inventory rollup (Phase 44).

use chrono::{DateTime, Utc};
use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

use crate::engine::enterprise_security;

#[derive(Debug, Clone, Serialize)]
pub struct FleetKeychainEntry {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub status: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetKeychainOverview {
    pub summary: String,
    pub air_gap_bundles: usize,
    pub api_keys: usize,
    pub entries: Vec<FleetKeychainEntry>,
}

pub async fn overview(pool: &DbPool) -> anyhow::Result<FleetKeychainOverview> {
    let sec = enterprise_security::overview(pool).await?;
    let bundles = enterprise_security::list_air_gap_bundles(pool).await?;

    let api_rows: Vec<(Uuid, String, String, Option<DateTime<Utc>>)> = crate::db::query_as(
        "SELECT id, name, role, strftime('%Y-%m-%dT%H:%M:%SZ', last_used_at) AS last_used_at FROM api_keys ORDER BY created_at DESC LIMIT 24",
    )
    .fetch_all(pool)
    .await?;

    let mut entries = Vec::new();

    for b in &bundles {
        entries.push(FleetKeychainEntry {
            kind: "air_gap".into(),
            id: b.id.to_string(),
            name: b.name.clone(),
            status: "exported".into(),
            summary: format!(
                "{} · {} KB",
                b.checksum.chars().take(18).collect::<String>(),
                b.size_bytes / 1024
            ),
        });
    }

    for (id, name, role, last_used) in &api_rows {
        entries.push(FleetKeychainEntry {
            kind: "api_key".into(),
            id: id.to_string(),
            name: name.clone(),
            status: if last_used.is_some() {
                "active"
            } else {
                "unused"
            }
            .into(),
            summary: format!(
                "role {}{}",
                role,
                last_used
                    .map(|t| format!(" · last used {}", t.format("%Y-%m-%d")))
                    .unwrap_or_default()
            ),
        });
    }

    Ok(FleetKeychainOverview {
        summary: format!(
            "{} credential(s) · {} air-gap bundle(s) · {} API key(s)",
            entries.len(),
            sec.air_gap_bundles,
            api_rows.len()
        ),
        air_gap_bundles: sec.air_gap_bundles,
        api_keys: api_rows.len(),
        entries,
    })
}
