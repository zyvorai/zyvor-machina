// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Stacks, migrations, HA, audit, notifications, projects, and scheduled jobs.
//! None of these are on the merged `POST /ec2` match. Stack create stays on REST:
//! the template is a structured document, not a query-string.

use std::collections::BTreeMap;

use axum::extract::{Path, Query, State};
use axum::Extension;
use axum::Json;
use uuid::Uuid;

use crate::auth::{require_admin, require_operator, AuthUser};
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

fn stack_id(p: &Params) -> Result<Uuid, Ec2Error> {
    need(p, "StackId")?.parse().map_err(|_| bad("InvalidParameterValue", "StackId must be a UUID"))
}

pub async fn describe_stacks(state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    let Json(rows) = crate::api::stacks::list_stacks(State(state.clone()), Extension(actor.clone())).await.map_err(api_err)?;
    let items: String = rows
        .iter()
        .map(|s| {
            format!(
                "<item><stackId>{}</stackId><stackName>{}</stackName><status>{}</status><autoHeal>{}</autoHeal><lastError>{}</lastError></item>",
                s.id,
                xml_escape(&s.name),
                xml_escape(&s.status),
                s.auto_heal,
                xml_escape(s.last_error.as_deref().unwrap_or(""))
            )
        })
        .collect();
    Ok(format!("<stackSet>{items}</stackSet>"))
}

pub async fn describe_stack_drift(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    let Json(row) = crate::api::stacks::get_drift(State(state.clone()), Extension(actor.clone()), Path(stack_id(p)?)).await.map_err(api_err)?;
    Ok(format!("<drift>{}</drift>", xml_escape(&row.to_string())))
}

pub async fn converge_stack(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let Json(row) = crate::api::stacks::converge_stack(State(state.clone()), Extension(actor.clone()), Path(stack_id(p)?)).await.map_err(api_err)?;
    Ok(format!("<result>{}</result>", xml_escape(&row.to_string())))
}

pub async fn set_stack_auto_heal(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let enabled = matches!(need(p, "Enabled")?.as_str(), "true" | "1");
    let _ = crate::api::stacks::set_auto_heal(
        State(state.clone()),
        Extension(actor.clone()),
        Path(stack_id(p)?),
        Json(crate::api::stacks::AutoHealBody { enabled }),
    )
    .await
    .map_err(api_err)?;
    Ok("<return>true</return>".into())
}

pub async fn delete_stack(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let _ = crate::api::stacks::delete_stack(State(state.clone()), Extension(actor.clone()), Path(stack_id(p)?)).await.map_err(api_err)?;
    Ok("<return>true</return>".into())
}

pub async fn describe_migration_jobs(state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    let Json(rows) = crate::api::migration_jobs::list_migration_jobs(State(state.clone()), Extension(actor.clone())).await.map_err(api_err)?;
    let items: String = rows
        .iter()
        .map(|j| {
            format!(
                "<item><jobId>{}</jobId><instanceId>{}</instanceId><sourceHost>{}</sourceHost><destHost>{}</destHost><live>{}</live><status>{}</status><progress>{}</progress></item>",
                j.id,
                crate::resource_ids::ec2_id(crate::resource_ids::Kind::Vm, j.vm_id),
                xml_escape(&j.source_host),
                xml_escape(&j.dest_host),
                j.live,
                xml_escape(&j.status),
                j.progress
            )
        })
        .collect();
    Ok(format!("<migrationJobSet>{items}</migrationJobSet>"))
}

pub async fn describe_ha_status(state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    let Json(row) = crate::api::ha::get_ha_status(State(state.clone()), Extension(actor.clone())).await.map_err(api_err)?;
    Ok(format!("<ha>{}</ha>", xml_escape(&serde_json::to_string(&row).unwrap_or_default())))
}

