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

/// A node as the ordering and drain rules see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeInfo {
    pub id: Uuid,
    pub host_id: Uuid,
    pub role: String,
    pub status: String,
}

/// Upgrade order: control-plane servers first (oldest first, so the bootstrap server leads), then
/// agents. `nodes` must already be ordered by creation.
pub fn upgrade_order(nodes: &[NodeInfo]) -> Vec<NodeInfo> {
    let servers = nodes.iter().filter(|n| n.role == "server");
    let agents = nodes.iter().filter(|n| n.role != "server");
    servers.chain(agents).cloned().collect()
}

/// The host that should run `kubectl drain` for `target`: the first ready server that is not the
/// target itself. None for a single-server cluster's own server, which has nowhere to drain to.
pub fn drain_host(nodes: &[NodeInfo], target: Uuid) -> Option<Uuid> {
    nodes
        .iter()
        .find(|n| n.role == "server" && n.status == "ready" && n.id != target)
        .map(|n| n.host_id)
}

/// Valid control-plane sizes: one server (SQLite), or an odd HA set for etcd quorum.
pub fn valid_server_count(n: usize) -> bool {
    matches!(n, 1 | 3 | 5)
}

pub struct ProvisionPlan {
    pub cluster_id: Uuid,
    pub version: String,
    pub server: (Uuid, Uuid), // (node row id, host id): the bootstrap server
    /// Extra control-plane servers (HA). Empty for a single-server cluster.
    pub extra_servers: Vec<(Uuid, Uuid)>,
    pub agents: Vec<(Uuid, Uuid)>,
    pub install_url: String,
}

/// Install the bootstrap server (with embedded etcd when more servers follow), store its token and
/// kubeconfig, join the other servers, then each agent, one at a time. The first failure marks the
/// cluster `error` and stops; nodes already installed stay (remove or retry).
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
                cluster_init: !plan.extra_servers.is_empty(),
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

        for (node_id, host_id) in &plan.extra_servers {
            if let Err(e) = join_node(
                &state,
                id,
                *node_id,
                *host_id,
                &plan.version,
                &plan.install_url,
                "join_server",
            )
            .await
            {
                set_node(pool, *node_id, "error", &e.to_string(), "").await;
                return Err(e.context("joining a server node failed"));
            }
        }
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
            // Mark the bootstrap server too when it never became ready.
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
    join_node(
        state,
        cluster_id,
        node_id,
        host_id,
        version,
        install_url,
        "join_agent",
    )
    .await
}

/// Join one host with `action` (`join_agent` or `join_server`).
async fn join_node(
    state: &AppState,
    cluster_id: Uuid,
    node_id: Uuid,
    host_id: Uuid,
    version: &str,
    install_url: &str,
    action: &str,
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
            action: action.into(),
            version: version.to_string(),
            server_url: api_url,
            token,
            node_name: node_name_for(&t.hostname),
            advertise_address: if action == "join_server" {
                t.address.trim().to_string()
            } else {
                String::new()
            },
            install_url: install_url.to_string(),
            ..Default::default()
        },
    )
    .await?;
    set_node(&state.pool, node_id, "ready", "", version).await;
    Ok(())
}

async fn cluster_nodes(pool: &DbPool, cluster_id: Uuid) -> anyhow::Result<Vec<NodeInfo>> {
    let rows: Vec<(Uuid, Uuid, String, String)> = crate::db::query_as(
        "SELECT id, host_id, role, status FROM k8s_cluster_nodes WHERE cluster_id = ? ORDER BY created_at",
    )
    .bind(cluster_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, host_id, role, status)| NodeInfo {
            id,
            host_id,
            role,
            status,
        })
        .collect())
}

/// Run a kubectl action (`drain`, `uncordon`, `delete_node`) for `node_name` on a server host.
async fn kubectl_on(
    state: &AppState,
    host_id: Uuid,
    action: &str,
    node_name: &str,
) -> anyhow::Result<()> {
    let t = target(state, host_id).await?;
    agent_client::k3s_node(
        &t.addr,
        K3sNodeCall {
            action: action.into(),
            node_name: node_name.into(),
            ..Default::default()
        },
    )
    .await?;
    Ok(())
}

