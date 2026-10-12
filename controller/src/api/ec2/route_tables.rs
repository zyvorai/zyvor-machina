// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Route tables and routes on `POST /ec2`. Every VPC has a main route table (created on first use, with the VPC's
//! `local` route); more tables are associated with subnets one by one.
//!
//! Routes are **stored plans** (`forwardingActive=false` in the XML): a route to an internet gateway, peering, instance or
//! interface changes nothing on a host. The one route that does something is the default route (`0.0.0.0/0`) to a NAT
//! gateway: the subnets that use that table get the host masquerade of `PUT /api/v1/cloud/subnets/{id}/nat`
//! (`reconcile_table`), and lose it again when the route is replaced or deleted.
//!
//! The legacy form (`VpcId` + `Target`/`DestinationCidrBlock`, no `RouteTableId`) still goes to the VPC's own route plan
//! (`cloud_routes`), which the main route table also lists.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::{Extension, Json};
use uuid::Uuid;

use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::{ec2_id, Kind};
use crate::state::AppState;

use super::more::resolve;
use super::netcommon::{all_tags, apply_tags, authorize_vpc, bad, ensure_defaults, main_route_table_id, need, passes, spec_tags, tags_of_item, tags_xml, valid_cidr, vpc_of_subnet};
use super::{indexed, xml_escape, Ec2Error, OWNER};

type Params = BTreeMap<String, String>;

const DEFAULT_ROUTE: &str = "0.0.0.0/0";

/// Parameters of CreateRoute/ReplaceRoute that name a target this backend does not have.
const UNSUPPORTED_TARGETS: &[&str] = &[
    "TransitGatewayId",
    "VpcEndpointId",
    "EgressOnlyInternetGatewayId",
    "CarrierGatewayId",
    "LocalGatewayId",
    "CoreNetworkArn",
    "DestinationIpv6CidrBlock",
    "DestinationPrefixListId",
];

// ---- targets ---------------------------------------------------------------------------------------------------------

/// The kind (`igw`, `nat`, `peering`, `instance`, `eni`) and id of the target a route request names.
async fn parse_target(state: &AppState, vpc: Uuid, p: &Params) -> Result<(&'static str, Uuid), Ec2Error> {
    for k in UNSUPPORTED_TARGETS {
        if p.contains_key(*k) {
            return Err(bad("UnsupportedOperation", format!("{k} is not supported by this backend")));
        }
    }
    let named: Vec<&str> = ["GatewayId", "NatGatewayId", "VpcPeeringConnectionId", "InstanceId", "NetworkInterfaceId"]
        .into_iter()
        .filter(|k| p.contains_key(*k))
        .collect();
    if named.len() != 1 {
        return Err(bad("InvalidParameterCombination", "specify exactly one of GatewayId, NatGatewayId, VpcPeeringConnectionId, InstanceId or NetworkInterfaceId"));
    }
    match named[0] {
        "GatewayId" => {
            let raw = need(p, "GatewayId")?;
            if !raw.starts_with("igw-") {
                return Err(bad("UnsupportedOperation", "only internet gateways (igw-) can be a GatewayId target"));
            }
            let igw = resolve(state, Kind::InternetGateway, &raw, "InvalidGatewayID.NotFound").await?;
            let attached: Option<Uuid> = crate::db::query_scalar("SELECT vpc_id FROM ec2_internet_gateways WHERE id = ?").bind(igw).fetch_one(&state.pool).await?;
            if attached != Some(vpc) {
                return Err(bad("InvalidParameterValue", "the internet gateway is not attached to this route table's VPC"));
            }
            Ok(("igw", igw))
        }
        "NatGatewayId" => {
            let nat = resolve(state, Kind::NatGateway, &need(p, "NatGatewayId")?, "InvalidNatGatewayID.NotFound").await?;
            let nat_vpc: Uuid = crate::db::query_scalar("SELECT vpc_id FROM ec2_nat_gateways WHERE id = ?").bind(nat).fetch_one(&state.pool).await?;
            if nat_vpc != vpc {
                return Err(bad("InvalidParameterValue", "the NAT gateway is in another VPC"));
            }
            Ok(("nat", nat))
        }
        "VpcPeeringConnectionId" => {
            let pcx = super::peering::peering_uuid(state, &need(p, "VpcPeeringConnectionId")?).await?;
            let ok: Option<i64> = crate::db::query_scalar(
                "SELECT 1 FROM cloud_peerings WHERE id = ? AND (requester_id = ? OR accepter_id = ?) AND status = 'planned'",
            )
            .bind(pcx)
            .bind(vpc)
            .bind(vpc)
            .fetch_optional(&state.pool)
            .await?;
            if ok.is_none() {
                return Err(bad("InvalidParameterValue", "an accepted peering connection of this VPC is required"));
            }
            Ok(("peering", pcx))
        }
        "InstanceId" => Ok(("instance", resolve(state, Kind::Vm, &need(p, "InstanceId")?, "InvalidInstanceID.NotFound").await?)),
        _ => Ok(("eni", resolve(state, Kind::Port, &need(p, "NetworkInterfaceId")?, "InvalidNetworkInterfaceID.NotFound").await?)),
    }
}