pub async fn describe_audit(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_admin(actor)?;
    let q = crate::api::audit::AuditQuery {
        action: p.get("AuditAction").cloned(),
        actor: p.get("Actor").cloned(),
        limit: p.get("MaxResults").and_then(|s| s.parse().ok()).unwrap_or(100),
    };
    let Json(rows) = crate::api::audit::list_audit_logs(State(state.clone()), Extension(actor.clone()), Query(q)).await.map_err(api_err)?;
    let items: String = rows
        .iter()
        .map(|r| format!("<item><eventId>{}</eventId><actor>{}</actor><action>{}</action><at>{}</at></item>", r.id, xml_escape(&r.actor), xml_escape(&r.action), r.created_at))
        .collect();
    Ok(format!("<auditSet>{items}</auditSet>"))
}

pub async fn describe_notifications(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    let q = crate::api::notifications::NotificationQuery {
        undelivered: matches!(p.get("Undelivered").map(String::as_str), Some("true") | Some("1")),
        limit: p.get("MaxResults").and_then(|s| s.parse().ok()).unwrap_or(100),
    };
    let Json(rows) = crate::api::notifications::list_notifications(State(state.clone()), Extension(actor.clone()), Query(q)).await.map_err(api_err)?;
    let items: String = rows
        .iter()
        .map(|n| format!("<item><notificationId>{}</notificationId><kind>{}</kind><delivered>{}</delivered></item>", n.id, xml_escape(&n.kind), n.delivered))
        .collect();
    Ok(format!("<notificationSet>{items}</notificationSet>"))
}

pub async fn describe_projects(state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    let Json(rows) = crate::api::projects::list_project_registry(State(state.clone()), Extension(actor.clone())).await.map_err(api_err)?;
    let items: String = rows
        .iter()
        .map(|r| format!("<item><projectId>{}</projectId><name>{}</name><description>{}</description><enabled>{}</enabled></item>", r.id, xml_escape(&r.name), xml_escape(&r.description), r.enabled))
        .collect();
    Ok(format!("<projectSet>{items}</projectSet>"))
}

pub async fn create_project(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_admin(actor)?;
    let body = crate::api::projects::CreateProjectBody { name: need(p, "Name")?, description: p.get("Description").cloned().unwrap_or_default() };
    let Json(row) = crate::api::projects::create_project(State(state.clone()), Extension(actor.clone()), Json(body)).await.map_err(api_err)?;
    Ok(format!("<projectId>{}</projectId><name>{}</name>", row.id, xml_escape(&row.name)))
}

pub async fn describe_scheduled_jobs(state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    let Json(rows) = crate::api::scheduled_jobs::list_scheduled_jobs(State(state.clone()), Extension(actor.clone())).await.map_err(api_err)?;
    let items: String = rows
        .iter()
        .map(|j| format!("<item><jobId>{}</jobId><name>{}</name><operation>{}</operation><intervalMinutes>{}</intervalMinutes><enabled>{}</enabled></item>", j.id, xml_escape(&j.name), xml_escape(&j.operation), j.interval_minutes, j.enabled))
        .collect();
    Ok(format!("<scheduledJobSet>{items}</scheduledJobSet>"))
}

pub async fn create_scheduled_job(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let body = crate::api::scheduled_jobs::CreateScheduledJobBody {
        name: need(p, "Name")?,
        operation: need(p, "Operation")?,
        payload: p.get("Payload").cloned().unwrap_or_else(|| "{}".into()),
        target_host_id: p.get("HostId").and_then(|s| s.parse().ok()),
        interval_minutes: p.get("IntervalMinutes").and_then(|s| s.parse().ok()).unwrap_or(60),
        enabled: !matches!(p.get("Enabled").map(String::as_str), Some("false") | Some("0")),
    };
    let Json(row) = crate::api::scheduled_jobs::create_scheduled_job(State(state.clone()), Extension(actor.clone()), Json(body)).await.map_err(api_err)?;
    Ok(format!("<jobId>{}</jobId><name>{}</name>", row.id, xml_escape(&row.name)))
}

pub async fn delete_scheduled_job(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let id: Uuid = need(p, "JobId")?.parse().map_err(|_| bad("InvalidParameterValue", "JobId must be a UUID"))?;
    let _ = crate::api::scheduled_jobs::delete_scheduled_job(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
    Ok("<return>true</return>".into())
}
