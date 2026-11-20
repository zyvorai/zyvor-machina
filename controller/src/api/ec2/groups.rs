// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Instance groups and subnet NAT. Groups are `asg-` ids, not the AWS Auto Scaling API.
//! NAT is the host masquerade already on `PUT /api/v1/cloud/subnets/{id}/nat`, not a `nat-` object.

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

pub async fn describe_instance_groups(state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let rows: Vec<(Uuid, Uuid, Uuid, String, String, bool)> = crate::db::query_as(
        "SELECT id, project_id, template_id, name, policy_json, paused FROM cloud_instance_groups ORDER BY name",
    )
    .fetch_all(&state.pool)
    .await?;
    let items: String = rows
        .into_iter()
        .map(|(id, project, template, name, policy, paused)| {
            format!(
                "<item><instanceGroupId>{}</instanceGroupId><instanceGroupName>{}</instanceGroupName><projectId>{project}</projectId><launchTemplateId>{}</launchTemplateId><paused>{paused}</paused><policy>{}</policy></item>",
                ec2_id(Kind::InstanceGroup, id),
                xml_escape(&name),
                ec2_id(Kind::LaunchTemplate, template),
                xml_escape(&policy)
            )
        })
        .collect();
    Ok(format!("<instanceGroups>{items}</instanceGroups>"))
}

pub async fn update_instance_group(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let id = resolve(state, Kind::InstanceGroup, &need(p, "InstanceGroupId")?, "InvalidParameterValue").await?;
    let min: u32 = need(p, "MinSize")?.parse().map_err(|_| bad("InvalidParameterValue", "MinSize must be a number"))?;
    let max: u32 = need(p, "MaxSize")?.parse().map_err(|_| bad("InvalidParameterValue", "MaxSize must be a number"))?;
    let desired: u32 = need(p, "DesiredCapacity")?.parse().map_err(|_| bad("InvalidParameterValue", "DesiredCapacity must be a number"))?;
    let cooldown: u32 = p.get("Cooldown").map(|s| s.parse()).transpose().map_err(|_| bad("InvalidParameterValue", "Cooldown must be a number"))?.unwrap_or(300);
    let paused = matches!(p.get("Paused").map(String::as_str), Some("true") | Some("1"));
    let scale_in = if matches!(p.get("ScaleIn").map(String::as_str), Some("sleep")) { "sleep" } else { "stop" };
    let body: crate::api::cloud::elastic::UpdateGroup = serde_json::from_value(serde_json::json!({
        "policy": { "min": min, "max": max, "desired": desired, "cooldown_secs": cooldown, "scale_in": scale_in },
        "paused": paused
    }))
    .map_err(|e| bad("InvalidParameterValue", e.to_string()))?;
    let _ = crate::api::cloud::elastic::update_group(State(state.clone()), Extension(actor.clone()), Path(id), Json(body))
        .await
        .map_err(api_err)?;
    Ok("<return>true</return>".into())
}

/// `ModifySubnetAttribute` with `Attribute=nat`. Host masquerade, IPv4, this host only.
pub async fn modify_subnet_attribute(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    if p.get("Attribute").map(String::as_str) != Some("nat") {
        return Err(bad("InvalidParameterValue", "Attribute must be nat"));
    }
    let id = resolve(state, Kind::Subnet, &need(p, "SubnetId")?, "InvalidSubnetID.NotFound").await?;
    let enabled = matches!(p.get("Value").map(String::as_str), Some("true") | Some("1"));
    let body: crate::api::cloud::network::SetNat = serde_json::from_value(serde_json::json!({ "enabled": enabled }))
        .map_err(|e| bad("InvalidParameterValue", e.to_string()))?;
    let _ = crate::api::cloud::network::set_subnet_nat(State(state.clone()), Extension(actor.clone()), Path(id), Json(body))
        .await
        .map_err(api_err)?;
    Ok("<return>true</return>".into())
}

/// `ModifyInstanceAttribute` with `Attribute=groupSet`. Replaces the instance's security groups.
pub async fn modify_group_set(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let vm = resolve(state, Kind::Vm, &need(p, "InstanceId")?, "InvalidInstanceID.NotFound").await?;
    let wanted = super::indexed(p, "GroupId");
    if wanted.is_empty() {
        return Err(bad("MissingParameter", "The request must contain the parameter GroupId.1"));
    }
    let current: Vec<Uuid> = crate::db::query_scalar("SELECT sg_id FROM instance_security_groups WHERE vm_id = ?")
        .bind(vm)
        .fetch_all(&state.pool)
        .await?;
    for sg in &current {
        let _ = crate::api::networking::detach_instance_security_group(
            State(state.clone()),
            Extension(actor.clone()),
            Path((vm, *sg)),
        )
        .await
        .map_err(api_err)?;
    }
    for gid in wanted {
        let sg = resolve(state, Kind::SecurityGroup, &gid, "InvalidGroup.NotFound").await?;
        let _ = crate::api::networking::attach_instance_security_group(
            State(state.clone()),
            Extension(actor.clone()),
            Path((vm, sg)),
        )
        .await
        .map_err(api_err)?;
    }
    Ok("<return>true</return>".into())
}