/// The element and EC2 id a route target is shown with.
fn target_element(kind: &str, id: Option<Uuid>) -> Option<(&'static str, String)> {
    let id = id?;
    Some(match kind {
        "igw" => ("gatewayId", ec2_id(Kind::InternetGateway, id)),
        "nat" => ("natGatewayId", ec2_id(Kind::NatGateway, id)),
        "peering" => ("vpcPeeringConnectionId", super::peering::pcx(id)),
        "instance" => ("instanceId", ec2_id(Kind::Vm, id)),
        "eni" => ("networkInterfaceId", ec2_id(Kind::Port, id)),
        _ => return None,
    })
}

fn route_xml(destination: &str, kind: &str, target: Option<Uuid>, live: bool, origin: &str) -> String {
    let (element, state_name) = match kind {
        "local" => ("<gatewayId>local</gatewayId>".to_string(), "active"),
        "blackhole" => (String::new(), "blackhole"),
        _ => match target_element(kind, target) {
            Some((tag, id)) => (format!("<{tag}>{id}</{tag}>"), if live { "active" } else { "blackhole" }),
            None => (String::new(), "blackhole"),
        },
    };
    format!(
        "<item><destinationCidrBlock>{}</destinationCidrBlock>{element}<state>{state_name}</state><origin>{origin}</origin><forwardingActive>false</forwardingActive></item>",
        xml_escape(destination)
    )
}

// ---- NAT follows the default route --------------------------------------------------------------------------------

async fn table_has_nat_default(state: &AppState, table: Uuid) -> bool {
    let found: Result<Option<i64>, _> = crate::db::query_scalar(
        "SELECT 1 FROM ec2_routes r JOIN ec2_nat_gateways n ON n.id = r.target_id WHERE r.route_table_id = ? AND r.destination = '0.0.0.0/0' AND r.target_kind = 'nat'",
    )
    .bind(table)
    .fetch_optional(&state.pool)
    .await;
    matches!(found, Ok(Some(_)))
}

/// The subnets that use a route table: the associated ones, and for a main table every subnet without an association.
async fn subnets_of_table(state: &AppState, table: Uuid) -> Vec<Uuid> {
    let mut out: Vec<Uuid> = crate::db::query_scalar("SELECT subnet_id FROM ec2_route_table_assocs WHERE route_table_id = ?")
        .bind(table)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();
    let main: Option<(Uuid, bool)> = crate::db::query_as("SELECT vpc_id, is_main FROM ec2_route_tables WHERE id = ?").bind(table).fetch_optional(&state.pool).await.ok().flatten();
    if let Some((vpc, true)) = main {
        let free: Vec<Uuid> = crate::db::query_scalar("SELECT id FROM cloud_subnets WHERE vpc_id = ? AND id NOT IN (SELECT subnet_id FROM ec2_route_table_assocs)")
            .bind(vpc)
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();
        out.extend(free);
    }
    out
}

async fn set_nat(state: &AppState, actor: &AuthUser, subnet: Uuid, enabled: bool) {
    let current: Option<bool> = crate::db::query_scalar("SELECT nat_enabled FROM cloud_subnets WHERE id = ?").bind(subnet).fetch_optional(&state.pool).await.ok().flatten();
    if current.is_none() || current == Some(enabled) {
        return;
    }
    let body: crate::api::cloud::network::SetNat = match serde_json::from_value(serde_json::json!({ "enabled": enabled })) {
        Ok(b) => b,
        Err(_) => return,
    };
    if let Err(e) = crate::api::cloud::network::set_subnet_nat(State(state.clone()), Extension(actor.clone()), Path(subnet), Json(body)).await {
        tracing::warn!(%subnet, enabled, "could not change the NAT of a subnet after a route change: {}", e.message);
    }
}

