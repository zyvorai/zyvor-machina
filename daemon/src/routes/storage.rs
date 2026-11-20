// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Extension, Path, Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};

use machina_core::libvirt::storage;
use machina_core::{CreateVolumeRequest, LibvirtManager, StoragePoolInfo, StorageVolumeInfo};

use crate::auth::{require_write, RequestActor};
use crate::conn_query::{spawn_libvirt_actor, ConnQuery};
use crate::error::{ok_json, AppError};

async fn list_pools(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<Vec<StoragePoolInfo>>, AppError> {
    let rows = spawn_libvirt_actor(manager, Some(&actor), conn_q, storage::list_pools).await?;
    Ok(Json(rows))
}

async fn list_volumes(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(pool_name): Path<String>,
) -> Result<Json<Vec<StorageVolumeInfo>>, AppError> {
    let pool2 = pool_name.clone();
    let rows = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        storage::list_volumes(conn, &pool2)
    })
    .await?;
    Ok(Json(rows))
}

async fn start_pool(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name2 = name.clone();
    require_write(&actor, "storage:write")?;
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        storage::start_pool(conn, &name2)
    })
    .await?;
    Ok(ok_json("started", &name))
}

async fn stop_pool(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name2 = name.clone();
    require_write(&actor, "storage:write")?;
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        storage::stop_pool(conn, &name2)
    })
    .await?;
    Ok(ok_json("stopped", &name))
}

async fn refresh_pool(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name2 = name.clone();
    require_write(&actor, "storage:write")?;
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        storage::refresh_pool(conn, &name2)
    })
    .await?;
    Ok(ok_json("refreshed", &name))
}

async fn set_pool_autostart(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path((name, enabled)): Path<(String, bool)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name2 = name.clone();
    require_write(&actor, "storage:write")?;
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        storage::set_pool_autostart(conn, &name2, enabled)
    })
    .await?;
    let label = if enabled { "enabled" } else { "disabled" };
    Ok(ok_json(label, &name))
}

async fn delete_volume(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path((pool_name, vol_name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool2 = pool_name.clone();
    let vol2 = vol_name.clone();
    require_write(&actor, "storage:write")?;
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        storage::delete_volume(conn, &pool2, &vol2)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "deleted", "pool": pool_name, "volume": vol_name }),
    ))
}

async fn create_volume(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(pool_name): Path<String>,
    Json(req): Json<CreateVolumeRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let vol_name = req.name.clone();
    let pool2 = pool_name.clone();
    let req2 = req.clone();
    require_write(&actor, "storage:write")?;
    // Unlike resize_volume (advanced.rs), this had no upper bound at all: a
    // storage:write actor could request an arbitrarily large capacity_gb,
    // relying entirely on the pool running out of space to fail it. Reuse the
    // same 1 GB–10 TB validator every other disk-size input in core goes
    // through (create.rs, device.rs, virt_builder.rs, virt_install.rs).
    machina_core::validate::validate_disk_gb(req2.capacity_gb)?;
    let path = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        storage::create_volume(conn, &pool2, &req2.name, req2.capacity_gb, &req2.format)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "created", "pool": pool_name, "volume": vol_name, "path": path }),
    ))
}

pub fn storage_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/storage/pools", get(list_pools))
        .route("/storage/pools/{name}/start", post(start_pool))
        .route("/storage/pools/{name}/stop", post(stop_pool))
        .route("/storage/pools/{name}/refresh", post(refresh_pool))
        .route(
            "/storage/pools/{name}/autostart/{enabled}",
            post(set_pool_autostart),
        )
        .route("/storage/pools/{pool_name}/volumes", get(list_volumes))
        .route("/storage/pools/{pool_name}/volumes", post(create_volume))
        .route(
            "/storage/pools/{pool_name}/volumes/{vol_name}",
            delete(delete_volume),
        )
}
