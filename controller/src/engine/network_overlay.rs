// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// NSX-class overlay segments — IPAM, micro-segmentation compiler, GitOps export.

use machina_core::{
    compile_micro_segment_rules, compute_firewall_score, ip_from_cidr_offset,
    segment_micro_seg_grade, simulate_connectivity, validate_cidr, ConnectivityMatrix,
    EastWestDefault, FirewallInventory, FirewallPosture, SegmentTier,
};
use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct SegmentRow {
    pub id: Uuid,
    pub name: String,
    pub tier: String,
    pub cidr: String,
    pub east_west_default: String,
    pub firewall_profile: Option<String>,
    pub gitops_namespace: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SegmentOverviewItem {
    pub id: String,
    pub name: String,
    pub tier: String,
    pub cidr: String,
    pub east_west_default: String,
    pub firewall_profile: Option<String>,
    pub gitops_namespace: String,
    pub network_count: usize,
    pub vm_count: usize,
    pub micro_seg_grade: String,
    pub micro_seg_score: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SegmentsOverview {
    pub segments: Vec<SegmentOverviewItem>,
    pub summary: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateSegmentRequest {
    pub name: String,
    pub tier: String,
    pub cidr: String,
    #[serde(default = "default_east_west")]
    pub east_west_default: String,
    pub firewall_profile: Option<String>,
    #[serde(default = "default_gitops_ns")]
    pub gitops_namespace: String,
    #[serde(default = "default_create_pool")]
    pub create_ipam_pool: bool,
}

fn default_east_west() -> String {
    "allow".into()
}

fn default_gitops_ns() -> String {
    "network-segments".into()
}

fn default_create_pool() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct IpamAllocation {
    pub reservation_id: String,
    pub pool_id: String,
    pub ip_address: String,
    pub hostname: Option<String>,
    pub segment_id: String,
    pub segment_name: String,
}

#[derive(Debug, Deserialize)]
pub struct IpamAllocateRequest {
    pub pool_id: Option<Uuid>,
    pub network_id: Option<Uuid>,
    pub hostname: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SegmentGitOpsExport {
    pub api_version: String,
    pub kind: String,
    pub namespace: String,
    pub segments: Vec<SegmentGitOpsDoc>,
    pub exported_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SegmentGitOpsDoc {
    pub name: String,
    pub spec_yaml: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SegmentConnectivityResult {
    pub segment_id: String,
    pub segment_name: String,
    pub vm_count: usize,
    pub rules: usize,
    pub matrix: ConnectivityMatrix,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmergencyUnlockResult {
    pub segment_id: String,
    pub segment_name: String,
    pub previous_east_west: String,
    pub unlocked: bool,
    pub summary: String,
}

pub async fn segments_overview(pool: &DbPool) -> anyhow::Result<SegmentsOverview> {
    let rows: Vec<SegmentRow> = crate::db::query_as(
        "SELECT id, name, tier, cidr, east_west_default, firewall_profile, gitops_namespace
         FROM network_segments ORDER BY tier, name",
    )
    .fetch_all(pool)
    .await?;

    let mut segments = Vec::new();
    for row in rows {
        let network_count: i64 =
            crate::db::query_scalar("SELECT COUNT(*) FROM networks WHERE segment_id = ?")
                .bind(row.id)
                .fetch_one(pool)
                .await?;

        let vm_count: i64 = crate::db::query_scalar(
            "SELECT COUNT(DISTINCT nr.vm_id) FROM network_reservations nr
             JOIN networks n ON n.id = nr.network_id
             WHERE n.segment_id = ? AND nr.vm_id IS NOT NULL",
        )
        .bind(row.id)
        .fetch_one(pool)
        .await?;

        let (grade, score) = segment_micro_seg_grade(
            &row.tier,
            &row.east_west_default,
            row.firewall_profile.as_deref(),
            vm_count as usize,
        );

        segments.push(SegmentOverviewItem {
            id: row.id.to_string(),
            name: row.name,
            tier: row.tier,
            cidr: row.cidr,
            east_west_default: row.east_west_default,
            firewall_profile: row.firewall_profile,
            gitops_namespace: row.gitops_namespace,
            network_count: network_count as usize,
            vm_count: vm_count as usize,
            micro_seg_grade: grade,
            micro_seg_score: score,
        });
    }

    let summary = format!(
        "{} segment(s) · {} VM(s) on overlay segments",
        segments.len(),
        segments.iter().map(|s| s.vm_count).sum::<usize>()
    );

    Ok(SegmentsOverview { segments, summary })
}

pub async fn create_segment(
    pool: &DbPool,
    req: &CreateSegmentRequest,
) -> anyhow::Result<SegmentRow> {
    validate_cidr(&req.cidr).map_err(|e| anyhow::anyhow!(e))?;
    if SegmentTier::parse(&req.tier).is_none() {
        anyhow::bail!("tier must be tier0 or tier1");
    }
    machina_spec::validate_name(&req.name)?;

    let id = Uuid::new_v4();
    let mut tx = pool.begin().await?;
    crate::db::query(
        "INSERT INTO network_segments
         (id, name, tier, cidr, east_west_default, firewall_profile, gitops_namespace)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(req.name.trim())
    .bind(req.tier.to_lowercase())
    .bind(req.cidr.trim())
    .bind(EastWestDefault::parse(&req.east_west_default).as_str())
    .bind(&req.firewall_profile)
    .bind(req.gitops_namespace.trim())
    .execute(&mut *tx)
    .await?;

    if req.create_ipam_pool {
        let pool_id = Uuid::new_v4();
        let gateway = ip_from_cidr_offset(&req.cidr, 1).ok();
        crate::db::query(
            "INSERT INTO network_ipam_pools (id, segment_id, cidr, gateway, dns_json, next_offset)
             VALUES (?, ?, ?, ?, ?, 10)",
        )
        .bind(pool_id)
        .bind(id)
        .bind(req.cidr.trim())
        .bind(gateway)
        .bind(serde_json::json!([]))
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    fetch_segment(pool, id).await
}

pub async fn bind_network(
    pool: &DbPool,
    network_id: Uuid,
    segment_id: Uuid,
) -> anyhow::Result<()> {
    let exists: Option<Uuid> = crate::db::query_scalar("SELECT id FROM network_segments WHERE id = ?")
        .bind(segment_id)
        .fetch_optional(pool)
        .await?;
    if exists.is_none() {
        anyhow::bail!("segment not found");
    }
    let r = crate::db::query("UPDATE networks SET segment_id = ? WHERE id = ?")
        .bind(segment_id)
        .bind(network_id)
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        anyhow::bail!("network not found");
    }
    Ok(())
}

pub async fn ipam_allocate(
    pool: &DbPool,
    segment_id: Uuid,
    req: &IpamAllocateRequest,
) -> anyhow::Result<IpamAllocation> {
    let segment = fetch_segment(pool, segment_id).await?;

    let pool_row: (Uuid, String, i32) = if let Some(pid) = req.pool_id {
        crate::db::query_as(
            "SELECT id, cidr, next_offset FROM network_ipam_pools WHERE id = ? AND segment_id = ?",
        )
        .bind(pid)
        .bind(segment_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("IPAM pool not found for segment"))?
    } else {
        crate::db::query_as(
            "SELECT id, cidr, next_offset FROM network_ipam_pools WHERE segment_id = ? ORDER BY created_at LIMIT 1",
        )
        .bind(segment_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("no IPAM pool for segment"))?
    };

    let (pool_id, cidr, offset) = pool_row;
    let ip = ip_from_cidr_offset(&cidr, offset as u32).map_err(|e| anyhow::anyhow!(e))?;
    let next = offset + 1;

    let network_id = if let Some(nid) = req.network_id {
        nid
    } else {
        let nid: Option<Uuid> = crate::db::query_scalar(
            "SELECT id FROM networks WHERE segment_id = ? ORDER BY name LIMIT 1",
        )
        .bind(segment_id)
        .fetch_optional(pool)
        .await?;
        nid.ok_or_else(|| anyhow::anyhow!("bind a network to segment before IPAM allocate"))?
    };

    let reservation_id = Uuid::new_v4();
    let mut tx = pool.begin().await?;
    crate::db::query("UPDATE network_ipam_pools SET next_offset = ? WHERE id = ?")
        .bind(next)
        .bind(pool_id)
        .execute(&mut *tx)
        .await?;
    crate::db::query(
        "INSERT INTO network_reservations (id, network_id, ip_address, pool_id, hostname)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(reservation_id)
    .bind(network_id)
    .bind(&ip)
    .bind(pool_id)
    .bind(&req.hostname)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(IpamAllocation {
        reservation_id: reservation_id.to_string(),
        pool_id: pool_id.to_string(),
        ip_address: ip,
        hostname: req.hostname.clone(),
        segment_id: segment_id.to_string(),
        segment_name: segment.name,
    })
}

pub async fn segment_connectivity(
    pool: &DbPool,
    segment_id: Uuid,
) -> anyhow::Result<SegmentConnectivityResult> {
    let segment = fetch_segment(pool, segment_id).await?;
    let east_west = EastWestDefault::parse(&segment.east_west_default);
    let rules = compile_micro_segment_rules(
        &segment.name,
        &segment.cidr,
        east_west,
        segment.firewall_profile.as_deref(),
    );

    let vm_count: i64 = crate::db::query_scalar(
        "SELECT COUNT(DISTINCT nr.vm_id) FROM network_reservations nr
         JOIN networks n ON n.id = nr.network_id
         WHERE n.segment_id = ? AND nr.vm_id IS NOT NULL",
    )
    .bind(segment_id)
    .fetch_one(pool)
    .await?;

    let inventory = stub_segment_inventory(&segment);
    let matrix = simulate_connectivity(&inventory, &rules);

    Ok(SegmentConnectivityResult {
        segment_id: segment_id.to_string(),
        segment_name: segment.name,
        vm_count: vm_count as usize,
        rules: rules.len(),
        matrix,
    })
}

pub async fn export_gitops(pool: &DbPool) -> anyhow::Result<SegmentGitOpsExport> {
    let rows: Vec<SegmentRow> = crate::db::query_as(
        "SELECT id, name, tier, cidr, east_west_default, firewall_profile, gitops_namespace
         FROM network_segments ORDER BY name",
    )
    .fetch_all(pool)
    .await?;

    let segments: Vec<SegmentGitOpsDoc> = rows
        .into_iter()
        .map(|r| {
            let yaml = format!(
                "apiVersion: zeus.machina/v1\nkind: NetworkSegment\nmetadata:\n  name: {}\n  namespace: {}\nspec:\n  tier: {}\n  cidr: {}\n  eastWestDefault: {}\n  firewallProfile: {}\n",
                r.name,
                r.gitops_namespace,
                r.tier,
                r.cidr,
                r.east_west_default,
                r.firewall_profile.unwrap_or_default()
            );
            SegmentGitOpsDoc {
                name: r.name,
                spec_yaml: yaml,
            }
        })
        .collect();

    let namespace = segments
        .first()
        .map(|_| "network-segments".to_string())
        .unwrap_or_else(|| "network-segments".into());

    Ok(SegmentGitOpsExport {
        api_version: "zeus.machina/v1".into(),
        kind: "NetworkSegmentBundle".into(),
        namespace,
        segments,
        exported_at: chrono::Utc::now().to_rfc3339(),
    })
}

pub async fn emergency_unlock(
    pool: &DbPool,
    segment_id: Uuid,
) -> anyhow::Result<EmergencyUnlockResult> {
    let segment = fetch_segment(pool, segment_id).await?;
    let previous = segment.east_west_default.clone();
    crate::db::query("UPDATE network_segments SET east_west_default = 'allow' WHERE id = ?")
        .bind(segment_id)
        .execute(pool)
        .await?;

    Ok(EmergencyUnlockResult {
        segment_id: segment_id.to_string(),
        segment_name: segment.name,
        previous_east_west: previous,
        unlocked: true,
        summary: "Emergency unlock stub — east-west set to allow (no dataplane apply)".into(),
    })
}

pub async fn list_ipam_pools(pool: &DbPool) -> anyhow::Result<Vec<IpamPoolRow>> {
    let rows = crate::db::query_as(
        "SELECT p.id, p.segment_id, s.name AS segment_name, p.cidr, p.gateway, p.next_offset,
                (SELECT COUNT(*) FROM network_reservations r WHERE r.pool_id = p.id) AS reservation_count
         FROM network_ipam_pools p
         JOIN network_segments s ON s.id = p.segment_id
         ORDER BY s.name, p.cidr",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct IpamPoolRow {
    pub id: Uuid,
    pub segment_id: Uuid,
    pub segment_name: String,
    pub cidr: String,
    pub gateway: Option<String>,
    pub next_offset: i32,
    pub reservation_count: i64,
}

async fn fetch_segment(pool: &DbPool, id: Uuid) -> anyhow::Result<SegmentRow> {
    crate::db::query_as(
        "SELECT id, name, tier, cidr, east_west_default, firewall_profile, gitops_namespace
         FROM network_segments WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.into())
}

fn stub_segment_inventory(segment: &SegmentRow) -> FirewallInventory {
    let posture = FirewallPosture {
        enabled: true,
        backend: machina_core::FirewallBackend::Nftables,
        profile: segment.firewall_profile.clone(),
        stealth_level: machina_core::StealthLevel::Standard,
        default_inbound: Some(
            if EastWestDefault::parse(&segment.east_west_default) == EastWestDefault::Deny {
                "deny".into()
            } else {
                "allow".into()
            },
        ),
        default_outbound: Some("allow".into()),
        backend_zone: None,
        status_line: Some(format!("segment overlay {}", segment.name)),
        drift_detected: false,
        last_changed: None,
    };
    let score = compute_firewall_score(&posture, &[], &[]);
    FirewallInventory {
        hostname: segment.name.clone(),
        posture,
        rules: vec![],
        open_ports: vec![],
        services: vec![],
        score,
        profiles_available: vec![],
        nftables_summary: None,
        activity: None,
    }
}

pub async fn fetch_host_lldp(
    agent_console_addr: &str,
) -> anyhow::Result<machina_core::libvirt::host_network::LldpInventory> {
    crate::agent_client::get_lldp(agent_console_addr).await
}

#[derive(Debug, Clone, Serialize)]
pub struct LldpTopologyContribution {
    pub nodes: Vec<LldpTopologyNode>,
    pub edges: Vec<LldpTopologyEdge>,
    pub warnings: Vec<LldpTopologyWarning>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LldpTopologyNode {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub state: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LldpTopologyEdge {
    pub from: String,
    pub to: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LldpTopologyWarning {
    pub severity: String,
    pub message: String,
}

fn switch_node_id(key: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut h);
    format!("switch-{:x}", h.finish())
}

fn switch_key(neighbor: &machina_core::libvirt::host_network::LldpNeighbor) -> String {
    if !neighbor.system_name.is_empty() {
        neighbor.system_name.clone()
    } else if !neighbor.chassis_id.is_empty() {
        format!("chassis:{}", neighbor.chassis_id)
    } else {
        format!("iface:{}", neighbor.local_interface)
    }
}

pub async fn refresh_lldp_cache(pool: &DbPool, max_age_secs: i64) -> anyhow::Result<usize> {
    let hosts: Vec<(Uuid, String, String)> = crate::db::query_as(
        "SELECT id, hostname, COALESCE(NULLIF(agent_console_addr, ''), agent_grpc_addr)
         FROM hosts WHERE state = 'online' ORDER BY hostname",
    )
    .fetch_all(pool)
    .await?;

    let mut refreshed = 0usize;
    for (host_id, hostname, addr) in hosts {
        if addr.is_empty() {
            continue;
        }
        let fetched_at: Option<chrono::DateTime<chrono::Utc>> =
            crate::db::query_scalar("SELECT fetched_at FROM host_lldp_cache WHERE host_id = ?")
                .bind(host_id)
                .fetch_optional(pool)
                .await?;
        if let Some(ts) = fetched_at {
            if (chrono::Utc::now() - ts).num_seconds() < max_age_secs {
                continue;
            }
        }

        match fetch_host_lldp(&addr).await {
            Ok(lldp) => {
                let neighbors_json = serde_json::to_value(&lldp.neighbors)?;
                crate::db::query(
                    "INSERT INTO host_lldp_cache (host_id, source, neighbors_json, summary, fetched_at)
                     VALUES (?, ?, ?, ?, datetime('now'))
                     ON CONFLICT (host_id) DO UPDATE SET
                       source = EXCLUDED.source,
                       neighbors_json = EXCLUDED.neighbors_json,
                       summary = EXCLUDED.summary,
                       fetched_at = datetime('now')",
                )
                .bind(host_id)
                .bind(&lldp.source)
                .bind(neighbors_json)
                .bind(&lldp.summary)
                .execute(pool)
                .await?;
                refreshed += 1;
            }
            Err(e) => {
                tracing::debug!("LLDP refresh failed for {hostname}: {e}");
            }
        }
    }
    Ok(refreshed)
}

pub async fn lldp_topology_from_cache(
    pool: &DbPool,
) -> anyhow::Result<LldpTopologyContribution> {
    let _ = refresh_lldp_cache(pool, 300).await;

    let rows: Vec<(Uuid, String, String, serde_json::Value, String)> = crate::db::query_as(
        "SELECT c.host_id, h.hostname, c.source, c.neighbors_json, c.summary
         FROM host_lldp_cache c
         JOIN hosts h ON h.id = c.host_id
         ORDER BY h.hostname",
    )
    .fetch_all(pool)
    .await?;

    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut warnings = Vec::new();
    let mut switch_ids: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for (host_id, hostname, source, neighbors_json, summary) in rows {
        let neighbors: Vec<machina_core::libvirt::host_network::LldpNeighbor> =
            serde_json::from_value(neighbors_json).unwrap_or_default();
        for neighbor in &neighbors {
            let key = switch_key(neighbor);
            let switch_id = switch_ids
                .entry(key.clone())
                .or_insert_with(|| {
                    let id = switch_node_id(&key);
                    let name = if neighbor.system_name.is_empty() {
                        neighbor.chassis_id.clone()
                    } else {
                        neighbor.system_name.clone()
                    };
                    nodes.push(LldpTopologyNode {
                        kind: "switch".into(),
                        id: id.clone(),
                        name: if name.is_empty() {
                            "switch".into()
                        } else {
                            name
                        },
                        state: Some(source.clone()),
                    });
                    id
                })
                .clone();
            edges.push(LldpTopologyEdge {
                from: host_id.to_string(),
                to: switch_id,
                label: "uplink".into(),
            });
        }
        if neighbors.is_empty() && !summary.is_empty() {
            warnings.push(LldpTopologyWarning {
                severity: "info".into(),
                message: format!("{hostname}: {summary}"),
            });
        }
    }

    Ok(LldpTopologyContribution {
        nodes,
        edges,
        warnings,
    })
}