/// Make the host masquerade of every subnet that uses `table` match whether the table sends `0.0.0.0/0` to a NAT gateway.
/// Only called when a change touched the default route, so a masquerade an operator switched on by hand survives other edits.
pub async fn reconcile_table(state: &AppState, actor: &AuthUser, table: Uuid) {
    let wants = table_has_nat_default(state, table).await;
    for subnet in subnets_of_table(state, table).await {
        set_nat(state, actor, subnet, wants).await;
    }
}

async fn table_of_subnet(state: &AppState, subnet: Uuid) -> Result<Uuid, Ec2Error> {
    let explicit: Option<Uuid> = crate::db::query_scalar("SELECT route_table_id FROM ec2_route_table_assocs WHERE subnet_id = ?").bind(subnet).fetch_optional(&state.pool).await?;
    match explicit {
        Some(t) => Ok(t),
        None => Ok(main_route_table_id(vpc_of_subnet(state, subnet).await?)),
    }
}

// ---- describe ------------------------------------------------------------------------------------------------------

pub async fn describe_route_tables(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    ensure_defaults(state).await?;
    let wanted = indexed(p, "RouteTableId");
    let only_vpc = match p.get("VpcId") {
        Some(v) => Some(resolve(state, Kind::Vpc, v, "InvalidVpcID.NotFound").await?),
        None => None,
    };
    let tags = all_tags(state).await?;
    let rows: Vec<(Uuid, Uuid, bool)> = crate::db::query_as("SELECT id, vpc_id, is_main FROM ec2_route_tables ORDER BY vpc_id, is_main DESC, created_at, id").fetch_all(&state.pool).await?;
    for w in &wanted {
        if !rows.iter().any(|r| &ec2_id(Kind::RouteTable, r.0) == w) {
            return Err(bad("InvalidRouteTableID.NotFound", format!("The route table '{w}' does not exist")));
        }
    }
    let cidrs: BTreeMap<Uuid, String> = crate::db::query_as::<_, (Uuid, String)>("SELECT id, cidr FROM cloud_vpcs").fetch_all(&state.pool).await?.into_iter().collect();
    let nat_ids: Vec<Uuid> = crate::db::query_scalar("SELECT id FROM ec2_nat_gateways").fetch_all(&state.pool).await?;
    let igw_rows: Vec<(Uuid, Option<Uuid>)> = crate::db::query_as("SELECT id, vpc_id FROM ec2_internet_gateways").fetch_all(&state.pool).await?;
    let mut items = String::new();
    for (id, vpc, is_main) in rows {
        let eid = ec2_id(Kind::RouteTable, id);
        if !wanted.is_empty() && !wanted.contains(&eid) {
            continue;
        }
        if only_vpc.is_some_and(|v| v != vpc) {
            continue;
        }
        let vpc_cidr = cidrs.get(&vpc).cloned().unwrap_or_default();
        // routes: the local one, the stored ones, and for the main table the VPC's older route plan
        struct R {
            dest: String,
            kind: String,
            target: Option<Uuid>,
            origin: &'static str,
        }
        let mut routes = vec![R { dest: vpc_cidr.clone(), kind: "local".into(), target: None, origin: "CreateRouteTable" }];
        let stored: Vec<(String, String, Option<Uuid>)> =
            crate::db::query_as("SELECT destination, target_kind, target_id FROM ec2_routes WHERE route_table_id = ? ORDER BY destination").bind(id).fetch_all(&state.pool).await?;
        routes.extend(stored.into_iter().map(|(dest, kind, target)| R { dest, kind, target, origin: "CreateRoute" }));
        if is_main {
            let legacy: Vec<(String, String, Option<Uuid>)> =
                crate::db::query_as("SELECT destination, target, target_id FROM cloud_routes WHERE vpc_id = ? ORDER BY destination").bind(vpc).fetch_all(&state.pool).await?;
            routes.extend(legacy.into_iter().map(|(dest, kind, target)| R { dest, kind, target, origin: "CreateRoute" }));
        }
        let live = |kind: &str, target: Option<Uuid>| match (kind, target) {
            ("igw", Some(t)) => igw_rows.iter().any(|(i, v)| *i == t && *v == Some(vpc)),
            ("nat", Some(t)) => nat_ids.contains(&t),
            _ => true,
        };
        let assocs: Vec<(Uuid, Uuid)> =
            crate::db::query_as("SELECT id, subnet_id FROM ec2_route_table_assocs WHERE route_table_id = ? ORDER BY id").bind(id).fetch_all(&state.pool).await?;
        let main_assoc = ec2_id(Kind::RouteTableAssociation, super::netcommon::derived_id(id, "main-association"));
        let assoc_ids: Vec<String> = assocs.iter().map(|(a, _)| ec2_id(Kind::RouteTableAssociation, *a)).collect();
        let assoc_subnets: Vec<String> = assocs.iter().map(|(_, s)| ec2_id(Kind::Subnet, *s)).collect();
        let t = tags_of_item(&tags, Kind::RouteTable, id);
        if !passes(p, &t, |n| match n {
            "route-table-id" => Some(vec![eid.clone()]),
            "vpc-id" => Some(vec![ec2_id(Kind::Vpc, vpc)]),
            "owner-id" => Some(vec![OWNER.to_string()]),
            "association.main" => Some(vec![is_main.to_string()]),
            "association.route-table-id" => Some(if is_main || !assocs.is_empty() { vec![eid.clone()] } else { vec![] }),
            "association.route-table-association-id" => Some(if is_main { assoc_ids.iter().cloned().chain(std::iter::once(main_assoc.clone())).collect() } else { assoc_ids.clone() }),
            "association.subnet-id" => Some(assoc_subnets.clone()),
            "route.destination-cidr-block" => Some(routes.iter().map(|r| r.dest.clone()).collect()),
            "route.gateway-id" => Some(
                routes
                    .iter()
                    .filter_map(|r| match r.kind.as_str() {
                        "local" => Some("local".to_string()),
                        "igw" => target_element("igw", r.target).map(|x| x.1),
                        _ => None,
                    })
                    .collect(),
            ),
            "route.nat-gateway-id" => Some(routes.iter().filter(|r| r.kind == "nat").filter_map(|r| target_element(&r.kind, r.target).map(|x| x.1)).collect()),
            "route.vpc-peering-connection-id" => Some(routes.iter().filter(|r| r.kind == "peering").filter_map(|r| target_element(&r.kind, r.target).map(|x| x.1)).collect()),
            "route.state" => Some(routes.iter().map(|r| if r.kind == "blackhole" || !live(&r.kind, r.target) { "blackhole".to_string() } else { "active".to_string() }).collect()),
            _ => None,
        }) {
            continue;
        }
        let route_set: String = routes.iter().map(|r| route_xml(&r.dest, &r.kind, r.target, live(&r.kind, r.target), r.origin)).collect();
        let mut assoc_set = String::new();
        if is_main {
            assoc_set.push_str(&format!(
                "<item><routeTableAssociationId>{main_assoc}</routeTableAssociationId><routeTableId>{eid}</routeTableId><main>true</main><associationState><state>associated</state></associationState></item>"
            ));
        }
        for (a, s) in &assocs {
            assoc_set.push_str(&format!(
                "<item><routeTableAssociationId>{}</routeTableAssociationId><routeTableId>{eid}</routeTableId><subnetId>{}</subnetId><main>false</main><associationState><state>associated</state></associationState></item>",
                ec2_id(Kind::RouteTableAssociation, *a),
                ec2_id(Kind::Subnet, *s)
            ));
        }
        items.push_str(&format!(
            "<item><routeTableId>{eid}</routeTableId><vpcId>{}</vpcId><ownerId>{OWNER}</ownerId><routeSet>{route_set}</routeSet><associationSet>{assoc_set}</associationSet><propagatingVgwSet/>{}</item>",
            ec2_id(Kind::Vpc, vpc),
            tags_xml(&tags, Kind::RouteTable, id)
        ));
    }
    Ok(format!("<routeTableSet>{items}</routeTableSet>"))
}

