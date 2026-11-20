// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::path::Path;

use serde::Serialize;
use crate::db::DbPool;

use super::host_shell;
use super::template_catalog;

#[derive(Debug, Serialize)]
pub struct TemplateReadiness {
    pub disk_exists: bool,
    pub host_online: i64,
    pub cloud_init: bool,
    pub ready: bool,
    /// When true, controller can download the golden image on first VM create.
    pub auto_fetch: bool,
    pub remediation: String,
    pub source_disk: String,
}

#[derive(Debug, Serialize)]
pub struct MissingTemplateImage {
    pub name: String,
    pub version: String,
    pub source_disk: String,
    pub category: String,
    pub icon: Option<String>,
    pub auto_fetch: bool,
}

pub async fn check_template_readiness(
    pool: &DbPool,
    name: &str,
    version: &str,
) -> anyhow::Result<TemplateReadiness> {
    let row: Option<(String, bool)> = crate::db::query_as(
        "SELECT source_disk, cloud_init FROM templates WHERE name = ? AND version = ?",
    )
    .bind(name)
    .bind(version)
    .fetch_optional(pool)
    .await?;

    let (source_disk, cloud_init) = row.ok_or_else(|| anyhow::anyhow!("template not found"))?;

    let host_online: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM hosts WHERE state = 'online'")
        .fetch_one(pool)
        .await?;

    let disk_exists = disk_exists_on_hosts(pool, &source_disk).await;
    let auto_fetch = template_catalog::download_url_for(name, version).is_some();

    let (ready, remediation) = if host_online == 0 {
        (
            false,
            "No online hosts — enroll a hypervisor and wait for heartbeat.".into(),
        )
    } else if disk_exists {
        (true, "Ready to deploy.".into())
    } else if auto_fetch {
        (
            true,
            format!(
                "Golden image not on host yet — will download automatically on first create to {}.",
                source_disk
            ),
        )
    } else {
        (
            false,
            format!(
                "Disk image missing at {} — upload qcow2 via Content Library or copy to the host images path.",
                source_disk
            ),
        )
    };

    Ok(TemplateReadiness {
        disk_exists,
        host_online,
        cloud_init,
        ready,
        auto_fetch,
        remediation,
        source_disk,
    })
}

/// Marketplace templates whose golden disk is absent on all online hosts.
pub async fn list_missing_marketplace_images(
    pool: &DbPool,
) -> anyhow::Result<Vec<MissingTemplateImage>> {
    let rows: Vec<(String, String, String, String, Option<String>)> = crate::db::query_as(
        "SELECT name, version, source_disk, category, icon FROM templates WHERE marketplace = TRUE ORDER BY featured DESC, name LIMIT 200",
    )
    .fetch_all(pool)
    .await?;

    let mut seen_disks = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (name, version, source_disk, category, icon) in rows {
        if !seen_disks.insert(source_disk.clone()) {
            continue;
        }
        if disk_exists_on_hosts(pool, &source_disk).await {
            continue;
        }
        let auto_fetch = template_catalog::download_url_for(&name, &version).is_some();
        out.push(MissingTemplateImage {
            name,
            version,
            source_disk,
            category,
            icon,
            auto_fetch,
        });
    }
    Ok(out)
}

pub async fn disk_exists_at(path: &str) -> bool {
    Path::new(path).is_file()
}

pub async fn disk_exists_on_hosts(pool: &DbPool, path: &str) -> bool {
    if disk_exists_at(path).await {
        return true;
    }
    let hosts: Vec<String> = crate::db::query_scalar(
        "SELECT COALESCE(NULLIF(address, ''), hostname) FROM hosts WHERE state = 'online' ORDER BY hostname LIMIT 200",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for addr in hosts {
        if host_shell::remote_file_exists(&addr, path).await {
            return true;
        }
    }
    false
}
