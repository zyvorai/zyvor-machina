// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Fleet-managed k3s clusters. A cluster is one server node plus agent nodes, each on an enrolled
//! host. The controller asks each host's agent (`K3sNode` RPC) to install, join, upgrade or remove
//! k3s; the agent only obeys when its operator set `MACHINA_AGENT_K3S_ENABLED=1`, and this module
//! only runs when `MACHINA_K3S_FLEET=1` on the controller. The join token and the admin kubeconfig
//! are kept encrypted with the same key as LLM provider keys.
//!
//! Provisioning runs as a background task and records progress in `k8s_clusters` /
//! `k8s_cluster_nodes`, so a client polls the cluster rather than holding a request open for the
//! length of an install. A controller restart mid-provision leaves the cluster in `provisioning`;
//! delete it and create it again.

use uuid::Uuid;

use crate::agent_client::{self, K3sNodeCall};
use crate::db::DbPool;
use crate::state::AppState;

pub const FLEET_ENV: &str = "MACHINA_K3S_FLEET";
/// Where the API server of a new cluster listens.
pub const API_PORT: u16 = 6443;

pub fn enabled() -> bool {
    std::env::var(FLEET_ENV).is_ok_and(|v| v == "1")
}

/// `https://<address>:6443` for the server host's address, or an error naming the problem.
pub fn api_url_for(address: &str) -> Result<String, String> {
    let a = address.trim();
    if a.is_empty() {
        return Err("the server host has no address recorded".into());
    }
    let host = if a.contains(':') && !a.starts_with('[') {
        format!("[{a}]") // bare IPv6
    } else {
        a.to_string()
    };
    let url = format!("https://{host}:{API_PORT}");
    machina_core::k3s_node::validate_server_url(&url)?;
    Ok(url)
}

/// A node name from a hostname: lowercase, invalid characters to '-', trimmed to a DNS label.
pub fn node_name_for(hostname: &str) -> String {
    let mut n: String = hostname
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else {
                '-'
            }
        })
        .collect();
    n.truncate(63);
    let n = n.trim_matches('-').to_string();
    if n.is_empty() {
        "node".into()
    } else {
        n
    }
}

