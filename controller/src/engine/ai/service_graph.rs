// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

use super::digital_twin::TwinEdge;

#[derive(Debug, Serialize)]
pub struct ServiceGraphNode {
    pub kind: String,
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ServiceGraph {
    pub nodes: Vec<ServiceGraphNode>,
    pub edges: Vec<TwinEdge>,
    pub service_count: usize,
}

pub async fn build(pool: &DbPool) -> anyhow::Result<ServiceGraph> {
    let twin = super::digital_twin::build_graph(pool).await?;
    let mut nodes: Vec<ServiceGraphNode> = twin
        .nodes
        .iter()
        .map(|n| ServiceGraphNode {
            kind: n.kind.clone(),
            id: n.id.clone(),
            name: n.name.clone(),
            parent: None,
        })
        .collect();

    let apps: Vec<(uuid::Uuid, String)> =
        crate::db::query_as("SELECT id, name FROM application_groups ORDER BY name")
            .fetch_all(pool)
            .await?;

    let mut edges = twin.edges.clone();
    let mut service_count = 0usize;

    for (gid, gname) in apps {
        let sid = format!("service-{gid}");
        nodes.push(ServiceGraphNode {
            kind: "service".into(),
            id: sid.clone(),
            name: gname,
            parent: None,
        });
        service_count += 1;

        let vms: Vec<(uuid::Uuid, String)> = crate::db::query_as(
            "SELECT v.id, v.name FROM application_group_vms agv
             JOIN vms v ON v.id = agv.vm_id WHERE agv.group_id = ?",
        )
        .bind(gid)
        .fetch_all(pool)
        .await?;

        for (vid, _) in vms {
            edges.push(TwinEdge {
                from: sid.clone(),
                to: vid.to_string(),
                label: "depends_on".into(),
            });
        }
    }

    Ok(ServiceGraph {
        nodes,
        edges,
        service_count,
    })
}
