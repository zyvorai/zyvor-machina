// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// vSAN-class storage tiers, snapshot retention, backup SLA stubs.

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct StorageTierRow {
    pub id: Uuid,
    pub name: String,
    pub tier_class: String,
    pub iops_tier: String,
    pub replication: String,
    pub snapshot_retention_days: i32,
    pub backup_rpo_hours: i32,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TierOverviewItem {
    pub id: String,
    pub name: String,
    pub tier_class: String,
    pub iops_tier: String,
    pub replication: String,
    pub snapshot_retention_days: i32,
    pub backup_rpo_hours: i32,
    pub description: String,
    pub pool_count: usize,
    pub capacity_gib: i64,
    pub used_gib: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TiersOverview {
    pub tiers: Vec<TierOverviewItem>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct BackupSlaRow {
    pub id: Uuid,
    pub pool_id: Uuid,
    pub pool_name: String,
    pub tier_name: Option<String>,
    pub rpo_hours: i32,
    pub rto_hours: i32,
    pub retention_days: i32,
    pub last_backup_at: Option<chrono::DateTime<chrono::Utc>>,
    pub compliance_grade: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupSlaOverview {
    pub policies: Vec<BackupSlaRow>,
    pub summary: String,
}

#[derive(Debug, Deserialize)]
pub struct BindPoolTierRequest {
    pub tier_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct UpsertBackupSlaRequest {
    pub rpo_hours: i32,
    pub rto_hours: i32,
    pub retention_days: i32,
}

pub async fn tiers_overview(pool: &DbPool) -> anyhow::Result<TiersOverview> {
    let rows: Vec<StorageTierRow> = match crate::db::query_as(
        "SELECT id, name, tier_class, iops_tier, replication, snapshot_retention_days, backup_rpo_hours, description
         FROM storage_tiers ORDER BY tier_class, name",
    )
    .fetch_all(pool)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("storage tiers query: {e}");
            return Ok(TiersOverview {
                tiers: vec![],
                summary: "Storage tiers schema not ready — run controller migrations".into(),
            });
        }
    };

    let mut tiers = Vec::new();
    for row in rows {
        let stats: (i64, i64, i64) = match crate::db::query_as(
            "SELECT COUNT(*), COALESCE(SUM(capacity_gib), 0), COALESCE(SUM(used_gib), 0)
             FROM storage_pools WHERE tier_id = ?",
        )
        .bind(row.id)
        .fetch_one(pool)
        .await
        {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("storage tier pool stats for {}: {e}", row.name);
                (0, 0, 0)
            }
        };

        tiers.push(TierOverviewItem {
            id: row.id.to_string(),
            name: row.name,
            tier_class: row.tier_class,
            iops_tier: row.iops_tier,
            replication: row.replication,
            snapshot_retention_days: row.snapshot_retention_days,
            backup_rpo_hours: row.backup_rpo_hours,
            description: row.description,
            pool_count: stats.0 as usize,
            capacity_gib: stats.1,
            used_gib: stats.2,
        });
    }

    let summary = format!(
        "{} tier(s) · {} pool(s) assigned",
        tiers.len(),
        tiers.iter().map(|t| t.pool_count).sum::<usize>()
    );

    Ok(TiersOverview { tiers, summary })
}

pub async fn bind_pool_tier(pool: &DbPool, pool_id: Uuid, tier_id: Uuid) -> anyhow::Result<()> {
    let exists: Option<Uuid> = crate::db::query_scalar("SELECT id FROM storage_tiers WHERE id = ?")
        .bind(tier_id)
        .fetch_optional(pool)
        .await?;
    if exists.is_none() {
        anyhow::bail!("tier not found");
    }
    let r = crate::db::query("UPDATE storage_pools SET tier_id = ? WHERE id = ?")
        .bind(tier_id)
        .bind(pool_id)
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        anyhow::bail!("storage pool not found");
    }
    Ok(())
}

pub async fn backup_sla_overview(pool: &DbPool) -> anyhow::Result<BackupSlaOverview> {
    ensure_sla_stubs(pool).await?;

    let policies = crate::db::query_as(
        "SELECT s.id, s.pool_id, p.name AS pool_name, t.name AS tier_name,
                s.rpo_hours, s.rto_hours, s.retention_days, s.last_backup_at, s.compliance_grade
         FROM storage_backup_sla s
         JOIN storage_pools p ON p.id = s.pool_id
         LEFT JOIN storage_tiers t ON t.id = p.tier_id
         ORDER BY p.name",
    )
    .fetch_all(pool)
    .await?;

    let summary = format!(
        "{} backup SLA policy(ies) (simulated compliance grades)",
        policies.len()
    );

    Ok(BackupSlaOverview { policies, summary })
}

pub async fn upsert_backup_sla(
    pool: &DbPool,
    pool_id: Uuid,
    req: &UpsertBackupSlaRequest,
) -> anyhow::Result<BackupSlaRow> {
    let _pool: String = crate::db::query_scalar("SELECT name FROM storage_pools WHERE id = ?")
        .bind(pool_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("storage pool not found"))?;

    let grade = if req.rpo_hours <= 4 {
        "A"
    } else if req.rpo_hours <= 24 {
        "B"
    } else {
        "C"
    };

    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO storage_backup_sla (id, pool_id, rpo_hours, rto_hours, retention_days, compliance_grade)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT (pool_id) DO UPDATE SET
           rpo_hours = EXCLUDED.rpo_hours,
           rto_hours = EXCLUDED.rto_hours,
           retention_days = EXCLUDED.retention_days,
           compliance_grade = EXCLUDED.compliance_grade",
    )
    .bind(id)
    .bind(pool_id)
    .bind(req.rpo_hours)
    .bind(req.rto_hours)
    .bind(req.retention_days)
    .bind(grade)
    .execute(pool)
    .await?;

    crate::db::query_as(
        "SELECT s.id, s.pool_id, p.name AS pool_name, t.name AS tier_name,
                s.rpo_hours, s.rto_hours, s.retention_days, s.last_backup_at, s.compliance_grade
         FROM storage_backup_sla s
         JOIN storage_pools p ON p.id = s.pool_id
         LEFT JOIN storage_tiers t ON t.id = p.tier_id
         WHERE s.pool_id = ?",
    )
    .bind(pool_id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.into())
}

