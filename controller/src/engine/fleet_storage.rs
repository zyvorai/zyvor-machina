// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Fleet Disk Utility rollup — pools + SMART (Phase 41).

use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

use crate::config::ControllerConfig;
use crate::engine::host_os;
use crate::engine::storage_tiers;

#[derive(Debug, Clone, Serialize)]
pub struct FleetStoragePoolItem {
    pub id: String,
    pub name: String,
    pub storage_class: String,
    pub used_gib: i64,
    pub capacity_gib: i64,
    pub used_pct: f64,
    pub tier_name: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetSmartDiskItem {
    pub host_id: String,
    pub hostname: String,
    pub device: String,
    pub passed: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetStorageOverview {
    pub summary: String,
    pub pool_count: i64,
    pub tier_count: usize,
    pub total_capacity_gib: i64,
    pub total_used_gib: i64,
    pub pools_over_85_pct: usize,
    pub smart_failure_count: usize,
    pub smart_hosts_affected: usize,
    pub smart_hosts_sampled: usize,
    pub pools: Vec<FleetStoragePoolItem>,
    pub smart_disks: Vec<FleetSmartDiskItem>,
}

pub async fn overview(
    pool: &DbPool,
    cfg: &ControllerConfig,
) -> anyhow::Result<FleetStorageOverview> {
    let tiers = storage_tiers::tiers_overview(pool)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("fleet storage tiers rollup: {e}");
            storage_tiers::TiersOverview {
                tiers: vec![],
                summary: "Storage tiers unavailable".into(),
            }
        });
    let tier_name = |tid: Option<Uuid>| -> Option<String> {
        tid.and_then(|id| {
            tiers
                .tiers
                .iter()
                .find(|t| t.id == id.to_string())
                .map(|t| t.name.clone())
        })
    };

    let rows: Vec<(Uuid, String, String, i64, i64, Option<Uuid>)> = match crate::db::query_as(
        "SELECT id, name, storage_class, used_gib, capacity_gib, tier_id FROM storage_pools ORDER BY name",
    )
    .fetch_all(pool)
    .await
    {
        Ok(r) => r,
        Err(_) => crate::db::query_as(
            "SELECT id, name, storage_class, used_gib, capacity_gib, NULL FROM storage_pools ORDER BY name",
        )
        .fetch_all(pool)
        .await?,
    };

    let mut total_capacity_gib = 0_i64;
    let mut total_used_gib = 0_i64;
    let mut pools_over_85_pct = 0usize;
    let mut pools = Vec::new();

    for (id, name, storage_class, used_gib, capacity_gib, tier_id) in rows {
        total_capacity_gib += capacity_gib;
        total_used_gib += used_gib;
        let used_pct = if capacity_gib > 0 {
            (used_gib as f64 / capacity_gib as f64) * 100.0
        } else {
            0.0
        };
        let status = if used_pct > 90.0 {
            pools_over_85_pct += 1;
            "critical"
        } else if used_pct > 85.0 {
            pools_over_85_pct += 1;
            "warn"
        } else {
            "ok"
        };
        pools.push(FleetStoragePoolItem {
            id: id.to_string(),
            name,
            storage_class,
            used_gib,
            capacity_gib,
            used_pct,
            tier_name: tier_name(tier_id),
            status: status.into(),
        });
    }

    let host_rows: Vec<(Uuid, String)> = crate::db::query_as(
        "SELECT id, hostname FROM hosts WHERE state = 'online' ORDER BY hostname LIMIT 12",
    )
    .fetch_all(pool)
    .await?;

    let mut smart_disks = Vec::new();
    let mut hosts_with_failures = std::collections::HashSet::new();
    let mut smart_hosts_sampled = 0usize;

    for (host_id, hostname) in host_rows {
        let Ok(obs) = host_os::linux_observability(pool, cfg, host_id).await else {
            continue;
        };
        let Some(arr) = obs.get("smart").and_then(|s| s.as_array()) else {
            continue;
        };
        smart_hosts_sampled += 1;
        for d in arr {
            // Fail closed: a missing/malformed "passed" field means we couldn't
            // determine the disk's SMART status, not that it's healthy. Defaulting
            // to `true` here hid disks with unreadable SMART data from the fleet
            // storage health rollup instead of surfacing them for investigation.
            let passed = d.get("passed").and_then(|v| v.as_bool()).unwrap_or(false);
            if passed {
                continue;
            }
            hosts_with_failures.insert(host_id);
            smart_disks.push(FleetSmartDiskItem {
                host_id: host_id.to_string(),
                hostname: hostname.clone(),
                device: d
                    .get("device")
                    .and_then(|v| v.as_str())
                    .unwrap_or("disk")
                    .into(),
                passed: false,
                summary: d
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("SMART failure")
                    .into(),
            });
        }
    }

    let smart_failure_count = smart_disks.len();
    let smart_hosts_affected = hosts_with_failures.len();

    Ok(FleetStorageOverview {
        summary: format!(
            "{} pool(s) · {} GiB / {} GiB used · {} tier(s) · {} SMART failure(s) on {} of {} sampled host(s)",
            pools.len(),
            total_used_gib,
            total_capacity_gib,
            tiers.tiers.len(),
            smart_failure_count,
            smart_hosts_affected,
            smart_hosts_sampled
        ),
        pool_count: pools.len() as i64,
        tier_count: tiers.tiers.len(),
        total_capacity_gib,
        total_used_gib,
        pools_over_85_pct,
        smart_failure_count,
        smart_hosts_affected,
        smart_hosts_sampled,
        pools,
        smart_disks,
    })
}