/// Re-run the installer at `version`: servers first (the bootstrap server leading), then agents,
/// one node at a time. Each node is drained first and uncordoned after, unless `skip_drain` is set
/// or it is the only server (nothing to drain it from). A failed drain stops the upgrade before
/// that node is touched. Nodes already upgraded stay upgraded.
pub async fn upgrade(
    state: AppState,
    cluster_id: Uuid,
    version: String,
    install_url: String,
    skip_drain: bool,
) {
    let pool = &state.pool;
    let run = async {
        let all = cluster_nodes(pool, cluster_id).await?;
        let ready: Vec<NodeInfo> = all
            .iter()
            .filter(|n| n.status == "ready")
            .cloned()
            .collect();
        for node in upgrade_order(&ready) {
            let t = target(&state, node.host_id).await?;
            let name = node_name_for(&t.hostname);
            // Drain from another ready server, if there is one.
            let drainer = if skip_drain {
                None
            } else {
                drain_host(&all, node.id)
            };
            if let Some(h) = drainer {
                if let Err(e) = kubectl_on(&state, h, "drain", &name).await {
                    set_node(
                        pool,
                        node.id,
                        "error",
                        &format!("drain failed, not upgraded: {e:#}"),
                        "",
                    )
                    .await;
                    return Err(e.context(format!("draining {} failed", t.hostname)));
                }
            }
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
            if let Err(e) = res {
                set_node(pool, node.id, "error", &e.to_string(), "").await;
                return Err(e.context(format!("upgrading {} failed", t.hostname)));
            }
            if let Some(h) = drainer {
                if let Err(e) = kubectl_on(&state, h, "uncordon", &name).await {
                    set_node(
                        pool,
                        node.id,
                        "error",
                        &format!("upgraded, but uncordon failed: {e:#}"),
                        &version,
                    )
                    .await;
                    return Err(e.context(format!("uncordoning {} failed", t.hostname)));
                }
            }
            set_node(pool, node.id, "ready", "", &version).await;
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

/// Remove k3s from one node and delete its row. With `drain`, the node is drained from a server
/// first and its Node object deleted afterwards; a failed drain leaves the node in place (status
/// `error`) so nothing is removed from under running pods. The node is also kept if the host refuses.
pub async fn remove_node(
    state: &AppState,
    node_id: Uuid,
    host_id: Uuid,
    drain: bool,
) -> anyhow::Result<()> {
    set_node(&state.pool, node_id, "removing", "", "").await;
    let t = target(state, host_id).await?;
    let name = node_name_for(&t.hostname);
    let drainer = if drain {
        let cluster_id: Uuid =
            crate::db::query_scalar("SELECT cluster_id FROM k8s_cluster_nodes WHERE id = ?")
                .bind(node_id)
                .fetch_one(&state.pool)
                .await?;
        drain_host(&cluster_nodes(&state.pool, cluster_id).await?, node_id)
    } else {
        None
    };
    if let Some(h) = drainer {
        if let Err(e) = kubectl_on(state, h, "drain", &name).await {
            set_node(
                &state.pool,
                node_id,
                "error",
                &format!("drain failed, not removed: {e:#}"),
                "",
            )
            .await;
            return Err(e.context("draining the node failed"));
        }
    }
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
            // The Node object lingers in the cluster after k3s is gone; best effort.
            if let Some(h) = drainer {
                if let Err(e) = kubectl_on(state, h, "delete_node", &name).await {
                    tracing::warn!(node = %name, "deleting the Node object failed: {e:#}");
                }
            }
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

/// Remove every node (agents first, servers last) and then the cluster row. No drain: the whole
/// cluster is going away. Stops at the first node that cannot be removed, leaving the cluster in
/// `error`.
pub async fn destroy(state: AppState, cluster_id: Uuid) {
    let pool = &state.pool;
    let run = async {
        let nodes: Vec<(Uuid, Uuid)> = crate::db::query_as(
            "SELECT id, host_id FROM k8s_cluster_nodes WHERE cluster_id = ?
             ORDER BY CASE WHEN role = 'server' THEN 1 ELSE 0 END, created_at DESC",
        )
        .bind(cluster_id)
        .fetch_all(pool)
        .await?;
        for (node_id, host_id) in nodes {
            remove_node(&state, node_id, host_id, false).await?;
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

    fn node(role: &str, status: &str) -> NodeInfo {
        NodeInfo {
            id: Uuid::new_v4(),
            host_id: Uuid::new_v4(),
            role: role.into(),
            status: status.into(),
        }
    }

    #[test]
    fn upgrade_goes_servers_first_oldest_first_then_agents() {
        let (s1, a1, s2, a2) = (
            node("server", "ready"),
            node("agent", "ready"),
            node("server", "ready"),
            node("agent", "ready"),
        );
        let order = upgrade_order(&[s1.clone(), a1.clone(), s2.clone(), a2.clone()]);
        assert_eq!(order, vec![s1, s2, a1, a2]);
    }

    #[test]
    fn drain_runs_from_another_ready_server() {
        let (s1, s2, a1) = (
            node("server", "ready"),
            node("server", "ready"),
            node("agent", "ready"),
        );
        let all = [s1.clone(), s2.clone(), a1.clone()];
        // An agent drains from the first ready server.
        assert_eq!(drain_host(&all, a1.id), Some(s1.host_id));
        // A server drains from a different one.
        assert_eq!(drain_host(&all, s1.id), Some(s2.host_id));
        // A server that is not ready cannot run kubectl.
        let down = [node("server", "error"), a1.clone()];
        assert_eq!(drain_host(&down, a1.id), None);
        // A single-server cluster cannot drain its own server, but can drain its agents.
        let single = [s1.clone(), a1.clone()];
        assert_eq!(drain_host(&single, s1.id), None);
        assert_eq!(drain_host(&single, a1.id), Some(s1.host_id));
    }

    #[test]
    fn only_odd_control_planes_are_allowed() {
        assert!([1, 3, 5].iter().all(|n| valid_server_count(*n)));
        assert!([0, 2, 4, 6, 7].iter().all(|n| !valid_server_count(*n)));
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