async fn ensure_sla_stubs(pool: &DbPool) -> anyhow::Result<()> {
    let pools: Vec<(Uuid, Option<Uuid>)> =
        crate::db::query_as("SELECT id, tier_id FROM storage_pools ORDER BY name")
            .fetch_all(pool)
            .await?;

    for (pool_id, tier_id) in pools {
        let exists: Option<Uuid> =
            crate::db::query_scalar("SELECT id FROM storage_backup_sla WHERE pool_id = ?")
                .bind(pool_id)
                .fetch_optional(pool)
                .await?;
        if exists.is_some() {
            continue;
        }

        let (rpo, retention): (i32, i32) = if let Some(tid) = tier_id {
            crate::db::query_as(
                "SELECT backup_rpo_hours, snapshot_retention_days FROM storage_tiers WHERE id = ?",
            )
            .bind(tid)
            .fetch_optional(pool)
            .await?
            .unwrap_or((24, 14))
        } else {
            (24, 14)
        };

        let grade = if rpo <= 4 {
            "A"
        } else if rpo <= 24 {
            "B"
        } else {
            "C"
        };
        let mut tx = pool.begin().await?;
        crate::db::query(
            "INSERT INTO storage_backup_sla (id, pool_id, rpo_hours, rto_hours, retention_days, compliance_grade)
             VALUES (?, ?, ?, 4, ?, ?)",
        )
        .bind(Uuid::new_v4())
        .bind(pool_id)
        .bind(rpo)
        .bind(retention)
        .bind(grade)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
    }
    Ok(())
}

pub async fn snapshot_policy_for_pool(
    pool: &DbPool,
    pool_id: Uuid,
) -> anyhow::Result<serde_json::Value> {
    let row: Option<(String, Option<String>, i32)> = crate::db::query_as(
        "SELECT p.name, t.name, COALESCE(t.snapshot_retention_days, 7)
         FROM storage_pools p
         LEFT JOIN storage_tiers t ON t.id = p.tier_id
         WHERE p.id = ?",
    )
    .bind(pool_id)
    .fetch_optional(pool)
    .await?;

    let Some((pool_name, tier_name, retention)) = row else {
        anyhow::bail!("storage pool not found");
    };

    Ok(serde_json::json!({
        "pool_id": pool_id.to_string(),
        "pool_name": pool_name,
        "tier": tier_name,
        "snapshot_retention_days": retention,
        "summary": format!("Retain snapshots {retention} day(s) on {pool_name} (stub policy)")
    }))
}
