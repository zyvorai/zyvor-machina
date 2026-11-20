// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Capacity, cost, rightsizing, placement, webhooks, and host fence.
//! Propose calls file an approval action. They do not resize or move a VM by themselves.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use uuid::Uuid;

use crate::auth::{require_admin, require_operator, AuthUser};
use crate::resource_ids::Kind;
use crate::state::AppState;

use super::more::api_err;
use super::{indexed, xml_escape, Ec2Error};

type Params = BTreeMap<String, String>;

fn bad(code: &'static str, msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad(code, msg)
}

fn need(p: &Params, k: &str) -> Result<String, Ec2Error> {
    p.get(k).cloned().ok_or_else(|| bad("MissingParameter", format!("The request must contain the parameter {k}")))
}

pub async fn describe_capacity(state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    let Json(row) = crate::api::reports::capacity_report(State(state.clone()), Extension(actor.clone())).await.map_err(api_err)?;
    Ok(format!(
        "<hostsOnline>{}</hostsOnline><hostsOffline>{}</hostsOffline><totalVms>{}</totalVms><runningVms>{}</runningVms><memoryHeadroomMib>{}</memoryHeadroomMib><avgCpuPercent>{}</avgCpuPercent><estimatedSmallVmsAddable>{}</estimatedSmallVmsAddable>",
        row.hosts_online, row.hosts_offline, row.total_vms, row.running_vms, row.memory_headroom_mib, row.avg_cpu_percent, row.estimated_small_vms_addable
    ))
}

pub async fn describe_cost_estimate(state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    let Json(row) = crate::api::reports::finops_report(State(state.clone()), Extension(actor.clone())).await.map_err(api_err)?;
    Ok(format!(
        "<vmCount>{}</vmCount><runningVms>{}</runningVms><totalVcpu>{}</totalVcpu><totalMemoryGib>{}</totalMemoryGib><estimatedMonthlyUsd>{}</estimatedMonthlyUsd>",
        row.vm_count, row.running_vms, row.total_vcpu, row.total_memory_gib, row.estimated_monthly_usd
    ))
}

pub async fn describe_rightsizing(state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    let Json(row) = crate::api::autopilot::rightsizing(State(state.clone()), Extension(actor.clone())).await.map_err(api_err)?;
    Ok(format!("<rightsizing>{}</rightsizing>", xml_escape(&row.to_string())))
}

pub async fn propose_resize(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let vm = super::more::resolve(state, Kind::Vm, &need(p, "InstanceId")?, "InvalidInstanceID.NotFound").await?;
    let body = crate::api::autopilot::ResizeBody {
        vm_id: vm,
        vcpus: p.get("Vcpus").and_then(|s| s.parse().ok()),
        memory_mib: p.get("MemoryMib").and_then(|s| s.parse().ok()),
    };
    let Json(row) = crate::api::autopilot::propose_resize(State(state.clone()), Extension(actor.clone()), Json(body)).await.map_err(api_err)?;
    Ok(format!("<action>{}</action>", xml_escape(&serde_json::to_string(&row).unwrap_or_default())))
}

pub async fn describe_consolidation(state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    let Json(plan) = crate::api::autopilot::consolidation(State(state.clone()), Extension(actor.clone())).await.map_err(api_err)?;
    Ok(format!("<consolidation>{}</consolidation>", xml_escape(&serde_json::to_string(&plan).unwrap_or_default())))
}

pub async fn propose_consolidation(state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let Json(row) = crate::api::autopilot::propose_consolidation(State(state.clone()), Extension(actor.clone())).await.map_err(api_err)?;
    Ok(format!("<action>{}</action>", xml_escape(&serde_json::to_string(&row).unwrap_or_default())))
}

pub async fn describe_placement(state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    // the REST handler stores the recommendations it computes, so a read-only role must not trigger it
    require_operator(actor)?;
    let Json(rows) = crate::api::placement::list_recommendations(State(state.clone())).await.map_err(api_err)?;
    Ok(format!("<placement>{}</placement>", xml_escape(&serde_json::to_string(&rows).unwrap_or_default())))
}

pub async fn create_webhook(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_admin(actor)?;
    let events = indexed(p, "Event");
    if events.is_empty() {
        return Err(bad("MissingParameter", "The request must contain the parameter Event.1"));
    }
    let body = crate::api::webhooks::CreateWebhookBody {
        url: need(p, "Url")?,
        events,
        secret: p.get("Secret").cloned().unwrap_or_default(),
    };
    let Json(row) = crate::api::webhooks::create_webhook(State(state.clone()), Extension(actor.clone()), Json(body)).await.map_err(api_err)?;
    Ok(format!("<webhookId>{}</webhookId><url>{}</url>", row.id, xml_escape(&row.url)))
}

pub async fn describe_webhooks(state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    require_admin(actor)?;
    let Json(rows) = crate::api::webhooks::list_webhooks(State(state.clone()), Extension(actor.clone())).await.map_err(api_err)?;
    let items: String = rows
        .iter()
        .map(|r| format!("<item><webhookId>{}</webhookId><url>{}</url><enabled>{}</enabled></item>", r.id, xml_escape(&r.url), r.enabled))
        .collect();
    Ok(format!("<webhookSet>{items}</webhookSet>"))
}

pub async fn delete_webhook(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_admin(actor)?;
    let id: Uuid = need(p, "WebhookId")?.parse().map_err(|_| bad("InvalidParameterValue", "WebhookId must be a UUID"))?;
    let _ = crate::api::webhooks::delete_webhook(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
    Ok("<return>true</return>".into())
}

pub async fn fence_host(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_admin(actor)?;
    let host: Uuid = need(p, "HostId")?.parse().map_err(|_| bad("InvalidParameterValue", "HostId must be a host UUID"))?;
    let Json(row) = crate::api::maintenance::fence_host_manual(State(state.clone()), Extension(actor.clone()), Path(host)).await.map_err(api_err)?;
    let fenced = row.get("fenced").and_then(|v| v.as_bool()).unwrap_or(false);
    Ok(format!("<hostId>{host}</hostId><fenced>{fenced}</fenced>"))
}
