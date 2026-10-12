// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Backup schedules, per-instance power/snapshot schedules, and host maintenance.
//! These are Machina actions. A VM schedule action is start, shutdown, stop, or snapshot.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::{ec2_id, Kind};
use crate::state::AppState;

use super::more::api_err;
use super::{xml_escape, Ec2Error};

type Params = BTreeMap<String, String>;

fn bad(code: &'static str, msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad(code, msg)
}

fn need(p: &Params, k: &str) -> Result<String, Ec2Error> {
    p.get(k).cloned().ok_or_else(|| bad("MissingParameter", format!("The request must contain the parameter {k}")))
}

pub async fn create_backup_schedule(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let body = crate::api::backups::CreateBackupScheduleBody {
        name: need(p, "Name")?,
        project: p.get("Project").cloned().unwrap_or_default(),
        tag_filter: p.get("TagFilter").cloned().unwrap_or_default(),
        backup_type: p.get("BackupType").cloned().unwrap_or_else(|| "full".into()),
        target_id: None,
        interval_hours: p.get("IntervalHours").and_then(|s| s.parse().ok()).unwrap_or(24),
        retain_count: p.get("RetainCount").and_then(|s| s.parse().ok()).unwrap_or(7),
        enabled: !matches!(p.get("Enabled").map(String::as_str), Some("false") | Some("0")),
    };
    let Json(row) = crate::api::backups::create_backup_schedule(State(state.clone()), Extension(actor.clone()), Json(body)).await.map_err(api_err)?;
    Ok(format!("<scheduleId>{}</scheduleId><name>{}</name>", row.id, xml_escape(&row.name)))
}

pub async fn describe_backup_schedules(state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    let Json(rows) = crate::api::backups::list_backup_schedules(State(state.clone()), Extension(actor.clone())).await.map_err(api_err)?;
    let items: String = rows
        .iter()
        .map(|r| {
            format!(
                "<item><scheduleId>{}</scheduleId><name>{}</name><tagFilter>{}</tagFilter><intervalHours>{}</intervalHours><retainCount>{}</retainCount><enabled>{}</enabled></item>",
                r.id,
                xml_escape(&r.name),
                xml_escape(&r.tag_filter),
                r.interval_hours,
                r.retain_count,
                r.enabled
            )
        })
        .collect();
    Ok(format!("<scheduleSet>{items}</scheduleSet>"))
}

pub async fn delete_backup_schedule(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let id: Uuid = need(p, "ScheduleId")?.parse().map_err(|_| bad("InvalidParameterValue", "ScheduleId must be a UUID"))?;
    let _ = crate::api::backups::delete_backup_schedule(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
    Ok("<return>true</return>".into())
}

pub async fn verify_backup(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let instance = super::more::resolve(state, Kind::Vm, &need(p, "InstanceId")?, "InvalidInstanceID.NotFound").await?;
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
    let Json(row) = crate::api::backups::verify_vm_backup(State(state.clone()), Extension(actor.clone()), Path((instance, id))).await.map_err(api_err)?;
    let ok = row.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    Ok(format!("<ok>{ok}</ok><message>{}</message>", xml_escape(row.get("message").and_then(|v| v.as_str()).unwrap_or(""))))
}

pub async fn create_vm_schedule(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let instance = super::more::resolve(state, Kind::Vm, &need(p, "InstanceId")?, "InvalidInstanceID.NotFound").await?;
    let body = crate::api::vm_schedules::CreateVmScheduleBody {
        action: need(p, "ActionName")?,
        interval_minutes: p.get("IntervalMinutes").and_then(|s| s.parse().ok()).unwrap_or(1440),
        retention: p.get("Retention").and_then(|s| s.parse().ok()),
        label: p.get("Label").cloned().unwrap_or_default(),
    };
    let Json(row) = crate::api::vm_schedules::create_vm_schedule(State(state.clone()), Extension(actor.clone()), Path(instance), Json(body))
        .await
        .map_err(api_err)?;
    Ok(format!("<scheduleId>{}</scheduleId><instanceId>{}</instanceId><action>{}</action>", row.id, ec2_id(Kind::Vm, row.vm_id), xml_escape(&row.action)))
}

pub async fn describe_vm_schedules(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    let instance = super::more::resolve(state, Kind::Vm, &need(p, "InstanceId")?, "InvalidInstanceID.NotFound").await?;
    let Json(rows) = crate::api::vm_schedules::list_vm_schedules(State(state.clone()), Extension(actor.clone()), Path(instance)).await.map_err(api_err)?;
    let items: String = rows
        .iter()
        .map(|r| {
            format!(
                "<item><scheduleId>{}</scheduleId><action>{}</action><intervalMinutes>{}</intervalMinutes><enabled>{}</enabled><nextRunAt>{}</nextRunAt></item>",
                r.id,
                xml_escape(&r.action),
                r.interval_minutes,
                r.enabled,
                xml_escape(&r.next_run_at)
            )
        })
        .collect();
    Ok(format!("<scheduleSet>{items}</scheduleSet>"))
}

pub async fn delete_vm_schedule(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let instance = super::more::resolve(state, Kind::Vm, &need(p, "InstanceId")?, "InvalidInstanceID.NotFound").await?;
    let id: Uuid = need(p, "ScheduleId")?.parse().map_err(|_| bad("InvalidParameterValue", "ScheduleId must be a UUID"))?;
    let _ = crate::api::vm_schedules::delete_vm_schedule(State(state.clone()), Extension(actor.clone()), Path((instance, id))).await.map_err(api_err)?;
    Ok("<return>true</return>".into())
}

pub async fn create_maintenance_schedule(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let host: Uuid = need(p, "HostId")?.parse().map_err(|_| bad("InvalidParameterValue", "HostId must be a host UUID"))?;
    let run_at: DateTime<Utc> = need(p, "RunAt")?.parse().map_err(|_| bad("InvalidParameterValue", "RunAt must be RFC3339"))?;
    let body = crate::api::maintenance::CreateScheduleBody {
        host_id: host,
        action: p.get("MaintenanceAction").cloned().unwrap_or_else(|| "enter".into()),
        evacuate: !matches!(p.get("Evacuate").map(String::as_str), Some("false") | Some("0")),
        run_at,
    };
    let Json(row) = crate::api::maintenance::create_schedule(State(state.clone()), Extension(actor.clone()), Json(body)).await.map_err(api_err)?;
    Ok(format!("<scheduleId>{}</scheduleId><hostId>{}</hostId><status>{}</status>", row.id, row.host_id, xml_escape(&row.status)))
}

pub async fn describe_maintenance_schedules(state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    let Json(rows) = crate::api::maintenance::list_schedules(State(state.clone()), Extension(actor.clone())).await.map_err(api_err)?;
    let items: String = rows
        .iter()
        .map(|r| format!("<item><scheduleId>{}</scheduleId><hostId>{}</hostId><action>{}</action><evacuate>{}</evacuate><status>{}</status></item>", r.id, r.host_id, xml_escape(&r.action), r.evacuate, xml_escape(&r.status)))
        .collect();
    Ok(format!("<scheduleSet>{items}</scheduleSet>"))
}

pub async fn delete_maintenance_schedule(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let id: Uuid = need(p, "ScheduleId")?.parse().map_err(|_| bad("InvalidParameterValue", "ScheduleId must be a UUID"))?;
    let _ = crate::api::maintenance::delete_schedule(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
    Ok("<return>true</return>".into())
}
