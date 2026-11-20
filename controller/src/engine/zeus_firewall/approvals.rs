// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

use crate::config::ControllerConfig;

use super::inventory::apply_profile;

#[derive(Debug, Clone, Serialize)]
pub struct FirewallApproval {
    pub id: Uuid,
    pub target_kind: String,
    pub target_id: Uuid,
    pub profile: Option<String>,
    pub plan_json: serde_json::Value,
    pub status: String,
    pub requested_by: String,
    pub reviewed_by: Option<String>,
    pub review_note: Option<String>,
    pub created_at: String,
    pub reviewed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApprovalRequest {
    pub target_id: String,
    pub profile: Option<String>,
    pub plan_json: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ReviewBody {
    pub note: Option<String>,
}

pub async fn request_approval(
    pool: &DbPool,
    body: ApprovalRequest,
    actor: &str,
) -> anyhow::Result<FirewallApproval> {
    let target_id = Uuid::parse_str(&body.target_id)?;
    let id = Uuid::new_v4();
    let plan_json = body
        .plan_json
        .unwrap_or_else(|| serde_json::json!({ "profile": body.profile, "dry_run": true }));

    crate::db::query(
        "INSERT INTO firewall_approvals (id, target_kind, target_id, profile, plan_json, requested_by)
         VALUES (?, 'host', ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(target_id)
    .bind(&body.profile)
    .bind(&plan_json)
    .bind(actor)
    .execute(pool)
    .await?;

    let _ = crate::db::query(
        "INSERT INTO events (id, kind, message, resource_type, resource_id, payload) VALUES (?, 'approval', ?, 'zeus_firewall', ?, '{\"severity\":\"warning\"}')",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(format!(
        "Firewall change approval requested by {actor} for profile {:?}",
        body.profile
    ))
    .bind(target_id)
    .execute(pool)
    .await;

    get_approval(pool, id).await
}

pub async fn list_approvals(
    pool: &DbPool,
    status: Option<&str>,
) -> anyhow::Result<Vec<FirewallApproval>> {
    let rows: Vec<(
        Uuid,
        String,
        Uuid,
        Option<String>,
        serde_json::Value,
        String,
        String,
        Option<String>,
        Option<String>,
        chrono::DateTime<chrono::Utc>,
        Option<chrono::DateTime<chrono::Utc>>,
    )> = if let Some(st) = status {
        crate::db::query_as(
            "SELECT id, target_kind, target_id, profile, plan_json, status, requested_by,
                    reviewed_by, review_note, created_at, reviewed_at
             FROM firewall_approvals WHERE status = ? ORDER BY created_at DESC LIMIT 100",
        )
        .bind(st)
        .fetch_all(pool)
        .await?
    } else {
        crate::db::query_as(
            "SELECT id, target_kind, target_id, profile, plan_json, status, requested_by,
                    reviewed_by, review_note, created_at, reviewed_at
             FROM firewall_approvals ORDER BY created_at DESC LIMIT 100",
        )
        .fetch_all(pool)
        .await?
    };

    Ok(rows.into_iter().map(map_row).collect())
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalApplyResult {
    pub approval: FirewallApproval,
    pub applied: bool,
    pub operations: usize,
    pub message: String,
}

pub async fn approve_and_apply(
    pool: &DbPool,
    cfg: &ControllerConfig,
    approval_id: Uuid,
    reviewer: &str,
    note: Option<&str>,
) -> anyhow::Result<ApprovalApplyResult> {
    let pending = get_approval(pool, approval_id).await?;
    if pending.status != "pending" {
        anyhow::bail!("approval not pending");
    }
    let target_id = pending.target_id.to_string();
    let profile = pending.profile.clone();
    let approval = review(pool, approval_id, reviewer, "approved", note).await?;

    let mut applied = false;
    let mut operations = 0usize;
    let mut message = "Approved without firewall apply (no profile)".into();

    if let Some(ref prof) = profile {
        match apply_profile(pool, cfg, &target_id, prof, reviewer, false).await {
            Ok(result) => {
                applied = true;
                operations = result.operations.len();
                message = format!("Approved and applied {prof} ({operations} operation(s))");
            }
            Err(e) => {
                message = format!("Approved but apply failed: {e}");
            }
        }
    }

    Ok(ApprovalApplyResult {
        approval,
        applied,
        operations,
        message,
    })
}

pub async fn approve(
    pool: &DbPool,
    approval_id: Uuid,
    reviewer: &str,
    note: Option<&str>,
) -> anyhow::Result<FirewallApproval> {
    review(pool, approval_id, reviewer, "approved", note).await
}

pub async fn reject(
    pool: &DbPool,
    approval_id: Uuid,
    reviewer: &str,
    note: Option<&str>,
) -> anyhow::Result<FirewallApproval> {
    review(pool, approval_id, reviewer, "rejected", note).await
}

async fn review(
    pool: &DbPool,
    approval_id: Uuid,
    reviewer: &str,
    status: &str,
    note: Option<&str>,
) -> anyhow::Result<FirewallApproval> {
    let updated = crate::db::query(
        "UPDATE firewall_approvals SET status = ?, reviewed_by = ?, review_note = ?, reviewed_at = datetime('now')
         WHERE id = ? AND status = 'pending'",
    )
    .bind(status)
    .bind(reviewer)
    .bind(note)
    .bind(approval_id)
    .execute(pool)
    .await?;

    if updated.rows_affected() == 0 {
        anyhow::bail!("approval not found or already reviewed");
    }

    get_approval(pool, approval_id).await
}

async fn get_approval(pool: &DbPool, id: Uuid) -> anyhow::Result<FirewallApproval> {
    let row: (
        Uuid,
        String,
        Uuid,
        Option<String>,
        serde_json::Value,
        String,
        String,
        Option<String>,
        Option<String>,
        chrono::DateTime<chrono::Utc>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = crate::db::query_as(
        "SELECT id, target_kind, target_id, profile, plan_json, status, requested_by,
                reviewed_by, review_note, created_at, reviewed_at
         FROM firewall_approvals WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;

    Ok(map_row(row))
}

fn map_row(
    row: (
        Uuid,
        String,
        Uuid,
        Option<String>,
        serde_json::Value,
        String,
        String,
        Option<String>,
        Option<String>,
        chrono::DateTime<chrono::Utc>,
        Option<chrono::DateTime<chrono::Utc>>,
    ),
) -> FirewallApproval {
    let (
        id,
        target_kind,
        target_id,
        profile,
        plan_json,
        status,
        requested_by,
        reviewed_by,
        review_note,
        created_at,
        reviewed_at,
    ) = row;
    FirewallApproval {
        id,
        target_kind,
        target_id,
        profile,
        plan_json,
        status,
        requested_by,
        reviewed_by,
        review_note,
        created_at: created_at.to_rfc3339(),
        reviewed_at: reviewed_at.map(|t| t.to_rfc3339()),
    }
}
