// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Machina actions on the EC2 endpoint. Stock boto3 will not call these; `aws ec2` will with
//! `--cli-input-json`, and the Terraform provider can stop special-casing the REST port.
//! Sleep does not invent an EC2 state code: stopped is already code 80. The response adds
//! `<sleeping>true</sleeping>`.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use uuid::Uuid;

use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::Kind;
use crate::state::AppState;

use super::more::{api_err, resolve};
use super::{indexed, instance_state, xml_escape, Ec2Error};

type Params = BTreeMap<String, String>;

fn bad(code: &'static str, msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad(code, msg)
}

fn need(p: &Params, k: &str) -> Result<String, Ec2Error> {
    p.get(k).cloned().ok_or_else(|| bad("MissingParameter", format!("The request must contain the parameter {k}")))
}

fn state_change(id: &str, previous: &str, current_code: u16, current_name: &str, sleeping: bool) -> String {
    let (pc, pn) = instance_state(previous);
    let extra = if sleeping { "<sleeping>true</sleeping>" } else { "" };
    format!(
        "<item><instanceId>{id}</instanceId><currentState><code>{current_code}</code><name>{current_name}</name></currentState><previousState><code>{pc}</code><name>{pn}</name></previousState>{extra}</item>"
    )
}

async fn observed(state: &AppState, id: Uuid) -> Result<String, Ec2Error> {
    let row: Option<String> = crate::db::query_scalar("SELECT observed_state FROM vms WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    row.ok_or_else(|| bad("InvalidInstanceID.NotFound", "the instance does not exist"))
}

pub async fn sleep_instances(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let mut items = String::new();
    for want in indexed(p, "InstanceId") {
        let id = resolve(state, Kind::Vm, &want, "InvalidInstanceID.NotFound").await?;
        let previous = observed(state, id).await?;
        let _ = crate::api::vms::sleep_vm(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
        items.push_str(&state_change(&want, &previous, 80, "stopped", true));
    }
    if items.is_empty() {
        return Err(bad("MissingParameter", "The request must contain the parameter InstanceId.1"));
    }
    Ok(format!("<instancesSet>{items}</instancesSet>"))
}

pub async fn wake_instances(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let mut items = String::new();
    for want in indexed(p, "InstanceId") {
        let id = resolve(state, Kind::Vm, &want, "InvalidInstanceID.NotFound").await?;
        let previous = observed(state, id).await?;
        let _ = crate::api::vms::wake_vm(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
        items.push_str(&state_change(&want, &previous, 0, "pending", false));
    }
    if items.is_empty() {
        return Err(bad("MissingParameter", "The request must contain the parameter InstanceId.1"));
    }
    Ok(format!("<instancesSet>{items}</instancesSet>"))
}

pub async fn describe_sleep_policies(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    let _ = actor;
    let mut items = String::new();
    for want in indexed(p, "InstanceId") {
        let id = resolve(state, Kind::Vm, &want, "InvalidInstanceID.NotFound").await?;
        let Json(view) = crate::api::vms::get_vm_sleep_policy(State(state.clone()), Extension(actor.clone()), Path(id))
            .await
            .map_err(api_err)?;
        let body = serde_json::to_string(&view).unwrap_or_else(|_| "{}".into());
        items.push_str(&format!("<item><instanceId>{want}</instanceId><policy>{}</policy></item>", xml_escape(&body)));
    }
    Ok(format!("<sleepPolicySet>{items}</sleepPolicySet>"))
}

pub async fn modify_sleep_policy(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let want = need(p, "InstanceId")?;
    let id = resolve(state, Kind::Vm, &want, "InvalidInstanceID.NotFound").await?;
    let minutes = match p.get("SleepAfterMinutes").map(String::as_str) {
        None | Some("") => None,
        Some(s) => Some(s.parse::<i64>().map_err(|_| bad("InvalidParameterValue", "SleepAfterMinutes must be a number"))?),
    };
    let _ = crate::api::vms::set_vm_sleep_policy(
        State(state.clone()),
        Extension(actor.clone()),
        Path(id),
        Json(crate::api::vms::SleepPolicyBody { sleep_after_minutes: minutes }),
    )
    .await
    .map_err(api_err)?;
    Ok("<return>true</return>".into())
}

pub async fn modify_preemptible(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let want = need(p, "InstanceId")?;
    let id = resolve(state, Kind::Vm, &want, "InvalidInstanceID.NotFound").await?;
    let preemptible = matches!(p.get("Value").map(String::as_str), Some("true") | Some("1"));
    let priority = p.get("Priority").map(|s| s.parse::<i64>()).transpose().map_err(|_| bad("InvalidParameterValue", "Priority must be a number"))?.unwrap_or(0);
    let body: crate::api::preempt::VmPreemptBody = serde_json::from_value(serde_json::json!({
        "preemptible": preemptible,
        "priority": priority,
    }))
    .map_err(|e| bad("InvalidParameterValue", e.to_string()))?;
    let _ = crate::api::preempt::set_vm(State(state.clone()), Extension(actor.clone()), Path(id), Json(body))
        .await
        .map_err(api_err)?;
    Ok("<return>true</return>".into())
}

fn point_id(id: Uuid) -> String {
    format!("rp-{}", &id.simple().to_string()[..17])
}

pub async fn create_restore_point(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let want = need(p, "InstanceId")?;
    let id = resolve(state, Kind::Vm, &want, "InvalidInstanceID.NotFound").await?;
    let note = p.get("Description").cloned();
    let Json(task) = crate::api::time_travel::create_restore_point(
        State(state.clone()),
        Extension(actor.clone()),
        Path(id),
        Some(Json(crate::api::time_travel::CreatePointBody { note })),
    )
    .await
    .map_err(api_err)?;
    Ok(format!("<instanceId>{want}</instanceId><taskId>{}</taskId>", xml_escape(&task.task_id)))
}

pub async fn describe_restore_points(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    let want = need(p, "InstanceId")?;
    let id = resolve(state, Kind::Vm, &want, "InvalidInstanceID.NotFound").await?;
    let Json(view) = crate::api::time_travel::get_time_travel(State(state.clone()), Extension(actor.clone()), Path(id))
        .await
        .map_err(api_err)?;
    let body = serde_json::to_string(&view).unwrap_or_else(|_| "{}".into());
    Ok(format!("<instanceId>{want}</instanceId><timeTravel>{}</timeTravel>", xml_escape(&body)))
}

pub async fn rewind_instance(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let want = need(p, "InstanceId")?;
    let id = resolve(state, Kind::Vm, &want, "InvalidInstanceID.NotFound").await?;
    let point_s = need(p, "RestorePointId")?;
    let hex = point_s.strip_prefix("rp-").unwrap_or(&point_s);
    if hex.len() != 17 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad("InvalidParameterValue", format!("The restore point '{point_s}' is not a valid id")));
    }
    let point: Option<Uuid> = crate::db::query_scalar("SELECT id FROM vm_restore_points WHERE vm_id = ? AND lower(hex(id)) LIKE ?")
        .bind(id)
        .bind(format!("{hex}%"))
        .fetch_optional(&state.pool)
        .await?;
    let Some(point) = point else {
        return Err(bad("InvalidParameterValue", format!("The restore point '{point_s}' does not exist")));
    };
    let Json(task) = crate::api::time_travel::rewind(State(state.clone()), Extension(actor.clone()), Path((id, point)))
        .await
        .map_err(api_err)?;
    Ok(format!("<instanceId>{want}</instanceId><restorePointId>{}</restorePointId><taskId>{}</taskId>", point_id(point), xml_escape(&task.task_id)))
}

pub async fn fork_instance(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let want = need(p, "InstanceId")?;
    let id = resolve(state, Kind::Vm, &want, "InvalidInstanceID.NotFound").await?;
    let name = need(p, "Name")?;
    let memory = matches!(p.get("IncludeMemory").map(String::as_str), Some("true") | Some("1"));
    let Json(task) = crate::api::time_travel::fork(
        State(state.clone()),
        Extension(actor.clone()),
        Path(id),
        Json(crate::api::time_travel::ForkBody {
            name,
            restore_point_id: None,
            memory,
            isolate: memory,
            reseed: true,
            start: true,
        }),
    )
    .await
    .map_err(api_err)?;
    Ok(format!("<sourceInstanceId>{want}</sourceInstanceId><taskId>{}</taskId>", xml_escape(&task.task_id)))
}
