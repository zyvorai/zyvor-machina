// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Snapshot and image actions. A snapshot is a `volume_snapshots` row (`snap-`). An image is a
//! template (`ami-`). `CreateImage` registers a private template pointing at the instance's first
//! volume and, when that volume is Atlas-backed, also takes a snapshot so the image has a copy.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use uuid::Uuid;

use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::{ec2_id, Kind};
use crate::state::AppState;

use super::more::{api_err, resolve};
use super::{indexed, xml_escape, Ec2Error};

type Params = BTreeMap<String, String>;

fn bad(code: &'static str, msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad(code, msg)
}

fn need(p: &Params, k: &str) -> Result<String, Ec2Error> {
    p.get(k).cloned().ok_or_else(|| bad("MissingParameter", format!("The request must contain the parameter {k}")))
}

pub async fn describe_snapshots(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    let Json(rows) = crate::api::volumes::list_all_volume_snapshots(State(state.clone()), Extension(actor.clone())).await.map_err(api_err)?;
    let wanted = indexed(p, "SnapshotId");
    let volume = p.get("VolumeId").cloned();
    let mut items: Vec<(String, String)> = Vec::new();
    for row in rows {
        let sid = ec2_id(Kind::Snapshot, row.id);
        let vid = ec2_id(Kind::Volume, row.volume_id);
        if !wanted.is_empty() && !wanted.contains(&sid) {
            continue;
        }
        if volume.as_ref().is_some_and(|v| v != &vid) {
            continue;
        }
        items.push((
            sid.clone(),
            format!(
                "<item><snapshotId>{}</snapshotId><volumeId>{}</volumeId><status>{}</status><description>{}</description><ownerId>{}</ownerId></item>",
                sid,
                vid,
                xml_escape(&row.status),
                xml_escape(&row.name),
                "000000000000"
            ),
        ));
    }
    items.sort_by(|a, b| a.0.cmp(&b.0));
    let limit = super::page::max_results(p.get("MaxResults").map(String::as_str)).map_err(|m| bad("InvalidParameterValue", m))?;
    let ids: Vec<String> = items.iter().map(|(id, _)| id.clone()).collect();
    let (slice, token) = super::page::page(&ids, p.get("NextToken").map(String::as_str), limit).map_err(|m| bad("InvalidParameterValue", m))?;
    let body: String = items.iter().filter(|(id, _)| slice.contains(id)).map(|(_, xml)| xml.clone()).collect();
    Ok(format!("<snapshotSet>{body}</snapshotSet>{}", super::page::token_xml(&token)))
}

pub async fn create_snapshot(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let volume_s = need(p, "VolumeId")?;
    let volume = resolve(state, Kind::Volume, &volume_s, "InvalidVolume.NotFound").await?;
    let atlas: Option<String> = crate::db::query_scalar("SELECT atlas_volume_id FROM volumes WHERE id = ?")
        .bind(volume)
        .fetch_optional(&state.pool)
        .await?
        .flatten();
    if atlas.is_none() {
        return Err(bad("SnapshotCreationPerVolumeRateExceeded", "local-pool volumes have no snapshot; the volume is not Atlas-backed"));
    }
    let name = p.get("Description").cloned().unwrap_or_else(|| format!("snap-{}", &Uuid::new_v4().simple().to_string()[..8]));
    let Json(row) = crate::api::volumes::create_volume_snapshot(
        State(state.clone()),
        Extension(actor.clone()),
        Path(volume),
        Json(crate::api::volumes::CreateSnapshotBody { name }),
    )
    .await
    .map_err(api_err)?;
    Ok(format!(
        "<snapshotId>{}</snapshotId><volumeId>{volume_s}</volumeId><status>{}</status>",
        ec2_id(Kind::Snapshot, row.id),
        xml_escape(&row.status)
    ))
}

