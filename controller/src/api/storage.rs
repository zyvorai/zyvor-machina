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
pub struct StoragePoolRow {
    pub id: Uuid,
    pub name: String,
    pub storage_class: String,
    pub backend: String,
    pub path: Option<String>,
    pub capacity_gib: i64,
    pub used_gib: i64,
    pub tier_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct CreateStoragePoolBody {
    pub name: String,
    #[serde(default = "default_class")]
    pub storage_class: String,
    #[serde(default = "default_backend")]
    pub backend: String,
    pub path: Option<String>,
    #[serde(default)]
    pub capacity_gib: i64,
    #[serde(default)]
    pub host_id: Option<Uuid>,
}

fn default_class() -> String {
    "silver".into()
}
fn default_backend() -> String {
    "directory".into()
}

pub async fn discover_storage_pools(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let imported = crate::engine::storage_sync::discover_all_online(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let rows = crate::db::query_as::<_, StoragePoolRow>(
        "SELECT id, name, storage_class, backend, path, capacity_gib, used_gib, tier_id
         FROM storage_pools ORDER BY name",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(serde_json::json!({
        "imported": imported,
        "pools": rows,
    })))
}

pub async fn get_storage_pool(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<StoragePoolRow>, ApiError> {
    require_operator(&actor)?;
    let row = crate::db::query_as::<_, StoragePoolRow>(
        "SELECT id, name, storage_class, backend, path, capacity_gib, used_gib, tier_id FROM storage_pools WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| ApiError::not_found("storage pool not found"))?;
    Ok(Json(row))
}

pub async fn list_storage_pools(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<StoragePoolRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, StoragePoolRow>(
        "SELECT id, name, storage_class, backend, path, capacity_gib, used_gib, tier_id
         FROM storage_pools ORDER BY name",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn create_storage_pool(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateStoragePoolBody>,
) -> Result<Json<StoragePoolRow>, ApiError> {
    require_operator(&actor)?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let cluster_id: Uuid = crate::db::query_scalar("SELECT id FROM clusters LIMIT 1")
        .fetch_one(&state.pool)
        .await?;
    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO storage_pools (id, cluster_id, name, storage_class, backend, path, capacity_gib)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(cluster_id)
    .bind(&body.name)
    .bind(&body.storage_class)
    .bind(&body.backend)
    .bind(&body.path)
    .bind(body.capacity_gib)
    .execute(&state.pool)
    .await?;

    let row = crate::db::query_as::<_, StoragePoolRow>(
        "SELECT id, name, storage_class, backend, path, capacity_gib, used_gib, tier_id FROM storage_pools WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;

    if body.path.is_some() {
        let host_id = match body.host_id {
            Some(h) => h,
            None => crate::db::query_scalar(
                "SELECT id FROM hosts WHERE state = 'online' ORDER BY hostname LIMIT 1",
            )
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::bad_request("no online host for storage provisioning"))?,
        };
        let _ = enqueue_task(
            &state,
            "storage.pool.provision",
            serde_json::json!({
                "pool_id": id.to_string(),
                "host_id": host_id.to_string(),
            }),
            Some("storage_pool"),
            Some(id),
            Some(host_id),
        )
        .await?;
    }

    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct PatchStoragePoolBody {
    pub path: Option<String>,
    pub capacity_gib: Option<i64>,
    pub used_gib: Option<i64>,
    pub tier_id: Option<Uuid>,
}

pub async fn patch_storage_pool(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchStoragePoolBody>,
) -> Result<Json<StoragePoolRow>, ApiError> {
    require_operator(&actor)?;
    if let Some(v) = &body.path {
        crate::db::query("UPDATE storage_pools SET path = ? WHERE id = ?")
            .bind(v)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(v) = body.capacity_gib {
        crate::db::query("UPDATE storage_pools SET capacity_gib = ? WHERE id = ?")
            .bind(v)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(v) = body.used_gib {
        crate::db::query("UPDATE storage_pools SET used_gib = ? WHERE id = ?")
            .bind(v)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(tier) = body.tier_id {
        crate::engine::storage_tiers::bind_pool_tier(&state.pool, id, tier)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
    }
    let row = crate::db::query_as::<_, StoragePoolRow>(
        "SELECT id, name, storage_class, backend, path, capacity_gib, used_gib, tier_id FROM storage_pools WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

pub async fn delete_storage_pool(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<StoragePoolHostQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let name = storage_pool_name(&state.pool, id).await?;
    // Best-effort: still prune the controller-side record even when no host is
    // reachable to run the libvirt delete (e.g. the host is offline). Surface the
    // host-side outcome instead of silently discarding it, so callers can tell a
    // clean delete from one where the libvirt pool may still exist on the host.
    let host_delete_error = match resolve_online_host(&state.pool, q.host_id).await {
        Ok(host_id) => invoke_pool_action(
            &state,
            host_id,
            "storage.pool.delete",
            &name,
            serde_json::json!({ "name": name }),
        )
        .await
        .err()
        .map(|e| e.message),
        Err(e) => Some(e.message),
    };
    crate::db::query("DELETE FROM storage_pools WHERE id = ?")
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
pub struct StoragePoolHostQuery {
    pub host_id: Option<Uuid>,
}

async fn storage_pool_name(pool: &crate::db::DbPool, id: Uuid) -> Result<String, ApiError> {
    crate::db::query_scalar("SELECT name FROM storage_pools WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ApiError::bad_request("storage pool not found"))
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
        .ok_or_else(|| ApiError::bad_request("no online host for libvirt storage operation"))
}

async fn invoke_pool_on_host(
    state: &AppState,
    host_id: Uuid,
    action: &str,
    pool_name: &str,
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
        &serde_json::json!({ "name": pool_name }),
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn activate_storage_pool(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<StoragePoolHostQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let name = storage_pool_name(&state.pool, id).await?;
    let host_id = resolve_online_host(&state.pool, q.host_id).await?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let result = crate::agent_client::host_libvirt_invoke(
        &mut client,
        "storage.pool.start",
        &serde_json::json!({ "name": name }),
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    let _ = crate::engine::storage_sync::sync_host_storage(&state.pool, host_id, &agent_addr).await;
    state.emit_event("storage.pool", format!("Activated storage pool {name}"));
    Ok(Json(result))
}

pub async fn deactivate_storage_pool(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<StoragePoolHostQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let name = storage_pool_name(&state.pool, id).await?;
    let host_id = resolve_online_host(&state.pool, q.host_id).await?;
    let result = invoke_pool_on_host(&state, host_id, "storage.pool.stop", &name).await?;
    state.emit_event("storage.pool", format!("Deactivated storage pool {name}"));
    Ok(Json(result))
}

pub async fn refresh_storage_pool(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<StoragePoolHostQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let name = storage_pool_name(&state.pool, id).await?;
    let host_id = resolve_online_host(&state.pool, q.host_id).await?;
    let result = invoke_pool_on_host(&state, host_id, "storage.pool.refresh", &name).await?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let _ = crate::engine::storage_sync::sync_host_storage(&state.pool, host_id, &agent_addr).await;
    Ok(Json(result))
}

pub async fn live_storage_pools(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<StoragePoolHostQuery>,
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
    let pools = crate::agent_client::host_libvirt_query(
        &mut client,
        "storage.pools.list",
        &serde_json::json!({}),
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "host_id": host_id, "pools": pools }),
    ))
}

async fn invoke_pool_action(
    state: &AppState,
    host_id: Uuid,
    action: &str,
    pool_name: &str,
    payload: serde_json::Value,
) -> Result<serde_json::Value, ApiError> {
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut body = payload;
    if let Some(obj) = body.as_object_mut() {
        obj.insert(
            "pool".into(),
            serde_json::Value::String(pool_name.to_string()),
        );
    }
    crate::agent_client::host_libvirt_invoke(&mut client, action, &body)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn list_storage_pool_volumes(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<StoragePoolHostQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let name = storage_pool_name(&state.pool, id).await?;
    let host_id = resolve_online_host(&state.pool, q.host_id).await?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let volumes = crate::agent_client::host_libvirt_query(
        &mut client,
        "storage.volumes.list",
        &serde_json::json!({ "pool": name }),
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "pool": name, "volumes": volumes }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct CreateStorageVolumeBody {
    pub name: String,
    pub capacity_gb: u64,
    #[serde(default = "default_vol_format")]
    pub format: String,
}

fn default_vol_format() -> String {
    "qcow2".into()
}

pub async fn create_storage_pool_volume(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<StoragePoolHostQuery>,
    Json(body): Json<CreateStorageVolumeBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let name = storage_pool_name(&state.pool, id).await?;
    let host_id = resolve_online_host(&state.pool, q.host_id).await?;
    let result = invoke_pool_action(
        &state,
        host_id,
        "storage.volume.create",
        &name,
        serde_json::json!({
            "name": body.name,
            "capacity_gb": body.capacity_gb.max(1),
            "format": body.format,
        }),
    )
    .await?;
    state.emit_event(
        "storage.volume",
        format!("Created volume {} in pool {name}", body.name),
    );
    Ok(Json(result))
}

/// Delete a volume file from a storage pool on `host_id` without a request context (used on instance delete).
pub(crate) async fn delete_pool_volume_file(
    state: &AppState,
    host_id: Uuid,
    pool_id: Uuid,
    vol_name: &str,
) -> Result<(), ApiError> {
    let name = storage_pool_name(&state.pool, pool_id).await?;
    invoke_pool_action(state, host_id, "storage.volume.delete", &name, serde_json::json!({ "name": vol_name })).await?;
    Ok(())
}

pub async fn delete_storage_pool_volume(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((id, vol_name)): Path<(Uuid, String)>,
    Query(q): Query<StoragePoolHostQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let name = storage_pool_name(&state.pool, id).await?;
    let host_id = resolve_online_host(&state.pool, q.host_id).await?;
    let result = invoke_pool_action(
        &state,
        host_id,
        "storage.volume.delete",
        &name,
        serde_json::json!({ "name": vol_name }),
    )
    .await?;
    state.emit_event(
        "storage.volume",
        format!("Deleted volume {vol_name} from pool {name}"),
    );
    Ok(Json(result))
}
