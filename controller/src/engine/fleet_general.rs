// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Fleet System Settings / General rollup (Phase 48).

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Clone, Serialize)]
pub struct FleetGeneralWallpaperOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetGeneralDockOption {
    pub path: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetGeneralOverview {
    pub summary: String,
    pub cluster_name: String,
    pub controller_version: String,
    pub hosts_online: i64,
    pub hosts_total: i64,
    pub vm_count: i64,
    pub active_tasks: i64,
    pub wallpaper_options: Vec<FleetGeneralWallpaperOption>,
    pub dock_defaults: Vec<FleetGeneralDockOption>,
    pub settings_url: String,
}

pub async fn overview(pool: &DbPool) -> anyhow::Result<FleetGeneralOverview> {
    let cluster_name: String = crate::db::query_scalar(
        "SELECT COALESCE(NULLIF(name, ''), 'machina') FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_optional(pool)
    .await?
    .unwrap_or_else(|| "machina".into());

    let hosts_total: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM hosts")
        .fetch_one(pool)
        .await?;
    let hosts_online: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM hosts WHERE state = 'online'")
        .fetch_one(pool)
        .await?;
    let vm_count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM vms")
        .fetch_one(pool)
        .await?;
    let active_tasks: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM tasks WHERE status IN ('pending', 'running')")
            .fetch_one(pool)
            .await?;

    let wallpaper_options = vec![
        FleetGeneralWallpaperOption {
            id: "tahoe".into(),
            label: "Tahoe".into(),
        },
        FleetGeneralWallpaperOption {
            id: "aurora".into(),
            label: "Aurora".into(),
        },
        FleetGeneralWallpaperOption {
            id: "midnight".into(),
            label: "Midnight".into(),
        },
        FleetGeneralWallpaperOption {
            id: "ocean".into(),
            label: "Ocean".into(),
        },
    ];

    let dock_defaults = vec![
        FleetGeneralDockOption {
            path: "/platform".into(),
            label: "Dashboard".into(),
        },
        FleetGeneralDockOption {
            path: "/platform/vms".into(),
            label: "Finder".into(),
        },
        FleetGeneralDockOption {
            path: "/platform/hosts".into(),
            label: "Hosts".into(),
        },
        FleetGeneralDockOption {
            path: "/platform/activity".into(),
            label: "Activity Monitor".into(),
        },
        FleetGeneralDockOption {
            path: "/platform/backups".into(),
            label: "Time Machine".into(),
        },
        FleetGeneralDockOption {
            path: "/platform/blueprints".into(),
            label: "Shortcuts".into(),
        },
        FleetGeneralDockOption {
            path: "/platform/projects".into(),
            label: "Stage Manager".into(),
        },
        FleetGeneralDockOption {
            path: "/platform/settings".into(),
            label: "Settings".into(),
        },
    ];

    let summary = format!(
        "{cluster_name} · {hosts_online}/{hosts_total} hosts online · {vm_count} VM(s) · {active_tasks} active task(s)"
    );

    Ok(FleetGeneralOverview {
        summary,
        cluster_name,
        controller_version: env!("CARGO_PKG_VERSION").into(),
        hosts_online,
        hosts_total,
        vm_count,
        active_tasks,
        wallpaper_options,
        dock_defaults,
        settings_url: "/platform/settings?section=general".into(),
    })
}
