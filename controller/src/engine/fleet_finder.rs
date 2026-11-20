// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Finder smart folders + tag index (Phase 39).

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Clone, Serialize)]
pub struct SmartFolder {
    pub id: String,
    pub label: String,
    pub count: i64,
    pub icon: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagFolder {
    pub tag: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectFolder {
    pub project: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetFinderOverview {
    pub summary: String,
    pub smart_folders: Vec<SmartFolder>,
    pub tags: Vec<TagFolder>,
    pub projects: Vec<ProjectFolder>,
}

pub async fn overview(pool: &DbPool) -> anyhow::Result<FleetFinderOverview> {
    let all: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM vms")
        .fetch_one(pool)
        .await?;
    let running: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM vms WHERE observed_state = 'running'")
            .fetch_one(pool)
            .await?;
    let stopped: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM vms WHERE observed_state IN ('shutoff', 'stopped')",
    )
    .fetch_one(pool)
    .await?;
    let missing: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM vms WHERE observed_state = 'missing'")
            .fetch_one(pool)
            .await?;
    let discovered: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM vms WHERE managed = FALSE")
        .fetch_one(pool)
        .await?;
    let untagged: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM vms WHERE tags IS NULL OR tags = '[]' OR tags = ''",
    )
    .fetch_one(pool)
    .await?;
    let high_cpu: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM vms v JOIN vm_metrics m ON m.vm_id = v.id WHERE m.cpu_percent > 85",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let unprotected: i64 = crate::db::query_scalar(
        r#"
        SELECT COUNT(*) FROM vms v
        WHERE NOT EXISTS (
            SELECT 1 FROM backup_records b
            WHERE b.vm_id = v.id AND b.status = 'completed'
              AND b.created_at > datetime('now', '-7 days')
        )
        "#,
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let ha_enabled: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM vms v JOIN ha_policies hp ON hp.vm_id = v.id WHERE hp.enabled = TRUE",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let no_ip: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM vms WHERE guest_ip IS NULL OR guest_ip = ''")
            .fetch_one(pool)
            .await
            .unwrap_or(0);
    let guest_agent_missing: i64 = crate::db::query_scalar(
        r#"
        SELECT COUNT(*) FROM vms
        WHERE COALESCE(inventory_source, 'libvirt') != 'kubevirt'
          AND (guest_tools_status IS NULL
               OR guest_tools_status NOT IN ('healthy', 'installed'))
        "#,
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let migration_ready: i64 = crate::db::query_scalar(
        r#"
        SELECT COUNT(*) FROM vms
        WHERE observed_state NOT IN ('running', 'missing')
          AND COALESCE(managed, TRUE) = TRUE
        "#,
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let needs_attention: i64 = crate::db::query_scalar(
        r#"
        SELECT COUNT(DISTINCT v.id) FROM vms v
        LEFT JOIN vm_metrics m ON m.vm_id = v.id
        WHERE NOT EXISTS (
            SELECT 1 FROM backup_records b
            WHERE b.vm_id = v.id AND b.status = 'completed'
              AND b.created_at > datetime('now', '-7 days')
        )
        OR (v.guest_ip IS NULL OR v.guest_ip = '')
        OR (COALESCE(v.inventory_source, 'libvirt') != 'kubevirt'
            AND (v.guest_tools_status IS NULL
                 OR v.guest_tools_status NOT IN ('healthy', 'installed')))
        OR (m.cpu_percent > 85)
        "#,
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let libvirt_src: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM vms WHERE COALESCE(inventory_source, 'libvirt') = 'libvirt'",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let kubevirt_src: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM vms WHERE inventory_source = 'kubevirt'")
            .fetch_one(pool)
            .await
            .unwrap_or(0);
    let vmware_src: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM vms WHERE inventory_source IN ('vmware', 'vsphere')",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let tag_rows: Vec<(String, i64)> = crate::db::query_as(
        "SELECT j.value AS tag, COUNT(*) FROM vms, json_each(COALESCE(tags,'[]')) j
         WHERE tags IS NOT NULL AND tags != '[]' AND tags != ''
         GROUP BY j.value
         ORDER BY COUNT(*) DESC, j.value
         LIMIT 40",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let project_rows: Vec<(String, i64)> = crate::db::query_as(
        r#"
        SELECT project, COUNT(*) FROM vms
        WHERE project IS NOT NULL AND project != ''
        GROUP BY project
        ORDER BY COUNT(*) DESC, project
        LIMIT 20
        "#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let smart_folders = vec![
        SmartFolder {
            id: "all".into(),
            label: "All Machines".into(),
            count: all,
            icon: "all".into(),
        },
        SmartFolder {
            id: "running".into(),
            label: "Running".into(),
            count: running,
            icon: "running".into(),
        },
        SmartFolder {
            id: "stopped".into(),
            label: "Stopped".into(),
            count: stopped,
            icon: "stopped".into(),
        },
        SmartFolder {
            id: "needs_attention".into(),
            label: "Needs Attention".into(),
            count: needs_attention,
            icon: "attention".into(),
        },
        SmartFolder {
            id: "unprotected".into(),
            label: "No Backup".into(),
            count: unprotected,
            icon: "backup".into(),
        },
        SmartFolder {
            id: "no_ip".into(),
            label: "No IP".into(),
            count: no_ip,
            icon: "network".into(),
        },
        SmartFolder {
            id: "guest_agent_missing".into(),
            label: "Guest Agent Missing".into(),
            count: guest_agent_missing,
            icon: "agent".into(),
        },
        SmartFolder {
            id: "migration_ready".into(),
            label: "Migration Ready".into(),
            count: migration_ready,
            icon: "migrate".into(),
        },
        SmartFolder {
            id: "discovered".into(),
            label: "Discovered".into(),
            count: discovered,
            icon: "discovered".into(),
        },
        SmartFolder {
            id: "missing".into(),
            label: "Missing".into(),
            count: missing,
            icon: "missing".into(),
        },
        SmartFolder {
            id: "untagged".into(),
            label: "Untagged".into(),
            count: untagged,
            icon: "untagged".into(),
        },
        SmartFolder {
            id: "high_cpu".into(),
            label: "High CPU".into(),
            count: high_cpu,
            icon: "cpu".into(),
        },
        SmartFolder {
            id: "ha_enabled".into(),
            label: "HA enabled".into(),
            count: ha_enabled,
            icon: "ha".into(),
        },
    ];

    let tags: Vec<TagFolder> = tag_rows
        .into_iter()
        .map(|(tag, count)| TagFolder { tag, count })
        .collect();

    let projects: Vec<ProjectFolder> = project_rows
        .into_iter()
        .map(|(project, count)| ProjectFolder { project, count })
        .collect();

    Ok(FleetFinderOverview {
        summary: format!(
            "{all} machines · {running} running · {stopped} stopped · {unprotected} need backup · libvirt {libvirt_src} · kubevirt {kubevirt_src} · vmware {vmware_src}",
        ),
        smart_folders,
        tags,
        projects,
    })
}
