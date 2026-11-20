// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! VPC and subnet lifecycle on `POST /ec2`. A VPC is host-local. Routes are stored plans:
//! `create_route` returns `forwarding_active: false`, and the XML says so.

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

async fn host_of(state: &AppState, p: &Params) -> Result<Uuid, Ec2Error> {
    if let Some(id) = p.get("HostId") {
        return id.parse().map_err(|_| bad("InvalidParameterValue", "HostId must be a host UUID"));
    }
    let zone = need(p, "AvailabilityZone")?;
    let id: Option<Uuid> = crate::db::query_scalar("SELECT id FROM hosts WHERE hostname = ?")
        .bind(&zone)
        .fetch_optional(&state.pool)
        .await?;
    id.ok_or_else(|| bad("InvalidParameterValue", format!("no host named '{zone}'")))
}

pub async fn create_vpc(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let project: Uuid = need(p, "ProjectId")?.parse().map_err(|_| bad("InvalidParameterValue", "ProjectId must be a project UUID"))?;
    let name = p.get("TagSpecification.1.Tag.1.Value").cloned().or_else(|| p.get("Name").cloned()).unwrap_or_else(|| format!("vpc-{}", &Uuid::new_v4().simple().to_string()[..8]));
    let host = host_of(state, p).await?;
    let body: crate::api::cloud::network::CreateVpc = serde_json::from_value(serde_json::json!({
        "name": name,
        "cidr": need(p, "CidrBlock")?,
        "host_id": host,
    }))
    .map_err(|e| bad("InvalidParameterValue", e.to_string()))?;
    let Json(row) = crate::api::cloud::network::create_vpc(State(state.clone()), Extension(actor.clone()), Path(project), Json(body))
        .await
        .map_err(api_err)?;
    Ok(format!(
        "<vpc><vpcId>{}</vpcId><cidrBlock>{}</cidrBlock><state>available</state><isDefault>false</isDefault></vpc>",
        ec2_id(Kind::Vpc, row.id),
        xml_escape(&row.cidr)
    ))
}

pub async fn delete_vpc(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let id = resolve(state, Kind::Vpc, &need(p, "VpcId")?, "InvalidVpcID.NotFound").await?;
    let _ = crate::api::cloud::network::delete_vpc(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
    Ok("<return>true</return>".into())
}

pub async fn create_subnet(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let vpc = resolve(state, Kind::Vpc, &need(p, "VpcId")?, "InvalidVpcID.NotFound").await?;
    let name = p.get("Name").cloned().unwrap_or_else(|| format!("subnet-{}", &Uuid::new_v4().simple().to_string()[..8]));
    let body: crate::api::cloud::network::CreateSubnet = serde_json::from_value(serde_json::json!({
        "name": name,
        "cidr": need(p, "CidrBlock")?,
    }))
    .map_err(|e| bad("InvalidParameterValue", e.to_string()))?;
    let Json(row) = crate::api::cloud::network::create_subnet(State(state.clone()), Extension(actor.clone()), Path(vpc), Json(body))
        .await
        .map_err(api_err)?;
    let id = row.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let cidr = row.get("cidr").and_then(|v| v.as_str()).unwrap_or("");
    let subnet_id = Uuid::parse_str(id).map(|u| ec2_id(Kind::Subnet, u)).unwrap_or_else(|_| id.to_string());
    Ok(format!(
        "<subnet><subnetId>{}</subnetId><vpcId>{}</vpcId><cidrBlock>{}</cidrBlock><state>pending</state></subnet>",
        xml_escape(&subnet_id),
        need(p, "VpcId")?,
        xml_escape(cidr)
    ))
}

pub async fn delete_subnet(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let id = resolve(state, Kind::Subnet, &need(p, "SubnetId")?, "InvalidSubnetID.NotFound").await?;
    let _ = crate::api::cloud::network::delete_subnet(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
    Ok("<return>true</return>".into())
}

pub async fn create_route(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let vpc = resolve(state, Kind::Vpc, &need(p, "VpcId")?, "InvalidVpcID.NotFound").await?;
    let target = p.get("Target").cloned().unwrap_or_else(|| "local".into());
    let body: crate::api::cloud::network::RouteBody = serde_json::from_value(serde_json::json!({
        "destination": need(p, "DestinationCidrBlock")?,
        "target": target,
        "target_id": p.get("TargetId").and_then(|s| Uuid::parse_str(s).ok()),
    }))
    .map_err(|e| bad("InvalidParameterValue", e.to_string()))?;
    let Json(row) = crate::api::cloud::network::create_route(State(state.clone()), Extension(actor.clone()), Path(vpc), Json(body))
        .await
        .map_err(api_err)?;
    let active = row.get("forwarding_active").and_then(|v| v.as_bool()).unwrap_or(false);
    Ok(format!("<return>true</return><forwardingActive>{active}</forwardingActive>"))
}

pub async fn delete_route(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let vpc = resolve(state, Kind::Vpc, &need(p, "VpcId")?, "InvalidVpcID.NotFound").await?;
    let route: Uuid = need(p, "RouteId")?.parse().map_err(|_| bad("InvalidParameterValue", "RouteId must be the route UUID"))?;
    let _ = crate::api::cloud::network::delete_route(State(state.clone()), Extension(actor.clone()), Path((vpc, route)))
        .await
        .map_err(api_err)?;
    Ok("<return>true</return>".into())
}

pub async fn describe_regions(_state: &AppState, _actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    Ok("<regionInfo><item><regionName>machina</regionName><regionEndpoint>ec2</regionEndpoint></item></regionInfo>".into())
}