// ---- tables ----------------------------------------------------------------------------------------------------------

pub async fn create_route_table(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let vpc = resolve(state, Kind::Vpc, &need(p, "VpcId")?, "InvalidVpcID.NotFound").await?;
    authorize_vpc(state, actor, vpc, true).await?;
    ensure_defaults(state).await?;
    let id = Uuid::new_v4();
    crate::db::query("INSERT INTO ec2_route_tables (id, vpc_id, is_main) VALUES (?, ?, FALSE)").bind(id).bind(vpc).execute(&state.pool).await?;
    apply_tags(state, Kind::RouteTable, id, &spec_tags(p, "route-table")).await?;
    let cidr: String = crate::db::query_scalar("SELECT cidr FROM cloud_vpcs WHERE id = ?").bind(vpc).fetch_one(&state.pool).await?;
    let tags = all_tags(state).await?;
    Ok(format!(
        "<routeTable><routeTableId>{}</routeTableId><vpcId>{}</vpcId><ownerId>{OWNER}</ownerId><routeSet>{}</routeSet><associationSet/><propagatingVgwSet/>{}</routeTable>",
        ec2_id(Kind::RouteTable, id),
        ec2_id(Kind::Vpc, vpc),
        route_xml(&cidr, "local", None, true, "CreateRouteTable"),
        tags_xml(&tags, Kind::RouteTable, id)
    ))
}

