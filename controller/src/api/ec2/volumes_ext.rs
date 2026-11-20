// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Volume modification history, volume status, multi-volume snapshots and the volume type a client asked for.
//!
//! Size growth is real (the REST `extend_volume`). The EBS volume type, IOPS and throughput have no counterpart in the
//! storage layer; they are **recorded** in `ec2_volume_attrs` and read back by `DescribeVolumes`, so a client that
//! provisioned `gp3` with 4000 IOPS sees exactly that and not a permanent difference. Per-volume IO limits that do act on the
//! guest are set with `ModifyVolumeAttribute`.

use std::collections::{BTreeMap, HashMap};

use axum::extract::{Path, State};
use axum::{Extension, Json};
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

pub const VOLUME_TYPES: &[&str] = &["gp2", "gp3", "io1", "io2", "st1", "sc1", "standard"];

/// The recorded type, IOPS and throughput of a volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeShape {
    pub volume_type: String,
    pub iops: Option<i64>,
    pub throughput: Option<i64>,
}

impl Default for VolumeShape {
    fn default() -> Self {
        Self { volume_type: "gp2".into(), iops: None, throughput: None }
    }
}

/// Validates `VolumeType`, `Iops` and `Throughput` the way EC2 does, against the type the volume will have.
pub fn parse_shape(p: &Params, current: &VolumeShape) -> Result<VolumeShape, Ec2Error> {
    let volume_type = p.get("VolumeType").cloned().unwrap_or_else(|| current.volume_type.clone());
    if !VOLUME_TYPES.contains(&volume_type.as_str()) {
        return Err(bad("InvalidParameterValue", format!("VolumeType must be one of {}", VOLUME_TYPES.join(", "))));
    }
    let number = |key: &str| -> Result<Option<i64>, Ec2Error> {
        p.get(key).map(|v| v.parse::<i64>().map_err(|_| bad("InvalidParameterValue", format!("{key} must be a number")))).transpose()
    };
    let iops = number("Iops")?;
    let throughput = number("Throughput")?;
    if let Some(i) = iops {
        if !matches!(volume_type.as_str(), "io1" | "io2" | "gp3") {
            return Err(bad("InvalidParameterCombination", "Iops can only be set for io1, io2 and gp3 volumes"));
        }
        if !(100..=256_000).contains(&i) {
            return Err(bad("InvalidParameterValue", "Iops must be between 100 and 256000"));
        }
    } else if matches!(volume_type.as_str(), "io1" | "io2") && current.iops.is_none() {
        return Err(bad("MissingParameter", "Iops is required for io1 and io2 volumes"));
    }
    if let Some(t) = throughput {
        if volume_type != "gp3" {
            return Err(bad("InvalidParameterCombination", "Throughput can only be set for gp3 volumes"));
        }
        if !(125..=1000).contains(&t) {
            return Err(bad("InvalidParameterValue", "Throughput must be between 125 and 1000"));
        }
    }
    // a type change drops values the new type cannot have
    let keep = |new: Option<i64>, old: Option<i64>, ok: bool| new.or(old.filter(|_| ok));
    Ok(VolumeShape {
        iops: keep(iops, current.iops, matches!(volume_type.as_str(), "io1" | "io2" | "gp3")),
        throughput: keep(throughput, current.throughput, volume_type == "gp3"),
        volume_type,
    })
}

pub async fn shape_of(state: &AppState, volume: Uuid) -> Result<VolumeShape, Ec2Error> {
    let row: Option<(String, Option<i64>, Option<i64>)> =
        crate::db::query_as("SELECT volume_type, iops, throughput FROM ec2_volume_attrs WHERE volume_id = ?").bind(volume).fetch_optional(&state.pool).await?;
    Ok(row.map(|(volume_type, iops, throughput)| VolumeShape { volume_type, iops, throughput }).unwrap_or_default())
}

pub async fn shapes(state: &AppState) -> Result<HashMap<Uuid, VolumeShape>, Ec2Error> {
    let rows: Vec<(Uuid, String, Option<i64>, Option<i64>)> =
        crate::db::query_as("SELECT volume_id, volume_type, iops, throughput FROM ec2_volume_attrs").fetch_all(&state.pool).await?;
    Ok(rows.into_iter().map(|(id, volume_type, iops, throughput)| (id, VolumeShape { volume_type, iops, throughput })).collect())
}

