// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, Query, State};
use axum::Extension;
use axum::Json;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::api::volumes::{CreateSnapshotBody, CreateVolumeBody, ListVolumesQuery, VolumeRow};
use crate::state::AppState;

use super::auth::{require_operator, OsActor};
use super::ids::{cinder_status, parse_uuid};
use super::OsError;

fn volume_json(v: &VolumeRow) -> Value {
    json!({
        "id": v.id.to_string(),
        "name": v.name,
        "status": cinder_status(&v.status),
        "size": v.size_gib,
        "volume_type": v.volume_class,
        "bootable": "false",
        "multiattach": false,
        "encrypted": false,
        "availability_zone": "nova",
        "attachments": match v.attached_vm_id {
            Some(vm) => vec![json!({
                "id": v.id.to_string(),
                "attachment_id": v.id.to_string(),
                "volume_id": v.id.to_string(),
                "server_id": vm.to_string(),
                "device": v.attached_device.clone().unwrap_or_else(|| "/dev/vdb".into()),
            })],
            None => vec![],
        },
        "metadata": {},
        "description": null,
        "user_id": "",
        "project_id": v.project_id.map(|p| p.to_string()),
        "created_at": "",
        "updated_at": "",
    })
}

pub async fn list_volumes(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let Json(rows) = crate::api::volumes::list_volumes(
        State(state),
        Extension(actor.user.clone()),
        Query(ListVolumesQuery {
            project_id: Some(actor.project_id),
            tag_key: None,
            tag_value: None,
        }),
    )
    .await?;
    Ok(Json(json!({
        "volumes": rows.iter().map(volume_json).collect::<Vec<_>>()
    })))
}

pub async fn get_volume(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    let Json(row) =
        crate::api::volumes::get_volume(State(state), Extension(actor.user.clone()), Path(id))
            .await?;
    Ok(Json(json!({ "volume": volume_json(&row) })))
}

/// `/v3/{project_id}/volumes/{id}` — project path segment ignored (token scopes the project).
pub async fn get_volume_scoped(
    state: State<AppState>,
    actor: Extension<OsActor>,
    Path((_project, id)): Path<(String, String)>,
) -> Result<Json<Value>, OsError> {
    get_volume(state, actor, Path(id)).await
}

pub async fn create_volume(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Json(body): Json<Value>,
) -> Result<(axum::http::StatusCode, Json<Value>), OsError> {
    require_operator(&actor)?;
    let vol = body
        .get("volume")
        .cloned()
        .ok_or_else(|| OsError::bad_request("volume object required"))?;
    let name = vol
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("volume")
        .to_string();
    if let Some(snap) = vol.get("snapshot_id").and_then(|v| v.as_str()) {
        let snap_id = Uuid::parse_str(snap).map_err(|_| OsError::bad_request("invalid snapshot_id"))?;
        let Json(row) = crate::api::volumes::create_volume_from_snapshot(
            State(state),
            Extension(actor.user.clone()),
            Path(snap_id),
            Json(crate::api::volumes::VolumeFromSnapshotBody { name }),
        )
        .await?;
        return Ok((
            axum::http::StatusCode::ACCEPTED,
            Json(json!({ "volume": volume_json(&row) })),
        ));
    }
    let size = vol
        .get("size")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| OsError::bad_request("size (GiB) required"))?;
    let Json(row) = crate::api::volumes::create_volume(
        State(state),
        Extension(actor.user.clone()),
        Json(CreateVolumeBody {
            name,
            size_gib: size,
            project_id: Some(actor.project_id),
            volume_class: vol
                .get("volume_type")
                .and_then(|v| v.as_str())
                .unwrap_or("silver")
                .into(),
            delete_on_termination: false,
        }),
    )
    .await?;
    Ok((
        axum::http::StatusCode::ACCEPTED,
        Json(json!({ "volume": volume_json(&row) })),
    ))
}

pub async fn delete_volume(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    let _ = crate::api::volumes::delete_volume(
        State(state),
        Extension(actor.user.clone()),
        Path(id),
    )
    .await?;
    Ok(axum::http::StatusCode::ACCEPTED)
}

pub async fn delete_volume_scoped(
    state: State<AppState>,
    actor: Extension<OsActor>,
    Path((_project, id)): Path<(String, String)>,
) -> Result<axum::http::StatusCode, OsError> {
    delete_volume(state, actor, Path(id)).await
}

