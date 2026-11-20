// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Fleet network rollup — System Settings + Network Lens (Phase 40).

use serde::Serialize;
use crate::db::DbPool;

use crate::engine::network_overlay;

#[derive(Debug, Clone, Serialize)]
pub struct FleetNetworkSegment {
    pub id: String,
    pub name: String,
    pub tier: String,
    pub cidr: String,
    pub east_west_default: String,
    pub vm_count: usize,
    pub network_count: usize,
    pub micro_seg_grade: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetNetworkOverview {
    pub summary: String,
    pub network_count: i64,
    pub segment_count: usize,
    pub ipam_pool_count: i64,
    pub hosts_online: i64,
    pub deny_east_west_count: usize,
    pub segments: Vec<FleetNetworkSegment>,
}

pub async fn overview(pool: &DbPool) -> anyhow::Result<FleetNetworkOverview> {
    let segments = network_overlay::segments_overview(pool).await?;
    let network_count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM networks")
        .fetch_one(pool)
        .await?;
    let ipam_pool_count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM network_ipam_pools")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let hosts_online: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM hosts WHERE state = 'online'")
        .fetch_one(pool)
        .await?;

    let deny_east_west_count = segments
        .segments
        .iter()
        .filter(|s| s.east_west_default.eq_ignore_ascii_case("deny"))
        .count();

    let seg_items: Vec<FleetNetworkSegment> = segments
        .segments
        .iter()
        .map(|s| FleetNetworkSegment {
            id: s.id.clone(),
            name: s.name.clone(),
            tier: s.tier.clone(),
            cidr: s.cidr.clone(),
            east_west_default: s.east_west_default.clone(),
            vm_count: s.vm_count,
            network_count: s.network_count,
            micro_seg_grade: s.micro_seg_grade.clone(),
        })
        .collect();

    Ok(FleetNetworkOverview {
        summary: format!(
            "{} · {} network(s) · {} segment(s) · {} IPAM pool(s) · {} host(s) online",
            segments.summary,
            network_count,
            seg_items.len(),
            ipam_pool_count,
            hosts_online
        ),
        network_count,
        segment_count: seg_items.len(),
        ipam_pool_count,
        hosts_online,
        deny_east_west_count,
        segments: seg_items,
    })
}
