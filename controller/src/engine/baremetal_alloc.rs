// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Zones and allocation for bare metal. An allocation request picks the smallest
//! `ready` machine that satisfies it (so large machines stay free for large requests),
//! with a compare-and-set on the lifecycle so two requests never get the same server.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbPool;

#[derive(Debug, Deserialize)]
pub struct AllocateRequest {
    /// Empty = any zone.
    #[serde(default)]
    pub zone: String,
    #[serde(default)]
    pub min_cpu_cores: i64,
    #[serde(default)]
    pub min_memory_mib: i64,
}

#[derive(Debug, Serialize)]
pub struct Allocation {
    pub id: Uuid,
    pub hostname: String,
    pub zone: String,
    pub cpu_cores: i64,
    pub memory_mib: i64,
    pub allocated_to: String,
}

pub async fn allocate(
    pool: &DbPool,
    req: &AllocateRequest,
    owner: &str,
) -> anyhow::Result<Allocation> {
    let cores = req.min_cpu_cores.max(0);
    let mem = req.min_memory_mib.max(0);
    let zone = req.zone.trim();
    // A lost race (another request took the same row) retries with the next candidate.
    for _ in 0..5 {
        let cand: Option<(Uuid, String, String, i64, i64)> = crate::db::query_as(
            "SELECT id, hostname, zone, cpu_cores, memory_mib FROM baremetal_servers
             WHERE lifecycle = 'ready' AND cpu_cores >= ? AND memory_mib >= ?
               AND (? = '' OR zone = ?)
             ORDER BY cpu_cores, memory_mib, hostname LIMIT 1",
        )
        .bind(cores)
        .bind(mem)
        .bind(zone)
        .bind(zone)
        .fetch_optional(pool)
        .await?;
        let Some((id, hostname, z, c, m)) = cand else {
            anyhow::bail!("no ready server matches the request");
        };
        let res = crate::db::query(
            "UPDATE baremetal_servers
             SET lifecycle = 'allocated', allocated_to = ?, allocated_at = CURRENT_TIMESTAMP
             WHERE id = ? AND lifecycle = 'ready'",
        )
        .bind(owner)
        .bind(id)
        .execute(pool)
        .await?;
        if res.rows_affected() == 1 {
            return Ok(Allocation {
                id,
                hostname,
                zone: z,
                cpu_cores: c,
                memory_mib: m,
                allocated_to: owner.to_string(),
            });
        }
    }
    anyhow::bail!("could not claim a server (busy); retry")
}

/// `allocated|deployed -> ready`, clearing the owner.
pub async fn release(pool: &DbPool, id: Uuid) -> anyhow::Result<()> {
    let res = crate::db::query(
        "UPDATE baremetal_servers
         SET lifecycle = 'ready', allocated_to = '', allocated_at = ''
         WHERE id = ? AND lifecycle IN ('allocated', 'deployed')",
    )
    .bind(id)
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        anyhow::bail!("server is not allocated or deployed");
    }
    Ok(())
}

pub async fn set_zone(pool: &DbPool, id: Uuid, zone: &str) -> anyhow::Result<()> {
    let z = zone.trim();
    if z.len() > 64 || z.chars().any(|c| c.is_control()) {
        anyhow::bail!("invalid zone name");
    }
    let res = crate::db::query("UPDATE baremetal_servers SET zone = ? WHERE id = ?")
        .bind(z)
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        anyhow::bail!("server not found");
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct ZoneSummary {
    pub zone: String,
    pub total: i64,
    pub ready: i64,
    pub allocated: i64,
    pub deployed: i64,
}

pub async fn zones(pool: &DbPool) -> anyhow::Result<Vec<ZoneSummary>> {
    let rows: Vec<(String, String, i64)> = crate::db::query_as(
        "SELECT zone, lifecycle, COUNT(*) FROM baremetal_servers GROUP BY zone, lifecycle",
    )
    .fetch_all(pool)
    .await?;
    let mut out: Vec<ZoneSummary> = Vec::new();
    for (zone, lc, n) in rows {
        let idx = match out.iter().position(|z| z.zone == zone) {
            Some(i) => i,
            None => {
                out.push(ZoneSummary {
                    zone: zone.clone(),
                    total: 0,
                    ready: 0,
                    allocated: 0,
                    deployed: 0,
                });
                out.len() - 1
            }
        };
        let z = &mut out[idx];
        z.total += n;
        match lc.as_str() {
            "ready" => z.ready += n,
            "allocated" => z.allocated += n,
            "deployed" => z.deployed += n,
            _ => {}
        }
    }
    out.sort_by(|a, b| a.zone.cmp(&b.zone));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn ready(pool: &DbPool, name: &str, zone: &str, cores: i64, mem: i64) -> Uuid {
        let id = Uuid::new_v4();
        crate::db::query(
            "INSERT INTO baremetal_servers
             (id, hostname, bmc_type, lifecycle, zone, cpu_cores, memory_mib)
             VALUES (?, ?, 'fake', 'ready', ?, ?, ?)",
        )
        .bind(id)
        .bind(name)
        .bind(zone)
        .bind(cores)
        .bind(mem)
        .execute(pool)
        .await
        .unwrap();
        id
    }

    #[tokio::test]
    async fn picks_the_smallest_fit_in_the_zone_and_never_twice() {
        let pool = crate::db::testing::pool().await;
        ready(&pool, "big", "a", 64, 524288).await;
        let mid = ready(&pool, "mid", "a", 32, 262144).await;
        ready(&pool, "other", "b", 16, 131072).await;
        let req = AllocateRequest {
            zone: "a".into(),
            min_cpu_cores: 16,
            min_memory_mib: 0,
        };
        let a = allocate(&pool, &req, "alice").await.unwrap();
        assert_eq!(a.id, mid);
        assert_eq!(a.allocated_to, "alice");
        let b = allocate(&pool, &req, "bob").await.unwrap();
        assert_eq!(b.hostname, "big");
        assert!(allocate(&pool, &req, "carol").await.is_err());
    }

    #[tokio::test]
    async fn release_returns_to_ready_and_zone_summary_counts() {
        let pool = crate::db::testing::pool().await;
        let id = ready(&pool, "m1", "z", 8, 8192).await;
        ready(&pool, "m2", "z", 8, 8192).await;
        allocate(
            &pool,
            &AllocateRequest {
                zone: "z".into(),
                min_cpu_cores: 0,
                min_memory_mib: 0,
            },
            "ops",
        )
        .await
        .unwrap();
        let z = zones(&pool).await.unwrap();
        let z = z.iter().find(|x| x.zone == "z").unwrap();
        assert_eq!((z.total, z.ready, z.allocated), (2, 1, 1));
        let held: Uuid = crate::db::query_scalar(
            "SELECT id FROM baremetal_servers WHERE lifecycle = 'allocated'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        release(&pool, held).await.unwrap();
        assert!(release(&pool, held).await.is_err());
        set_zone(&pool, id, "moved").await.unwrap();
        assert!(set_zone(&pool, id, &"x".repeat(65)).await.is_err());
    }
}