async fn table_vpc(state: &AppState, table: Uuid) -> Result<(Uuid, bool), Ec2Error> {
    Ok(crate::db::query_as("SELECT vpc_id, is_main FROM ec2_route_tables WHERE id = ?").bind(table).fetch_one(&state.pool).await?)
}

pub async fn delete_route_table(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let table = resolve(state, Kind::RouteTable, &need(p, "RouteTableId")?, "InvalidRouteTableID.NotFound").await?;
    let (vpc, is_main) = table_vpc(state, table).await?;
    authorize_vpc(state, actor, vpc, true).await?;
    if is_main {
        return Err(bad("DependencyViolation", "the main route table of a VPC cannot be deleted"));
    }
    let used: Option<i64> = crate::db::query_scalar("SELECT 1 FROM ec2_route_table_assocs WHERE route_table_id = ?").bind(table).fetch_optional(&state.pool).await?;
    if used.is_some() {
        return Err(bad("DependencyViolation", "the route table has subnet associations"));
    }
    crate::db::query("DELETE FROM ec2_routes WHERE route_table_id = ?").bind(table).execute(&state.pool).await?;
    crate::db::query("DELETE FROM ec2_route_tables WHERE id = ?").bind(table).execute(&state.pool).await?;
    Ok("<return>true</return>".into())
}

// ---- associations --------------------------------------------------------------------------------------------------

/// Apply the masquerade of the new effective table to a subnet whose table changed, when either side has a NAT default.
async fn subnet_table_changed(state: &AppState, actor: &AuthUser, subnet: Uuid, before: Uuid, after: Uuid) {
    let was = table_has_nat_default(state, before).await;
    let now = table_has_nat_default(state, after).await;
    if was || now {
        set_nat(state, actor, subnet, now).await;
    }
}

pub async fn associate_route_table(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    ensure_defaults(state).await?;
    let table = resolve(state, Kind::RouteTable, &need(p, "RouteTableId")?, "InvalidRouteTableID.NotFound").await?;
    let subnet = resolve(state, Kind::Subnet, &need(p, "SubnetId")?, "InvalidSubnetID.NotFound").await?;
    let (vpc, _) = table_vpc(state, table).await?;
    authorize_vpc(state, actor, vpc, true).await?;
    if vpc_of_subnet(state, subnet).await? != vpc {
        return Err(bad("InvalidParameterValue", "the subnet and the route table are in different VPCs"));
    }
    let before = table_of_subnet(state, subnet).await?;
    let exists: Option<i64> = crate::db::query_scalar("SELECT 1 FROM ec2_route_table_assocs WHERE subnet_id = ?").bind(subnet).fetch_optional(&state.pool).await?;
    if exists.is_some() {
        return Err(bad("Resource.AlreadyAssociated", "the subnet already has a route table association"));
    }
    let id = Uuid::new_v4();
    crate::db::query("INSERT INTO ec2_route_table_assocs (id, route_table_id, subnet_id) VALUES (?, ?, ?)").bind(id).bind(table).bind(subnet).execute(&state.pool).await?;
    subnet_table_changed(state, actor, subnet, before, table).await;
    Ok(format!("<associationId>{}</associationId><associationState><state>associated</state></associationState>", ec2_id(Kind::RouteTableAssociation, id)))
}