async fn set_cluster(pool: &DbPool, id: Uuid, status: &str, message: &str) {
    let _ = crate::db::query(
        "UPDATE k8s_clusters SET status = ?, message = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(status)
    .bind(message)
    .bind(id)
    .execute(pool)
    .await;
}

async fn set_node(pool: &DbPool, id: Uuid, status: &str, message: &str, version: &str) {
    let _ = crate::db::query(
        "UPDATE k8s_cluster_nodes SET status = ?, message = ?,
                version = CASE WHEN ? <> '' THEN ? ELSE version END,
                updated_at = datetime('now') WHERE id = ?",
    )
    .bind(status)
    .bind(message)
    .bind(version)
    .bind(version)
    .bind(id)
    .execute(pool)
    .await;
}

/// Everything the agent call needs about a host.
struct Target {
    addr: String,
    hostname: String,
    address: String,
}

async fn target(state: &AppState, host_id: Uuid) -> anyhow::Result<Target> {
    let (hostname, addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id).await?;
    let address: String = crate::db::query_scalar("SELECT address FROM hosts WHERE id = ?")
        .bind(host_id)
        .fetch_one(&state.pool)
        .await?;
    Ok(Target {
        addr,
        hostname,
        address,
    })
}

pub struct ProvisionPlan {
    pub cluster_id: Uuid,
    pub version: String,
    pub server: (Uuid, Uuid), // (node row id, host id)
    pub agents: Vec<(Uuid, Uuid)>,
    pub install_url: String,
}

/// Install the server, store its token and kubeconfig, then join each agent in turn. The first
/// failure marks the cluster `error` and stops; nodes already installed stay (remove or retry).
pub async fn provision(state: AppState, plan: ProvisionPlan) {
    let pool = &state.pool;
    let id = plan.cluster_id;
    let run = async {
        let (server_node, server_host) = plan.server;
        let t = target(&state, server_host).await?;
        let api_url = api_url_for(&t.address).map_err(anyhow::Error::msg)?;
        let node_name = node_name_for(&t.hostname);
        let res = agent_client::k3s_node(
            &t.addr,
            K3sNodeCall {
                action: "install_server".into(),
                version: plan.version.clone(),
                node_name: node_name.clone(),
                advertise_address: t.address.trim().to_string(),
                install_url: plan.install_url.clone(),
                ..Default::default()
            },
        )
        .await?;
        let token_enc = crate::engine::ai::crypto::store_api_key(&res.node_token)?;
        let kubeconfig_enc = crate::engine::ai::crypto::store_api_key(&res.kubeconfig)?;
        crate::db::query(
            "UPDATE k8s_clusters SET api_url = ?, token_enc = ?, kubeconfig_enc = ? WHERE id = ?",
        )
        .bind(&api_url)
        .bind(&token_enc)
        .bind(&kubeconfig_enc)
        .bind(id)
        .execute(pool)
        .await?;
        set_node(pool, server_node, "ready", "", &plan.version).await;

        for (node_id, host_id) in &plan.agents {
            if let Err(e) = join_agent(
                &state,
                id,
                *node_id,
                *host_id,
                &plan.version,
                &plan.install_url,
            )
            .await
            {
                set_node(pool, *node_id, "error", &e.to_string(), "").await;
                return Err(e.context("joining an agent node failed"));
            }
        }
        anyhow::Ok(())
    };
    match run.await {
        Ok(()) => set_cluster(pool, id, "ready", "").await,
        Err(e) => {
            tracing::warn!(cluster = %id, "k3s cluster provision failed: {e:#}");
            // Mark the server node too when it never became ready.
            let _ = crate::db::query(
                "UPDATE k8s_cluster_nodes SET status = 'error', message = ?
                 WHERE id = ? AND status = 'pending'",
            )
            .bind(format!("{e:#}"))
            .bind(plan.server.0)
            .execute(pool)
            .await;
            set_cluster(pool, id, "error", &format!("{e:#}")).await;
        }
    }
}

/// Join one host to an existing cluster as an agent node.
pub async fn join_agent(
    state: &AppState,
    cluster_id: Uuid,
    node_id: Uuid,
    host_id: Uuid,
    version: &str,
    install_url: &str,
) -> anyhow::Result<()> {
    let (api_url, token_enc): (String, String) =
        crate::db::query_as("SELECT api_url, token_enc FROM k8s_clusters WHERE id = ?")
            .bind(cluster_id)
            .fetch_one(&state.pool)
            .await?;
    if api_url.is_empty() || token_enc.is_empty() {
        anyhow::bail!("the cluster has no server yet");
    }
    let token = crate::engine::ai::crypto::load_api_key(&token_enc)?;
    let t = target(state, host_id).await?;
    agent_client::k3s_node(
        &t.addr,
        K3sNodeCall {
            action: "join_agent".into(),
            version: version.to_string(),
            server_url: api_url,
            token,
            node_name: node_name_for(&t.hostname),
            install_url: install_url.to_string(),
            ..Default::default()
        },
    )
    .await?;
    set_node(&state.pool, node_id, "ready", "", version).await;
    Ok(())
}

/// Re-run the installer at `version` on the server first, then each agent, one at a time. Stops at
/// the first failure and records it; nodes already upgraded stay upgraded.
pub async fn upgrade(state: AppState, cluster_id: Uuid, version: String, install_url: String) {
    let pool = &state.pool;
    let run = async {
        let nodes: Vec<(Uuid, Uuid, String)> = crate::db::query_as(
            "SELECT id, host_id, role FROM k8s_cluster_nodes
             WHERE cluster_id = ? AND status = 'ready'
             ORDER BY CASE WHEN role = 'server' THEN 0 ELSE 1 END, created_at",
        )
        .bind(cluster_id)
        .fetch_all(pool)
        .await?;
        for (node_id, host_id, _role) in nodes {
            let t = target(&state, host_id).await?;
            let res = agent_client::k3s_node(
                &t.addr,
                K3sNodeCall {
                    action: "upgrade".into(),
                    version: version.clone(),
                    install_url: install_url.clone(),
                    ..Default::default()
                },
            )
            .await;
            match res {
                Ok(_) => set_node(pool, node_id, "ready", "", &version).await,
                Err(e) => {
                    set_node(pool, node_id, "error", &e.to_string(), "").await;
                    return Err(e.context(format!("upgrading {} failed", t.hostname)));
                }
            }
        }
        crate::db::query("UPDATE k8s_clusters SET version = ? WHERE id = ?")
            .bind(&version)
            .bind(cluster_id)
            .execute(pool)
            .await?;
        anyhow::Ok(())
    };
    match run.await {
        Ok(()) => set_cluster(pool, cluster_id, "ready", "").await,
        Err(e) => set_cluster(pool, cluster_id, "error", &format!("{e:#}")).await,
    }
}

/// Remove k3s from one node and delete its row. The node is kept (status `error`) if the host
/// refuses, so the failure stays visible.
pub async fn remove_node(state: &AppState, node_id: Uuid, host_id: Uuid) -> anyhow::Result<()> {
    set_node(&state.pool, node_id, "removing", "", "").await;
    let t = target(state, host_id).await?;
    match agent_client::k3s_node(
        &t.addr,
        K3sNodeCall {
            action: "remove".into(),
            ..Default::default()
        },
    )
    .await
    {
        Ok(_) => {
            crate::db::query("DELETE FROM k8s_cluster_nodes WHERE id = ?")
                .bind(node_id)
                .execute(&state.pool)
                .await?;
            Ok(())
        }
        Err(e) => {
            set_node(&state.pool, node_id, "error", &e.to_string(), "").await;
            Err(e)
        }
    }
}

/// Remove every node (agents first, server last) and then the cluster row. Stops at the first node
/// that cannot be removed, leaving the cluster in `error`.
pub async fn destroy(state: AppState, cluster_id: Uuid) {
    let pool = &state.pool;
    let run = async {
        let nodes: Vec<(Uuid, Uuid)> = crate::db::query_as(
            "SELECT id, host_id FROM k8s_cluster_nodes WHERE cluster_id = ?
             ORDER BY CASE WHEN role = 'server' THEN 1 ELSE 0 END, created_at",
        )
        .bind(cluster_id)
        .fetch_all(pool)
        .await?;
        for (node_id, host_id) in nodes {
            remove_node(&state, node_id, host_id).await?;
        }
        crate::db::query("DELETE FROM k8s_clusters WHERE id = ?")
            .bind(cluster_id)
            .execute(pool)
            .await?;
        anyhow::Ok(())
    };
    if let Err(e) = run.await {
        set_cluster(pool, cluster_id, "error", &format!("delete failed: {e:#}")).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_url_from_an_address() {
        assert_eq!(api_url_for("10.0.0.5").unwrap(), "https://10.0.0.5:6443");
        assert_eq!(
            api_url_for(" k8s.example.com ").unwrap(),
            "https://k8s.example.com:6443"
        );
        assert_eq!(api_url_for("fd00::5").unwrap(), "https://[fd00::5]:6443");
        assert!(api_url_for("").is_err());
        assert!(api_url_for("10.0.0.5/evil").is_err());
    }

    #[test]
    fn node_names_are_dns_labels() {
        assert_eq!(node_name_for("Host_01.example.com"), "host-01");
        assert_eq!(node_name_for("---"), "node");
        assert!(machina_core::k3s_node::validate_node_name(&node_name_for("A_b.c")).is_ok());
        assert!(node_name_for(&"x".repeat(100)).len() <= 63);
    }

    #[tokio::test]
    async fn fleet_is_off_unless_opted_in() {
        std::env::remove_var(FLEET_ENV);
        assert!(!enabled());
    }

    #[tokio::test]
    async fn schema_accepts_a_cluster_and_nodes_and_cascades() {
        let pool = crate::db::testing::pool().await;
        let host = Uuid::new_v4();
        crate::db::query("INSERT INTO hosts (id, hostname, address) VALUES (?, 'h1', '10.0.0.5')")
            .bind(host)
            .execute(&pool)
            .await
            .unwrap();
        let cid = Uuid::new_v4();
        crate::db::query(
            "INSERT INTO k8s_clusters (id, name, version) VALUES (?, 'dev', 'v1.31.4+k3s1')",
        )
        .bind(cid)
        .execute(&pool)
        .await
        .unwrap();
        let nid = Uuid::new_v4();
        crate::db::query(
            "INSERT INTO k8s_cluster_nodes (id, cluster_id, host_id, role) VALUES (?, ?, ?, 'server')",
        )
        .bind(nid)
        .bind(cid)
        .bind(host)
        .execute(&pool)
        .await
        .unwrap();
        set_node(&pool, nid, "ready", "", "v1.31.4+k3s1").await;
        let (status, version): (String, String) =
            crate::db::query_as("SELECT status, version FROM k8s_cluster_nodes WHERE id = ?")
                .bind(nid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            (status.as_str(), version.as_str()),
            ("ready", "v1.31.4+k3s1")
        );
        set_cluster(&pool, cid, "ready", "").await;
        // A second node on the same host in the same cluster is refused.
        let dup = crate::db::query(
            "INSERT INTO k8s_cluster_nodes (id, cluster_id, host_id, role) VALUES (?, ?, ?, 'agent')",
        )
        .bind(Uuid::new_v4())
        .bind(cid)
        .bind(host)
        .execute(&pool)
        .await;
        assert!(dup.is_err());
        // Deleting the cluster removes its nodes.
        crate::db::query("DELETE FROM k8s_clusters WHERE id = ?")
            .bind(cid)
            .execute(&pool)
            .await
            .unwrap();
        let n: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM k8s_cluster_nodes")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }
}
