// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwinNode {
    pub kind: String,
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwinEdge {
    pub from: String,
    pub to: String,
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct DigitalTwinGraph {
    pub nodes: Vec<TwinNode>,
    pub edges: Vec<TwinEdge>,
    pub node_count: usize,
    pub edge_count: usize,
}

#[derive(Debug, Deserialize)]
pub struct ImpactRequest {
    pub action: String,
    pub target_kind: String,
    pub target_id: String,
}

#[derive(Debug, Serialize)]
pub struct ImpactAnalysis {
    pub action: String,
    pub target: String,
    pub severity: String,
    pub summary: String,
    pub affected_vms: Vec<String>,
    pub affected_applications: Vec<String>,
    pub storage_risks: Vec<String>,
    pub network_notes: Vec<String>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SimulatedImpact {
    #[serde(flatten)]
    pub impact: ImpactAnalysis,
    pub estimated_downtime_sec: i64,
    pub vms_at_risk: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_unavailable_gib: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct SimulateRequest {
    pub scenarios: Vec<ImpactRequest>,
}

#[derive(Debug, Serialize)]
pub struct SimulateResult {
    pub results: Vec<SimulatedImpact>,
}

pub async fn build_graph(pool: &DbPool) -> anyhow::Result<DigitalTwinGraph> {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    let cluster_name: String = crate::db::query_scalar("SELECT name FROM clusters LIMIT 1")
        .fetch_one(pool)
        .await
        .unwrap_or_else(|_| "default".into());
    nodes.push(TwinNode {
        kind: "cluster".into(),
        id: "cluster".into(),
        name: cluster_name,
        state: None,
    });

    let hosts: Vec<(Uuid, String, String)> =
        crate::db::query_as("SELECT id, hostname, state FROM hosts ORDER BY hostname")
            .fetch_all(pool)
            .await?;
    for (hid, name, st) in &hosts {
        let id = hid.to_string();
        nodes.push(TwinNode {
            kind: "host".into(),
            id: id.clone(),
            name: name.clone(),
            state: Some(st.clone()),
        });
        edges.push(TwinEdge {
            from: "cluster".into(),
            to: id,
            label: "contains".into(),
        });
    }

    let vms: Vec<(Uuid, String, Option<Uuid>, String)> =
        crate::db::query_as("SELECT id, name, host_id, observed_state FROM vms ORDER BY name LIMIT 200")
            .fetch_all(pool)
            .await?;
    for (vid, name, host_id, st) in vms {
        let id = vid.to_string();
        nodes.push(TwinNode {
            kind: "vm".into(),
            id: id.clone(),
            name,
            state: Some(st),
        });
        if let Some(h) = host_id {
            edges.push(TwinEdge {
                from: h.to_string(),
                to: id,
                label: "runs".into(),
            });
        }
    }

    let pools: Vec<(Uuid, String)> =
        crate::db::query_as("SELECT id, name FROM storage_pools ORDER BY name")
            .fetch_all(pool)
            .await?;
    for (pid, name) in pools {
        let id = format!("storage-{pid}");
        nodes.push(TwinNode {
            kind: "storage".into(),
            id: id.clone(),
            name,
            state: None,
        });
        edges.push(TwinEdge {
            from: "cluster".into(),
            to: id,
            label: "storage".into(),
        });
    }

    let networks: Vec<(Uuid, String)> =
        crate::db::query_as("SELECT id, name FROM networks ORDER BY name")
            .fetch_all(pool)
            .await?;
    for (nid, name) in networks {
        let id = format!("network-{nid}");
        nodes.push(TwinNode {
            kind: "network".into(),
            id: id.clone(),
            name,
            state: None,
        });
        edges.push(TwinEdge {
            from: "cluster".into(),
            to: id,
            label: "network".into(),
        });
    }

    let segments: Vec<(Uuid, String, String)> =
        crate::db::query_as("SELECT id, name, tier FROM network_segments ORDER BY name")
            .fetch_all(pool)
            .await
            .unwrap_or_default();
    for (sid, name, tier) in segments {
        let id = format!("segment-{sid}");
        nodes.push(TwinNode {
            kind: "segment".into(),
            id: id.clone(),
            name: format!("{name} ({tier})"),
            state: None,
        });
        edges.push(TwinEdge {
            from: "cluster".into(),
            to: id,
            label: "overlay".into(),
        });
    }

    if let Ok(lldp) = crate::engine::network_overlay::lldp_topology_from_cache(pool).await {
        for node in lldp.nodes {
            if !nodes.iter().any(|n| n.id == node.id) {
                nodes.push(TwinNode {
                    kind: node.kind,
                    id: node.id,
                    name: node.name,
                    state: node.state,
                });
            }
        }
        for edge in lldp.edges {
            edges.push(TwinEdge {
                from: edge.from,
                to: edge.to,
                label: edge.label,
            });
        }
    }

    let edge_count = edges.len();
    let node_count = nodes.len();
    Ok(DigitalTwinGraph {
        nodes,
        edges,
        node_count,
        edge_count,
    })
}

pub async fn analyze_impact(
    pool: &DbPool,
    req: &ImpactRequest,
) -> anyhow::Result<ImpactAnalysis> {
    let action = req.action.to_lowercase();
    let kind = req.target_kind.to_lowercase();

    match (action.as_str(), kind.as_str()) {
        ("shutdown", "host") | ("stop", "host") | ("maintenance", "host") => {
            host_shutdown_impact(pool, &req.target_id).await
        }
        ("migrate", "host") | ("evacuate", "host") => {
            host_migrate_impact(pool, &req.target_id).await
        }
        ("isolate", "network") | ("shutdown", "network") => {
            network_isolate_impact(pool, &req.target_id).await
        }
        ("isolate", "segment") | ("shutdown", "segment") => {
            segment_isolate_impact(pool, &req.target_id).await
        }
        ("isolate", "switch") | ("shutdown", "switch") => {
            switch_isolate_impact(pool, &req.target_id).await
        }
        ("shutdown", "vm") | ("stop", "vm") | ("delete", "vm") => {
            vm_shutdown_impact(pool, &req.target_id).await
        }
        ("shutdown", "storage") | ("drain", "storage") => {
            storage_shutdown_impact(pool, &req.target_id).await
        }
        _ => Ok(ImpactAnalysis {
            action: req.action.clone(),
            target: format!("{}:{}", req.target_kind, req.target_id),
            severity: "info".into(),
            summary: "Supported: host shutdown/migrate, network/segment isolate, vm shutdown, storage drain.".into(),
            affected_vms: vec![],
            affected_applications: vec![],
            storage_risks: vec![],
            network_notes: vec![],
            recommendations: vec![
                "Use target_kind host/vm/network/segment/storage with shutdown/migrate/isolate/drain.".into(),
            ],
        }),
    }
}

async fn storage_shutdown_impact(
    pool: &DbPool,
    target: &str,
) -> anyhow::Result<ImpactAnalysis> {
    let pool_id = resolve_storage(pool, target).await?;
    let (name, capacity_gib, used_gib): (String, i64, i64) =
        crate::db::query_as("SELECT name, capacity_gib, used_gib FROM storage_pools WHERE id = ?")
            .bind(pool_id)
            .fetch_one(pool)
            .await?;

    let vm_count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM vms")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    let pct = if capacity_gib > 0 {
        used_gib as f64 / capacity_gib as f64 * 100.0
    } else {
        0.0
    };

    let severity = if vm_count >= 10 {
        "critical"
    } else if vm_count > 0 {
        "high"
    } else {
        "low"
    };

    Ok(ImpactAnalysis {
        action: "shutdown".into(),
        target: format!("storage:{name}"),
        severity: severity.into(),
        summary: format!(
            "Draining storage pool {name} ({used_gib}/{capacity_gib} GiB, {pct:.0}% full) affects disk I/O for all attached VMs."
        ),
        affected_vms: vec![format!("{vm_count} VM(s) with disks")],
        affected_applications: vec![],
        storage_risks: vec![
            "All VMs with disks on this pool lose write path.".into(),
            "Snapshots and clones on this pool become unavailable.".into(),
        ],
        network_notes: vec![],
        recommendations: vec![
            "Migrate VM disks to alternate pool before maintenance.".into(),
            "Verify backup targets are not exclusively on this pool.".into(),
        ],
    })
}

async fn resolve_storage(pool: &DbPool, target: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(target) {
        return Ok(id);
    }
    let id: Option<Uuid> = crate::db::query_scalar("SELECT id FROM storage_pools WHERE name = ?")
        .bind(target)
        .fetch_optional(pool)
        .await?;
    id.ok_or_else(|| anyhow::anyhow!("storage pool not found: {target}"))
}

async fn host_shutdown_impact(pool: &DbPool, target: &str) -> anyhow::Result<ImpactAnalysis> {
    let host_id = resolve_host(pool, target).await?;
    let vms: Vec<(Uuid, String, String)> = crate::db::query_as(
        "SELECT id, name, observed_state FROM vms WHERE host_id = ? ORDER BY name LIMIT 500",
    )
    .bind(host_id)
    .fetch_all(pool)
    .await?;

    let vm_names: Vec<String> = vms.iter().map(|(_, n, _)| n.clone()).collect();
    let running: Vec<_> = vms.iter().filter(|(_, _, st)| st == "running").collect();

    let mut affected_apps = Vec::new();
    for (vid, name, _) in &vms {
        let apps: Vec<String> = crate::db::query_scalar(
            "SELECT ag.name FROM application_group_vms agv
             JOIN application_groups ag ON ag.id = agv.group_id
             WHERE agv.vm_id = ?",
        )
        .bind(vid)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
        for app in apps {
            if !affected_apps.contains(&app) {
                affected_apps.push(app);
            }
        }
        let _ = name;
    }

    let host_name: String = crate::db::query_scalar("SELECT hostname FROM hosts WHERE id = ?")
        .bind(host_id)
        .fetch_one(pool)
        .await?;

    let severity = if running.len() >= 3 {
        "critical"
    } else if running.is_empty() {
        "low"
    } else {
        "high"
    };

    let summary = if vm_names.is_empty() {
        format!("Host {host_name} has no VMs — minimal blast radius.")
    } else {
        format!(
            "Shutting down host {host_name} affects {} VM(s) ({} running).",
            vm_names.len(),
            running.len()
        )
    };

    let mut recommendations = Vec::new();
    if !running.is_empty() {
        recommendations.push("Live-migrate running VMs before host maintenance.".into());
    }
    if affected_apps.len() > 1 {
        recommendations.push("Application groups span this host — check anti-affinity.".into());
    }

    Ok(ImpactAnalysis {
        action: "shutdown".into(),
        target: format!("host:{host_name}"),
        severity: severity.into(),
        summary,
        affected_vms: vm_names,
        affected_applications: affected_apps,
        storage_risks: vec!["Shared storage pools remain online; verify multipath.".into()],
        network_notes: vec!["VMs on this host lose compute; bridge networks may show gaps.".into()],
        recommendations,
    })
}

async fn vm_shutdown_impact(pool: &DbPool, target: &str) -> anyhow::Result<ImpactAnalysis> {
    let vm_id = resolve_vm(pool, target).await?;
    let (name, host_id, state): (String, Option<Uuid>, String) =
        crate::db::query_as("SELECT name, host_id, observed_state FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_one(pool)
            .await?;

    let apps: Vec<String> = crate::db::query_scalar(
        "SELECT ag.name FROM application_group_vms agv
         JOIN application_groups ag ON ag.id = agv.group_id
         WHERE agv.vm_id = ?",
    )
    .bind(vm_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let host_name = if let Some(h) = host_id {
        crate::db::query_scalar::<_, String>("SELECT hostname FROM hosts WHERE id = ?")
            .bind(h)
            .fetch_optional(pool)
            .await?
            .unwrap_or_else(|| h.to_string())
    } else {
        "unplaced".into()
    };

    let severity = if apps.iter().any(|a| a.to_lowercase().contains("prod")) {
        "critical"
    } else if state == "running" {
        "high"
    } else {
        "medium"
    };

    Ok(ImpactAnalysis {
        action: "shutdown".into(),
        target: format!("vm:{name}"),
        severity: severity.into(),
        summary: format!("Stopping VM {name} on host {host_name} ({state})."),
        affected_vms: vec![name],
        affected_applications: apps,
        storage_risks: vec![],
        network_notes: vec!["Dependent services may lose connectivity to this VM.".into()],
        recommendations: vec![
            "Create backup before shutdown if data retention required.".into(),
            "Check application group dependencies.".into(),
        ],
    })
}

async fn host_migrate_impact(pool: &DbPool, target: &str) -> anyhow::Result<ImpactAnalysis> {
    let host_id = resolve_host(pool, target).await?;
    let host_name: String = crate::db::query_scalar("SELECT hostname FROM hosts WHERE id = ?")
        .bind(host_id)
        .fetch_one(pool)
        .await?;

    let vms: Vec<(Uuid, String, String)> = crate::db::query_as(
        "SELECT id, name, observed_state FROM vms WHERE host_id = ? ORDER BY name LIMIT 500",
    )
    .bind(host_id)
    .fetch_all(pool)
    .await?;

    let running: Vec<_> = vms.iter().filter(|(_, _, st)| st == "running").collect();
    let vm_names: Vec<String> = vms.iter().map(|(_, n, _)| n.clone()).collect();

    let recs = crate::engine::placement::compute_recommendations(pool)
        .await
        .unwrap_or_default();
    let dest_hosts: Vec<String> = recs
        .iter()
        .filter(|r| r.from_host_id == host_id.to_string())
        .map(|r| r.to_host_name.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .take(3)
        .collect();

    let severity = if running.len() >= 5 {
        "critical"
    } else if running.is_empty() {
        "low"
    } else {
        "high"
    };

    let summary = if vm_names.is_empty() {
        format!("Host {host_name} has no VMs — evacuation is trivial.")
    } else {
        format!(
            "Evacuating host {host_name} requires migrating {} VM(s) ({} running).",
            vm_names.len(),
            running.len()
        )
    };

    let mut recommendations = vec![
        "Run live migration during a maintenance window.".into(),
        "Verify shared storage and network reachability on destination hosts.".into(),
    ];
    if !dest_hosts.is_empty() {
        recommendations.push(format!(
            "Placement suggests targets: {}.",
            dest_hosts.join(", ")
        ));
    }

    Ok(ImpactAnalysis {
        action: "migrate".into(),
        target: format!("host:{host_name}"),
        severity: severity.into(),
        summary,
        affected_vms: vm_names,
        affected_applications: vec![],
        storage_risks: vec!["Ensure multipath and pool capacity on destination hosts.".into()],
        network_notes: vec!["Live migration preserves L2 connectivity on shared bridges.".into()],
        recommendations,
    })
}

async fn network_isolate_impact(pool: &DbPool, target: &str) -> anyhow::Result<ImpactAnalysis> {
    let network_id = resolve_network(pool, target).await?;
    let net_name: String = crate::db::query_scalar("SELECT name FROM networks WHERE id = ?")
        .bind(network_id)
        .fetch_one(pool)
        .await?;

    let vms: Vec<String> = crate::db::query_scalar(
        "SELECT DISTINCT v.name FROM network_reservations nr
         JOIN vms v ON v.id = nr.vm_id
         WHERE nr.network_id = ? ORDER BY v.name",
    )
    .bind(network_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let severity = if vms.len() >= 10 {
        "critical"
    } else if vms.is_empty() {
        "low"
    } else {
        "high"
    };

    let summary = if vms.is_empty() {
        format!("Network {net_name} has no attached VMs in reservations — low blast radius.")
    } else {
        format!(
            "Isolating network {net_name} disrupts connectivity for {} VM(s).",
            vms.len()
        )
    };

    Ok(ImpactAnalysis {
        action: "isolate".into(),
        target: format!("network:{net_name}"),
        severity: severity.into(),
        summary,
        affected_vms: vms,
        affected_applications: vec![],
        storage_risks: vec![],
        network_notes: vec![
            "East-west traffic on this segment stops; verify firewall and routing fallbacks."
                .into(),
            "Application groups sharing this network may split-brain.".into(),
        ],
        recommendations: vec![
            "Drain workloads to alternate networks before isolation.".into(),
            "Update security groups and load balancer backends.".into(),
        ],
    })
}

async fn segment_isolate_impact(pool: &DbPool, target: &str) -> anyhow::Result<ImpactAnalysis> {
    let segment_id = resolve_segment(pool, target).await?;
    let (name, east_west): (String, String) =
        crate::db::query_as("SELECT name, east_west_default FROM network_segments WHERE id = ?")
            .bind(segment_id)
            .fetch_one(pool)
            .await?;

    let vms: Vec<String> = crate::db::query_scalar(
        "SELECT DISTINCT v.name FROM network_reservations nr
         JOIN networks n ON n.id = nr.network_id
         JOIN vms v ON v.id = nr.vm_id
         WHERE n.segment_id = ? ORDER BY v.name",
    )
    .bind(segment_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let network_count: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM networks WHERE segment_id = ?")
            .bind(segment_id)
            .fetch_one(pool)
            .await?;

    let severity = if vms.len() >= 10 {
        "critical"
    } else if vms.is_empty() {
        "low"
    } else {
        "high"
    };

    let summary = if vms.is_empty() {
        format!(
            "Segment {name} ({network_count} network(s)) has no VM reservations — low blast radius."
        )
    } else {
        format!(
            "Isolating overlay segment {name} (east-west={east_west}) affects {} VM(s) across {network_count} network(s).",
            vms.len()
        )
    };

    Ok(ImpactAnalysis {
        action: "isolate".into(),
        target: format!("segment:{name}"),
        severity: severity.into(),
        summary,
        affected_vms: vms,
        affected_applications: vec![],
        storage_risks: vec![],
        network_notes: vec![
            "Micro-segmentation policy blocks east-west on this overlay (stub).".into(),
            "Verify Zeus firewall profile and emergency unlock before production isolation.".into(),
        ],
        recommendations: vec![
            "Run segment connectivity matrix before isolation.".into(),
            "Migrate workloads to alternate tier-1 segment if deny-all is enabled.".into(),
        ],
    })
}

async fn switch_isolate_impact(pool: &DbPool, target: &str) -> anyhow::Result<ImpactAnalysis> {
    let switch_id = target.strip_prefix("switch-").unwrap_or(target);
    // Escape LIKE wildcards in the user-supplied switch id: an unescaped `%`/`_`
    // (e.g. `switch-%`) would match every host's LLDP cache and report the entire
    // fleet as the blast radius ("critical"), or silently broaden the match.
    let escaped = switch_id
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let rows: Vec<(Uuid, String)> = crate::db::query_as(
        "SELECT c.host_id, h.hostname
         FROM host_lldp_cache c
         JOIN hosts h ON h.id = c.host_id
         WHERE c.neighbors_json LIKE ? ESCAPE '\\'
         ORDER BY h.hostname",
    )
    .bind(format!("%{escaped}%"))
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let hostnames: Vec<String> = rows.iter().map(|(_, name)| name.clone()).collect();
    let vms: Vec<String> = if rows.is_empty() {
        vec![]
    } else {
        let placeholders = std::iter::repeat_n("?", rows.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT name FROM vms WHERE host_id IN ({placeholders}) ORDER BY name LIMIT 50"
        );
        let mut q = crate::db::query_scalar::<_, String>(&sql);
        for (id, _) in &rows {
            q = q.bind(id);
        }
        q.fetch_all(pool).await.unwrap_or_default()
    };

    let severity = if vms.len() >= 10 {
        "critical"
    } else if hostnames.is_empty() {
        "low"
    } else {
        "high"
    };

    Ok(ImpactAnalysis {
        action: "isolate".into(),
        target: format!("switch:{target}"),
        severity: severity.into(),
        summary: if hostnames.is_empty() {
            format!("Switch {target} has no cached LLDP uplinks — verify topology refresh.")
        } else {
            format!(
                "Isolating switch uplink affects {} host(s) and {} VM(s).",
                hostnames.len(),
                vms.len()
            )
        },
        affected_vms: vms,
        affected_applications: vec![],
        storage_risks: vec![],
        network_notes: hostnames
            .iter()
            .map(|h| format!("Host {h} loses northbound LLDP uplink."))
            .collect(),
        recommendations: vec![
            "Confirm redundant uplinks before switch maintenance.".into(),
            "Refresh LLDP cache from Platform Topology.".into(),
        ],
    })
}

async fn resolve_segment(pool: &DbPool, target: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(target) {
        return Ok(id);
    }
    let id: Option<Uuid> = crate::db::query_scalar("SELECT id FROM network_segments WHERE name = ?")
        .bind(target)
        .fetch_optional(pool)
        .await?;
    id.ok_or_else(|| anyhow::anyhow!("segment not found: {target}"))
}

async fn resolve_network(pool: &DbPool, target: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(target) {
        return Ok(id);
    }
    let id: Option<Uuid> = crate::db::query_scalar("SELECT id FROM networks WHERE name = ?")
        .bind(target)
        .fetch_optional(pool)
        .await?;
    id.ok_or_else(|| anyhow::anyhow!("network not found: {target}"))
}

async fn resolve_host(pool: &DbPool, target: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(target) {
        return Ok(id);
    }
    let id: Option<Uuid> = crate::db::query_scalar("SELECT id FROM hosts WHERE hostname = ?")
        .bind(target)
        .fetch_optional(pool)
        .await?;
    id.ok_or_else(|| anyhow::anyhow!("host not found: {target}"))
}

async fn resolve_vm(pool: &DbPool, target: &str) -> anyhow::Result<Uuid> {
    if let Ok(id) = Uuid::parse_str(target) {
        return Ok(id);
    }
    let id: Option<Uuid> = crate::db::query_scalar("SELECT id FROM vms WHERE name = ?")
        .bind(target)
        .fetch_optional(pool)
        .await?;
    id.ok_or_else(|| anyhow::anyhow!("vm not found: {target}"))
}

pub async fn simulate_batch(
    pool: &DbPool,
    req: &SimulateRequest,
) -> anyhow::Result<SimulateResult> {
    let mut results = Vec::new();
    for scenario in &req.scenarios {
        let impact = analyze_impact(pool, scenario).await?;
        let vms_at_risk = impact.affected_vms.len() as i64;
        let estimated_downtime_sec = match scenario.action.as_str() {
            a if a.contains("migrate") => 120,
            a if a.contains("shutdown") || a.contains("failure") => 300,
            _ => 60,
        };
        let storage_unavailable_gib =
            if scenario.target_kind == "storage" || !impact.storage_risks.is_empty() {
                let used: i64 =
                    crate::db::query_scalar("SELECT COALESCE(SUM(used_gib), 0) FROM storage_pools")
                        .fetch_one(pool)
                        .await
                        .unwrap_or(0);
                Some(used)
            } else {
                None
            };
        results.push(SimulatedImpact {
            impact,
            estimated_downtime_sec,
            vms_at_risk,
            storage_unavailable_gib,
        });
    }
    Ok(SimulateResult { results })
}
