// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Extension, Path, Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};

use machina_core::libvirt::snapshot;
use machina_core::{CreateSnapshotRequest, LibvirtManager, SnapshotInfo};

use crate::auth::{require_write, RequestActor};
use crate::conn_query::{spawn_libvirt_actor, ConnQuery};
use crate::error::AppError;
use crate::routes::fluxvm::client as fluxvm_client;

async fn list_all_snapshots(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<Vec<SnapshotInfo>>, AppError> {
    let rows =
        spawn_libvirt_actor(manager, Some(&actor), conn_q, snapshot::list_all_snapshots).await?;
    Ok(Json(rows))
}

async fn list_vm_snapshots(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(vm_name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<Vec<SnapshotInfo>>, AppError> {
    if conn_q.is_fluxvm() {
        let snaps = fluxvm_client()?.snapshots(&vm_name).await?;
        return Ok(Json(
            snaps.iter().map(|s| s.to_snapshot_info(&vm_name)).collect(),
        ));
    }
    let vm2 = vm_name.clone();
    let rows = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        snapshot::list_snapshots(conn, &vm2)
    })
    .await?;
    Ok(Json(rows))
}

async fn create_snapshot_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(vm_name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<CreateSnapshotRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    if conn_q.is_fluxvm() {
        fluxvm_client()?.snapshot(&vm_name, &req.name).await?;
        return Ok(Json(
            serde_json::json!({ "status": "created", "vm": vm_name, "snapshot": req.name }),
        ));
    }
    let vm2 = vm_name.clone();
    let snap_name = req.name.clone();
    let req2 = req.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        snapshot::create_snapshot(conn, &vm2, &req2)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "created", "vm": vm_name, "snapshot": snap_name }),
    ))
}

async fn delete_snapshot_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path((vm_name, snap_name)): Path<(String, String)>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    if conn_q.is_fluxvm() {
        fluxvm_client()?
            .delete_snapshot(&vm_name, &snap_name)
            .await?;
        return Ok(Json(
            serde_json::json!({ "status": "deleted", "vm": vm_name, "snapshot": snap_name }),
        ));
    }
    let vm2 = vm_name.clone();
    let snap2 = snap_name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        snapshot::delete_snapshot(conn, &vm2, &snap2)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "deleted", "vm": vm_name, "snapshot": snap_name }),
    ))
}

async fn revert_snapshot_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path((vm_name, snap_name)): Path<(String, String)>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    if conn_q.is_fluxvm() {
        fluxvm_client()?.restore(&vm_name, &snap_name).await?;
        return Ok(Json(
            serde_json::json!({ "status": "reverted", "vm": vm_name, "snapshot": snap_name }),
        ));
    }
    let vm2 = vm_name.clone();
    let snap2 = snap_name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        snapshot::revert_snapshot(conn, &vm2, &snap2)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "reverted", "vm": vm_name, "snapshot": snap_name }),
    ))
}

pub fn snapshot_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/snapshots", get(list_all_snapshots))
        .route("/vms/{vm_name}/snapshots", get(list_vm_snapshots))
        .route("/vms/{vm_name}/snapshots", post(create_snapshot_handler))
        .route(
            "/vms/{vm_name}/snapshots/{snap_name}",
            delete(delete_snapshot_handler),
        )
        .route(
            "/vms/{vm_name}/snapshots/{snap_name}/revert",
            post(revert_snapshot_handler),
        )
}
