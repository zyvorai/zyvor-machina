// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::Serialize;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct TopologyNode {
    pub kind: String,
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TopologyEdge {
    pub from: String,
    pub to: String,
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct TopologyWarning {
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix_action: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TopologyGraph {
    pub nodes: Vec<TopologyNode>,
    pub edges: Vec<TopologyEdge>,
    pub warnings: Vec<TopologyWarning>,
}

pub async fn cluster_topology(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<TopologyGraph>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(build_topology(&state.pool, None).await?))
}

pub async fn vm_topology(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<TopologyGraph>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(build_topology(&state.pool, Some(id)).await?))
}

pub(crate) async fn build_topology(
    pool: &crate::db::DbPool,
    vm_filter: Option<Uuid>,
) -> Result<TopologyGraph, ApiError> {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut warnings = Vec::new();

    let cluster_name: String = crate::db::query_scalar("SELECT name FROM clusters LIMIT 1")
        .fetch_one(pool)
        .await
        .unwrap_or_else(|_| "default".into());
    nodes.push(TopologyNode {
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
        nodes.push(TopologyNode {
            kind: "host".into(),
            id: hid.to_string(),
            name: name.clone(),
            state: Some(st.clone()),
        });
        edges.push(TopologyEdge {
            from: "cluster".into(),
            to: hid.to_string(),
            label: "contains".into(),
        });
    }

    let vms: Vec<(
        Uuid,
        String,
        Option<Uuid>,
        String,
        crate::db::Json<Vec<String>>,
    )> = if let Some(vid) = vm_filter {
        crate::db::query_as(
            "SELECT id, name, host_id, observed_state, COALESCE(tags, '[]') FROM vms WHERE id = ?",
        )
        .bind(vid)
        .fetch_all(pool)
        .await?
    } else {
        crate::db::query_as(
            "SELECT id, name, host_id, observed_state, COALESCE(tags, '[]') FROM vms ORDER BY name LIMIT 100",
        )
        .fetch_all(pool)
        .await?
    };

    let host_vm_count: std::collections::HashMap<Uuid, Vec<String>> = {
        let mut m: std::collections::HashMap<Uuid, Vec<String>> = std::collections::HashMap::new();
        for (_, name, host_id, _, _) in &vms {
            if let Some(h) = host_id {
                m.entry(*h).or_default().push(name.clone());
            }
        }
        m
    };

    for (count_host, vm_names) in &host_vm_count {
        if vm_names.len() >= 2 {
            let prod: Vec<_> = vms
                .iter()
                .filter(|(_, n, h, _, tags)| {
                    h == &Some(*count_host)
                        && vm_names.contains(n)
                        && tags.iter().any(|t| t == "prod" || t == "production")
                })
                .collect();
            if prod.len() >= 2 {
                warnings.push(TopologyWarning {
                    severity: "warning".into(),
                    message: format!(
                        "Anti-affinity recommended: {} VMs on same host",
                        prod.iter()
                            .map(|(_, n, _, _, _)| n.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    fix_action: Some("enable_anti_affinity".into()),
                });
            }
        }
    }

    for (vid, name, host_id, st, _) in vms {
        nodes.push(TopologyNode {
            kind: "vm".into(),
            id: vid.to_string(),
            name,
            state: Some(st),
        });
        if let Some(h) = host_id {
            edges.push(TopologyEdge {
                from: h.to_string(),
                to: vid.to_string(),
                label: "runs".into(),
            });
        }
    }

    let segments: Vec<(Uuid, String, String)> =
        crate::db::query_as("SELECT id, name, tier FROM network_segments ORDER BY name")
            .fetch_all(pool)
            .await
            .unwrap_or_default();

    for (sid, name, tier) in &segments {
        let node_id = format!("segment-{sid}");
        nodes.push(TopologyNode {
            kind: "segment".into(),
            id: node_id.clone(),
            name: format!("{name} ({tier})"),
            state: None,
        });
        edges.push(TopologyEdge {
            from: "cluster".into(),
            to: node_id.clone(),
            label: "overlay".into(),
        });

        let bound: Vec<(Uuid, String)> =
            crate::db::query_as("SELECT id, name FROM networks WHERE segment_id = ?")
                .bind(sid)
                .fetch_all(pool)
                .await
                .unwrap_or_default();
        for (nid, net_name) in bound {
            let net_node = format!("network-{nid}");
            if !nodes.iter().any(|n| n.id == net_node) {
                nodes.push(TopologyNode {
                    kind: "network".into(),
                    id: net_node.clone(),
                    name: net_name,
                    state: None,
                });
                edges.push(TopologyEdge {
                    from: "cluster".into(),
                    to: net_node.clone(),
                    label: "network".into(),
                });
            }
            edges.push(TopologyEdge {
                from: node_id.clone(),
                to: net_node,
                label: "member".into(),
            });
        }
    }

    let online_hosts: Vec<(Uuid, String, String)> = crate::db::query_as(
        "SELECT id, hostname, COALESCE(NULLIF(agent_console_addr, ''), agent_grpc_addr)
         FROM hosts WHERE state = 'online' ORDER BY hostname LIMIT 20",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let lldp = crate::engine::network_overlay::lldp_topology_from_cache(pool)
        .await
        .unwrap_or_else(
            |_| crate::engine::network_overlay::LldpTopologyContribution {
                nodes: vec![],
                edges: vec![],
                warnings: vec![],
            },
        );
    let cache_empty = lldp.nodes.is_empty();

    for node in lldp.nodes {
        if !nodes.iter().any(|n| n.id == node.id) {
            nodes.push(TopologyNode {
                kind: node.kind,
                id: node.id,
                name: node.name,
                state: node.state,
            });
        }
    }
    for edge in lldp.edges {
        edges.push(TopologyEdge {
            from: edge.from,
            to: edge.to,
            label: edge.label,
        });
    }
    for warn in lldp.warnings {
        warnings.push(TopologyWarning {
            severity: warn.severity,
            message: warn.message,
            fix_action: None,
        });
    }

    // Fallback: probe agents directly when cache is empty but hosts are online.
    if cache_empty {
        for (hid, hostname, console_addr) in online_hosts {
            if let Ok(lldp_live) =
                crate::engine::network_overlay::fetch_host_lldp(&console_addr).await
            {
                for (i, neighbor) in lldp_live.neighbors.iter().enumerate() {
                    let switch_id = format!("switch-{hid}-{i}");
                    let switch_name = if neighbor.system_name.is_empty() {
                        neighbor.chassis_id.clone()
                    } else {
                        neighbor.system_name.clone()
                    };
                    nodes.push(TopologyNode {
                        kind: "switch".into(),
                        id: switch_id.clone(),
                        name: switch_name,
                        state: Some(lldp_live.source.clone()),
                    });
                    edges.push(TopologyEdge {
                        from: hid.to_string(),
                        to: switch_id,
                        label: "uplink".into(),
                    });
                }
                if lldp_live.neighbors.is_empty() && !lldp_live.summary.is_empty() {
                    warnings.push(TopologyWarning {
                        severity: "info".into(),
                        message: format!("{hostname}: {}", lldp_live.summary),
                        fix_action: None,
                    });
                }
            }
        }
    }

    Ok(TopologyGraph {
        nodes,
        edges,
        warnings,
    })
}
