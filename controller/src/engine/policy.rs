// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde_json::Value;
use crate::db::{DbConn, DbPool};
use uuid::Uuid;

#[derive(Debug)]
pub struct PolicyViolation {
    pub rule_name: String,
    pub message: String,
    pub remediation: String,
}

/// Quota and policy check on its own connection. Prefer [`evaluate_vm_create_tx`] when the caller is about to insert
/// the VM: only a check made inside the same write transaction as the insert is race-free.
pub async fn evaluate_vm_create(
    pool: &DbPool,
    project: &str,
    tags: &[String],
    vcpus: i32,
    memory_mib: i64,
    storage_gib: i64,
    ha_enabled: bool,
) -> Result<(), PolicyViolation> {
    let mut conn = pool.acquire().await.map_err(unavailable)?;
    evaluate_vm_create_tx(
        &mut conn,
        project,
        tags,
        vcpus,
        memory_mib,
        storage_gib,
        ha_enabled,
    )
    .await
}

/// Same check, run on the caller's connection. Call it inside a `BEGIN IMMEDIATE` transaction, right before inserting
/// the VM: SQLite then serialises concurrent creates, so two requests at a project's limit cannot both pass.
pub async fn evaluate_vm_create_tx(
    conn: &mut DbConn,
    project: &str,
    tags: &[String],
    vcpus: i32,
    memory_mib: i64,
    storage_gib: i64,
    ha_enabled: bool,
) -> Result<(), PolicyViolation> {
    check_project_quota(conn, project, 1, vcpus, memory_mib, storage_gib).await?;

    let rows: Vec<(String, Value)> =
        crate::db::query_as("SELECT name, rule_json FROM policy_rules WHERE enabled = TRUE")
            .fetch_all(&mut *conn)
            .await
            .map_err(unavailable)?;

    for (name, rule) in rows {
        if let Some(v) = check_rule(&name, &rule, tags, ha_enabled, vcpus) {
            return Err(v);
        }
    }
    Ok(())
}

/// A quota that cannot be read must not become "no quota": fail closed, and say it is a retryable condition.
fn unavailable(e: sqlx::Error) -> PolicyViolation {
    PolicyViolation {
        rule_name: "policy_unavailable".into(),
        message: format!("Could not check quotas and policies right now ({e})"),
        remediation: "Try again in a moment.".into(),
    }
}

/// Whether `vms` more VMs with these totals fit the project's quota.
pub async fn check_project_quota_batch(
    pool: &DbPool,
    project: &str,
    vms: i64,
    vcpus: i32,
    memory_mib: i64,
    storage_gib: i64,
) -> Result<(), PolicyViolation> {
    let mut conn = pool.acquire().await.map_err(unavailable)?;
    check_project_quota(&mut conn, project, vms, vcpus, memory_mib, storage_gib).await
}

type QuotaRow = (i32, i32, i64, i64, i64, i64, i64, i64);

/// (limits, current usage) for a project, or None when it has no quota row.
async fn load_quota(
    conn: &mut DbConn,
    project: &str,
) -> Result<Option<QuotaRow>, PolicyViolation> {
    crate::db::query_as(
        "SELECT q.max_vms, q.max_vcpu, q.max_memory_mib, q.max_storage_gib,
                COALESCE((SELECT COUNT(*) FROM vms WHERE COALESCE(project, 'default') = ?), 0),
                COALESCE((SELECT SUM(vcpus) FROM vms WHERE COALESCE(project, 'default') = ?), 0),
                COALESCE((SELECT SUM(memory_mib) FROM vms WHERE COALESCE(project, 'default') = ?), 0),
                COALESCE((SELECT SUM(size_gib) FROM vm_disks d JOIN vms v ON v.id = d.vm_id WHERE COALESCE(v.project, 'default') = ?), 0)
         FROM project_quotas q WHERE q.project = ?",
    )
    .bind(project)
    .bind(project)
    .bind(project)
    .bind(project)
    .bind(project)
    .fetch_optional(&mut *conn)
    .await
    .map_err(unavailable)
}