pub async fn save_shape(state: &AppState, volume: Uuid, s: &VolumeShape) -> Result<(), Ec2Error> {
    crate::db::query(
        "INSERT INTO ec2_volume_attrs (volume_id, volume_type, iops, throughput) VALUES (?, ?, ?, ?) \
         ON CONFLICT (volume_id) DO UPDATE SET volume_type = excluded.volume_type, iops = excluded.iops, throughput = excluded.throughput",
    )
    .bind(volume)
    .bind(&s.volume_type)
    .bind(s.iops)
    .bind(s.throughput)
    .execute(&state.pool)
    .await?;
    Ok(())
}

/// Elements that describe a volume's shape.
pub fn shape_xml(s: &VolumeShape) -> String {
    let mut x = format!("<volumeType>{}</volumeType>", xml_escape(&s.volume_type));
    if let Some(i) = s.iops {
        x.push_str(&format!("<iops>{i}</iops>"));
    }
    if let Some(t) = s.throughput {
        x.push_str(&format!("<throughput>{t}</throughput>"));
    }
    x
}

/// `CreateVolume` options that cannot be honoured, refused before anything is created.
pub fn reject_create_options(p: &Params) -> Result<(), Ec2Error> {
    if p.get("Encrypted").is_some_and(|v| v == "true") || p.contains_key("KmsKeyId") {
        return Err(bad("UnsupportedOperation", "encrypted volumes are not supported"));
    }
    if p.get("MultiAttachEnabled").is_some_and(|v| v == "true") {
        return Err(bad("UnsupportedOperation", "MultiAttachEnabled is not supported"));
    }
    if p.contains_key("OutpostArn") {
        return Err(bad("UnsupportedOperation", "OutpostArn is not supported"));
    }
    Ok(())
}

// ---- ModifyVolume --------------------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn modification_xml(volume: &str, state: &str, orig: (i64, &VolumeShape), target: (i64, &VolumeShape), start: &str, end: &str) -> String {
    let (os, osh) = orig;
    let (ts, tsh) = target;
    let opt = |tag: &str, v: Option<i64>| v.map(|v| format!("<{tag}>{v}</{tag}>")).unwrap_or_default();
    format!(
        "<volumeId>{volume}</volumeId><modificationState>{state}</modificationState><targetSize>{ts}</targetSize>{}{}<targetVolumeType>{}</targetVolumeType><originalSize>{os}</originalSize>{}{}<originalVolumeType>{}</originalVolumeType><progress>{}</progress><startTime>{}</startTime><endTime>{}</endTime>",
        opt("targetIops", tsh.iops),
        opt("targetThroughput", tsh.throughput),
        xml_escape(&tsh.volume_type),
        opt("originalIops", osh.iops),
        opt("originalThroughput", osh.throughput),
        xml_escape(&osh.volume_type),
        if state == "completed" { 100 } else { 0 },
        xml_escape(start),
        xml_escape(end)
    )
}

/// `ModifyVolume`: grow the volume (really) and record the type, IOPS and throughput (see the module notes).
pub async fn modify_volume(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let volume = need(p, "VolumeId")?;
    let id = resolve(state, Kind::Volume, &volume, "InvalidVolume.NotFound").await?;
    if p.get("MultiAttachEnabled").is_some_and(|v| v == "true") {
        return Err(bad("UnsupportedOperation", "MultiAttachEnabled is not supported"));
    }
    if !["Size", "VolumeType", "Iops", "Throughput"].iter().any(|k| p.contains_key(*k)) {
        return Err(bad("MissingParameter", "give at least one of Size, VolumeType, Iops or Throughput"));
    }
    let current_size: i64 = crate::db::query_scalar("SELECT size_gib FROM volumes WHERE id = ?").bind(id).fetch_one(&state.pool).await?;
    let before = shape_of(state, id).await?;
    let after = parse_shape(p, &before)?;
    let target_size: i64 = match p.get("Size") {
        Some(s) => s.parse().map_err(|_| bad("InvalidParameterValue", "Size must be a number of GiB"))?,
        None => current_size,
    };
    if target_size < current_size {
        return Err(bad("InvalidParameterValue", format!("Size cannot shrink a volume ({current_size} GiB now)")));
    }
    let start = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S.000Z").to_string();
    if target_size > current_size {
        let _ = crate::api::volumes::extend_volume(
            State(state.clone()),
            Extension(actor.clone()),
            Path(id),
            Json(crate::api::volumes::ExtendVolumeBody { new_size_gib: target_size }),
        )
        .await
        .map_err(api_err)?;
    }
    if after != before || p.contains_key("VolumeType") || p.contains_key("Iops") || p.contains_key("Throughput") {
        save_shape(state, id, &after).await?;
    }
    crate::db::query(
        "INSERT INTO ec2_volume_modifications (id, volume_id, original_size, target_size, original_type, target_type, original_iops, target_iops, \
         original_throughput, target_throughput, state, end_time) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'completed', CURRENT_TIMESTAMP)",
    )
    .bind(Uuid::new_v4())
    .bind(id)
    .bind(current_size)
    .bind(target_size)
    .bind(&before.volume_type)
    .bind(&after.volume_type)
    .bind(before.iops)
    .bind(after.iops)
    .bind(before.throughput)
    .bind(after.throughput)
    .execute(&state.pool)
    .await?;
    Ok(format!("<volumeModification>{}</volumeModification>", modification_xml(&volume, "completed", (current_size, &before), (target_size, &after), &start, &start)))
}