pub async fn delete_snapshot(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let id = resolve(state, Kind::Snapshot, &need(p, "SnapshotId")?, "InvalidSnapshot.NotFound").await?;
    let _ = crate::api::volumes::delete_volume_snapshot(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
    Ok("<return>true</return>".into())
}

/// `CreateVolume` with `SnapshotId`. Size stays the snapshot's size; the REST clone does not resize.
pub async fn create_volume_from_snapshot(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let snap = resolve(state, Kind::Snapshot, &need(p, "SnapshotId")?, "InvalidSnapshot.NotFound").await?;
    let name = p.get("Tag.1.Value").cloned().unwrap_or_else(|| format!("vol-{}", &Uuid::new_v4().simple().to_string()[..8]));
    let Json(v) = crate::api::volumes::create_volume_from_snapshot(
        State(state.clone()),
        Extension(actor.clone()),
        Path(snap),
        Json(crate::api::volumes::VolumeFromSnapshotBody { name }),
    )
    .await
    .map_err(api_err)?;
    Ok(format!(
        "<volumeId>{}</volumeId><size>{}</size><snapshotId>{}</snapshotId><availabilityZone>machina-a</availabilityZone><status>creating</status><volumeType>gp2</volumeType>",
        ec2_id(Kind::Volume, v.id),
        v.size_gib,
        need(p, "SnapshotId")?
    ))
}

async fn image_name_version(state: &AppState, image: &str) -> Result<(String, String), Ec2Error> {
    let id = resolve(state, Kind::Image, image, "InvalidAMIID.NotFound").await?;
    let row: Option<(String, String)> = crate::db::query_as("SELECT name, version FROM templates WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    row.ok_or_else(|| bad("InvalidAMIID.NotFound", format!("The image id '{image}' does not exist")))
}

pub async fn create_image(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let name = need(p, "Name")?;
    let instance = resolve(state, Kind::Vm, &need(p, "InstanceId")?, "InvalidInstanceID.NotFound").await?;
    let volume: Option<(Uuid, String, bool)> = crate::db::query_as(
        "SELECT id, name, atlas_volume_id IS NOT NULL FROM volumes WHERE attached_vm_id = ? ORDER BY attached_device LIMIT 1",
    )
    .bind(instance)
    .fetch_optional(&state.pool)
    .await?;
    let Some((volume_id, volume_name, atlas)) = volume else {
        return Err(bad("IncorrectInstanceState", "the instance has no volume to image"));
    };
    let mut description = format!("image of {}", ec2_id(Kind::Vm, instance));
    if atlas {
        let snap = crate::api::volumes::create_volume_snapshot(
            State(state.clone()),
            Extension(actor.clone()),
            Path(volume_id),
            Json(crate::api::volumes::CreateSnapshotBody { name: format!("{name}-snap") }),
        )
        .await
        .map_err(api_err)?;
        description = format!("{description}; snapshot {}", ec2_id(Kind::Snapshot, snap.0.id));
    }
    let body = crate::api::templates::CreateTemplateBody {
        name: name.clone(),
        version: "1".into(),
        source_disk: volume_name,
        cloud_init: true,
        os_family: None,
        category: "instance".into(),
        description,
        featured: false,
        marketplace: false,
        icon: None,
    };
    let Json(row) = crate::api::templates::create_template(State(state.clone()), Extension(actor.clone()), Json(body)).await.map_err(api_err)?;
    let _ = crate::api::templates::set_template_visibility(
        State(state.clone()),
        Extension(actor.clone()),
        Path((name, "1".into())),
        Json(crate::api::templates::VisibilityBody { visibility: "private".into() }),
    )
    .await;
    Ok(format!("<imageId>{}</imageId>", ec2_id(Kind::Image, row.id)))
}

pub async fn deregister_image(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let image = need(p, "ImageId")?;
    let (name, version) = image_name_version(state, &image).await?;
    let _ = crate::api::templates::set_template_visibility(
        State(state.clone()),
        Extension(actor.clone()),
        Path((name, version)),
        Json(crate::api::templates::VisibilityBody { visibility: "private".into() }),
    )
    .await
    .map_err(api_err)?;
    Ok("<return>true</return>".into())
}

pub async fn modify_image_attribute(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let image = need(p, "ImageId")?;
    let (name, version) = image_name_version(state, &image).await?;
    if p.get("LaunchPermission.Add.1.Group").is_some_and(|g| g == "all") {
        let _ = crate::api::templates::set_template_visibility(
            State(state.clone()),
            Extension(actor.clone()),
            Path((name, version)),
            Json(crate::api::templates::VisibilityBody { visibility: "public".into() }),
        )
        .await
        .map_err(api_err)?;
        return Ok("<return>true</return>".into());
    }
    if p.get("LaunchPermission.Remove.1.Group").is_some_and(|g| g == "all") {
        let _ = crate::api::templates::set_template_visibility(
            State(state.clone()),
            Extension(actor.clone()),
            Path((name, version)),
            Json(crate::api::templates::VisibilityBody { visibility: "private".into() }),
        )
        .await
        .map_err(api_err)?;
        return Ok("<return>true</return>".into());
    }
    if let Some(project) = p.get("LaunchPermission.Add.1.UserId") {
        let _ = crate::api::templates::share_template(
            State(state.clone()),
            Extension(actor.clone()),
            Path((name, version, project.clone())),
        )
        .await
        .map_err(api_err)?;
        return Ok("<return>true</return>".into());
    }
    if let Some(project) = p.get("LaunchPermission.Remove.1.UserId") {
        let _ = crate::api::templates::unshare_template(
            State(state.clone()),
            Extension(actor.clone()),
            Path((name, version, project.clone())),
        )
        .await
        .map_err(api_err)?;
        return Ok("<return>true</return>".into());
    }
    Err(bad("InvalidParameterValue", "LaunchPermission.Add.1.Group=all or LaunchPermission.Add.1.UserId is required"))
}
