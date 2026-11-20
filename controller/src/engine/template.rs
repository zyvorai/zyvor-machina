// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use crate::db::DbPool;
use uuid::Uuid;

pub fn parse_template_ref(template_ref: &str) -> (String, String) {
    if let Some((n, v)) = template_ref.split_once('@') {
        (n.to_string(), v.to_string())
    } else {
        (template_ref.to_string(), "1.0.0".into())
    }
}

pub async fn resolve_template_disk(
    pool: &DbPool,
    template_ref: &str,
) -> anyhow::Result<String> {
    let (name, version) = if let Some((n, v)) = template_ref.split_once('@') {
        (n.to_string(), Some(v.to_string()))
    } else {
        (template_ref.to_string(), None)
    };

    let disk: String = if let Some(ver) = version {
        crate::db::query_scalar("SELECT source_disk FROM templates WHERE name = ? AND version = ?")
            .bind(&name)
            .bind(&ver)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("template not found: {name}@{ver}"))?
    } else {
        crate::db::query_scalar(
            "SELECT source_disk FROM templates WHERE name = ? ORDER BY created_at DESC LIMIT 1",
        )
        .bind(&name)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("template not found: {name}"))?
    };
    Ok(disk)
}

/// Whether `project` may launch from an image: public images are open to everyone; a private one only to its owning
/// project and the projects it was shared with.
pub fn image_allows(visibility: &str, owner: &str, project: &str, shared_with: &[String]) -> bool {
    visibility != "private" || owner == project || shared_with.iter().any(|p| p == project)
}

/// `Some(reason)` when `project` may not launch from `template_ref` (`name` or `name@version`). An unknown image is not
/// this function's business: the normal "template not found" path reports it.
pub async fn image_access_error(
    pool: &DbPool,
    template_ref: &str,
    project: &str,
) -> anyhow::Result<Option<String>> {
    let row: Option<(Uuid, String, String)> = match template_ref.split_once('@') {
        Some((n, v)) => {
            crate::db::query_as("SELECT id, COALESCE(visibility, 'public'), COALESCE(project, '') FROM templates WHERE name = ? AND version = ?")
                .bind(n)
                .bind(v)
                .fetch_optional(pool)
                .await?
        }
        None => {
            crate::db::query_as("SELECT id, COALESCE(visibility, 'public'), COALESCE(project, '') FROM templates WHERE name = ? ORDER BY created_at DESC LIMIT 1")
                .bind(template_ref)
                .fetch_optional(pool)
                .await?
        }
    };
    let Some((id, visibility, owner)) = row else {
        return Ok(None);
    };
    if visibility != "private" {
        return Ok(None);
    }
    let shared: Vec<String> = crate::db::query_scalar("SELECT project FROM image_shares WHERE template_id = ?")
        .bind(id)
        .fetch_all(pool)
        .await?;
    Ok((!image_allows(&visibility, &owner, project, &shared)).then(|| {
        format!(
            "the image '{template_ref}' is private{}; ask its owner to share it with project '{project}'",
            if owner.is_empty() { String::new() } else { format!(" to project '{owner}'") }
        )
    }))
}

pub async fn resolve_template_firewall_profile(
    pool: &DbPool,
    template_ref: &str,
) -> anyhow::Result<Option<String>> {
    let (name, version) = if let Some((n, v)) = template_ref.split_once('@') {
        (n.to_string(), Some(v.to_string()))
    } else {
        (template_ref.to_string(), None)
    };
    let profile: Option<String> = if let Some(ver) = version {
        crate::db::query_scalar("SELECT firewall_profile FROM templates WHERE name = ? AND version = ?")
            .bind(&name)
            .bind(&ver)
            .fetch_optional(pool)
            .await?
    } else {
        crate::db::query_scalar(
            "SELECT firewall_profile FROM templates WHERE name = ? ORDER BY created_at DESC LIMIT 1",
        )
        .bind(&name)
        .fetch_optional(pool)
        .await?
    };
    Ok(profile)
}

pub async fn upsert_ha_policy(
    pool: &DbPool,
    vm_id: Uuid,
    enabled: bool,
    restart_attempts: i32,
    restart_priority: &str,
    fence_on_failure: bool,
    anti_affinity: bool,
) -> anyhow::Result<()> {
    let existing: Option<Uuid> = crate::db::query_scalar("SELECT id FROM ha_policies WHERE vm_id = ?")
        .bind(vm_id)
        .fetch_optional(pool)
        .await?;

    if let Some(id) = existing {
        crate::db::query(
            "UPDATE ha_policies SET enabled = ?, restart_attempts = ?, restart_priority = ?,
             fence_on_failure = ?, anti_affinity = ? WHERE id = ?",
        )
        .bind(enabled)
        .bind(restart_attempts)
        .bind(restart_priority)
        .bind(fence_on_failure)
        .bind(anti_affinity)
        .bind(id)
        .execute(pool)
        .await?;
    } else {
        crate::db::query(
            "INSERT INTO ha_policies (id, vm_id, enabled, restart_attempts, restart_priority, fence_on_failure, anti_affinity)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4())
        .bind(vm_id)
        .bind(enabled)
        .bind(restart_attempts)
        .bind(restart_priority)
        .bind(fence_on_failure)
        .bind(anti_affinity)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn get_ha_policy(pool: &DbPool, vm_id: Uuid) -> anyhow::Result<Option<HaPolicyRow>> {
    Ok(crate::db::query_as(
        "SELECT enabled, restart_attempts, restart_priority, fence_on_failure, anti_affinity
         FROM ha_policies WHERE vm_id = ?",
    )
    .bind(vm_id)
    .fetch_optional(pool)
    .await?)
}

/// Replace `{{ key }}` / `{{key}}` placeholders (Jinja-style subset).
pub fn apply_template_vars(
    input: &str,
    vars: &std::collections::HashMap<String, String>,
) -> String {
    let mut out = input.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{{ {k} }}}}"), v);
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    out
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct HaPolicyRow {
    pub enabled: bool,
    pub restart_attempts: i32,
    pub restart_priority: String,
    pub fence_on_failure: bool,
    pub anti_affinity: bool,
}

#[cfg(test)]
mod image_access_tests {
    use super::image_allows;

    #[test]
    fn public_is_open_private_is_owner_and_shares_only() {
        assert!(image_allows("public", "core", "anyone", &[]));
        assert!(image_allows("private", "core", "core", &[]));
        assert!(image_allows("private", "core", "lab", &["lab".into()]));
        assert!(!image_allows("private", "core", "lab", &["ops".into()]));
        assert!(!image_allows("private", "", "lab", &[]), "an ownerless private image is open to its shares only");
    }
}
