// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, Query, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct NetworkRow {
    pub id: Uuid,
    pub name: String,
    pub backend: String,
    pub vlan_id: Option<i32>,
    pub bridge: Option<String>,
    pub segment_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct CreateNetworkBody {
    pub name: String,
    #[serde(default = "default_backend")]
    pub backend: String,
    pub vlan_id: Option<i32>,
    pub bridge: Option<String>,
    #[serde(default)]
    pub host_id: Option<Uuid>,
    #[serde(default)]
    pub segment_id: Option<Uuid>,
    pub firewall_profile: Option<String>,
}

fn default_backend() -> String {
    "linux-bridge".into()
}

pub async fn get_network(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<NetworkRow>, ApiError> {
    require_operator(&actor)?;
    let row = crate::db::query_as::<_, NetworkRow>(
        "SELECT id, name, backend, vlan_id, bridge, segment_id FROM networks WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| ApiError::not_found("network not found"))?;
    Ok(Json(row))
}

pub async fn list_networks(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<NetworkRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, NetworkRow>(
        "SELECT id, name, backend, vlan_id, bridge, segment_id FROM networks ORDER BY name",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn create_network(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateNetworkBody>,
) -> Result<Json<NetworkRow>, ApiError> {
    require_operator(&actor)?;
    if body.backend == "cloud-isolated" || body.name.starts_with("mc-") {
        return Err(ApiError::bad_request(
            "cloud networks must be created through the VPC subnet API",
        ));
    }
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let cluster_id: Uuid = crate::db::query_scalar("SELECT id FROM clusters LIMIT 1")
        .fetch_one(&state.pool)
        .await?;
    // Validate segment_id references an existing segment before inserting. The
    // PATCH path validates via network_overlay::bind_network, but create bound the
    // raw column with no check, so a bogus segment_id silently produced a dangling
    // overlay reference.
    if let Some(seg_id) = body.segment_id {
        let exists: bool =
            crate::db::query_scalar("SELECT EXISTS(SELECT 1 FROM network_segments WHERE id = ?)")
                .bind(seg_id)
                .fetch_one(&state.pool)
                .await?;
        if !exists {
            return Err(ApiError::bad_request(
                "segment_id does not reference an existing network segment",
            ));
        }
    }
    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO networks (id, cluster_id, name, backend, vlan_id, bridge, segment_id)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(cluster_id)
    .bind(&body.name)
    .bind(&body.backend)
    .bind(body.vlan_id)
    .bind(&body.bridge)
    .bind(body.segment_id)
    .execute(&state.pool)
    .await?;

    if let Some(seg_id) = body.segment_id {
        if let Some(prof) = body.firewall_profile.as_deref() {
            crate::db::query(
                "UPDATE network_segments SET firewall_profile = COALESCE(?, firewall_profile) WHERE id = ?",
            )
            .bind(prof)
            .bind(seg_id)
            .execute(&state.pool)
            .await?;
        }
    }

    let row = crate::db::query_as::<_, NetworkRow>(
        "SELECT id, name, backend, vlan_id, bridge, segment_id FROM networks WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;

    let host_id = match body.host_id {
        Some(h) => h,
        None => crate::db::query_scalar(
            "SELECT id FROM hosts WHERE state = 'online' ORDER BY hostname LIMIT 1",
        )
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::bad_request("no online host for network provisioning"))?,
    };
    let _ = enqueue_task(
        &state,
        "network.provision",
        serde_json::json!({
            "network_id": id.to_string(),
            "host_id": host_id.to_string(),
        }),
        Some("network"),
        Some(id),
        Some(host_id),
    )
    .await?;

    Ok(Json(row))
}

pub async fn discover_networks(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let imported = crate::engine::network_sync::discover_all_online(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let rows = crate::db::query_as::<_, NetworkRow>(
        "SELECT id, name, backend, vlan_id, bridge, segment_id FROM networks ORDER BY name",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(serde_json::json!({
        "imported": imported,
        "networks": rows,
    })))
}

#[derive(Debug, Deserialize)]
pub struct PatchNetworkBody {
    pub vlan_id: Option<i32>,
    pub bridge: Option<String>,
    pub segment_id: Option<Uuid>,
}

pub async fn patch_network(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchNetworkBody>,
) -> Result<Json<NetworkRow>, ApiError> {
    require_operator(&actor)?;
    crate::api::cloud::protect_network(&state, id).await?;
    if let Some(v) = body.vlan_id {
        crate::db::query("UPDATE networks SET vlan_id = ? WHERE id = ?")
            .bind(v)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(v) = &body.bridge {
        crate::db::query("UPDATE networks SET bridge = ? WHERE id = ?")
            .bind(v)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(seg) = body.segment_id {
        crate::engine::network_overlay::bind_network(&state.pool, id, seg)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
    }
    let row = crate::db::query_as::<_, NetworkRow>(
        "SELECT id, name, backend, vlan_id, bridge, segment_id FROM networks WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

pub async fn delete_network(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<NetworkHostQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    crate::api::cloud::protect_network(&state, id).await?;
    let name = network_name(&state.pool, id).await?;
    // Best-effort: still prune the controller-side record even when no host is
    // reachable to run the libvirt delete (e.g. the host is offline). Surface the
    // host-side outcome instead of silently discarding it, so callers can tell a
    // clean delete from one where the libvirt network may still exist on the host.
    let host_delete_error = match resolve_online_host(&state.pool, q.host_id).await {
        Ok(host_id) => invoke_network_on_host(&state, host_id, "network.delete", &name)
            .await
            .err()
            .map(|e| e.message),
        Err(e) => Some(e.message),
    };
    crate::db::query("DELETE FROM networks WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({
        "deleted": true,
        "name": name,
        "host_delete_error": host_delete_error,
    })))
}

#[derive(Debug, Deserialize)]
pub struct NetworkHostQuery {
    pub host_id: Option<Uuid>,
}

async fn network_name(pool: &crate::db::DbPool, id: Uuid) -> Result<String, ApiError> {
    crate::db::query_scalar("SELECT name FROM networks WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ApiError::bad_request("network not found"))
}

async fn resolve_online_host(
    pool: &crate::db::DbPool,
    host_id: Option<Uuid>,
) -> Result<Uuid, ApiError> {
    if let Some(h) = host_id {
        return Ok(h);
    }
    crate::db::query_scalar("SELECT id FROM hosts WHERE state = 'online' ORDER BY hostname LIMIT 1")
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ApiError::bad_request("no online host for libvirt network operation"))
}

async fn invoke_network_on_host(
    state: &AppState,
    host_id: Uuid,
    action: &str,
    network_name: &str,
) -> Result<serde_json::Value, ApiError> {
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    crate::agent_client::host_libvirt_invoke(
        &mut client,
        action,
        &serde_json::json!({ "name": network_name }),
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn activate_network(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<NetworkHostQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    crate::api::cloud::protect_network(&state, id).await?;
    let name = network_name(&state.pool, id).await?;
    let host_id = resolve_online_host(&state.pool, q.host_id).await?;
    let result = invoke_network_on_host(&state, host_id, "network.start", &name).await?;
    state.emit_event("network", format!("Activated libvirt network {name}"));
    Ok(Json(result))
}

pub async fn deactivate_network(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<NetworkHostQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    crate::api::cloud::protect_network(&state, id).await?;
    let name = network_name(&state.pool, id).await?;
    let host_id = resolve_online_host(&state.pool, q.host_id).await?;
    let result = invoke_network_on_host(&state, host_id, "network.stop", &name).await?;
    state.emit_event("network", format!("Deactivated libvirt network {name}"));
    Ok(Json(result))
}

pub async fn live_networks(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<NetworkHostQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let host_id = resolve_online_host(&state.pool, q.host_id).await?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let nets = crate::agent_client::host_libvirt_query(
        &mut client,
        "networks.list",
        &serde_json::json!({}),
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "host_id": host_id, "networks": nets }),
    ))
}
