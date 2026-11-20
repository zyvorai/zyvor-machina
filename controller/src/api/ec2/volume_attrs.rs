// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Volume attributes and the native L4 load balancer. The balancer is not ELBv2: one listener,
//! kernel round-robin, TCP or HTTP health checks.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use uuid::Uuid;

use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::Kind;
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

pub async fn describe_volume_attribute(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let id = resolve(state, Kind::Volume, &need(p, "VolumeId")?, "InvalidVolume.NotFound").await?;
    let row: Option<(bool, Option<i64>, Option<i64>, Option<i64>, Option<i64>)> = crate::db::query_as(
        "SELECT delete_on_termination, read_iops, write_iops, read_bps, write_bps FROM volumes WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let Some((del, ri, wi, rb, wb)) = row else {
        return Err(bad("InvalidVolume.NotFound", "the volume does not exist"));
    };
    let n = |v: Option<i64>| v.map(|n| n.to_string()).unwrap_or_default();
    Ok(format!(
        "<volumeId>{}</volumeId><deleteOnTermination><value>{del}</value></deleteOnTermination><readIops>{}</readIops><writeIops>{}</writeIops><readBps>{}</readBps><writeBps>{}</writeBps>",
        need(p, "VolumeId")?,
        n(ri),
        n(wi),
        n(rb),
        n(wb)
    ))
}

pub async fn modify_volume_attribute(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let id = resolve(state, Kind::Volume, &need(p, "VolumeId")?, "InvalidVolume.NotFound").await?;
    if let Some(v) = p.get("DeleteOnTermination.Value").or_else(|| p.get("DeleteOnTermination")) {
        let value = matches!(v.as_str(), "true" | "1");
        let _ = crate::api::volumes::set_volume_delete_on_termination(
            State(state.clone()),
            Extension(actor.clone()),
            Path(id),
            Json(crate::api::volumes::DeleteOnTerminationBody { value }),
        )
        .await
        .map_err(api_err)?;
    }
    let read_iops = p.get("ReadIops").and_then(|s| s.parse().ok());
    let write_iops = p.get("WriteIops").and_then(|s| s.parse().ok());
    let read_bps = p.get("ReadBps").and_then(|s| s.parse().ok());
    let write_bps = p.get("WriteBps").and_then(|s| s.parse().ok());
    if read_iops.is_some() || write_iops.is_some() || read_bps.is_some() || write_bps.is_some() {
        let _ = crate::api::volumes::set_volume_iotune(
            State(state.clone()),
            Extension(actor.clone()),
            Path(id),
            Json(crate::api::volumes::IoTuneBody { read_iops, write_iops, read_bps, write_bps }),
        )
        .await
        .map_err(api_err)?;
    }
    Ok("<return>true</return>".into())
}

fn lb_id(id: Uuid) -> String {
    format!("lb-{}", &id.simple().to_string()[..17])
}

pub async fn describe_load_balancers(state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    let Json(rows) = crate::api::load_balancers::list_load_balancers(State(state.clone()), Extension(actor.clone())).await.map_err(api_err)?;
    let items: String = rows
        .iter()
        .map(|r| {
            format!(
                "<item><loadBalancerId>{}</loadBalancerId><loadBalancerName>{}</loadBalancerName><protocol>{}</protocol><listenerPort>{}</listenerPort><state>{}</state><healthCheck>{}</healthCheck></item>",
                lb_id(r.id),
                xml_escape(&r.name),
                xml_escape(&r.protocol),
                r.listener_port,
                xml_escape(&r.status),
                xml_escape(&r.hc_protocol)
            )
        })
        .collect();
    Ok(format!("<loadBalancerSet>{items}</loadBalancerSet>"))
}

pub async fn create_load_balancer(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let host: Uuid = need(p, "HostId")?.parse().map_err(|_| bad("InvalidParameterValue", "HostId must be a host UUID"))?;
    let port: u16 = need(p, "ListenerPort")?.parse().map_err(|_| bad("InvalidParameterValue", "ListenerPort must be a number"))?;
    let body = crate::api::load_balancers::CreateLoadBalancerBody {
        name: need(p, "LoadBalancerName")?,
        host_id: host,
        listener_port: port,
        protocol: p.get("Protocol").cloned().unwrap_or_else(|| "tcp".into()),
        project_id: p.get("ProjectId").and_then(|s| Uuid::parse_str(s).ok()),
    };
    let Json(row) = crate::api::load_balancers::create_load_balancer(State(state.clone()), Extension(actor.clone()), Json(body))
        .await
        .map_err(api_err)?;
    Ok(format!("<loadBalancerId>{}</loadBalancerId><dnsName>{}</dnsName>", lb_id(row.id), xml_escape(&row.name)))
}

pub async fn delete_load_balancer(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let raw = need(p, "LoadBalancerId")?;
    let hex = raw.strip_prefix("lb-").unwrap_or(&raw);
    if hex.len() != 17 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad("InvalidParameterValue", format!("The load balancer '{raw}' is not a valid id")));
    }
    let id: Option<Uuid> = crate::db::query_scalar("SELECT id FROM load_balancers WHERE lower(hex(id)) LIKE ?")
        .bind(format!("{hex}%"))
        .fetch_optional(&state.pool)
        .await?;
    let Some(id) = id else {
        return Err(bad("InvalidParameterValue", format!("The load balancer '{raw}' does not exist")));
    };
    let _ = crate::api::load_balancers::delete_load_balancer(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
    Ok("<return>true</return>".into())
}

/// Attach `GroupId.N` after `RunInstances` has ids. The run body has no security-group field.
pub async fn attach_run_groups(state: &AppState, actor: &AuthUser, p: &Params, vms: &[Uuid]) -> Result<(), Ec2Error> {
    let groups = super::indexed(p, "SecurityGroupId");
    if groups.is_empty() {
        return Ok(());
    }
    for vm in vms {
        for gid in &groups {
            let sg = resolve(state, Kind::SecurityGroup, gid, "InvalidGroup.NotFound").await?;
            let _ = crate::api::networking::attach_instance_security_group(
                State(state.clone()),
                Extension(actor.clone()),
                Path((*vm, sg)),
            )
            .await
            .map_err(api_err)?;
        }
    }
    Ok(())
}