pub async fn disassociate_route_table(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let assoc = resolve(state, Kind::RouteTableAssociation, &need(p, "AssociationId")?, "InvalidAssociationID.NotFound").await?;
    let (table, subnet): (Uuid, Uuid) = crate::db::query_as("SELECT route_table_id, subnet_id FROM ec2_route_table_assocs WHERE id = ?").bind(assoc).fetch_one(&state.pool).await?;
    let (vpc, _) = table_vpc(state, table).await?;
    authorize_vpc(state, actor, vpc, true).await?;
    crate::db::query("DELETE FROM ec2_route_table_assocs WHERE id = ?").bind(assoc).execute(&state.pool).await?;
    let after = main_route_table_id(vpc);
    subnet_table_changed(state, actor, subnet, table, after).await;
    Ok("<return>true</return>".into())
}

pub async fn replace_route_table_association(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let raw = need(p, "AssociationId")?;
    let assoc = resolve(state, Kind::RouteTableAssociation, &raw, "InvalidAssociationID.NotFound").await.map_err(|_| {
        // the id of a main-table association is derived, not stored
        bad("InvalidAssociationID.NotFound", format!("The association '{raw}' does not exist (changing the main route table is not supported)"))
    })?;
    let new_table = resolve(state, Kind::RouteTable, &need(p, "RouteTableId")?, "InvalidRouteTableID.NotFound").await?;
    let (old_table, subnet): (Uuid, Uuid) = crate::db::query_as("SELECT route_table_id, subnet_id FROM ec2_route_table_assocs WHERE id = ?").bind(assoc).fetch_one(&state.pool).await?;
    let (vpc, _) = table_vpc(state, new_table).await?;
    authorize_vpc(state, actor, vpc, true).await?;
    if table_vpc(state, old_table).await?.0 != vpc {
        return Err(bad("InvalidParameterValue", "the route table is in another VPC"));
    }
    let id = Uuid::new_v4();
    crate::db::query("DELETE FROM ec2_route_table_assocs WHERE id = ?").bind(assoc).execute(&state.pool).await?;
    crate::db::query("INSERT INTO ec2_route_table_assocs (id, route_table_id, subnet_id) VALUES (?, ?, ?)").bind(id).bind(new_table).bind(subnet).execute(&state.pool).await?;
    subnet_table_changed(state, actor, subnet, old_table, new_table).await;
    Ok(format!("<newAssociationId>{}</newAssociationId><associationState><state>associated</state></associationState>", ec2_id(Kind::RouteTableAssociation, id)))
}

// ---- routes --------------------------------------------------------------------------------------------------------

async fn route_destination(state: &AppState, vpc: Uuid, p: &Params) -> Result<String, Ec2Error> {
    let dest = need(p, "DestinationCidrBlock")?;
    if !valid_cidr(&dest) {
        return Err(bad("InvalidParameterValue", "DestinationCidrBlock must be an IPv4 network address with a prefix, such as 10.1.0.0/16"));
    }
    let vpc_cidr: String = crate::db::query_scalar("SELECT cidr FROM cloud_vpcs WHERE id = ?").bind(vpc).fetch_one(&state.pool).await?;
    if dest == vpc_cidr {
        return Err(bad("RouteAlreadyExists", "the local route of the VPC already covers that destination"));
    }
    Ok(dest)
}

pub async fn create_route(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    if !p.contains_key("RouteTableId") {
        if p.contains_key("VpcId") {
            return super::vpc::create_route(state, actor, p).await;
        }
        return Err(bad("MissingParameter", "The request must contain the parameter RouteTableId"));
    }
    require_operator(actor)?;
    ensure_defaults(state).await?;
    let table = resolve(state, Kind::RouteTable, &need(p, "RouteTableId")?, "InvalidRouteTableID.NotFound").await?;
    let (vpc, _) = table_vpc(state, table).await?;
    authorize_vpc(state, actor, vpc, true).await?;
    let dest = route_destination(state, vpc, p).await?;
    let (kind, target) = parse_target(state, vpc, p).await?;
    let exists: Option<i64> = crate::db::query_scalar("SELECT 1 FROM ec2_routes WHERE route_table_id = ? AND destination = ?").bind(table).bind(&dest).fetch_optional(&state.pool).await?;
    if exists.is_some() {
        return Err(bad("RouteAlreadyExists", format!("A route for {dest} already exists in this route table")));
    }
    crate::db::query("INSERT INTO ec2_routes (id, route_table_id, destination, target_kind, target_id) VALUES (?, ?, ?, ?, ?)")
        .bind(Uuid::new_v4())
        .bind(table)
        .bind(&dest)
        .bind(kind)
        .bind(target)
        .execute(&state.pool)
        .await?;
    if dest == DEFAULT_ROUTE {
        reconcile_table(state, actor, table).await;
    }
    Ok("<return>true</return><forwardingActive>false</forwardingActive>".into())
}

