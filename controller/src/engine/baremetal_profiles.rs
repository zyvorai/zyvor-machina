// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Deploy profiles and the one-step "give me N machines like this" path. A profile names
//! an image, the SSH keys, a zone and a minimum size. `provision` allocates N ready
//! machines and deploys each. With `auto_deploy` and a `desired_count`, a background
//! loop keeps that many machines allocated, deploying or deployed as machines become ready.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbPool;
use crate::engine::baremetal_alloc::{allocate, release, AllocateRequest};
use crate::engine::baremetal_deploy::{begin_deploy, DeployRequest};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Profile {
    pub name: String,
    pub image: String,
    pub ssh_keys: String,
    pub timezone: String,
    pub zone: String,
    pub min_cpu_cores: i64,
    pub min_memory_mib: i64,
    pub auto_deploy: bool,
    pub desired_count: i64,
}

#[derive(Debug, Deserialize)]
pub struct ProfileBody {
    pub name: String,
    pub image: String,
    #[serde(default)]
    pub ssh_keys: Vec<String>,
    #[serde(default = "tz")]
    pub timezone: String,
    #[serde(default)]
    pub zone: String,
    #[serde(default)]
    pub min_cpu_cores: i64,
    #[serde(default)]
    pub min_memory_mib: i64,
    #[serde(default)]
    pub auto_deploy: bool,
    #[serde(default)]
    pub desired_count: i64,
}

fn tz() -> String {
    "UTC".into()
}

const COLS: &str = "name, image, ssh_keys, timezone, zone, min_cpu_cores, min_memory_mib, auto_deploy, desired_count";

pub async fn list(pool: &DbPool) -> anyhow::Result<Vec<Profile>> {
    Ok(crate::db::query_as::<_, Profile>(&format!(
        "SELECT {COLS} FROM baremetal_profiles ORDER BY name"
    ))
    .fetch_all(pool)
    .await?)
}

pub async fn get(pool: &DbPool, name: &str) -> anyhow::Result<Option<Profile>> {
    Ok(crate::db::query_as::<_, Profile>(&format!(
        "SELECT {COLS} FROM baremetal_profiles WHERE name = ?"
    ))
    .bind(name)
    .fetch_optional(pool)
    .await?)
}

pub async fn upsert(pool: &DbPool, b: &ProfileBody) -> anyhow::Result<Profile> {
    let name = b.name.trim();
    if !super::baremetal_images::valid_name(name) {
        anyhow::bail!("profile name must be 1-64 characters of a-z, 0-9, '.', '-', '_'");
    }
    if b.image.trim().is_empty() {
        anyhow::bail!("image is required");
    }
    if b.ssh_keys.is_empty() {
        anyhow::bail!(
            "at least one ssh key is required (deployed machines have no password login)"
        );
    }
    if b.auto_deploy && b.desired_count <= 0 {
        anyhow::bail!("auto_deploy needs a desired_count greater than zero");
    }
    if b.desired_count < 0 || b.desired_count > 1000 {
        anyhow::bail!("desired_count must be between 0 and 1000");
    }
    let keys = serde_json::to_string(&b.ssh_keys)?;
    crate::db::query(
        "INSERT INTO baremetal_profiles
         (name, image, ssh_keys, timezone, zone, min_cpu_cores, min_memory_mib, auto_deploy, desired_count)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (name) DO UPDATE SET image = excluded.image, ssh_keys = excluded.ssh_keys,
           timezone = excluded.timezone, zone = excluded.zone,
           min_cpu_cores = excluded.min_cpu_cores, min_memory_mib = excluded.min_memory_mib,
           auto_deploy = excluded.auto_deploy, desired_count = excluded.desired_count",
    )
    .bind(name)
    .bind(b.image.trim())
    .bind(keys)
    .bind(&b.timezone)
    .bind(b.zone.trim())
    .bind(b.min_cpu_cores.max(0))
    .bind(b.min_memory_mib.max(0))
    .bind(b.auto_deploy)
    .bind(b.desired_count)
    .execute(pool)
    .await?;
    get(pool, name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("profile vanished"))
}

