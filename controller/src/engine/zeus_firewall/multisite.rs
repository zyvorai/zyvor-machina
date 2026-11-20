// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Multi-site federated Zeus Firewall (Phase 24).

use machina_core::{gather_cloud_inventory, profile_by_name, simulate_connectivity};
use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

use crate::config::ControllerConfig;

use super::gitops;
use super::inventory::{overview as firewall_overview, target_detail};

#[derive(Debug, Clone, Serialize)]
pub struct FirewallSiteRow {
    pub id: String,
    pub name: String,
    pub region: String,
    pub role: String,
    pub gitops_namespace: String,
    pub lockdown_enabled: bool,
    pub geo_fence: Option<String>,
    pub dr_pair: Option<String>,
    pub target_count: usize,
    pub critical_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MultisiteOverview {
    pub sites: Vec<FirewallSiteRow>,
    pub policy_conflicts: Vec<PolicyConflict>,
    pub compliance_rollup: SiteComplianceRollup,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyConflict {
    pub id: String,
    pub policy_name: String,
    pub sites: Vec<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SiteComplianceRollup {
    pub sites: Vec<SiteComplianceRow>,
    pub average_score: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SiteComplianceRow {
    pub site: String,
    pub targets: usize,
    pub critical: usize,
    pub grade: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FederatedExport {
    pub api_version: String,
    pub kind: String,
    pub sites: Vec<FederatedSiteBundle>,
    pub exported_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FederatedSiteBundle {
    pub site: String,
    pub namespace: String,
    pub policies: Vec<gitops::GitOpsPolicyDoc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DrTemplateBundle {
    pub primary_site: String,
    pub dr_site: String,
    pub profiles: Vec<DrProfilePair>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DrProfilePair {
    pub primary_profile: String,
    pub dr_profile: String,
    pub geo_fence: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SiteDriftReport {
    pub sites: Vec<SiteDriftRow>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SiteDriftRow {
    pub site: String,
    pub peer: String,
    pub drift_fields: Vec<String>,
    pub captured_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CrossSiteConnectivity {
    pub sites: Vec<String>,
    pub matrix: Vec<CrossSiteHop>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CrossSiteHop {
    pub from_site: String,
    pub to_site: String,
    pub allowed: bool,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct MultisiteSyncRequest {
    pub source_site: String,
    pub target_site: String,
    #[serde(default)]
    pub include_lockdown: bool,
    #[serde(default)]
    pub apply_profiles: bool,
    #[serde(default = "default_lockdown_profile")]
    pub lockdown_profile: String,
}

fn default_lockdown_profile() -> String {
    "MetalLockdown".into()
}

#[derive(Debug, Clone, Serialize)]
pub struct MultisiteSyncResult {
    pub synced_policies: usize,
    pub lockdown_applied: bool,
    pub hosts_applied: usize,
    pub apply_errors: Vec<String>,
    pub summary: String,
}

pub async fn ensure_default_sites(pool: &DbPool) -> anyhow::Result<()> {
    let count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM firewall_sites")
        .fetch_one(pool)
        .await?;
    if count == 0 {
        crate::db::query(
            "INSERT INTO firewall_sites (id, name, region, role, gitops_namespace, dr_pair) VALUES (?, 'primary-local', 'local', 'primary', 'site-primary', 'dr-replica'),
             (?, 'dr-replica', 'dr', 'replica', 'site-dr', 'primary-local')",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(uuid::Uuid::new_v4())
        .execute(pool)
        .await?;
    }

    let policy_count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM firewall_site_policies")
        .fetch_one(pool)
        .await?;
    if policy_count == 0 {
        let primary_id: Uuid =
            crate::db::query_scalar("SELECT id FROM firewall_sites WHERE name = 'primary-local'")
                .fetch_one(pool)
                .await?;
        let dr_id: Uuid =
            crate::db::query_scalar("SELECT id FROM firewall_sites WHERE name = 'dr-replica'")
                .fetch_one(pool)
                .await?;
        for (site_id, name, profile) in [
            (primary_id, "fleet-production", "ProductionServer"),
            (primary_id, "fleet-public", "WebServer"),
            (dr_id, "fleet-dr-standby", "MetalLockdown"),
        ] {
            crate::db::query(
                "INSERT INTO firewall_site_policies (id, site_id, policy_name, profile, spec_yaml)
                 VALUES (?, ?, ?, ?, ?)
                 ON CONFLICT (site_id, policy_name) DO NOTHING",
            )
            .bind(uuid::Uuid::new_v4())
            .bind(site_id)
            .bind(name)
            .bind(profile)
            .bind(format!(
                "apiVersion: zeus.machina/v1\nkind: MachineFirewallPolicy\nmetadata:\n  name: {name}\nspec:\n  profile: {profile}\n"
            ))
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

pub async fn overview(
    pool: &DbPool,
    cfg: &ControllerConfig,
) -> anyhow::Result<MultisiteOverview> {
    ensure_default_sites(pool).await?;
    let ov = firewall_overview(pool, cfg).await?;
    let rows: Vec<(
        Uuid,
        String,
        String,
        String,
        String,
        bool,
        Option<String>,
        Option<String>,
    )> = crate::db::query_as(
        "SELECT id, name, region, role, gitops_namespace, lockdown_enabled, geo_fence, dr_pair
             FROM firewall_sites ORDER BY name",
    )
    .fetch_all(pool)
    .await?;

    let per_site = (ov.targets.len() / rows.len().max(1)).max(1);
    let critical_each = ov.critical_count / rows.len().max(1);

    let sites: Vec<FirewallSiteRow> = rows
        .into_iter()
        .map(
            |(id, name, region, role, ns, lock, geo, dr)| FirewallSiteRow {
                id: id.to_string(),
                name: name.clone(),
                region,
                role,
                gitops_namespace: ns,
                lockdown_enabled: lock,
                geo_fence: geo,
                dr_pair: dr,
                target_count: per_site,
                critical_count: critical_each,
            },
        )
        .collect();

    let policy_conflicts = detect_policy_conflicts(pool).await?;
    let compliance_rollup = site_compliance_rollup(&sites);

    let summary = format!(
        "{} site(s) · {} policy conflict(s) · fleet {} critical (simulated per site)",
        sites.len(),
        policy_conflicts.len(),
        ov.critical_count
    );

    Ok(MultisiteOverview {
        sites,
        policy_conflicts,
        compliance_rollup,
        summary,
    })
}

fn site_compliance_rollup(sites: &[FirewallSiteRow]) -> SiteComplianceRollup {
    let mut rows = Vec::new();
    let mut total_score = 0u32;
    for s in sites {
        let grade = if s.critical_count > 0 {
            "C"
        } else if s.lockdown_enabled {
            "A"
        } else {
            "B"
        };
        let score = match grade {
            "A" => 92,
            "B" => 78,
            _ => 62,
        };
        total_score += score;
        rows.push(SiteComplianceRow {
            site: s.name.clone(),
            targets: s.target_count,
            critical: s.critical_count,
            grade: grade.into(),
        });
    }
    SiteComplianceRollup {
        average_score: if sites.is_empty() {
            0
        } else {
            total_score / sites.len() as u32
        },
        sites: rows,
    }
}

async fn detect_policy_conflicts(pool: &DbPool) -> anyhow::Result<Vec<PolicyConflict>> {
    let rows: Vec<(String, String, String)> = crate::db::query_as(
        "SELECT s.name, p.policy_name, p.profile FROM firewall_site_policies p
         JOIN firewall_sites s ON s.id = p.site_id
         ORDER BY p.policy_name, s.name",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    if rows.is_empty() {
        let global: Vec<(String, String)> =
            crate::db::query_as("SELECT name, spec_yaml FROM firewall_policies ORDER BY name LIMIT 20")
                .fetch_all(pool)
                .await?;
        if global.len() > 1 {
            return Ok(vec![PolicyConflict {
                id: "global-divergence".into(),
                policy_name: global[0].0.clone(),
                sites: vec!["primary-local".into(), "dr-replica".into()],
                detail: "Global policies may diverge across sites without site-scoped GitOps"
                    .into(),
            }]);
        }
        return Ok(vec![]);
    }

    let mut by_name: std::collections::HashMap<String, Vec<(String, String)>> =
        std::collections::HashMap::new();
    for (site, name, profile) in rows {
        by_name.entry(name).or_default().push((site, profile));
    }
    let mut conflicts = Vec::new();
    for (name, entries) in by_name {
        let profiles: std::collections::HashSet<_> =
            entries.iter().map(|(_, p)| p.clone()).collect();
        if profiles.len() > 1 {
            conflicts.push(PolicyConflict {
                id: format!("conflict-{name}"),
                policy_name: name,
                sites: entries.iter().map(|(s, _)| s.clone()).collect(),
                detail: format!("Profile mismatch across {} site(s)", entries.len()),
            });
        }
    }
    Ok(conflicts)
}

pub async fn federated_export(pool: &DbPool) -> anyhow::Result<FederatedExport> {
    ensure_default_sites(pool).await?;
    let export = gitops::export_policies(pool).await?;
    let sites: Vec<(String, String)> =
        crate::db::query_as("SELECT name, gitops_namespace FROM firewall_sites ORDER BY name")
            .fetch_all(pool)
            .await?;

    let bundles: Vec<FederatedSiteBundle> = sites
        .into_iter()
        .map(|(site, ns)| FederatedSiteBundle {
            site: site.clone(),
            namespace: ns.clone(),
            policies: export
                .policies
                .iter()
                .map(|p| gitops::GitOpsPolicyDoc {
                    name: format!("{ns}/{}", p.name),
                    spec_yaml: p.spec_yaml.clone(),
                })
                .collect(),
        })
        .collect();

    Ok(FederatedExport {
        api_version: "zeus.machina/v1".into(),
        kind: "FederatedFirewallPolicyBundle".into(),
        sites: bundles,
        exported_at: chrono::Utc::now().to_rfc3339(),
    })
}

pub async fn dr_template_bundle(pool: &DbPool) -> anyhow::Result<DrTemplateBundle> {
    ensure_default_sites(pool).await?;
    Ok(DrTemplateBundle {
        primary_site: "primary-local".into(),
        dr_site: "dr-replica".into(),
        profiles: vec![
            DrProfilePair {
                primary_profile: "ProductionServer".into(),
                dr_profile: "MetalLockdown".into(),
                geo_fence: Some("geo:primary-only".into()),
            },
            DrProfilePair {
                primary_profile: "Public".into(),
                dr_profile: "Emergency".into(),
                geo_fence: Some("geo:dr-standby".into()),
            },
        ],
        summary: "Primary/DR profile pairs with geo-fenced allow stubs".into(),
    })
}

pub async fn cross_site_sync(
    pool: &DbPool,
    cfg: &ControllerConfig,
    req: MultisiteSyncRequest,
    actor: &str,
) -> anyhow::Result<MultisiteSyncResult> {
    ensure_default_sites(pool).await?;
    let source_id: Uuid = crate::db::query_scalar("SELECT id FROM firewall_sites WHERE name = ?")
        .bind(&req.source_site)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("unknown source site"))?;
    let target_id: Uuid = crate::db::query_scalar("SELECT id FROM firewall_sites WHERE name = ?")
        .bind(&req.target_site)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("unknown target site"))?;

    let policies: Vec<(String, String, String)> = crate::db::query_as(
        "SELECT policy_name, spec_yaml, profile FROM firewall_site_policies WHERE site_id = ?",
    )
    .bind(source_id)
    .fetch_all(pool)
    .await?;

    let mut synced = 0usize;
    let mut profiles_to_apply: Vec<String> = Vec::new();
    for (name, yaml, profile) in policies {
        crate::db::query(
            "INSERT INTO firewall_site_policies (id, site_id, policy_name, profile, spec_yaml)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (site_id, policy_name) DO UPDATE SET
               spec_yaml = EXCLUDED.spec_yaml,
               profile = EXCLUDED.profile,
               updated_at = datetime('now')",
        )
        .bind(Uuid::new_v4())
        .bind(target_id)
        .bind(&name)
        .bind(&profile)
        .bind(&yaml)
        .execute(pool)
        .await?;
        synced += 1;
        if !profiles_to_apply.contains(&profile) {
            profiles_to_apply.push(profile);
        }
    }

    let lockdown_applied = if req.include_lockdown {
        crate::db::query("UPDATE firewall_sites SET lockdown_enabled = true WHERE id = ?")
            .bind(target_id)
            .execute(pool)
            .await?;
        if !profiles_to_apply.contains(&req.lockdown_profile) {
            profiles_to_apply.push(req.lockdown_profile.clone());
        }
        true
    } else {
        false
    };

    let mut hosts_applied = 0usize;
    let mut apply_errors = Vec::new();
    if req.apply_profiles || req.include_lockdown {
        // Scope host application to the target site only. Without the `site`
        // filter this pulled every online host in the whole fleet, so a sync
        // aimed at one site's lockdown/profile would get pushed onto hosts
        // belonging to every other site too — the same class of bug as an
        // update touching rows it doesn't own (see gitops.rs's source-scoped
        // replace/upsert for the reference pattern).
        let online_hosts: Vec<(Uuid,)> = crate::db::query_as(
            "SELECT id FROM hosts WHERE state = 'online' AND site = ? ORDER BY hostname LIMIT 200",
        )
        .bind(&req.target_site)
        .fetch_all(pool)
        .await?;

        if req.apply_profiles {
            if let Some(profile) = profiles_to_apply.first() {
                for (host_id,) in &online_hosts {
                    let hid = host_id.to_string();
                    match super::inventory::apply_profile(pool, cfg, &hid, profile, actor, false)
                        .await
                    {
                        Ok(_) => hosts_applied += 1,
                        Err(e) => apply_errors.push(format!("{hid} {profile}: {e}")),
                    }
                }
            }
        }

        if req.include_lockdown {
            for (host_id,) in &online_hosts {
                let hid = host_id.to_string();
                match super::inventory::apply_profile(
                    pool,
                    cfg,
                    &hid,
                    &req.lockdown_profile,
                    actor,
                    false,
                )
                .await
                {
                    Ok(_) => hosts_applied += 1,
                    Err(e) => apply_errors.push(format!("{hid} {}: {e}", req.lockdown_profile)),
                }
            }
        }
    }

    let detail = serde_json::json!({
        "source": req.source_site,
        "target": req.target_site,
        "synced": synced,
        "lockdown": lockdown_applied,
        "hosts_applied": hosts_applied,
        "apply_errors": apply_errors.len(),
    });
    crate::db::query(
        "INSERT INTO firewall_site_timeline (id, site_id, kind, detail_json, actor) VALUES (?, ?, 'sync', ?, ?)",
    )
    .bind(Uuid::new_v4())
    .bind(target_id)
    .bind(detail)
    .bind(actor)
    .execute(pool)
    .await?;

    Ok(MultisiteSyncResult {
        synced_policies: synced,
        lockdown_applied,
        hosts_applied,
        apply_errors: apply_errors.into_iter().take(10).collect(),
        summary: format!(
            "Synced {synced} policy(ies) from {} → {}{}",
            req.source_site,
            req.target_site,
            if hosts_applied > 0 {
                format!(" · applied to {hosts_applied} host/profile pair(s)")
            } else {
                String::new()
            }
        ),
    })
}

pub async fn site_drift_compare(pool: &DbPool) -> anyhow::Result<SiteDriftReport> {
    ensure_default_sites(pool).await?;
    let pairs: Vec<(String, Option<String>)> =
        crate::db::query_as("SELECT name, dr_pair FROM firewall_sites ORDER BY name")
            .fetch_all(pool)
            .await?;

    let mut rows = Vec::new();
    for (site, peer) in pairs {
        if let Some(peer_name) = peer {
            rows.push(SiteDriftRow {
                site: site.clone(),
                peer: peer_name.clone(),
                drift_fields: vec![
                    "profile_version".into(),
                    "geo_fence".into(),
                    "stretch_deny".into(),
                ],
                captured_at: chrono::Utc::now().to_rfc3339(),
            });
            let _ = crate::db::query(
                "INSERT INTO firewall_site_drift (id, site_id, peer_site_id, drift_json)
                 SELECT ?, s.id, p.id, ? FROM firewall_sites s
                 JOIN firewall_sites p ON p.name = ? WHERE s.name = ?",
            )
            .bind(Uuid::new_v4())
            .bind(serde_json::json!({"fields": ["profile_version", "geo_fence"]}))
            .bind(&peer_name)
            .bind(&site)
            .execute(pool)
            .await;
        }
    }

    Ok(SiteDriftReport {
        summary: format!("{} site pair(s) compared (stub)", rows.len()),
        sites: rows,
    })
}

pub async fn cross_site_connectivity(
    pool: &DbPool,
    cfg: &ControllerConfig,
) -> anyhow::Result<CrossSiteConnectivity> {
    ensure_default_sites(pool).await?;
    let site_names: Vec<String> =
        crate::db::query_scalar("SELECT name FROM firewall_sites ORDER BY name")
            .fetch_all(pool)
            .await?;

    let cloud = gather_cloud_inventory();
    let mut matrix = Vec::new();
    for (i, a) in site_names.iter().enumerate() {
        for (j, b) in site_names.iter().enumerate() {
            if i == j {
                continue;
            }
            let allowed = !cloud
                .security_groups
                .iter()
                .any(|r| r.source == "0.0.0.0/0" && a.contains("primary") && b.contains("dr"));
            matrix.push(CrossSiteHop {
                from_site: a.clone(),
                to_site: b.clone(),
                allowed,
                reason: if allowed {
                    "Stretch deny rules stub — cross-site allowed".into()
                } else {
                    "Public SG blocks stretch replication path (stub)".into()
                },
            });
        }
    }

    if let Ok(detail) = target_detail(pool, cfg, "local").await {
        if let Some(prof) = profile_by_name("ProductionServer") {
            let after_rules: Vec<machina_core::ZeusFirewallRule> = prof
                .rules
                .iter()
                .enumerate()
                .map(|(i, r)| machina_core::ZeusFirewallRule {
                    id: format!("xsite-{i}"),
                    direction: r.direction.clone(),
                    protocol: r.protocol.clone(),
                    ports: r.ports.clone(),
                    sources: r.sources.clone(),
                    targets: vec![],
                    action: r.action.clone(),
                    temporary: false,
                    expires_at: None,
                    description: Some(r.name.clone()),
                    scope: "site".into(),
                    backend_ref: None,
                })
                .collect();
            let _ = simulate_connectivity(&detail.inventory, &after_rules);
        }
    }

    Ok(CrossSiteConnectivity {
        sites: site_names.clone(),
        summary: format!("{} cross-site hop(s) simulated", matrix.len()),
        matrix,
    })
}

pub async fn federated_siem_tag(
    pool: &DbPool,
    hours: u32,
) -> anyhow::Result<serde_json::Value> {
    let sites: Vec<String> = crate::db::query_scalar("SELECT name FROM firewall_sites ORDER BY name")
        .fetch_all(pool)
        .await?;
    Ok(serde_json::json!({
        "tag": "multisite",
        "sites": sites,
        "hours": hours,
        "summary": "Federated SIEM export stub — events tagged by site namespace",
    }))
}

pub async fn merge_timeline(
    pool: &DbPool,
    limit: i64,
) -> anyhow::Result<Vec<serde_json::Value>> {
    // Clamp the client-supplied limit: SQLite treats a negative LIMIT as "no limit",
    // so an unclamped value (e.g. -1) would return the entire timeline unbounded.
    let limit = limit.clamp(1, 500);
    let rows: Vec<(
        String,
        String,
        serde_json::Value,
        chrono::DateTime<chrono::Utc>,
    )> = crate::db::query_as(
        "SELECT s.name, t.kind, t.detail_json, t.created_at
         FROM firewall_site_timeline t
         JOIN firewall_sites s ON s.id = t.site_id
         ORDER BY t.created_at DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(site, kind, detail, at)| {
            serde_json::json!({
                "site": site,
                "kind": kind,
                "detail": detail,
                "created_at": at.to_rfc3339(),
            })
        })
        .collect())
}
