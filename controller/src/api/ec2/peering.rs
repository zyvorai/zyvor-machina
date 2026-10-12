// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! VPC peering and subnet IPAM. Accepting a peering sets status `planned`. Nothing forwards
//! packets. Subnet addresses are the managed pool on a subnet, not secondary IPs on an ENI.

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

pub(super) fn pcx(id: Uuid) -> String {
    format!("pcx-{}", &id.simple().to_string()[..17])
}

pub(super) async fn peering_uuid(state: &AppState, raw: &str) -> Result<Uuid, Ec2Error> {
    let hex = raw.strip_prefix("pcx-").unwrap_or(raw);
    if hex.len() != 17 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad("InvalidVpcPeeringConnectionId.NotFound", format!("The peering '{raw}' is not a valid id")));
    }
    let id: Option<Uuid> = crate::db::query_scalar("SELECT id FROM cloud_peerings WHERE lower(hex(id)) LIKE ?")
        .bind(format!("{hex}%"))
        .fetch_optional(&state.pool)
        .await?;
    id.ok_or_else(|| bad("InvalidVpcPeeringConnectionId.NotFound", format!("The peering '{raw}' does not exist")))
}

pub async fn describe_vpc_peering_connections(state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let rows: Vec<(Uuid, Uuid, Uuid, String)> = crate::db::query_as(
        "SELECT id, requester_id, accepter_id, status FROM cloud_peerings ORDER BY id",
    )
    .fetch_all(&state.pool)
    .await?;
    let items: String = rows
        .into_iter()
        .map(|(id, requester, accepter, status)| {
            format!(
                "<item><vpcPeeringConnectionId>{}</vpcPeeringConnectionId><requesterVpcInfo><vpcId>{}</vpcId></requesterVpcInfo><accepterVpcInfo><vpcId>{}</vpcId></accepterVpcInfo><status><code>{}</code><message>forwarding is not active</message></status><forwardingActive>false</forwardingActive></item>",
                pcx(id),
                ec2_id(Kind::Vpc, requester),
                ec2_id(Kind::Vpc, accepter),
                xml_escape(&status)
            )
        })
        .collect();
    Ok(format!("<vpcPeeringConnectionSet>{items}</vpcPeeringConnectionSet>"))
}

pub async fn create_vpc_peering_connection(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let requester = resolve(state, Kind::Vpc, &need(p, "VpcId")?, "InvalidVpcID.NotFound").await?;
    let accepter = resolve(state, Kind::Vpc, &need(p, "PeerVpcId")?, "InvalidVpcID.NotFound").await?;
    let body: crate::api::cloud::network::PeeringRequest =
        serde_json::from_value(serde_json::json!({ "accepter_id": accepter })).map_err(|e| bad("InvalidParameterValue", e.to_string()))?;
    let _ = crate::api::cloud::network::create_peering(State(state.clone()), Extension(actor.clone()), Path(requester), Json(body))
        .await
        .map_err(api_err)?;
    let id: Uuid = crate::db::query_scalar("SELECT id FROM cloud_peerings WHERE requester_id = ? AND accepter_id = ?")
        .bind(requester)
        .bind(accepter)
        .fetch_one(&state.pool)
        .await?;
    Ok(format!(
        "<vpcPeeringConnectionId>{}</vpcPeeringConnectionId><status><code>pending-acceptance</code></status><forwardingActive>false</forwardingActive>",
        pcx(id)
    ))
}

pub async fn accept_vpc_peering_connection(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let id = peering_uuid(state, &need(p, "VpcPeeringConnectionId")?).await?;
    let _ = crate::api::cloud::network::accept_peering(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
    Ok(format!("<vpcPeeringConnectionId>{}</vpcPeeringConnectionId><status><code>planned</code></status><forwardingActive>false</forwardingActive>", pcx(id)))
}

pub async fn allocate_subnet_address(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let subnet = resolve(state, Kind::Subnet, &need(p, "SubnetId")?, "InvalidSubnetID.NotFound").await?;
    let key = need(p, "RequestKey")?;
    let body: crate::api::cloud::network::Allocate =
        serde_json::from_value(serde_json::json!({ "request_key": key })).map_err(|e| bad("InvalidParameterValue", e.to_string()))?;
    let _ = crate::api::cloud::network::allocate_address(State(state.clone()), Extension(actor.clone()), Path(subnet), Json(body))
        .await
        .map_err(api_err)?;
    let row: Option<(Uuid, String)> = crate::db::query_as("SELECT id, address FROM cloud_ip_allocations WHERE subnet_id = ? AND request_key = ?")
        .bind(subnet)
        .bind(&key)
        .fetch_optional(&state.pool)
        .await?;
    let Some((id, address)) = row else {
        return Err(bad("InternalError", "allocation was not stored"));
    };
    Ok(format!(
        "<allocationId>{}</allocationId><subnetId>{}</subnetId><privateIpAddress>{}</privateIpAddress>",
        id,
        need(p, "SubnetId")?,
        xml_escape(&address)
    ))
}

pub async fn release_subnet_address(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let subnet = resolve(state, Kind::Subnet, &need(p, "SubnetId")?, "InvalidSubnetID.NotFound").await?;
    let allocation: Uuid = need(p, "AllocationId")?.parse().map_err(|_| bad("InvalidParameterValue", "AllocationId must be the allocation UUID"))?;
    let _ = crate::api::cloud::network::release_address(State(state.clone()), Extension(actor.clone()), Path((subnet, allocation)))
        .await
        .map_err(api_err)?;
    Ok("<return>true</return>".into())
}
