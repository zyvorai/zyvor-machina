// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Air-gap bundle export, FIPS crypto profile matrix, and tenant isolation
// policy stubs (Horizon phase 28). Vault/MFA inventory previously lived here
// too but was removed — no real Vault or MFA backend exists anywhere in this
// deployment, and a simulated one (even honestly labeled, see the prior
// "vault-sync-all-honesty" fix) misrepresents itself as a working feature.

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AirGapBundleRow {
    pub id: Uuid,
    pub name: String,
    pub checksum: String,
    pub manifest_json: serde_json::Value,
    pub size_bytes: i64,
    pub exported_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnterpriseSecurityOverview {
    pub air_gap_bundles: usize,
    pub tenant_policies: usize,
    pub fips_profiles: usize,
    pub summary: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateAirGapBundleRequest {
    pub name: String,
}

pub async fn overview(pool: &DbPool) -> anyhow::Result<EnterpriseSecurityOverview> {
    let air_gap_bundles: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM air_gap_bundles")
        .fetch_one(pool)
        .await?;
    let tenant_policies: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM tenant_isolation_policies")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let fips_profiles: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM fips_crypto_profiles")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    let summary = format!(
        "{} tenant policies · {} FIPS profile(s) · {} air-gap bundle(s)",
        tenant_policies, fips_profiles, air_gap_bundles
    );

    Ok(EnterpriseSecurityOverview {
        air_gap_bundles: air_gap_bundles as usize,
        tenant_policies: tenant_policies as usize,
        fips_profiles: fips_profiles as usize,
        summary,
    })
}

pub async fn list_air_gap_bundles(pool: &DbPool) -> anyhow::Result<Vec<AirGapBundleRow>> {
    crate::db::query_as(
        "SELECT id, name, checksum, manifest_json, size_bytes,
                strftime('%Y-%m-%dT%H:%M:%SZ', exported_at) AS exported_at
         FROM air_gap_bundles ORDER BY exported_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.into())
}

pub async fn create_air_gap_bundle(
    pool: &DbPool,
    req: &CreateAirGapBundleRequest,
) -> anyhow::Result<AirGapBundleRow> {
    let name = req.name.trim();
    if name.is_empty() {
        anyhow::bail!("name required");
    }

    let hosts: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM hosts")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let vms: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM vms")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let templates: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM templates")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let pools: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM storage_pools")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    let manifest = serde_json::json!({
        "bundle": name,
        "version": "1.0.0",
        "kind": "machina-air-gap-inventory",
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "inventory": {
            "hosts": hosts,
            "vms": vms,
            "templates": templates,
            "storage_pools": pools,
        },
        "includes": ["cluster-settings.yaml", "templates/", "policies/", "certificates/"],
        "note": "Simulated export manifest — no live Vault or bundle runner on main."
    });

    let manifest_str = manifest.to_string();
    let size_bytes = manifest_str.len() as i64;
    let checksum = format!("sha256:{}", sha256_hex(&manifest_str));
    let id = Uuid::new_v4();

    crate::db::query(
        "INSERT INTO air_gap_bundles (id, name, checksum, manifest_json, size_bytes)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(name)
    .bind(&checksum)
    .bind(&manifest)
    .bind(size_bytes)
    .execute(pool)
    .await?;

    crate::db::query_as(
        "SELECT id, name, checksum, manifest_json, size_bytes,
                strftime('%Y-%m-%dT%H:%M:%SZ', exported_at) AS exported_at
         FROM air_gap_bundles WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.into())
}

pub async fn get_air_gap_bundle(pool: &DbPool, id: Uuid) -> anyhow::Result<AirGapBundleRow> {
    crate::db::query_as(
        "SELECT id, name, checksum, manifest_json, size_bytes,
                strftime('%Y-%m-%dT%H:%M:%SZ', exported_at) AS exported_at
         FROM air_gap_bundles WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("bundle not found"))
}

pub async fn delete_air_gap_bundle(pool: &DbPool, id: Uuid) -> anyhow::Result<()> {
    let res = crate::db::query("DELETE FROM air_gap_bundles WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        anyhow::bail!("bundle not found");
    }
    Ok(())
}

