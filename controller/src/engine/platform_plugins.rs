// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Platform plugin marketplace — catalog inventory and install stubs.

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

use crate::config::ControllerConfig;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct PluginRow {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub version: String,
    pub author: String,
    pub featured: bool,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MarketplaceOverview {
    pub plugins: Vec<PluginRow>,
    pub installed_count: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginInstallResult {
    pub slug: String,
    pub name: String,
    pub installed: bool,
    pub summary: String,
}

pub async fn marketplace_overview(
    pool: &DbPool,
    _cfg: &ControllerConfig,
) -> anyhow::Result<MarketplaceOverview> {
    let mut plugins: Vec<PluginRow> = crate::db::query_as(
        "SELECT id, slug, name, category, description, version, author, featured, installed
         FROM platform_plugins ORDER BY featured DESC, category, name",
    )
    .fetch_all(pool)
    .await?;

    // Native eBPF is installed when machina-bpfd answers on at least one host,
    // not when this table's install-stub says so.
    if let Some(p) = plugins.iter_mut().find(|p| p.slug == "native-bpf") {
        p.installed = crate::engine::bpf::fleet_status(pool).await.reachable;
    }

    let installed_count = plugins.iter().filter(|p| p.installed).count();
    let summary = format!(
        "{} plugin(s) · {} installed (stub — no dataplane apply)",
        plugins.len(),
        installed_count
    );

    Ok(MarketplaceOverview {
        plugins,
        installed_count,
        summary,
    })
}

pub async fn install_plugin(pool: &DbPool, slug: &str) -> anyhow::Result<PluginInstallResult> {
    let slug = slug.trim();
    if slug.is_empty() {
        anyhow::bail!("slug required");
    }

    let row: PluginRow = crate::db::query_as(
        "SELECT id, slug, name, category, description, version, author, featured, installed
         FROM platform_plugins WHERE slug = ?",
    )
    .bind(slug)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("plugin not found: {slug}"))?;

    crate::db::query("UPDATE platform_plugins SET installed = TRUE WHERE slug = ?")
        .bind(slug)
        .execute(pool)
        .await?;

    Ok(PluginInstallResult {
        slug: row.slug.clone(),
        name: row.name.clone(),
        installed: true,
        summary: format!(
            "Installed {} v{} — configure via daemon integrations (stub)",
            row.name, row.version
        ),
    })
}

pub async fn uninstall_plugin(
    pool: &DbPool,
    slug: &str,
) -> anyhow::Result<PluginInstallResult> {
    let slug = slug.trim();
    let row: PluginRow = crate::db::query_as(
        "SELECT id, slug, name, category, description, version, author, featured, installed
         FROM platform_plugins WHERE slug = ?",
    )
    .bind(slug)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("plugin not found: {slug}"))?;

    if row.slug == "zeus-firewall" {
        anyhow::bail!("zeus-firewall is a core platform plugin and cannot be uninstalled");
    }

    crate::db::query("UPDATE platform_plugins SET installed = FALSE WHERE slug = ?")
        .bind(slug)
        .execute(pool)
        .await?;

    Ok(PluginInstallResult {
        slug: row.slug.clone(),
        name: row.name.clone(),
        installed: false,
        summary: format!("Removed {} from installed list (stub)", row.slug),
    })
}

#[derive(Debug, Deserialize)]
pub struct PluginPublishRequest {
    pub slug: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub version: String,
    #[serde(default = "default_author")]
    pub author: String,
    #[serde(default)]
    pub featured: bool,
}

fn default_author() -> String {
    "Community".into()
}

pub async fn publish_plugin(
    pool: &DbPool,
    req: &PluginPublishRequest,
) -> anyhow::Result<PluginRow> {
    machina_spec::validate_name(&req.slug).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO platform_plugins (id, slug, name, category, description, version, author, featured, installed)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, FALSE)
         ON CONFLICT (slug) DO UPDATE SET
           name = EXCLUDED.name,
           category = EXCLUDED.category,
           description = EXCLUDED.description,
           version = EXCLUDED.version,
           author = EXCLUDED.author,
           featured = EXCLUDED.featured",
    )
    .bind(id)
    .bind(req.slug.trim())
    .bind(req.name.trim())
    .bind(req.category.trim())
    .bind(req.description.trim())
    .bind(req.version.trim())
    .bind(req.author.trim())
    .bind(req.featured)
    .execute(pool)
    .await?;

    crate::db::query_as(
        "SELECT id, slug, name, category, description, version, author, featured, installed
         FROM platform_plugins WHERE slug = ?",
    )
    .bind(req.slug.trim())
    .fetch_one(pool)
    .await
    .map_err(|e| e.into())
}
