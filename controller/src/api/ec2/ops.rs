// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Instance backups. A backup is a Machina backup record, not an EBS snapshot.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use uuid::Uuid;

use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::{ec2_id, Kind};
use crate::state::AppState;

use super::more::{api_err, resolve};
use super::{xml_escape, Ec2Error};

type Params = BTreeMap<String, String>;

fn bad(code: &'static str, msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad(code, msg)
}

fn need(p: &Params, k: &str) -> Result<String, Ec2Error> {
    p.get(k).cloned().ok_or_else(|| bad("MissingParameter", format!("The request must contain the parameter {k}")))
}

fn backup_id(id: Uuid) -> String {
    format!("bak-{}", &id.simple().to_string()[..17])
}

pub async fn create_backup(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let instance = resolve(state, Kind::Vm, &need(p, "InstanceId")?, "InvalidInstanceID.NotFound").await?;
    let Json(task) = crate::api::backups::create_vm_backup(
        State(state.clone()),
        Extension(actor.clone()),
        Path(instance),
        Json(crate::api::backups::CreateBackupBody { backup_type: p.get("BackupType").cloned().unwrap_or_else(|| "full".into()), target_id: None }),
    )
    .await
    .map_err(api_err)?;
    Ok(format!("<instanceId>{}</instanceId><taskId>{}</taskId>", need(p, "InstanceId")?, xml_escape(&task.task_id)))
}

pub async fn describe_backups(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    let instance = resolve(state, Kind::Vm, &need(p, "InstanceId")?, "InvalidInstanceID.NotFound").await?;
    let Json(rows) = crate::api::backups::list_vm_backups(State(state.clone()), Extension(actor.clone()), Path(instance)).await.map_err(api_err)?;
    let items: String = rows
        .iter()
        .map(|r| {
            format!(
                "<item><backupId>{}</backupId><instanceId>{}</instanceId><type>{}</type><status>{}</status><verifyStatus>{}</verifyStatus><createdAt>{}</createdAt></item>",
                backup_id(r.id),
                ec2_id(Kind::Vm, r.vm_id),
                xml_escape(&r.backup_type),
                xml_escape(&r.status),
                xml_escape(&r.verify_status),
                r.created_at
            )
        })
        .collect();
    Ok(format!("<backupSet>{items}</backupSet>"))
}

pub async fn restore_backup(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let instance = resolve(state, Kind::Vm, &need(p, "InstanceId")?, "InvalidInstanceID.NotFound").await?;
    let raw = need(p, "BackupId")?;
    let hex = raw.strip_prefix("bak-").unwrap_or(&raw);
    if hex.len() != 17 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad("InvalidParameterValue", format!("The backup '{raw}' is not a valid id")));
    }
    let id: Option<Uuid> = crate::db::query_scalar("SELECT id FROM backup_records WHERE vm_id = ? AND lower(hex(id)) LIKE ?")
        .bind(instance)
        .bind(format!("{hex}%"))
        .fetch_optional(&state.pool)
        .await?;
    let Some(id) = id else {
        return Err(bad("InvalidParameterValue", format!("The backup '{raw}' does not exist")));
    };
    let Json(task) = crate::api::backups::restore_vm_backup(State(state.clone()), Extension(actor.clone()), Path((instance, id)))
        .await
        .map_err(api_err)?;
    Ok(format!("<instanceId>{}</instanceId><taskId>{}</taskId>", need(p, "InstanceId")?, xml_escape(&task.task_id)))
}