pub async fn delete(pool: &DbPool, name: &str) -> anyhow::Result<()> {
    let res = crate::db::query("DELETE FROM baremetal_profiles WHERE name = ?")
        .bind(name)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        anyhow::bail!("profile not found");
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct Provisioned {
    pub server_id: Uuid,
    pub hostname: String,
    pub status: String,
}

/// Allocate and deploy up to `count` machines for `profile`. Stops at the first shortage
/// and returns what was started plus why it stopped, so a partial result is visible.
pub async fn provision(
    pool: &DbPool,
    profile: &str,
    count: usize,
    owner: &str,
    controller_base: &str,
) -> anyhow::Result<(Vec<Provisioned>, Option<String>)> {
    let p = get(pool, profile)
        .await?
        .ok_or_else(|| anyhow::anyhow!("profile '{profile}' not found"))?;
    let keys: Vec<String> = serde_json::from_str(&p.ssh_keys).unwrap_or_default();
    let mut out = Vec::new();
    for _ in 0..count {
        let a = match allocate(
            pool,
            &AllocateRequest {
                zone: p.zone.clone(),
                min_cpu_cores: p.min_cpu_cores,
                min_memory_mib: p.min_memory_mib,
            },
            owner,
        )
        .await
        {
            Ok(a) => a,
            Err(e) => return Ok((out, Some(e.to_string()))),
        };
        crate::db::query("UPDATE baremetal_servers SET profile = ? WHERE id = ?")
            .bind(&p.name)
            .bind(a.id)
            .execute(pool)
            .await?;
        let req = DeployRequest {
            os: p.image.clone(),
            ssh_authorized_keys: keys.clone(),
            timezone: p.timezone.clone(),
            ttl_hours: 0,
        };
        match begin_deploy(pool, a.id, &req, controller_base).await {
            Ok(_) => out.push(Provisioned {
                server_id: a.id,
                hostname: a.hostname,
                status: "deploying".into(),
            }),
            Err(e) => {
                // Give the machine back rather than leave it allocated and idle.
                let _ = release(pool, a.id).await;
                let _ = crate::db::query("UPDATE baremetal_servers SET profile = '' WHERE id = ?")
                    .bind(a.id)
                    .execute(pool)
                    .await;
                return Ok((out, Some(format!("{}: {e}", a.hostname))));
            }
        }
    }
    Ok((out, None))
}

/// Machines of a profile that count toward its desired number.
async fn in_use(pool: &DbPool, profile: &str) -> anyhow::Result<i64> {
    Ok(crate::db::query_scalar(
        "SELECT COUNT(*) FROM baremetal_servers
         WHERE profile = ? AND lifecycle IN ('allocated', 'deploying', 'deployed')",
    )
    .bind(profile)
    .fetch_one(pool)
    .await?)
}

/// One reconcile pass over the auto-deploy profiles; returns how many deploys it started.
pub async fn tick(pool: &DbPool, controller_base: &str) -> anyhow::Result<usize> {
    let profiles: Vec<Profile> = list(pool)
        .await?
        .into_iter()
        .filter(|p| p.auto_deploy && p.desired_count > 0)
        .collect();
    let mut started = 0;
    for p in profiles {
        let missing = (p.desired_count - in_use(pool, &p.name).await?).clamp(0, 10) as usize;
        if missing == 0 {
            continue;
        }
        let (done, why) = provision(pool, &p.name, missing, "auto-deploy", controller_base).await?;
        started += done.len();
        if let Some(why) = why {
            tracing::debug!("auto-deploy {}: {why}", p.name);
        }
    }
    Ok(started)
}

pub fn spawn(state: AppState) {
    let secs: u64 = std::env::var("MACHINA_METAL_AUTODEPLOY_SECS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(60);
    if secs == 0 {
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(secs));
        interval.tick().await;
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            let base = state
                .config
                .public_base_url
                .trim_end_matches('/')
                .to_string();
            if let Err(e) = tick(&state.pool, &base).await {
                tracing::warn!("bare-metal auto-deploy: {e:#}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(name: &str, auto: bool, count: i64) -> ProfileBody {
        ProfileBody {
            name: name.into(),
            image: "ubuntu-26.04".into(),
            ssh_keys: vec!["ssh-ed25519 AAAAC3 ops@x".into()],
            timezone: "UTC".into(),
            zone: "a".into(),
            min_cpu_cores: 8,
            min_memory_mib: 0,
            auto_deploy: auto,
            desired_count: count,
        }
    }

    async fn setup(pool: &DbPool, ready: usize) {
        crate::db::query("INSERT INTO clusters (id, name) VALUES (?, 'c')")
            .bind(Uuid::from_u128(1))
            .execute(pool)
            .await
            .unwrap();
        crate::db::query(
            "INSERT INTO baremetal_images (id, name, format, size_bytes, checksum, path, status)
             VALUES (?, 'ubuntu-26.04', 'qcow2', 1, 'ab', 'u.img', 'ready')",
        )
        .bind(Uuid::new_v4())
        .execute(pool)
        .await
        .unwrap();
        for i in 0..ready {
            crate::db::query(
                "INSERT INTO baremetal_servers
                 (id, hostname, bmc_type, lifecycle, zone, cpu_cores, memory_mib, mac_address)
                 VALUES (?, ?, 'fake', 'ready', 'a', 16, 65536, ?)",
            )
            .bind(Uuid::new_v4())
            .bind(format!("m{i}"))
            .bind(format!("aa:bb:cc:00:00:{i:02x}"))
            .execute(pool)
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn profile_validation() {
        let pool = crate::db::testing::pool().await;
        assert!(upsert(&pool, &body("web", false, 0)).await.is_ok());
        assert!(upsert(&pool, &body("Bad Name", false, 0)).await.is_err());
        assert!(upsert(&pool, &body("web2", true, 0)).await.is_err());
        let mut nokeys = body("web3", false, 0);
        nokeys.ssh_keys.clear();
        assert!(upsert(&pool, &nokeys).await.is_err());
        assert!(delete(&pool, "web").await.is_ok());
        assert!(delete(&pool, "web").await.is_err());
    }

    #[tokio::test]
    async fn provision_deploys_n_and_reports_a_shortage() {
        let pool = crate::db::testing::pool().await;
        setup(&pool, 2).await;
        upsert(&pool, &body("web", false, 0)).await.unwrap();
        let (done, why) = provision(&pool, "web", 3, "ops", "https://c:5093")
            .await
            .unwrap();
        assert_eq!(done.len(), 2);
        assert!(why.unwrap().contains("no ready server"));
        let n: i64 = crate::db::query_scalar(
            "SELECT COUNT(*) FROM baremetal_servers WHERE lifecycle = 'deploying' AND profile = 'web'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(n, 2);
        assert!(provision(&pool, "nope", 1, "ops", "https://c")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn auto_deploy_tops_up_to_the_desired_count_and_no_further() {
        let pool = crate::db::testing::pool().await;
        setup(&pool, 5).await;
        upsert(&pool, &body("web", true, 3)).await.unwrap();
        assert_eq!(tick(&pool, "https://c:5093").await.unwrap(), 3);
        // Already satisfied: nothing more, although two machines are still ready.
        assert_eq!(tick(&pool, "https://c:5093").await.unwrap(), 0);
        let ready: i64 = crate::db::query_scalar(
            "SELECT COUNT(*) FROM baremetal_servers WHERE lifecycle = 'ready'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(ready, 2);
    }

    #[tokio::test]
    async fn a_failed_deploy_returns_the_machine() {
        let pool = crate::db::testing::pool().await;
        setup(&pool, 1).await;
        // Profile whose image does not exist and is not an installer OS.
        let mut b = body("ghost", false, 0);
        b.image = "no-such-image".into();
        upsert(&pool, &b).await.unwrap();
        let (done, why) = provision(&pool, "ghost", 1, "ops", "https://c")
            .await
            .unwrap();
        assert!(done.is_empty() && why.is_some());
        let lc: String = crate::db::query_scalar("SELECT lifecycle FROM baremetal_servers LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(lc, "ready");
    }
}
