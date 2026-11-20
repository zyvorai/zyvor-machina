// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Unified Infrastructure Graph Brain — merges twin, topology, service, storage, network, backup edges.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

use super::digital_twin::DigitalTwinGraph;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub kind: String,
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_score: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct InfraGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub node_count: usize,
    pub edge_count: usize,
}

#[derive(Debug, Deserialize)]
pub struct GraphScope {
    pub host_id: Option<Uuid>,
    pub vm_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct PathRequest {
    pub from: String,
    pub to: String,
    pub port: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct PathEvidence {
    pub source: String,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct PathBlocker {
    pub kind: String,
    pub message: String,
    pub remediation: String,
}

#[derive(Debug, Serialize)]
pub struct PathResult {
    pub can_reach: bool,
    pub explanation: String,
    pub hops: Vec<String>,
    pub blockers: Vec<PathBlocker>,
    pub confidence: f64,
    pub evidence: Vec<PathEvidence>,
}

#[derive(Debug, Deserialize)]
pub struct GraphQueryRequest {
    pub query: String,
}

#[derive(Debug, Serialize)]
pub struct GraphQueryHit {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct GraphQueryResult {
    pub query: String,
    pub hits: Vec<GraphQueryHit>,
    pub filters_applied: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ObjectExplain {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub purpose: String,
    pub owner: Option<String>,
    pub resources: Vec<String>,
    pub risks: Vec<String>,
    pub health_score: Option<i32>,
    pub health_label: Option<String>,
    pub related: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct GraphAtTime {
    pub timestamp: DateTime<Utc>,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub diff_summary: String,
    pub current_node_count: usize,
    pub node_delta: i64,
    pub added_nodes: Vec<String>,
    pub removed_nodes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TimelineReplayEntry {
    pub at: DateTime<Utc>,
    pub source: String,
    pub kind: String,
    pub message: String,
    pub resource: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TimelineReplay {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub entries: Vec<TimelineReplayEntry>,
    pub graph_changes: Vec<String>,
}

pub async fn build(pool: &DbPool, scope: &GraphScope) -> anyhow::Result<InfraGraph> {
    let twin = super::digital_twin::build_graph(pool).await?;
    let mut nodes: Vec<GraphNode> = twin
        .nodes
        .iter()
        .map(|n| GraphNode {
            kind: n.kind.clone(),
            id: n.id.clone(),
            name: n.name.clone(),
            state: n.state.clone(),
            health_score: None,
        })
        .collect();
    let mut edges: Vec<GraphEdge> = twin
        .edges
        .iter()
        .map(|e| GraphEdge {
            from: e.from.clone(),
            to: e.to.clone(),
            label: e.label.clone(),
        })
        .collect();

    // VM ↔ storage (vm_disks)
    let disks: Vec<(Uuid, String, Uuid, String)> = crate::db::query_as(
        "SELECT d.vm_id, v.name, d.id, COALESCE(d.storage_class, 'default')
         FROM vm_disks d JOIN vms v ON v.id = d.vm_id ORDER BY v.name LIMIT 500",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for (vm_id, vm_name, disk_id, sc) in disks {
        let pool_id = format!("storage-{sc}");
        if !nodes.iter().any(|n| n.id == pool_id) {
            nodes.push(GraphNode {
                kind: "storage".into(),
                id: pool_id.clone(),
                name: sc.clone(),
                state: None,
                health_score: None,
            });
        }
        edges.push(GraphEdge {
            from: vm_id.to_string(),
            to: pool_id,
            label: "attached_to".into(),
        });
        let _ = (vm_name, disk_id);
    }

    // VM ↔ network from spec_json bridges
    let vms_spec: Vec<(Uuid, String, Option<serde_json::Value>)> =
        crate::db::query_as("SELECT id, name, spec_json FROM vms ORDER BY name LIMIT 300")
            .fetch_all(pool)
            .await
            .unwrap_or_default();
    for (vm_id, _name, spec) in vms_spec {
        if let Some(spec) = spec {
            if let Some(nics) = spec.get("networks").and_then(|v| v.as_array()) {
                for nic in nics {
                    let net_name = nic
                        .get("network")
                        .or_else(|| nic.get("bridge"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("default");
                    let net_id = format!("network-br-{net_name}");
                    if !nodes.iter().any(|n| n.id == net_id) {
                        nodes.push(GraphNode {
                            kind: "bridge".into(),
                            id: net_id.clone(),
                            name: net_name.into(),
                            state: None,
                            health_score: None,
                        });
                    }
                    edges.push(GraphEdge {
                        from: vm_id.to_string(),
                        to: net_id,
                        label: "connected_to".into(),
                    });
                }
            }
        }
    }

    // Application groups → depends_on
    let apps: Vec<(Uuid, String)> =
        crate::db::query_as("SELECT id, name FROM application_groups ORDER BY name")
            .fetch_all(pool)
            .await
            .unwrap_or_default();
    for (gid, gname) in apps {
        let sid = format!("application-{gid}");
        nodes.push(GraphNode {
            kind: "application".into(),
            id: sid.clone(),
            name: gname,
            state: None,
            health_score: None,
        });
        let vms: Vec<Uuid> =
            crate::db::query_scalar("SELECT vm_id FROM application_group_vms WHERE group_id = ?")
                .bind(gid)
                .fetch_all(pool)
                .await
                .unwrap_or_default();
        for vid in vms {
            edges.push(GraphEdge {
                from: sid.clone(),
                to: vid.to_string(),
                label: "depends_on".into(),
            });
        }
    }

    // Backups
    let backups: Vec<(Uuid, Uuid, String)> = crate::db::query_as(
        "SELECT b.id, b.vm_id, v.name FROM backup_records b
         JOIN vms v ON v.id = b.vm_id ORDER BY b.created_at DESC LIMIT 200",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for (bid, vm_id, _vname) in backups {
        let bk_id = format!("backup-{bid}");
        nodes.push(GraphNode {
            kind: "backup".into(),
            id: bk_id.clone(),
            name: format!("backup {bid}"),
            state: None,
            health_score: None,
        });
        edges.push(GraphEdge {
            from: vm_id.to_string(),
            to: bk_id,
            label: "backed_up_by".into(),
        });
    }

    // Users (security graph lite)
    let users: Vec<(Uuid, String)> =
        crate::db::query_as("SELECT id, username FROM users ORDER BY username LIMIT 50")
            .fetch_all(pool)
            .await
            .unwrap_or_default();
    for (uid, uname) in users {
        nodes.push(GraphNode {
            kind: "user".into(),
            id: uid.to_string(),
            name: uname,
            state: None,
            health_score: None,
        });
        edges.push(GraphEdge {
            from: uid.to_string(),
            to: "cluster".into(),
            label: "administers".into(),
        });
    }

    // Health scores on hosts/vms from DNA/doctor heuristics
    for n in &mut nodes {
        if n.kind == "host" {
            let Ok(host_id) = Uuid::parse_str(&n.id) else {
                continue;
            };
            if let Ok(st) = crate::db::query_scalar::<_, String>("SELECT state FROM hosts WHERE id = ?")
                .bind(host_id)
                .fetch_optional(pool)
                .await
            {
                n.state = st;
                n.health_score = Some(if n.state.as_deref() == Some("online") {
                    85
                } else {
                    40
                });
            }
        }
        if n.kind == "vm" {
            let Ok(vm_id) = Uuid::parse_str(&n.id) else {
                continue;
            };
            if let Ok((st, mem, used)) = crate::db::query_as::<_, (String, i64, Option<i64>)>(
                "SELECT v.observed_state, v.memory_mib, m.memory_used_mib FROM vms v
                 LEFT JOIN vm_metrics m ON m.vm_id = v.id WHERE v.id = ?",
            )
            .bind(vm_id)
            .fetch_one(pool)
            .await
            {
                n.state = Some(st.clone());
                let score = if st != "running" {
                    50
                } else if let Some(u) = used {
                    let ratio = u as f64 / mem.max(1) as f64;
                    if ratio > 0.9 {
                        55
                    } else if ratio > 0.7 {
                        70
                    } else {
                        90
                    }
                } else {
                    75
                };
                n.health_score = Some(score);
            }
        }
    }

    // Scope filter
    if let Some(hid) = scope.host_id {
        let hid_s = hid.to_string();
        let vm_ids: Vec<String> = edges
            .iter()
            .filter(|e| e.from == hid_s && e.label == "runs")
            .map(|e| e.to.clone())
            .collect();
        nodes.retain(|n| {
            n.id == hid_s
                || n.kind == "cluster"
                || vm_ids.contains(&n.id)
                || n.kind == "bridge"
                || n.kind == "network"
        });
        let keep: std::collections::HashSet<String> = nodes.iter().map(|n| n.id.clone()).collect();
        edges.retain(|e| keep.contains(&e.from) && keep.contains(&e.to));
    }
    if let Some(vid) = scope.vm_id {
        let vid_s = vid.to_string();
        nodes.retain(|n| {
            n.id == vid_s || n.kind == "bridge" || n.kind == "network" || n.kind == "host"
        });
        let keep: std::collections::HashSet<String> = nodes.iter().map(|n| n.id.clone()).collect();
        edges.retain(|e| keep.contains(&e.from) || keep.contains(&e.to));
    }

    let edge_count = edges.len();
    let node_count = nodes.len();
    Ok(InfraGraph {
        nodes,
        edges,
        node_count,
        edge_count,
    })
}

/// Build unified graph including Zeus Firewall connectivity matrix edges.
pub async fn build_enriched(
    pool: &DbPool,
    cfg: &crate::config::ControllerConfig,
    scope: &GraphScope,
) -> anyhow::Result<InfraGraph> {
    let mut graph = build(pool, scope).await?;
    if let Err(e) = append_firewall_edges(pool, cfg, &mut graph.nodes, &mut graph.edges).await {
        // Don't silently drop all firewall edges from the graph — log so an operator
        // can tell "no firewall blocks" apart from "firewall data unavailable".
        tracing::warn!("infra_graph: firewall edges omitted (enrichment failed): {e}");
    }
    graph.node_count = graph.nodes.len();
    graph.edge_count = graph.edges.len();
    Ok(graph)
}

fn profile_rules(profile: &str) -> Vec<machina_core::ZeusFirewallRule> {
    let prof = match machina_core::profile_by_name(profile)
        .or_else(|| machina_core::profile_by_name("Private"))
        .or_else(|| machina_core::builtin_profiles().into_iter().next())
    {
        Some(p) => p,
        None => return vec![],
    };
    prof.rules
        .iter()
        .enumerate()
        .map(|(i, r)| machina_core::ZeusFirewallRule {
            id: format!("graph-{i}"),
            direction: r.direction.clone(),
            protocol: r.protocol.clone(),
            ports: r.ports.clone(),
            sources: r.sources.clone(),
            targets: vec![],
            action: r.action.clone(),
            temporary: false,
            expires_at: None,
            description: Some(r.name.clone()),
            scope: "host".into(),
            backend_ref: None,
        })
        .collect()
}

pub async fn append_firewall_edges(
    pool: &DbPool,
    cfg: &crate::config::ControllerConfig,
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
) -> anyhow::Result<()> {
    use machina_core::simulate_connectivity;

    let hosts: Vec<(Uuid, String)> = crate::db::query_as(
        "SELECT id, hostname FROM hosts WHERE state = 'online' ORDER BY hostname LIMIT 50",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for (hid, hostname) in hosts {
        let ft_id = format!("firewall-{hid}");
        if !nodes.iter().any(|n| n.id == ft_id) {
            nodes.push(GraphNode {
                kind: "firewall_target".into(),
                id: ft_id.clone(),
                name: format!("Zyra FW {hostname}"),
                state: Some("active".into()),
                health_score: None,
            });
        }
        if !edges
            .iter()
            .any(|e| e.from == hid.to_string() && e.to == ft_id)
        {
            edges.push(GraphEdge {
                from: hid.to_string(),
                to: ft_id.clone(),
                label: "protected_by".into(),
            });
        }
        let detail = match crate::engine::zeus_firewall::inventory::target_detail(
            pool,
            cfg,
            &hid.to_string(),
        )
        .await
        {
            Ok(d) => d,
            Err(_) => continue,
        };
        let profile = detail.target.profile.as_deref().unwrap_or("Balanced");
        let rules = profile_rules(profile);
        let matrix = simulate_connectivity(&detail.inventory, &rules);
        for cell in matrix.blocks.iter().take(5) {
            let port_id = format!("port-{}-{}", cell.port, cell.protocol);
            if !nodes.iter().any(|n| n.id == port_id) {
                nodes.push(GraphNode {
                    kind: "segment".into(),
                    id: port_id.clone(),
                    name: format!("{}:{}", cell.protocol, cell.port),
                    state: Some("blocked".into()),
                    health_score: None,
                });
            }
            edges.push(GraphEdge {
                from: ft_id.clone(),
                to: port_id,
                label: "blocks".into(),
            });
        }
        for cell in matrix.allows.iter().take(5) {
            let port_id = format!("port-allow-{}-{}", cell.port, cell.protocol);
            if !nodes.iter().any(|n| n.id == port_id) {
                nodes.push(GraphNode {
                    kind: "segment".into(),
                    id: port_id.clone(),
                    name: format!("allow {}:{}", cell.protocol, cell.port),
                    state: Some("allowed".into()),
                    health_score: None,
                });
            }
            edges.push(GraphEdge {
                from: ft_id.clone(),
                to: port_id,
                label: "allows".into(),
            });
        }
    }
    Ok(())
}

async fn firewall_path_blocker(
    pool: &DbPool,
    cfg: &crate::config::ControllerConfig,
    host_id: Uuid,
    port: i32,
) -> Option<PathBlocker> {
    use machina_core::simulate_connectivity;

    // SECURITY/correctness: a *failed* firewall lookup must not be reported as
    // "no block found" (which the caller turns into `can_reach = true`). Surface an
    // explicit unknown-status blocker so reachability is treated as unconfirmed.
    let detail = match crate::engine::zeus_firewall::inventory::target_detail(
        pool,
        cfg,
        &host_id.to_string(),
    )
    .await
    {
        Ok(d) => d,
        Err(e) => {
            return Some(PathBlocker {
                kind: "firewall_unknown".into(),
                message: format!("Zeus firewall status could not be verified for this host: {e}"),
                remediation: "Firewall reachability is unconfirmed — retry once the firewall \
                     inventory backend is reachable before treating the path as open."
                    .into(),
            });
        }
    };
    let profile = detail.target.profile.as_deref().unwrap_or("Balanced");
    let rules = profile_rules(profile);
    let matrix = simulate_connectivity(&detail.inventory, &rules);
    let probe_port = if port > 0 && port <= 65535 {
        port as u16
    } else {
        5432
    };
    if let Some(block) = matrix.blocks.iter().find(|c| c.port == probe_port) {
        return Some(PathBlocker {
            kind: "firewall_deny".into(),
            message: format!(
                "Zeus firewall blocks {}:{} — {}",
                block.source, block.port, block.reason
            ),
            remediation: "Add inbound allow rule or use app-tier profile on destination host."
                .into(),
        });
    }
    if matrix.allows.iter().any(|c| c.port == probe_port) {
        return None;
    }
    Some(PathBlocker {
        kind: "firewall_deny".into(),
        message: format!("No Zeus firewall allow rule for TCP port {probe_port} (default deny)."),
        remediation: "Apply Balanced or custom profile allowing app-tier → host traffic.".into(),
    })
}

async fn resolve_vm(
    pool: &DbPool,
    name: &str,
) -> anyhow::Result<Option<(Uuid, String, Option<Uuid>, String)>> {
    // Exact match, not LIKE: `name` is a concrete VM identifier from the request, and
    // an unescaped `LIKE` bind treats `_`/`%` as wildcards — so `web_01` would resolve
    // to an arbitrary `webX01`/`web-01`, and `%` would resolve to "some VM". Path
    // analysis run against the wrong VM is silently misleading.
    let row: Option<(Uuid, String, Option<Uuid>, String)> =
        crate::db::query_as("SELECT id, name, host_id, observed_state FROM vms WHERE name = ? LIMIT 1")
            .bind(name)
            .fetch_optional(pool)
            .await?;
    Ok(row)
}

pub async fn explain_path(
    pool: &DbPool,
    cfg: &crate::config::ControllerConfig,
    req: &PathRequest,
) -> anyhow::Result<PathResult> {
    let from_vm = resolve_vm(pool, &req.from).await?;
    let to_vm = resolve_vm(pool, &req.to).await?;
    let port_str = req.port.map(|p| format!(":{p}")).unwrap_or_default();
    let mut evidence = Vec::new();
    let mut blockers = Vec::new();
    let mut hops = Vec::new();

    let (Some((a_id, a_name, a_host, a_state)), Some((b_id, b_name, b_host, b_state))) =
        (from_vm, to_vm)
    else {
        blockers.push(PathBlocker {
            kind: "missing_vm".into(),
            message: "One or both VMs not found in inventory.".into(),
            remediation: "Sync host inventory and verify VM names.".into(),
        });
        return Ok(PathResult {
            can_reach: false,
            explanation: format!(
                "Cannot trace path — VM '{}' or '{}' not in fleet inventory.",
                req.from, req.to
            ),
            hops,
            blockers,
            confidence: 0.95,
            evidence,
        });
    };

    hops.push(format!("{a_name} ({a_id})"));
    if a_state != "running" {
        blockers.push(PathBlocker {
            kind: "vm_down".into(),
            message: format!("Source VM {a_name} is {a_state}."),
            remediation: "Start the VM or check why it stopped.".into(),
        });
        evidence.push(PathEvidence {
            source: "vm_state".into(),
            detail: format!("{a_name} observed_state={a_state}"),
        });
    }
    if let Some(h) = a_host {
        let host_state: Option<String> = crate::db::query_scalar("SELECT state FROM hosts WHERE id = ?")
            .bind(h)
            .fetch_optional(pool)
            .await?;
        hops.push(format!("host {h}"));
        if host_state.as_deref() != Some("online") {
            blockers.push(PathBlocker {
                kind: "host_offline".into(),
                message: format!("Host {h} is not online."),
                remediation: "Restore host connectivity or migrate VM.".into(),
            });
            evidence.push(PathEvidence {
                source: "host_state".into(),
                detail: format!("host {h} state={host_state:?}"),
            });
        }
    }
    hops.push("cluster network / bridge".into());
    if let Some(h) = b_host {
        hops.push(format!("host {h}"));
    }
    hops.push(format!("{b_name} ({b_id})"));

    if b_state != "running" {
        blockers.push(PathBlocker {
            kind: "vm_down".into(),
            message: format!("Destination VM {b_name} is {b_state}."),
            remediation: "Start destination VM.".into(),
        });
    }

    let same_host = a_host.is_some() && a_host == b_host;
    let graph = build(
        pool,
        &GraphScope {
            host_id: None,
            vm_id: None,
        },
    )
    .await?;
    let a_bridges: Vec<String> = graph
        .edges
        .iter()
        .filter(|e| e.from == a_id.to_string() && e.label == "connected_to")
        .map(|e| e.to.clone())
        .collect();
    let b_bridges: Vec<String> = graph
        .edges
        .iter()
        .filter(|e| e.from == b_id.to_string() && e.label == "connected_to")
        .map(|e| e.to.clone())
        .collect();
    let shared_bridge = a_bridges.iter().any(|b| b_bridges.contains(b));

    if !same_host && !shared_bridge && !a_bridges.is_empty() && !b_bridges.is_empty() {
        blockers.push(PathBlocker {
            kind: "segment_mismatch".into(),
            message: "VMs appear on different bridges/segments — L3 routing or firewall may block traffic.".into(),
            remediation: "Verify VLAN/segment membership and firewall rules between segments.".into(),
        });
        evidence.push(PathEvidence {
            source: "graph".into(),
            detail: format!("A bridges={a_bridges:?} B bridges={b_bridges:?}"),
        });
    }

    if let Some(h) = b_host {
        if let Some(fb) = firewall_path_blocker(pool, cfg, h, req.port.unwrap_or(5432)).await {
            evidence.push(PathEvidence {
                source: "zeus_firewall".into(),
                detail: fb.message.clone(),
            });
            blockers.push(fb);
        }
    } else {
        // No destination host_id → the firewall check cannot run. Don't imply the
        // path is open; record that reachability is unverified.
        blockers.push(PathBlocker {
            kind: "firewall_unknown".into(),
            message: "Destination host is unknown — firewall reachability not evaluated.".into(),
            remediation:
                "Ensure the destination VM is mapped to a host before trusting this result.".into(),
        });
    }

    // Recent network/firewall audit
    let recent: Option<(String,)> = crate::db::query_as(
        "SELECT action FROM audit_logs
         WHERE created_at > datetime('now', '-4 hours')
           AND (action LIKE '%network%' OR action LIKE '%firewall%')
         ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    if let Some((action,)) = recent {
        blockers.push(PathBlocker {
            kind: "recent_change".into(),
            message: format!("Recent network/firewall change: {action}"),
            remediation: "Review audit log and rollback if unintended.".into(),
        });
        evidence.push(PathEvidence {
            source: "audit".into(),
            detail: action,
        });
    }

    let can_reach = blockers.is_empty();
    let explanation = if can_reach {
        if same_host {
            format!(
                "{a_name} and {b_name} share host {a_host:?} — L2 reachability likely unless guest firewall blocks TCP{port_str}."
            )
        } else if shared_bridge {
            format!(
                "{a_name} and {b_name} share bridge — verify guest firewalls allow TCP{port_str}."
            )
        } else {
            format!(
                "{a_name} → {b_name}: cross-host path — verify routing, VLAN, and security groups for TCP{port_str}."
            )
        }
    } else {
        format!(
            "Path from {a_name} to {b_name} blocked: {}",
            blockers
                .first()
                .map(|b| b.message.as_str())
                .unwrap_or("unknown")
        )
    };

    Ok(PathResult {
        can_reach,
        explanation,
        hops,
        blockers,
        confidence: if can_reach { 0.78 } else { 0.88 },
        evidence,
    })
}

pub async fn query(pool: &DbPool, req: &GraphQueryRequest) -> anyhow::Result<GraphQueryResult> {
    let q = req.query.to_lowercase();
    let mut hits = Vec::new();
    let mut filters = Vec::new();

    // Each detected intent adds an AND'd SQL condition, so a query combining
    // multiple intents (e.g. "ubuntu vms over 8gb ram") returns only VMs
    // matching every condition — not the union of running each in isolation,
    // which previously let an unfiltered "ubuntu" match slip past a claimed
    // "memory_mib > 8192" filter that was never actually applied to it.
    let mut conditions: Vec<String> = Vec::new();
    let mut min_mib: Option<i64> = None;

    if q.contains("ubuntu") {
        filters.push("os_family/ubuntu".into());
        conditions.push(
            "(name LIKE '%ubuntu%' OR os_family LIKE '%ubuntu%' OR tags LIKE '%ubuntu%')".into(),
        );
    }

    if q.contains("ram") || q.contains("memory") || q.contains("gb") {
        let threshold = if q.contains("8gb") || q.contains("8 gb") {
            filters.push("memory_mib > 8192".into());
            8192i64
        } else if q.contains("4gb") {
            4096
        } else {
            2048
        };
        conditions.push("memory_mib > ?".into());
        min_mib = Some(threshold);
    }

    if !conditions.is_empty() {
        let sql = format!(
            "SELECT id, name, memory_mib, COALESCE(os_family, 'linux') FROM vms
             WHERE {} ORDER BY name LIMIT 50",
            conditions.join(" AND ")
        );
        let mut query = crate::db::query_as::<_, (Uuid, String, i64, String)>(&sql);
        if let Some(min) = min_mib {
            query = query.bind(min);
        }
        let rows = query.fetch_all(pool).await?;
        for (id, name, mem, os) in rows {
            hits.push(GraphQueryHit {
                kind: "vm".into(),
                id: id.to_string(),
                name,
                detail: format!("{mem} MiB · {os}"),
            });
        }
    }

    if hits.is_empty() {
        let escaped = req
            .query
            .trim()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let rows: Vec<(Uuid, String, String)> = crate::db::query_as(
            "SELECT id, name, observed_state FROM vms WHERE name LIKE ? ESCAPE '\\' ORDER BY name LIMIT 20",
        )
        .bind(format!("%{escaped}%"))
        .fetch_all(pool)
        .await?;
        for (id, name, st) in rows {
            hits.push(GraphQueryHit {
                kind: "vm".into(),
                id: id.to_string(),
                name,
                detail: st,
            });
        }
    }

    Ok(GraphQueryResult {
        query: req.query.clone(),
        hits,
        filters_applied: filters,
    })
}

pub async fn explain_object(
    pool: &DbPool,
    kind: &str,
    id: &str,
) -> anyhow::Result<ObjectExplain> {
    match kind {
        "vm" => {
            let row: Option<(String, Option<String>, i64, i32, String, Option<String>)> =
                if let Ok(uuid) = uuid::Uuid::parse_str(id) {
                    crate::db::query_as(
                        "SELECT v.name, v.project, v.memory_mib, v.vcpus, v.observed_state, v.tags
                         FROM vms v WHERE v.id = ? OR v.name = ?",
                    )
                    .bind(uuid)
                    .bind(id)
                    .fetch_optional(pool)
                    .await?
                } else {
                    crate::db::query_as(
                        "SELECT v.name, v.project, v.memory_mib, v.vcpus, v.observed_state, v.tags
                         FROM vms v WHERE v.name = ?",
                    )
                    .bind(id)
                    .fetch_optional(pool)
                    .await?
                };
            let Some((name, project, mem, vcpu, state, tags)) = row else {
                anyhow::bail!("VM not found");
            };
            let mut risks = Vec::new();
            if state != "running" {
                risks.push(format!("VM is {state}"));
            }
            if tags.as_deref().unwrap_or("").is_empty() {
                risks.push("Missing ownership/tags".into());
            }
            let score = if state == "running" { 82 } else { 45 };
            Ok(ObjectExplain {
                kind: "vm".into(),
                id: id.into(),
                name,
                purpose: project.clone().unwrap_or_else(|| "General workload".into()),
                owner: project,
                resources: vec![format!("{vcpu} vCPU"), format!("{mem} MiB RAM")],
                risks,
                health_score: Some(score),
                health_label: Some(if score >= 80 { "Healthy" } else { "Attention" }.into()),
                related: vec!["Open VM Doctor for full checks".into()],
            })
        }
        "host" => {
            let row: Option<(String, String, i64)> = if let Ok(uuid) = uuid::Uuid::parse_str(id) {
                crate::db::query_as(
                    "SELECT hostname, state, vm_count FROM hosts WHERE id = ? OR hostname = ?",
                )
                .bind(uuid)
                .bind(id)
                .fetch_optional(pool)
                .await?
            } else {
                crate::db::query_as("SELECT hostname, state, vm_count FROM hosts WHERE hostname = ?")
                    .bind(id)
                    .fetch_optional(pool)
                    .await?
            };
            let Some((name, state, vms)) = row else {
                anyhow::bail!("Host not found");
            };
            Ok(ObjectExplain {
                kind: "host".into(),
                id: id.into(),
                name: name.clone(),
                purpose: "Hypervisor node".into(),
                owner: None,
                resources: vec![format!("{vms} VMs")],
                risks: if state != "online" {
                    vec!["Host offline".into()]
                } else {
                    vec![]
                },
                health_score: Some(if state == "online" { 88 } else { 30 }),
                health_label: Some(state),
                related: vec![],
            })
        }
        _ => anyhow::bail!("Unsupported kind: {kind}"),
    }
}

pub async fn graph_at(pool: &DbPool, ts: DateTime<Utc>) -> anyhow::Result<GraphAtTime> {
    let current = build(
        pool,
        &GraphScope {
            host_id: None,
            vm_id: None,
        },
    )
    .await?;
    let created: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM audit_logs WHERE action LIKE '%create%' AND created_at <= ?",
    )
    .bind(ts)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let deleted: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM audit_logs WHERE action LIKE '%delete%' AND created_at <= ?",
    )
    .bind(ts)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let vm_names_at: Vec<String> = crate::db::query_scalar(
        "SELECT DISTINCT COALESCE(json_extract(detail, '$.name'), CAST(resource_id AS TEXT)) FROM audit_logs
         WHERE action LIKE '%vm%' AND action LIKE '%create%' AND created_at <= ?
         ORDER BY 1 LIMIT 50",
    )
    .bind(ts)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    let current_vm_names: Vec<String> = crate::db::query_scalar("SELECT name FROM vms ORDER BY name")
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    let added: Vec<String> = current_vm_names
        .iter()
        .filter(|n| !vm_names_at.contains(n))
        .take(10)
        .cloned()
        .collect();
    let removed: Vec<String> = vm_names_at
        .iter()
        .filter(|n| !current_vm_names.contains(n))
        .take(10)
        .cloned()
        .collect();
    let node_delta = added.len() as i64 - removed.len() as i64;
    Ok(GraphAtTime {
        timestamp: ts,
        nodes: current.nodes.clone(),
        edges: current.edges.clone(),
        diff_summary: format!(
            "Reconstructed at {ts}: {created} create events, {deleted} delete events in audit history."
        ),
        current_node_count: current.node_count,
        node_delta,
        added_nodes: added,
        removed_nodes: removed,
    })
}

pub async fn timeline_replay(
    pool: &DbPool,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    resource: Option<&str>,
) -> anyhow::Result<TimelineReplay> {
    let mut entries = Vec::new();
    let audits: Vec<(DateTime<Utc>, String, String, Option<String>)> = crate::db::query_as(
        "SELECT strftime('%Y-%m-%dT%H:%M:%SZ', created_at), actor, action, resource_type
         FROM audit_logs WHERE created_at BETWEEN ? AND ? ORDER BY created_at ASC LIMIT 200",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    for (at, actor, action, rt) in audits {
        let msg = format!("{actor} — {action}");
        if resource.is_some_and(|r| !msg.contains(r) && rt.as_deref() != Some(r)) {
            continue;
        }
        entries.push(TimelineReplayEntry {
            at,
            source: "audit".into(),
            kind: action.clone(),
            message: msg,
            resource: rt,
        });
    }
    let events: Vec<(DateTime<Utc>, String, String)> = crate::db::query_as(
        "SELECT strftime('%Y-%m-%dT%H:%M:%SZ', created_at), kind, message FROM events
         WHERE created_at BETWEEN ? AND ? ORDER BY created_at ASC LIMIT 100",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    for (at, kind, message) in events {
        entries.push(TimelineReplayEntry {
            at,
            source: "event".into(),
            kind,
            message,
            resource: None,
        });
    }
    entries.sort_by_key(|a| a.at);
    let graph_changes: Vec<String> = entries
        .iter()
        .filter(|e| {
            e.kind.contains("create") || e.kind.contains("delete") || e.kind.contains("migrate")
        })
        .map(|e| format!("{}: {}", e.at.to_rfc3339(), e.message))
        .collect();
    Ok(TimelineReplay {
        from,
        to,
        entries,
        graph_changes,
    })
}

/// Merge twin into infra graph helper for digital twin simulations.
pub fn merge_twin(twin: &DigitalTwinGraph) -> InfraGraph {
    InfraGraph {
        nodes: twin
            .nodes
            .iter()
            .map(|n| GraphNode {
                kind: n.kind.clone(),
                id: n.id.clone(),
                name: n.name.clone(),
                state: n.state.clone(),
                health_score: None,
            })
            .collect(),
        edges: twin
            .edges
            .iter()
            .map(|e| GraphEdge {
                from: e.from.clone(),
                to: e.to.clone(),
                label: e.label.clone(),
            })
            .collect(),
        node_count: twin.node_count,
        edge_count: twin.edge_count,
    }
}