pub async fn volume_action(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<axum::http::StatusCode, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    if let Some(attach) = body.get("os-attach") {
        let server = attach
            .get("instance_uuid")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OsError::bad_request("instance_uuid required"))?;
        let vm = Uuid::parse_str(server).map_err(|_| OsError::bad_request("invalid instance_uuid"))?;
        let target = attach
            .get("mountpoint")
            .and_then(|v| v.as_str())
            .unwrap_or("vdb")
            .trim_start_matches("/dev/")
            .to_string();
        let _ = crate::api::volumes::attach_volume(
            State(state),
            Extension(actor.user.clone()),
            Path(id),
            Json(crate::api::volumes::AttachVolumeBody {
                vm_id: vm,
                target_dev: target,
                delete_on_termination: None,
            }),
        )
        .await?;
        return Ok(axum::http::StatusCode::ACCEPTED);
    }
    if body.get("os-detach").is_some() {
        let _ = crate::api::volumes::detach_volume(
            State(state),
            Extension(actor.user.clone()),
            Path(id),
        )
        .await?;
        return Ok(axum::http::StatusCode::ACCEPTED);
    }
    if let Some(ext) = body.get("os-extend") {
        let new_size = ext
            .get("new_size")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| OsError::bad_request("os-extend.new_size required"))?;
        let _ = crate::api::volumes::extend_volume(
            State(state),
            Extension(actor.user.clone()),
            Path(id),
            Json(crate::api::volumes::ExtendVolumeBody {
                new_size_gib: new_size,
            }),
        )
        .await?;
        return Ok(axum::http::StatusCode::ACCEPTED);
    }
    Err(OsError::not_implemented(
        "supported volume actions: os-attach, os-detach, os-extend",
    ))
}

pub async fn volume_action_scoped(
    state: State<AppState>,
    actor: Extension<OsActor>,
    Path((_project, id)): Path<(String, String)>,
    body: Json<Value>,
) -> Result<axum::http::StatusCode, OsError> {
    volume_action(state, actor, Path(id), body).await
}

fn snapshot_json(id: Uuid, volume_id: Uuid, name: &str, status: &str) -> Value {
    json!({
        "id": id.to_string(),
        "volume_id": volume_id.to_string(),
        "name": name,
        "status": status,
        "size": 0,
        "metadata": {},
        "created_at": "",
        "updated_at": "",
    })
}

pub async fn list_snapshots(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let Json(rows) = crate::api::volumes::list_all_volume_snapshots(
        State(state),
        Extension(actor.user.clone()),
    )
    .await?;
    let snaps: Vec<Value> = rows
        .into_iter()
        .map(|s| snapshot_json(s.id, s.volume_id, &s.name, &s.status))
        .collect();
    Ok(Json(json!({ "snapshots": snaps })))
}

pub async fn get_snapshot(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    let Json(rows) = crate::api::volumes::list_all_volume_snapshots(
        State(state),
        Extension(actor.user.clone()),
    )
    .await?;
    let s = rows
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| OsError::not_found("snapshot not found"))?;
    Ok(Json(json!({
        "snapshot": snapshot_json(s.id, s.volume_id, &s.name, &s.status)
    })))
}

pub async fn get_snapshot_scoped(
    state: State<AppState>,
    actor: Extension<OsActor>,
    Path((_project, id)): Path<(String, String)>,
) -> Result<Json<Value>, OsError> {
    get_snapshot(state, actor, Path(id)).await
}

pub async fn create_snapshot(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Json(body): Json<Value>,
) -> Result<(axum::http::StatusCode, Json<Value>), OsError> {
    require_operator(&actor)?;
    let snap = body
        .get("snapshot")
        .cloned()
        .ok_or_else(|| OsError::bad_request("snapshot object required"))?;
    let volume_id = snap
        .get("volume_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| OsError::bad_request("volume_id required"))?;
    let volume_id = parse_uuid(volume_id)?;
    let name = snap
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("snapshot")
        .to_string();
    let Json(row) = crate::api::volumes::create_volume_snapshot(
        State(state),
        Extension(actor.user.clone()),
        Path(volume_id),
        Json(CreateSnapshotBody { name }),
    )
    .await?;
    Ok((
        axum::http::StatusCode::ACCEPTED,
        Json(json!({
            "snapshot": snapshot_json(row.id, row.volume_id, &row.name, &row.status)
        })),
    ))
}

pub async fn delete_snapshot(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    let _ = crate::api::volumes::delete_volume_snapshot(
        State(state),
        Extension(actor.user.clone()),
        Path(id),
    )
    .await?;
    Ok(axum::http::StatusCode::ACCEPTED)
}

pub async fn delete_snapshot_scoped(
    state: State<AppState>,
    actor: Extension<OsActor>,
    Path((_project, id)): Path<(String, String)>,
) -> Result<axum::http::StatusCode, OsError> {
    delete_snapshot(state, actor, Path(id)).await
}