type ModRow = (Uuid, i64, i64, String, String, Option<i64>, Option<i64>, Option<i64>, Option<i64>, String, String, Option<String>);

pub async fn describe_volumes_modifications(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let wanted = indexed(p, "VolumeId");
    let rows: Vec<ModRow> = crate::db::query_as(
        "SELECT volume_id, original_size, target_size, original_type, target_type, original_iops, target_iops, original_throughput, target_throughput, \
         state, start_time, end_time FROM ec2_volume_modifications ORDER BY start_time, id",
    )
    .fetch_all(&state.pool)
    .await?;
    let states = super::parse_filters(p).into_iter().find(|(n, _)| n == "modification-state").map(|(_, v)| v);
    let ids = super::parse_filters(p).into_iter().find(|(n, _)| n == "volume-id").map(|(_, v)| v);
    let mut items = String::new();
    for (vol, os, ts, otype, ttype, oiops, tiops, othr, tthr, st, start, end) in rows {
        let eid = ec2_id(Kind::Volume, vol);
        if !wanted.is_empty() && !wanted.contains(&eid) {
            continue;
        }
        if ids.as_ref().is_some_and(|v| !super::foundation::any_match(v, &eid)) || states.as_ref().is_some_and(|v| !super::foundation::any_match(v, &st)) {
            continue;
        }
        let o = VolumeShape { volume_type: otype, iops: oiops, throughput: othr };
        let t = VolumeShape { volume_type: ttype, iops: tiops, throughput: tthr };
        items.push_str(&format!("<item>{}</item>", modification_xml(&eid, &st, (os, &o), (ts, &t), &start, end.as_deref().unwrap_or(""))));
    }
    Ok(format!("<volumeModificationSet>{items}</volumeModificationSet>"))
}

// ---- status --------------------------------------------------------------------------------------------------------

pub async fn describe_volume_status(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let wanted = indexed(p, "VolumeId");
    let rows: Vec<(Uuid, String)> = crate::db::query_as("SELECT id, status FROM volumes ORDER BY name").fetch_all(&state.pool).await?;
    for w in &wanted {
        if !rows.iter().any(|(id, _)| &ec2_id(Kind::Volume, *id) == w) {
            return Err(bad("InvalidVolume.NotFound", format!("The volume '{w}' does not exist")));
        }
    }
    let mut items = String::new();
    for (id, status) in rows {
        let eid = ec2_id(Kind::Volume, id);
        if !wanted.is_empty() && !wanted.contains(&eid) {
            continue;
        }
        let (overall, io) = match status.as_str() {
            "error" => ("impaired", "failed"),
            "creating" => ("insufficient-data", "insufficient-data"),
            _ => ("ok", "passed"),
        };
        items.push_str(&format!(
            "<item><volumeId>{eid}</volumeId><availabilityZone>machina-a</availabilityZone><volumeStatus><status>{overall}</status><details><item><name>io-enabled</name><status>{io}</status></item><item><name>io-performance</name><status>not-applicable</status></item></details></volumeStatus><eventsSet/><actionsSet/></item>"
        ));
    }
    Ok(format!("<volumeStatusSet>{items}</volumeStatusSet>"))
}