/// Quota check for making an existing machine bigger: only increases count, and the machine count does not change.
pub async fn evaluate_vm_resize_tx(
    conn: &mut DbConn,
    project: &str,
    add_vcpus: i64,
    add_memory_mib: i64,
) -> Result<(), PolicyViolation> {
    let Some((_, max_vcpu, max_mem, _, _, cur_vcpu, cur_mem, _)) =
        load_quota(conn, project).await?
    else {
        return Ok(());
    };
    if add_vcpus > 0 && max_vcpu > 0 && cur_vcpu + add_vcpus > i64::from(max_vcpu) {
        return Err(PolicyViolation {
            rule_name: "project_quota".into(),
            message: format!("Project '{project}' vCPU quota exceeded ({max_vcpu})"),
            remediation: "Increase max_vcpu quota or choose a smaller instance type.".into(),
        });
    }
    if add_memory_mib > 0 && max_mem > 0 && cur_mem + add_memory_mib > max_mem {
        return Err(PolicyViolation {
            rule_name: "project_quota".into(),
            message: format!("Project '{project}' memory quota exceeded ({max_mem} MiB)"),
            remediation: "Increase max_memory_mib quota or choose a smaller instance type.".into(),
        });
    }
    Ok(())
}

async fn check_project_quota(
    conn: &mut DbConn,
    project: &str,
    vms: i64,
    vcpus: i32,
    memory_mib: i64,
    storage_gib: i64,
) -> Result<(), PolicyViolation> {
    let row = load_quota(conn, project).await?;

    let Some((max_vms, max_vcpu, max_mem, max_storage, cur_vms, cur_vcpu, cur_mem, cur_storage)) =
        row
    else {
        return Ok(());
    };

    if max_vms > 0 && cur_vms + vms > max_vms as i64 {
        return Err(PolicyViolation {
            rule_name: "project_quota".into(),
            message: format!("Project '{project}' VM count quota exceeded ({max_vms})"),
            remediation: "Increase max_vms in project quotas or delete unused VMs.".into(),
        });
    }
    if max_vcpu > 0 && cur_vcpu + i64::from(vcpus) > i64::from(max_vcpu) {
        return Err(PolicyViolation {
            rule_name: "project_quota".into(),
            message: format!("Project '{project}' vCPU quota exceeded ({max_vcpu})"),
            remediation: "Increase max_vcpu quota or reduce VM size.".into(),
        });
    }
    if max_mem > 0 && cur_mem + memory_mib > max_mem {
        return Err(PolicyViolation {
            rule_name: "project_quota".into(),
            message: format!("Project '{project}' memory quota exceeded ({max_mem} MiB)"),
            remediation: "Increase max_memory_mib quota or use smaller VMs.".into(),
        });
    }
    if max_storage > 0 && cur_storage + storage_gib > max_storage {
        return Err(PolicyViolation {
            rule_name: "project_quota".into(),
            message: format!("Project '{project}' storage quota exceeded ({max_storage} GiB)"),
            remediation: "Increase max_storage_gib quota or shrink disks.".into(),
        });
    }
    Ok(())
}

fn check_rule(
    name: &str,
    rule: &Value,
    tags: &[String],
    ha_enabled: bool,
    vcpus: i32,
) -> Option<PolicyViolation> {
    let when = rule.get("when")?;
    if let Some(tag) = when.get("tags_contains").and_then(|v| v.as_str()) {
        if !tags.iter().any(|t| t.contains(tag)) {
            return None;
        }
    }
    if let Some(req) = rule.get("require") {
        if req.get("ha_enabled").and_then(|v| v.as_bool()) == Some(true) && !ha_enabled {
            return Some(PolicyViolation {
                rule_name: name.into(),
                message: format!("Policy '{name}' requires HA to be enabled"),
                remediation: "Enable HA on the VM spec or remove the production tag.".into(),
            });
        }
        if let Some(max) = req.get("max_vcpu").and_then(|v| v.as_i64()) {
            if i64::from(vcpus) > max {
                return Some(PolicyViolation {
                    rule_name: name.into(),
                    message: format!("Policy '{name}' limits vCPU to {max}"),
                    remediation: "Reduce vCPU count or request a policy exception.".into(),
                });
            }
        }
    }
    None
}

