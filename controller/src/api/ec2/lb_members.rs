// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Members and health checks on the native L4 load balancer. Not target groups.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
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

async fn balancer(state: &AppState, raw: &str) -> Result<Uuid, Ec2Error> {
    let hex = raw.strip_prefix("lb-").unwrap_or(raw);
    if hex.len() != 17 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad("InvalidParameterValue", format!("The load balancer '{raw}' is not a valid id")));
    }
    let id: Option<Uuid> = crate::db::query_scalar("SELECT id FROM load_balancers WHERE lower(hex(id)) LIKE ?")
        .bind(format!("{hex}%"))
        .fetch_optional(&state.pool)
        .await?;
    id.ok_or_else(|| bad("InvalidParameterValue", format!("The load balancer '{raw}' does not exist")))
}

pub async fn describe_load_balancer_members(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    let id = balancer(state, &need(p, "LoadBalancerId")?).await?;
    let Json(rows) = crate::api::load_balancers::list_lb_members(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
    let items: String = rows
        .iter()
        .map(|m| {
            format!(
                "<item><memberId>{}</memberId><instanceId>{}</instanceId><port>{}</port><weight>{}</weight><enabled>{}</enabled><health>{}</health><detail>{}</detail></item>",
                m.id,
                ec2_id(Kind::Vm, m.vm_id),
                m.port,
                m.weight,
                m.enabled,
                xml_escape(&m.health),
                xml_escape(&m.health_detail)
            )
        })
        .collect();
    Ok(format!("<memberSet>{items}</memberSet>"))
}

pub async fn register_instances(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let lb = balancer(state, &need(p, "LoadBalancerId")?).await?;
    let instance = super::more::resolve(state, Kind::Vm, &need(p, "InstanceId")?, "InvalidInstanceID.NotFound").await?;
    let port: u16 = need(p, "Port")?.parse().map_err(|_| bad("InvalidParameterValue", "Port must be a number"))?;
    let weight: u32 = p.get("Weight").map(|s| s.parse()).transpose().map_err(|_| bad("InvalidParameterValue", "Weight must be a number"))?.unwrap_or(1);
    let Json(row) = crate::api::load_balancers::add_lb_member(
        State(state.clone()),
        Extension(actor.clone()),
        Path(lb),
        Json(crate::api::load_balancers::AddLbMemberBody { vm_id: instance, port, weight }),
    )
    .await
    .map_err(api_err)?;
    Ok(format!("<memberId>{}</memberId><instanceId>{}</instanceId><health>{}</health>", row.id, ec2_id(Kind::Vm, row.vm_id), xml_escape(&row.health)))
}

pub async fn deregister_instances(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let lb = balancer(state, &need(p, "LoadBalancerId")?).await?;
    let member: Uuid = need(p, "MemberId")?.parse().map_err(|_| bad("InvalidParameterValue", "MemberId must be the member UUID"))?;
    let _ = crate::api::load_balancers::delete_lb_member(State(state.clone()), Extension(actor.clone()), Path((lb, member))).await.map_err(api_err)?;
    Ok("<return>true</return>".into())
}

pub async fn configure_health_check(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let lb = balancer(state, &need(p, "LoadBalancerId")?).await?;
    let protocol = need(p, "Protocol")?;
    let body = crate::api::load_balancers::HealthCheckBody {
        protocol,
        port: p.get("Port").and_then(|s| s.parse().ok()),
        path: p.get("Path").cloned().unwrap_or_else(|| "/".into()),
        interval_secs: p.get("Interval").and_then(|s| s.parse().ok()).unwrap_or(30),
        timeout_secs: p.get("Timeout").and_then(|s| s.parse().ok()).unwrap_or(5),
        healthy_threshold: p.get("HealthyThreshold").and_then(|s| s.parse().ok()).unwrap_or(2),
        unhealthy_threshold: p.get("UnhealthyThreshold").and_then(|s| s.parse().ok()).unwrap_or(2),
    };
    let _ = crate::api::load_balancers::set_health_check(State(state.clone()), Extension(actor.clone()), Path(lb), Json(body))
        .await
        .map_err(api_err)?;
    Ok("<return>true</return>".into())
}