// ---- snapshots -------------------------------------------------------------------------------------------------------

/// `CreateSnapshots`: a snapshot of every volume of an instance (Atlas-backed volumes, like `CreateSnapshot`).
pub async fn create_snapshots(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let instance = need(p, "InstanceSpecification.InstanceId")?;
    let vm = resolve(state, Kind::Vm, &instance, "InvalidInstanceID.NotFound").await?;
    let exclude_boot = p.get("InstanceSpecification.ExcludeBootVolume").is_some_and(|v| v == "true");
    let volumes: Vec<(Uuid, Option<String>, bool)> =
        crate::db::query_as("SELECT id, attached_device, atlas_volume_id IS NOT NULL FROM volumes WHERE attached_vm_id = ? ORDER BY attached_device")
            .bind(vm)
            .fetch_all(&state.pool)
            .await?;
    let volumes: Vec<_> = volumes.into_iter().filter(|(_, dev, _)| !(exclude_boot && dev.as_deref() == Some("vda"))).collect();
    if volumes.is_empty() {
        return Err(bad("IncorrectState", "the instance has no volume to snapshot"));
    }
    if let Some((_, dev, _)) = volumes.iter().find(|(_, _, atlas)| !atlas) {
        return Err(bad(
            "UnsupportedOperation",
            format!("the volume at /dev/{} is not Atlas-backed; local-pool volumes cannot be snapshotted (nothing was created)", dev.as_deref().unwrap_or("?")),
        ));
    }
    let description = p.get("Description").cloned().unwrap_or_else(|| format!("snapshot of {instance}"));
    let mut items = String::new();
    for (volume, dev, _) in volumes {
        let name = format!("{description} ({})", dev.unwrap_or_default());
        let Json(row) = crate::api::volumes::create_volume_snapshot(
            State(state.clone()),
            Extension(actor.clone()),
            Path(volume),
            Json(crate::api::volumes::CreateSnapshotBody { name }),
        )
        .await
        .map_err(api_err)?;
        items.push_str(&format!(
            "<item><snapshotId>{}</snapshotId><volumeId>{}</volumeId><status>{}</status><description>{}</description><ownerId>000000000000</ownerId></item>",
            ec2_id(Kind::Snapshot, row.id),
            ec2_id(Kind::Volume, volume),
            xml_escape(&row.status),
            xml_escape(&description)
        ));
    }
    Ok(format!("<snapshotSet>{items}</snapshotSet>"))
}

