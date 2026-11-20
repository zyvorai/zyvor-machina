// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct SecurityGraphNode {
    pub id: String,
    pub kind: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SecurityGraphEdge {
    pub from: String,
    pub to: String,
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct SecurityGraph {
    pub nodes: Vec<SecurityGraphNode>,
    pub edges: Vec<SecurityGraphEdge>,
}

#[derive(Debug, Deserialize)]
pub struct AttackPathQuery {
    pub source: String,
    pub target_vm: String,
}

#[derive(Debug, Serialize)]
pub struct AttackPathResult {
    pub source: String,
    pub target_vm: String,
    pub path: Vec<String>,
    pub edges: Vec<String>,
    pub risk_score: f32,
    pub summary: String,
}

pub async fn build_graph(pool: &DbPool) -> anyhow::Result<SecurityGraph> {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    let users: Vec<(Uuid, String, String)> =
        crate::db::query_as("SELECT id, username, role FROM users ORDER BY username")
            .fetch_all(pool)
            .await?;
    for (id, name, role) in users {
        let nid = format!("user-{id}");
        let risk = if role == "admin" {
            Some("high".into())
        } else {
            Some("medium".into())
        };
        nodes.push(SecurityGraphNode {
            id: nid.clone(),
            kind: "user".into(),
            label: name,
            risk,
        });
        edges.push(SecurityGraphEdge {
            from: nid,
            to: "cluster".into(),
            label: "authenticates".into(),
        });
    }

    nodes.push(SecurityGraphNode {
        id: "cluster".into(),
        kind: "cluster".into(),
        label: "cluster".into(),
        risk: None,
    });

    let hosts: Vec<(Uuid, String)> =
        crate::db::query_as("SELECT id, hostname FROM hosts ORDER BY hostname")
            .fetch_all(pool)
            .await?;
    for (id, name) in hosts {
        let hid = format!("host-{id}");
        nodes.push(SecurityGraphNode {
            id: hid.clone(),
            kind: "host".into(),
            label: name,
            risk: Some("medium".into()),
        });
        edges.push(SecurityGraphEdge {
            from: "cluster".into(),
            to: hid.clone(),
            label: "manages".into(),
        });
    }

    let vms: Vec<(Uuid, String, Option<Uuid>)> =
        crate::db::query_as("SELECT id, name, host_id FROM vms ORDER BY name LIMIT 100")
            .fetch_all(pool)
            .await?;
    for (id, name, host_id) in vms {
        let vid = format!("vm-{id}");
        nodes.push(SecurityGraphNode {
            id: vid.clone(),
            kind: "vm".into(),
            label: name,
            risk: Some("workload".into()),
        });
        if let Some(h) = host_id {
            edges.push(SecurityGraphEdge {
                from: format!("host-{h}"),
                to: vid,
                label: "runs".into(),
            });
        }
    }

    let networks: Vec<(Uuid, String)> =
        crate::db::query_as("SELECT id, name FROM networks ORDER BY name")
            .fetch_all(pool)
            .await?;
    for (id, name) in networks {
        let nnid = format!("network-{id}");
        nodes.push(SecurityGraphNode {
            id: nnid.clone(),
            kind: "network".into(),
            label: name,
            risk: Some("segment".into()),
        });
        edges.push(SecurityGraphEdge {
            from: "cluster".into(),
            to: nnid,
            label: "provides".into(),
        });
    }

    let keys: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM api_keys")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    if keys > 0 {
        nodes.push(SecurityGraphNode {
            id: "api-keys".into(),
            kind: "credential".into(),
            label: format!("{keys} API key(s)"),
            risk: Some("high".into()),
        });
        edges.push(SecurityGraphEdge {
            from: "api-keys".into(),
            to: "cluster".into(),
            label: "api_access".into(),
        });
    }

    Ok(SecurityGraph { nodes, edges })
}

pub async fn attack_path(
    pool: &DbPool,
    q: &AttackPathQuery,
) -> anyhow::Result<AttackPathResult> {
    let graph = build_graph(pool).await?;
    let target_id: Option<Uuid> = if let Ok(u) = Uuid::parse_str(&q.target_vm) {
        Some(u)
    } else {
        crate::db::query_scalar("SELECT id FROM vms WHERE name = ?")
            .bind(&q.target_vm)
            .fetch_optional(pool)
            .await?
    };
    let target_label = if let Some(id) = target_id {
        crate::db::query_scalar::<_, String>("SELECT name FROM vms WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .unwrap_or(q.target_vm.clone())
    } else {
        q.target_vm.clone()
    };

    let vm_node = if let Some(id) = target_id {
        format!("vm-{id}")
    } else if let Some(n) = graph
        .nodes
        .iter()
        .find(|n| n.kind == "vm" && n.label.eq_ignore_ascii_case(&q.target_vm))
    {
        n.id.clone()
    } else {
        format!("vm-{}", q.target_vm)
    };
    let source_node = if q.source.to_lowercase().contains("admin")
        || q.source.to_lowercase().contains("attacker")
    {
        graph
            .nodes
            .iter()
            .find(|n| n.kind == "user" && n.risk.as_deref() == Some("high"))
            .or_else(|| graph.nodes.iter().find(|n| n.kind == "user"))
            .map(|n| n.id.clone())
            .unwrap_or_else(|| "api-keys".into())
    } else {
        graph
            .nodes
            .iter()
            .find(|n| n.label.eq_ignore_ascii_case(&q.source))
            .map(|n| n.id.clone())
            .unwrap_or_else(|| format!("user-{}", q.source))
    };

    let mut path = vec![source_node.clone()];
    let mut edge_labels = Vec::new();

    if source_node.starts_with("user-") || source_node == "api-keys" {
        path.push("cluster".into());
        edge_labels.push("authenticates → cluster".into());
        if let Some(host_edge) = graph
            .edges
            .iter()
            .find(|e| e.to == vm_node || e.to.starts_with("vm-"))
        {
            if let Some(host) = graph.nodes.iter().find(|n| n.id == host_edge.from) {
                path.push(host.id.clone());
                edge_labels.push(format!("manages → {}", host.label));
            }
        }
    }
    path.push(vm_node.clone());
    edge_labels.push(format!("runs → {target_label}"));

    let risk_score = if source_node == "api-keys" {
        0.88
    } else {
        0.72
    };
    let hop_count = edge_labels.len();

    Ok(AttackPathResult {
        source: q.source.clone(),
        target_vm: target_label.clone(),
        path,
        edges: edge_labels,
        risk_score,
        summary: format!(
            "Potential path from '{}' to database/workload VM '{}' — {} hop(s), risk {:.0}%.",
            q.source,
            target_label,
            hop_count,
            risk_score * 100.0
        ),
    })
}
