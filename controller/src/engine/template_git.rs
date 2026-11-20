// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Optional git-backed template sync from `MACHINA_TEMPLATES_GIT_DIR` (`*.json` manifests).

use std::path::Path;

use serde::Deserialize;
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct GitTemplateManifest {
    name: String,
    version: String,
    source_disk: String,
    #[serde(default)]
    cloud_init: bool,
    #[serde(default)]
    os_family: Option<String>,
    #[serde(default = "default_category")]
    category: String,
    #[serde(default)]
    workload: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    git_ref: String,
}

fn default_category() -> String {
    "Linux".into()
}

/// Scan `dir/*.json` and upsert into `templates` with `git_ref` set.
pub async fn sync_templates_from_git(pool: &DbPool, dir: &Path) -> anyhow::Result<usize> {
    let dir = dir.to_path_buf();
    // File scanning is blocking — run it off the async executor.
    let manifests: Vec<(String, GitTemplateManifest)> = tokio::task::spawn_blocking(move || {
        if !dir.is_dir() {
            anyhow::bail!("templates git dir not found: {}", dir.display());
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let raw = std::fs::read_to_string(&path)?;
            let m: GitTemplateManifest = serde_json::from_str(&raw)?;
            let git_ref = if m.git_ref.is_empty() {
                path.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string()
            } else {
                m.git_ref.clone()
            };
            out.push((git_ref, m));
        }
        Ok(out)
    })
    .await??;

    let mut synced = 0usize;
    for (git_ref, m) in manifests {
        crate::db::query(
            "INSERT INTO templates (id, name, version, source_disk, cloud_init, os_family, category, workload, description, featured, marketplace, git_ref, approval_status)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, FALSE, TRUE, ?, 'approved')
             ON CONFLICT (name, version) DO UPDATE SET
               source_disk = EXCLUDED.source_disk,
               workload = EXCLUDED.workload,
               description = EXCLUDED.description,
               git_ref = EXCLUDED.git_ref",
        )
        .bind(Uuid::new_v4())
        .bind(&m.name)
        .bind(&m.version)
        .bind(&m.source_disk)
        .bind(m.cloud_init)
        .bind(&m.os_family)
        .bind(&m.category)
        .bind(&m.workload)
        .bind(&m.description)
        .bind(&git_ref)
        .execute(pool)
        .await?;
        synced += 1;
    }
    Ok(synced)
}