/// `CopySnapshot`: snapshots live in the one Atlas region. A copy would share the original's backing snapshot (deleting
/// one would break the other), so it is refused rather than faked.
pub fn copy_snapshot() -> Result<String, Ec2Error> {
    Err(bad(
        "UnsupportedOperation",
        "CopySnapshot is not supported: snapshots live in the one storage region and a copy would share the original's backing snapshot. Create a volume from the snapshot and snapshot that instead",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pairs: &[(&str, &str)]) -> Params {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn shape_validation_follows_ebs_rules() {
        let d = VolumeShape::default();
        assert_eq!(parse_shape(&p(&[]), &d).unwrap(), d);
        let g3 = parse_shape(&p(&[("VolumeType", "gp3"), ("Iops", "4000"), ("Throughput", "250")]), &d).unwrap();
        assert_eq!(g3, VolumeShape { volume_type: "gp3".into(), iops: Some(4000), throughput: Some(250) });
        assert_eq!(parse_shape(&p(&[("Iops", "4000")]), &d).unwrap_err().code, "InvalidParameterCombination");
        assert_eq!(parse_shape(&p(&[("VolumeType", "io1")]), &d).unwrap_err().code, "MissingParameter");
        assert!(parse_shape(&p(&[("VolumeType", "io1"), ("Iops", "3000")]), &d).is_ok());
        assert_eq!(parse_shape(&p(&[("VolumeType", "gp3"), ("Throughput", "50")]), &d).unwrap_err().code, "InvalidParameterValue");
        assert_eq!(parse_shape(&p(&[("VolumeType", "gp2"), ("Throughput", "250")]), &d).unwrap_err().code, "InvalidParameterCombination");
        assert_eq!(parse_shape(&p(&[("VolumeType", "tape")]), &d).unwrap_err().code, "InvalidParameterValue");
        // moving back to gp2 drops what gp2 cannot hold
        assert_eq!(parse_shape(&p(&[("VolumeType", "gp2")]), &g3).unwrap(), d);
        // growing without naming the type keeps it
        assert_eq!(parse_shape(&p(&[("Size", "20")]), &g3).unwrap(), g3);
    }

    #[test]
    fn create_options_that_are_not_supported_are_refused() {
        assert!(reject_create_options(&p(&[("Encrypted", "true")])).is_err());
        assert!(reject_create_options(&p(&[("KmsKeyId", "k")])).is_err());
        assert!(reject_create_options(&p(&[("MultiAttachEnabled", "true")])).is_err());
        assert!(reject_create_options(&p(&[("Encrypted", "false"), ("Size", "5")])).is_ok());
    }

    #[test]
    fn shape_xml_has_only_what_is_set() {
        assert_eq!(shape_xml(&VolumeShape::default()), "<volumeType>gp2</volumeType>");
        assert_eq!(
            shape_xml(&VolumeShape { volume_type: "gp3".into(), iops: Some(3000), throughput: Some(125) }),
            "<volumeType>gp3</volumeType><iops>3000</iops><throughput>125</throughput>"
        );
    }

    #[test]
    fn copy_snapshot_is_refused_with_the_reason() {
        let e = copy_snapshot().unwrap_err();
        assert_eq!(e.code, "UnsupportedOperation");
        assert!(e.message.contains("backing snapshot"));
    }

    async fn insert_volume(state: &AppState, size: i64) -> Uuid {
        let id = Uuid::new_v4();
        crate::db::query("INSERT INTO volumes (id, name, size_gib, status) VALUES (?, 'v', ?, 'available')").bind(id).bind(size).execute(&state.pool).await.unwrap();
        id
    }

    #[tokio::test]
    async fn shapes_round_trip_and_modifications_are_listed_and_filtered() {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        let vol = insert_volume(&state, 10).await;
        assert_eq!(shape_of(&state, vol).await.unwrap(), VolumeShape::default());
        let s = VolumeShape { volume_type: "gp3".into(), iops: Some(4000), throughput: Some(250) };
        save_shape(&state, vol, &s).await.unwrap();
        assert_eq!(shape_of(&state, vol).await.unwrap(), s);
        assert_eq!(shapes(&state).await.unwrap().get(&vol), Some(&s));
        crate::db::query(
            "INSERT INTO ec2_volume_modifications (id, volume_id, original_size, target_size, original_type, target_type, target_iops, state, end_time) \
             VALUES (?, ?, 10, 10, 'gp2', 'gp3', 4000, 'completed', CURRENT_TIMESTAMP)",
        )
        .bind(Uuid::new_v4())
        .bind(vol)
        .execute(&state.pool)
        .await
        .unwrap();
        let eid = ec2_id(Kind::Volume, vol);
        let x = describe_volumes_modifications(&state, &p(&[("VolumeId.1", &eid)])).await.unwrap();
        assert!(x.contains("<targetVolumeType>gp3</targetVolumeType>") && x.contains("<targetIops>4000</targetIops>") && x.contains("<originalVolumeType>gp2</originalVolumeType>"), "{x}");
        let none = describe_volumes_modifications(&state, &p(&[("Filter.1.Name", "modification-state"), ("Filter.1.Value.1", "failed")])).await.unwrap();
        assert!(!none.contains("<item>"), "{none}");
    }

    #[tokio::test]
    async fn status_reports_impaired_and_unknown_volumes() {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        let vol = insert_volume(&state, 5).await;
        let eid = ec2_id(Kind::Volume, vol);
        let x = describe_volume_status(&state, &p(&[("VolumeId.1", &eid)])).await.unwrap();
        assert!(x.contains("<status>ok</status>") && x.contains("<name>io-enabled</name><status>passed</status>"), "{x}");
        crate::db::query("UPDATE volumes SET status = 'error' WHERE id = ?").bind(vol).execute(&state.pool).await.unwrap();
        assert!(describe_volume_status(&state, &p(&[])).await.unwrap().contains("<status>impaired</status>"));
        assert_eq!(describe_volume_status(&state, &p(&[("VolumeId.1", "vol-00000000000000000")])).await.unwrap_err().code, "InvalidVolume.NotFound");
    }
}