pub async fn upsert_project_quota(
    pool: &DbPool,
    project: &str,
    max_vms: i32,
    max_vcpu: i32,
    max_memory_mib: i64,
    max_storage_gib: i64,
) -> anyhow::Result<()> {
    crate::db::query(
        "INSERT INTO project_quotas (project, max_vms, max_vcpu, max_memory_mib, max_storage_gib, updated_at)
         VALUES (?, ?, ?, ?, ?, datetime('now'))
         ON CONFLICT (project) DO UPDATE SET
           max_vms = EXCLUDED.max_vms,
           max_vcpu = EXCLUDED.max_vcpu,
           max_memory_mib = EXCLUDED.max_memory_mib,
           max_storage_gib = EXCLUDED.max_storage_gib,
           updated_at = datetime('now')",
    )
    .bind(project)
    .bind(max_vms)
    .bind(max_vcpu)
    .bind(max_memory_mib)
    .bind(max_storage_gib)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_policy_rules(
    pool: &DbPool,
) -> anyhow::Result<Vec<(Uuid, String, bool, Value)>> {
    Ok(
        crate::db::query_as("SELECT id, name, enabled, rule_json FROM policy_rules ORDER BY name")
            .fetch_all(pool)
            .await?,
    )
}

// These build their own SQLite tables and race real connections for SQLite's write lock; the PostgreSQL equivalent of the
// quota race (advisory lock via db::begin_write) is covered by the integration run against a real server.
#[cfg(all(test, feature = "sqlite"))]
mod quota_race_tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::sync::Arc;

    /// The create path: take the write lock, check the quota on that connection, insert, commit.
    async fn create_one(pool: &DbPool, n: usize) -> bool {
        let mut tx = crate::db::begin_write(&pool).await.unwrap();
        let ok = evaluate_vm_create_tx(&mut tx, "p", &[], 1, 512, 10, false)
            .await
            .is_ok();
        if ok {
            crate::db::query(
                "INSERT INTO vms (id, name, project, vcpus, memory_mib) VALUES (?, ?, 'p', 1, 512)",
            )
            .bind(format!("id{n}"))
            .bind(format!("vm{n}"))
            .execute(&mut *tx)
            .await
            .unwrap();
        }
        tx.commit().await.unwrap();
        ok
    }

    #[tokio::test]
    async fn concurrent_creates_at_the_limit_cannot_exceed_the_quota() {
        let dir = std::env::temp_dir().join(format!("machina-quota-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let url = format!("sqlite://{}/q.db?mode=rwc", dir.display());
        let pool = Arc::new(
            SqlitePoolOptions::new()
                .max_connections(4)
                .acquire_timeout(std::time::Duration::from_secs(30))
                .connect(&url)
                .await
                .unwrap(),
        );
        for ddl in [
            "CREATE TABLE vms (id TEXT PRIMARY KEY, name TEXT, project TEXT, vcpus INTEGER, memory_mib INTEGER)",
            "CREATE TABLE vm_disks (id TEXT, vm_id TEXT, size_gib INTEGER)",
            "CREATE TABLE policy_rules (name TEXT, rule_json TEXT, enabled BOOLEAN)",
            "CREATE TABLE project_quotas (project TEXT, max_vms INTEGER, max_vcpu INTEGER, max_memory_mib INTEGER, max_storage_gib INTEGER)",
            "INSERT INTO project_quotas VALUES ('p', 3, 0, 0, 0)",
        ] {
            crate::db::query(ddl).execute(&*pool).await.unwrap();
        }
        let mut tasks = Vec::new();
        for n in 0..12 {
            let pool = pool.clone();
            tasks.push(tokio::spawn(async move { create_one(&pool, n).await }));
        }
        let mut accepted = 0;
        for t in tasks {
            if t.await.unwrap() {
                accepted += 1;
            }
        }
        assert_eq!(
            accepted, 3,
            "exactly the quota's worth of creates may succeed"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn resizing_counts_only_increases_against_cpu_and_memory_quota() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE vms (id TEXT PRIMARY KEY, name TEXT, project TEXT, vcpus INTEGER, memory_mib INTEGER)",
            "CREATE TABLE vm_disks (id TEXT, vm_id TEXT, size_gib INTEGER)",
            "CREATE TABLE project_quotas (project TEXT, max_vms INTEGER, max_vcpu INTEGER, max_memory_mib INTEGER, max_storage_gib INTEGER)",
            "INSERT INTO project_quotas VALUES ('p', 0, 8, 16384, 0)",
            "INSERT INTO vms VALUES ('a', 'a', 'p', 4, 8192)",
            "INSERT INTO vms VALUES ('b', 'b', 'p', 3, 4096)",
        ] {
            crate::db::query(ddl).execute(&pool).await.unwrap();
        }
        let mut c = pool.acquire().await.unwrap();
        // 7 of 8 vCPU and 12288 of 16384 MiB used
        assert!(evaluate_vm_resize_tx(&mut c, "p", 1, 4096).await.is_ok()); // exactly at both limits
        assert!(evaluate_vm_resize_tx(&mut c, "p", 2, 0).await.is_err()); // one vCPU over
        assert!(evaluate_vm_resize_tx(&mut c, "p", 0, 4097).await.is_err()); // one MiB over
        assert!(evaluate_vm_resize_tx(&mut c, "p", -3, -4096).await.is_ok()); // shrinking is always fine
        assert!(evaluate_vm_resize_tx(&mut c, "no-quota", 100, 1_000_000)
            .await
            .is_ok()); // no quota row: unlimited
    }
}
