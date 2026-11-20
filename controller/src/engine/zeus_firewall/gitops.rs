// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct GitOpsExport {
    pub api_version: String,
    pub kind: String,
    pub policies: Vec<GitOpsPolicyDoc>,
    pub exported_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitOpsPolicyDoc {
    pub name: String,
    pub spec_yaml: String,
}

#[derive(Debug, Deserialize)]
pub struct GitOpsSyncRequest {
    pub policies: Vec<GitOpsPolicyDoc>,
    #[serde(default)]
    pub replace: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitOpsSyncResult {
    pub upserted: usize,
    pub removed: usize,
    pub sync_id: Uuid,
    /// Policy names that collided with an existing `source = 'manual'` row and
    /// were left untouched — overwriting them would silently reclassify a
    /// manually-created policy as git-managed, which the next `replace` sync
    /// could then delete outright the moment git drops that name. Rename the
    /// manual policy or the git one to resolve.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub manual_conflicts: Vec<String>,
}

pub async fn export_policies(pool: &DbPool) -> anyhow::Result<GitOpsExport> {
    let rows: Vec<(String, String)> =
        crate::db::query_as("SELECT name, spec_yaml FROM firewall_policies ORDER BY name")
            .fetch_all(pool)
            .await?;

    Ok(GitOpsExport {
        api_version: "zeus.machina/v1".into(),
        kind: "MachineFirewallPolicyBundle".into(),
        policies: rows
            .into_iter()
            .map(|(name, spec_yaml)| GitOpsPolicyDoc { name, spec_yaml })
            .collect(),
        exported_at: chrono::Utc::now().to_rfc3339(),
    })
}

pub async fn sync_policies(
    pool: &DbPool,
    req: GitOpsSyncRequest,
    actor: &str,
) -> anyhow::Result<GitOpsSyncResult> {
    let mut tx = pool.begin().await?;
    let mut upserted = 0usize;
    let mut removed = 0usize;
    let mut manual_conflicts = Vec::new();

    // Scoped replace: `replace=true` prunes only policies this gitops sync itself
    // previously created/owns (source = 'git'), never rows an admin added by hand
    // through the UI/API (source = 'manual'). A full unscoped
    // `DELETE FROM firewall_policies WHERE name NOT IN (...)` would wipe
    // manually-added rules whenever the git source is incomplete or stale
    // relative to them — this keeps the blast radius to git-managed rows only.
    if req.replace {
        let names: Vec<String> = req.policies.iter().map(|p| p.name.clone()).collect();
        if names.is_empty() {
            let r = crate::db::query("DELETE FROM firewall_policies WHERE source = 'git'")
                .execute(&mut *tx)
                .await?;
            removed = r.rows_affected() as usize;
        } else {
            let names_json = serde_json::to_string(&names).unwrap_or_default();
            let r = crate::db::query(
                "DELETE FROM firewall_policies WHERE source = 'git' \
                 AND name NOT IN (SELECT value FROM json_each(?))",
            )
            .bind(&names_json)
            .execute(&mut *tx)
            .await?;
            removed = r.rows_affected() as usize;
        }
    }

    for policy in &req.policies {
        let existing: Option<(Uuid, String)> =
            crate::db::query_as("SELECT id, source FROM firewall_policies WHERE name = ?")
                .bind(&policy.name)
                .fetch_optional(&mut *tx)
                .await?;

        if let Some((id, source)) = existing {
            // A gitops sync claims/overwrites a policy of this name — but only
            // when it already owns that name (source = 'git') or the row
            // predates provenance tracking. A name collision with a
            // `source = 'manual'` row must NOT be silently overwritten: doing
            // so would reclassify a manually-created policy as git-managed,
            // and the next `replace` sync could then delete it outright the
            // moment git drops that name — the exact accidental-wipe class
            // the scoped `replace` above exists to prevent, just reached via
            // a name collision instead of directly.
            if source == "manual" {
                manual_conflicts.push(policy.name.clone());
                continue;
            }
            crate::db::query(
                "UPDATE firewall_policies SET spec_yaml = ?, source = 'git', updated_at = datetime('now') WHERE id = ?",
            )
            .bind(&policy.spec_yaml)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        } else {
            crate::db::query(
                "INSERT INTO firewall_policies (id, name, spec_yaml, source) VALUES (?, ?, ?, 'git')",
            )
            .bind(Uuid::new_v4())
            .bind(&policy.name)
            .bind(&policy.spec_yaml)
            .execute(&mut *tx)
            .await?;
        }
        upserted += 1;
    }

    let sync_id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO firewall_policy_sync_log (id, direction, policy_count, actor, detail_json)
         VALUES (?, 'import', ?, ?, ?)",
    )
    .bind(sync_id)
    .bind(upserted as i64)
    .bind(actor)
    .bind(serde_json::json!({
        "replace": req.replace,
        "removed": removed,
        "manual_conflicts": manual_conflicts,
    }))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(GitOpsSyncResult {
        upserted,
        removed,
        sync_id,
        manual_conflicts,
    })
}
