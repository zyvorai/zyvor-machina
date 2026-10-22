// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Fleet-managed k3s clusters (admin only, off unless `MACHINA_K3S_FLEET=1`). Create, scale, upgrade
//! and delete are asynchronous: the call records the intent and returns the cluster, a background
//! task does the work on the hosts, and the client polls `GET /api/v1/k8s-clusters/{id}`.

use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, AuthUser};
use crate::engine::k8s_clusters as engine;
use crate::state::AppState;
use crate::tasks::enqueue::write_audit;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ClusterRow {
    pub id: Uuid,
    pub name: String,
    pub version: String,
    pub status: String,
    pub message: String,
    pub api_url: String,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct NodeRow {
    pub id: Uuid,
    pub cluster_id: Uuid,
    pub host_id: Uuid,
    pub role: String,
    pub node_name: String,
    pub version: String,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ClusterDetail {
    #[serde(flatten)]
    pub cluster: ClusterRow,
    pub nodes: Vec<NodeRow>,
}

const CLUSTER_COLS: &str =
    "id, name, version, status, message, api_url, created_by, created_at, updated_at";
const NODE_COLS: &str = "id, cluster_id, host_id, role, node_name, version, status, message";

fn require_enabled() -> Result<(), ApiError> {
    if engine::enabled() {
        Ok(())
    } else {
        Err(ApiError::forbidden("fleet k3s clusters are disabled")
            .with_code("k8s_fleet_disabled")
            .with_remediation(
                "Set MACHINA_K3S_FLEET=1 on the controller, and MACHINA_AGENT_K3S_ENABLED=1 on each host that may run k3s.",
            ))
    }
}

async fn load(state: &AppState, id: Uuid) -> Result<ClusterDetail, ApiError> {
    let cluster: ClusterRow = crate::db::query_as(&format!(
        "SELECT {CLUSTER_COLS} FROM k8s_clusters WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("cluster not found"))?;
    let nodes: Vec<NodeRow> = crate::db::query_as(&format!(
        "SELECT {NODE_COLS} FROM k8s_cluster_nodes WHERE cluster_id = ? ORDER BY CASE WHEN role = 'server' THEN 0 ELSE 1 END, created_at"
    ))
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    Ok(ClusterDetail { cluster, nodes })
}

fn validate_install_url(u: &Option<String>) -> Result<String, ApiError> {
    match u.as_deref().map(str::trim) {
        None | Some("") => Ok(String::new()),
        Some(u) => machina_core::k3s_node::validate_install_url(u)
            .map(|()| u.to_string())
            .map_err(ApiError::bad_request),
    }
}

async fn host_exists(state: &AppState, id: Uuid) -> Result<(), ApiError> {
    let found: Option<(Uuid,)> = crate::db::query_as("SELECT id FROM hosts WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    if found.is_some() {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!("host {id} not found")))
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    pub name: String,
    pub version: String,
    /// A single-server cluster's server. Use `server_host_ids` for an HA control plane.
    #[serde(default)]
    pub server_host_id: Option<Uuid>,
    /// An HA control plane: 3 or 5 servers (odd, for etcd quorum). The first bootstraps the cluster.
    #[serde(default)]
    pub server_host_ids: Option<Vec<Uuid>>,
    #[serde(default)]
    pub agent_host_ids: Vec<Uuid>,
    #[serde(default)]
    pub install_url: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateBody>,
) -> Result<Json<ClusterDetail>, ApiError> {
    require_admin(&actor)?;
    require_enabled()?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    machina_core::k3s_node::validate_version(&body.version).map_err(ApiError::bad_request)?;
    let install_url = validate_install_url(&body.install_url)?;
    if body.agent_host_ids.len() > 50 {
        return Err(ApiError::bad_request("at most 50 agent nodes per request"));
    }
    let servers: Vec<Uuid> = match (body.server_host_id, body.server_host_ids.clone()) {
        (Some(_), Some(_)) => {
            return Err(ApiError::bad_request(
                "give server_host_id or server_host_ids, not both",
            ))
        }
        (Some(one), None) => vec![one],
        (None, Some(many)) => many,
        (None, None) => return Err(ApiError::bad_request("a server host is required")),
    };
    if !engine::valid_server_count(servers.len()) {
        return Err(ApiError::bad_request(
            "a control plane is 1 server, or 3 or 5 for HA (etcd needs an odd number)",
        ));
    }
    let mut all = servers.clone();
    all.extend(&body.agent_host_ids);
    let unique: std::collections::HashSet<_> = all.iter().collect();
    if unique.len() != all.len() {
        return Err(ApiError::bad_request("each host can appear only once"));
    }
    for h in &all {
        host_exists(&state, *h).await?;
    }

    let cluster_id = Uuid::new_v4();
    let server_nodes: Vec<(Uuid, Uuid)> = servers.iter().map(|h| (Uuid::new_v4(), *h)).collect();
    let agents: Vec<(Uuid, Uuid)> = body
        .agent_host_ids
        .iter()
        .map(|h| (Uuid::new_v4(), *h))
        .collect();

    let mut tx = state.pool.begin().await?;
    crate::db::query(
        "INSERT INTO k8s_clusters (id, name, version, created_by) VALUES (?, ?, ?, ?)",
    )
    .bind(cluster_id)
    .bind(&body.name)
    .bind(&body.version)
    .bind(&actor.username)
    .execute(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref d) if d.is_unique_violation() => {
            ApiError::conflict("a cluster with that name exists", "choose another name")
        }
        other => other.into(),
    })?;
    for (node_id, host_id, role) in server_nodes
        .iter()
        .map(|(n, h)| (*n, *h, "server"))
        .chain(agents.iter().map(|(n, h)| (*n, *h, "agent")))
    {
        crate::db::query(
            "INSERT INTO k8s_cluster_nodes (id, cluster_id, host_id, role) VALUES (?, ?, ?, ?)",
        )
        .bind(node_id)
        .bind(cluster_id)
        .bind(host_id)
        .bind(role)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    write_audit(
        &state,
        &actor.username,
        "k8s_cluster.create",
        "k8s_cluster",
        Some(cluster_id),
        serde_json::json!({ "name": body.name, "version": body.version, "nodes": all.len(), "servers": servers.len() }),
    )
    .await?;

    tokio::spawn(engine::provision(
        state.clone(),
        engine::ProvisionPlan {
            cluster_id,
            version: body.version.clone(),
            server: server_nodes[0],
            extra_servers: server_nodes[1..].to_vec(),
            agents,
            install_url,
        },
    ));
    load(&state, cluster_id).await.map(Json)
}

pub async fn list(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<ClusterRow>>, ApiError> {
    crate::auth::require_operator(&actor)?;
    require_enabled()?;
    let rows: Vec<ClusterRow> = crate::db::query_as(&format!(
        "SELECT {CLUSTER_COLS} FROM k8s_clusters ORDER BY name LIMIT 500"
    ))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn get(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<ClusterDetail>, ApiError> {
    crate::auth::require_operator(&actor)?;
    require_enabled()?;
    load(&state, id).await.map(Json)
}

#[derive(Debug, Deserialize)]
pub struct AddNodeBody {
    pub host_id: Uuid,
    #[serde(default)]
    pub install_url: Option<String>,
}

pub async fn add_node(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<AddNodeBody>,
) -> Result<Json<ClusterDetail>, ApiError> {
    require_admin(&actor)?;
    require_enabled()?;
    let install_url = validate_install_url(&body.install_url)?;
    let detail = load(&state, id).await?;
    if detail.cluster.status != "ready" {
        return Err(ApiError::conflict(
            format!("cluster is {}", detail.cluster.status),
            "wait until it is ready",
        ));
    }
    host_exists(&state, body.host_id).await?;
    let node_id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO k8s_cluster_nodes (id, cluster_id, host_id, role) VALUES (?, ?, ?, 'agent')",
    )
    .bind(node_id)
    .bind(id)
    .bind(body.host_id)
    .execute(&state.pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref d) if d.is_unique_violation() => {
            ApiError::conflict("that host is already in the cluster", "pick another host")
        }
        other => other.into(),
    })?;
    write_audit(
        &state,
        &actor.username,
        "k8s_cluster.add_node",
        "k8s_cluster",
        Some(id),
        serde_json::json!({ "host_id": body.host_id }),
    )
    .await?;
    let version = detail.cluster.version.clone();
    let st = state.clone();
    tokio::spawn(async move {
        if let Err(e) =
            engine::join_agent(&st, id, node_id, body.host_id, &version, &install_url).await
        {
            let _ = crate::db::query(
                "UPDATE k8s_cluster_nodes SET status = 'error', message = ? WHERE id = ?",
            )
            .bind(format!("{e:#}"))
            .bind(node_id)
            .execute(&st.pool)
            .await;
        }
    });
    load(&state, id).await.map(Json)
}

#[derive(Debug, Default, Deserialize)]
pub struct RemoveQuery {
    /// Skip draining the node first (pods on it are lost, not rescheduled).
    #[serde(default)]
    pub force: bool,
}

pub async fn remove_node(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((id, node_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<RemoveQuery>,
) -> Result<Json<ClusterDetail>, ApiError> {
    require_admin(&actor)?;
    require_enabled()?;
    let node: NodeRow = crate::db::query_as(&format!(
        "SELECT {NODE_COLS} FROM k8s_cluster_nodes WHERE id = ? AND cluster_id = ?"
    ))
    .bind(node_id)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("node not found"))?;
    if node.role == "server" {
        return Err(ApiError::bad_request(
            "a control-plane server cannot be removed on its own; delete the cluster",
        ));
    }
    write_audit(
        &state,
        &actor.username,
        "k8s_cluster.remove_node",
        "k8s_cluster",
        Some(id),
        serde_json::json!({ "host_id": node.host_id, "force": q.force }),
    )
    .await?;
    let st = state.clone();
    tokio::spawn(async move {
        let _ = engine::remove_node(&st, node_id, node.host_id, !q.force).await;
    });
    load(&state, id).await.map(Json)
}

#[derive(Debug, Deserialize)]
pub struct UpgradeBody {
    pub version: String,
    #[serde(default)]
    pub install_url: Option<String>,
    /// Skip draining each node before it is upgraded.
    #[serde(default)]
    pub skip_drain: bool,
}

pub async fn upgrade(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpgradeBody>,
) -> Result<Json<ClusterDetail>, ApiError> {
    require_admin(&actor)?;
    require_enabled()?;
    machina_core::k3s_node::validate_version(&body.version).map_err(ApiError::bad_request)?;
    let install_url = validate_install_url(&body.install_url)?;
    let detail = load(&state, id).await?;
    if detail.cluster.status != "ready" {
        return Err(ApiError::conflict(
            format!("cluster is {}", detail.cluster.status),
            "wait until it is ready",
        ));
    }
    crate::db::query("UPDATE k8s_clusters SET status = 'upgrading', message = '' WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    write_audit(
        &state,
        &actor.username,
        "k8s_cluster.upgrade",
        "k8s_cluster",
        Some(id),
        serde_json::json!({ "from": detail.cluster.version, "to": body.version, "skip_drain": body.skip_drain }),
    )
    .await?;
    tokio::spawn(engine::upgrade(
        state.clone(),
        id,
        body.version,
        install_url,
        body.skip_drain,
    ));
    load(&state, id).await.map(Json)
}

pub async fn delete(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<ClusterDetail>, ApiError> {
    require_admin(&actor)?;
    require_enabled()?;
    let detail = load(&state, id).await?;
    if !matches!(detail.cluster.status.as_str(), "ready" | "error") {
        return Err(ApiError::conflict(
            format!("cluster is {}", detail.cluster.status),
            "wait for the current operation to finish",
        ));
    }
    crate::db::query("UPDATE k8s_clusters SET status = 'deleting', message = '' WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    write_audit(
        &state,
        &actor.username,
        "k8s_cluster.delete",
        "k8s_cluster",
        Some(id),
        serde_json::json!({ "name": detail.cluster.name }),
    )
    .await?;
    tokio::spawn(engine::destroy(state.clone(), id));
    load(&state, id).await.map(Json)
}

/// The admin kubeconfig with the server address rewritten from 127.0.0.1 to the cluster's API URL.
pub async fn kubeconfig(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    require_admin(&actor)?;
    require_enabled()?;
    let (api_url, enc): (String, String) =
        crate::db::query_as("SELECT api_url, kubeconfig_enc FROM k8s_clusters WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::not_found("cluster not found"))?;
    if enc.is_empty() {
        return Err(ApiError::not_found("the cluster has no kubeconfig yet"));
    }
    let raw = crate::engine::ai::crypto::load_api_key(&enc)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    write_audit(
        &state,
        &actor.username,
        "k8s_cluster.kubeconfig",
        "k8s_cluster",
        Some(id),
        serde_json::json!({}),
    )
    .await?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/yaml"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        machina_core::k3s_node::kubeconfig_for(&raw, &api_url),
    ))
}
