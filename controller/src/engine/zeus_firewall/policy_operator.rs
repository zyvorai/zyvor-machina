// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use crate::db::DbPool;

pub async fn expire_stale_approvals(pool: &DbPool) -> anyhow::Result<u64> {
    let sla_hours: i32 = crate::db::query_scalar(
        "SELECT firewall_approval_sla_hours FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_optional(pool)
    .await?
    .unwrap_or(72);

    let r = crate::db::query(
        "UPDATE firewall_approvals SET status = 'expired', review_note = 'SLA exceeded'
         WHERE status = 'pending' AND created_at < datetime('now', '-' || ? || ' hours')",
    )
    .bind(sla_hours)
    .execute(pool)
    .await?;

    Ok(r.rows_affected())
}

pub async fn reconcile_gitops_policies(pool: &DbPool) -> anyhow::Result<usize> {
    let count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM firewall_policies")
        .fetch_one(pool)
        .await?;
    let _ = crate::db::query(
        "INSERT INTO firewall_policy_reconcile_log (id, policies_synced, detail_json) VALUES (?, ?, ?)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(count)
    .bind(serde_json::json!({ "note": "GitOps reconcile tick — policies registered in DB" }))
    .execute(pool)
    .await?;
    Ok(count as usize)
}