/// Real SHA-256 hex digest. Previously a 64-bit non-cryptographic SipHash
/// (`DefaultHasher`) was labelled `sha256:`, misrepresenting integrity — trivially
/// collidable and giving no tamper protection for the air-gap inventory export.
fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    hex::encode(hasher.finalize())
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct FipsCryptoProfileRow {
    pub id: Uuid,
    pub name: String,
    pub tls_min_version: String,
    pub fips_mode: String,
    pub cipher_suites: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FipsMatrix {
    pub active_profile: String,
    pub openssl_version: String,
    pub profiles: Vec<FipsCryptoProfileRow>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TenantIsolationItem {
    pub project_name: String,
    pub vm_count: i64,
    pub network_isolation: String,
    pub max_vms: i32,
    pub max_storage_gib: i32,
    pub enforce_quotas: bool,
    pub quota_status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TenantIsolationOverview {
    pub projects: Vec<TenantIsolationItem>,
    pub enforced_count: usize,
    pub summary: String,
}

#[derive(Debug, Deserialize)]
pub struct UpsertTenantPolicyRequest {
    pub network_isolation: Option<String>,
    pub max_vms: Option<i32>,
    pub max_storage_gib: Option<i32>,
    pub enforce_quotas: Option<bool>,
}

pub async fn fips_matrix(pool: &DbPool) -> anyhow::Result<FipsMatrix> {
    let profiles: Vec<FipsCryptoProfileRow> = crate::db::query_as(
        "SELECT id, name, tls_min_version, fips_mode, cipher_suites, notes FROM fips_crypto_profiles ORDER BY name",
    )
    .fetch_all(pool)
    .await?;
    let active_profile = profiles
        .first()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "platform-default".into());
    let openssl_version = std::process::Command::new("openssl")
        .arg("version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "openssl unavailable".into());
    let fips_active = profiles.iter().any(|p| p.fips_mode == "required");
    Ok(FipsMatrix {
        active_profile: active_profile.clone(),
        openssl_version,
        summary: format!(
            "Active profile {active_profile} · FIPS required: {}",
            if fips_active { "yes" } else { "no" }
        ),
        profiles,
    })
}

pub async fn tenant_isolation_overview(
    pool: &DbPool,
) -> anyhow::Result<TenantIsolationOverview> {
    let vm_counts: Vec<(String, i64)> = crate::db::query_as(
        "SELECT COALESCE(NULLIF(project, ''), 'default') AS name, COUNT(*)
         FROM vms GROUP BY 1 ORDER BY 1",
    )
    .fetch_all(pool)
    .await?;
    let policies: Vec<(String, String, i32, i32, bool)> = crate::db::query_as(
        "SELECT project_name, network_isolation, max_vms, max_storage_gib, enforce_quotas
         FROM tenant_isolation_policies ORDER BY project_name",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut projects = Vec::new();
    for (name, vm_count) in vm_counts {
        let pol = policies.iter().find(|p| p.0 == name);
        let (network_isolation, max_vms, max_storage_gib, enforce_quotas) = pol
            .map(|p| (p.1.clone(), p.2, p.3, p.4))
            .unwrap_or(("shared".into(), 0, 0, false));
        let quota_status = if !enforce_quotas {
            "not enforced".into()
        } else if max_vms > 0 && vm_count > max_vms as i64 {
            "vm quota exceeded".into()
        } else {
            "within quota".into()
        };
        projects.push(TenantIsolationItem {
            project_name: name,
            vm_count,
            network_isolation,
            max_vms,
            max_storage_gib,
            enforce_quotas,
            quota_status,
        });
    }

    let enforced_count = projects.iter().filter(|p| p.enforce_quotas).count();
    Ok(TenantIsolationOverview {
        summary: format!(
            "{} workspace(s) · {} with enforced quotas",
            projects.len(),
            enforced_count
        ),
        enforced_count,
        projects,
    })
}

pub async fn upsert_tenant_policy(
    pool: &DbPool,
    project_name: &str,
    req: &UpsertTenantPolicyRequest,
) -> anyhow::Result<TenantIsolationItem> {
    let name = project_name.trim();
    if name.is_empty() {
        anyhow::bail!("project name required");
    }
    let network_isolation = req
        .network_isolation
        .clone()
        .unwrap_or_else(|| "shared".into());
    let max_vms = req.max_vms.unwrap_or(0).max(0);
    let max_storage_gib = req.max_storage_gib.unwrap_or(0).max(0);
    let enforce = req.enforce_quotas.unwrap_or(false);

    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO tenant_isolation_policies (id, project_name, network_isolation, max_vms, max_storage_gib, enforce_quotas, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, datetime('now'))
         ON CONFLICT (project_name) DO UPDATE SET
           network_isolation = EXCLUDED.network_isolation,
           max_vms = EXCLUDED.max_vms,
           max_storage_gib = EXCLUDED.max_storage_gib,
           enforce_quotas = EXCLUDED.enforce_quotas,
           updated_at = datetime('now')",
    )
    .bind(id)
    .bind(name)
    .bind(&network_isolation)
    .bind(max_vms)
    .bind(max_storage_gib)
    .bind(enforce)
    .execute(pool)
    .await?;

    let ov = tenant_isolation_overview(pool).await?;
    ov.projects
        .into_iter()
        .find(|p| p.project_name == name)
        .ok_or_else(|| anyhow::anyhow!("policy not found after upsert"))
}