pub async fn replace_route(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let table = resolve(state, Kind::RouteTable, &need(p, "RouteTableId")?, "InvalidRouteTableID.NotFound").await?;
    let (vpc, _) = table_vpc(state, table).await?;
    authorize_vpc(state, actor, vpc, true).await?;
    let dest = route_destination(state, vpc, p).await?;
    let (kind, target) = parse_target(state, vpc, p).await?;
    let done = crate::db::query("UPDATE ec2_routes SET target_kind = ?, target_id = ? WHERE route_table_id = ? AND destination = ?")
        .bind(kind)
        .bind(target)
        .bind(table)
        .bind(&dest)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if done == 0 {
        return Err(bad("InvalidRoute.NotFound", format!("There is no route for {dest} in this route table")));
    }
    if dest == DEFAULT_ROUTE {
        reconcile_table(state, actor, table).await;
    }
    Ok("<return>true</return>".into())
}

pub async fn delete_route(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    if !p.contains_key("RouteTableId") {
        if p.contains_key("VpcId") {
            return super::vpc::delete_route(state, actor, p).await;
        }
        return Err(bad("MissingParameter", "The request must contain the parameter RouteTableId"));
    }
    require_operator(actor)?;
    let table = resolve(state, Kind::RouteTable, &need(p, "RouteTableId")?, "InvalidRouteTableID.NotFound").await?;
    let (vpc, _) = table_vpc(state, table).await?;
    authorize_vpc(state, actor, vpc, true).await?;
    let dest = need(p, "DestinationCidrBlock")?;
    let vpc_cidr: String = crate::db::query_scalar("SELECT cidr FROM cloud_vpcs WHERE id = ?").bind(vpc).fetch_one(&state.pool).await?;
    if dest == vpc_cidr {
        return Err(bad("InvalidParameterValue", "the local route cannot be deleted"));
    }
    let done = crate::db::query("DELETE FROM ec2_routes WHERE route_table_id = ? AND destination = ?").bind(table).bind(&dest).execute(&state.pool).await?.rows_affected();
    if done == 0 {
        return Err(bad("InvalidRoute.NotFound", format!("There is no route for {dest} in this route table")));
    }
    if dest == DEFAULT_ROUTE {
        reconcile_table(state, actor, table).await;
    }
    Ok("<return>true</return>".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_xml_marks_dead_targets_and_never_claims_forwarding() {
        let t = Uuid::parse_str("0123456789abcdef0123456789abcdef").unwrap();
        let live = route_xml("0.0.0.0/0", "igw", Some(t), true, "CreateRoute");
        assert!(live.contains("<gatewayId>igw-0123456789abcdef0</gatewayId>"));
        assert!(live.contains("<state>active</state>"));
        assert!(live.contains("<forwardingActive>false</forwardingActive>"));
        let dead = route_xml("0.0.0.0/0", "nat", Some(t), false, "CreateRoute");
        assert!(dead.contains("<natGatewayId>nat-0123456789abcdef0</natGatewayId>"));
        assert!(dead.contains("<state>blackhole</state>"));
        let local = route_xml("10.0.0.0/16", "local", None, true, "CreateRouteTable");
        assert!(local.contains("<gatewayId>local</gatewayId>"));
        assert!(route_xml("10.9.0.0/16", "blackhole", None, true, "CreateRoute").contains("<state>blackhole</state>"));
    }

    #[test]
    fn target_element_names_follow_the_kind() {
        let t = Uuid::new_v4();
        assert_eq!(target_element("nat", Some(t)).unwrap().0, "natGatewayId");
        assert_eq!(target_element("peering", Some(t)).unwrap().0, "vpcPeeringConnectionId");
        assert_eq!(target_element("instance", Some(t)).unwrap().0, "instanceId");
        assert!(target_element("blackhole", Some(t)).is_none());
        assert!(target_element("igw", None).is_none());
    }
}
