// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Internet gateways, NAT gateways, DHCP options and VPC attributes on `POST /ec2`, plus the read-only describes Terraform
//! issues while refreshing a VPC (prefix lists, endpoints, egress-only gateways).
//!
//! What is real: a NAT gateway turns on the host masquerade of the subnets whose route table sends `0.0.0.0/0` to it (the
//! `PUT /api/v1/cloud/subnets/{id}/nat` mechanism, see `route_tables::reconcile_table`). Internet gateways and DHCP options
//! are stored plans: nothing forwards because of them, and the XML says so where AWS has no field for it.

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::{ec2_id, Kind};
use crate::state::AppState;

use super::more::resolve;
use super::netcommon::{all_tags, apply_tags, authorize_vpc, bad, need, passes, spec_tags, tags_of_item, tags_xml, vpc_of_subnet};
use super::{indexed, xml_escape, Ec2Error, OWNER};

type Params = BTreeMap<String, String>;

/// `2026-10-07 14:03:11` → `2026-10-07T14:03:11.000Z`.
pub fn iso(created: &str) -> String {
    let t = created.replace(' ', "T");
    if t.ends_with('Z') {
        t
    } else {
        format!("{t}.000Z")
    }
}

fn project_param(p: &Params) -> Result<Option<Uuid>, Ec2Error> {
    p.get("ProjectId").map(|s| s.parse().map_err(|_| bad("InvalidParameterValue", "ProjectId must be a project UUID"))).transpose()
}

// ---- internet gateways -------------------------------------------------------------------------------------------

pub async fn create_internet_gateway(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let project = project_param(p)?;
    if let Some(project) = project {
        let mut conn = state.pool.acquire().await?;
        crate::api::cloud::access(&mut conn, actor, project, true).await.map_err(super::more::api_err)?;
    }
    let id = Uuid::new_v4();
    crate::db::query("INSERT INTO ec2_internet_gateways (id, project_id) VALUES (?, ?)").bind(id).bind(project).execute(&state.pool).await?;
    apply_tags(state, Kind::InternetGateway, id, &spec_tags(p, "internet-gateway")).await?;
    let tags = all_tags(state).await?;
    Ok(format!(
        "<internetGateway><internetGatewayId>{}</internetGatewayId><ownerId>{OWNER}</ownerId><attachmentSet/>{}</internetGateway>",
        ec2_id(Kind::InternetGateway, id),
        tags_xml(&tags, Kind::InternetGateway, id)
    ))
}

pub async fn describe_internet_gateways(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let wanted = indexed(p, "InternetGatewayId");
    let tags = all_tags(state).await?;
    let rows: Vec<(Uuid, Option<Uuid>)> =
        crate::db::query_as("SELECT id, vpc_id FROM ec2_internet_gateways ORDER BY created_at, id").fetch_all(&state.pool).await?;
    for w in &wanted {
        if !rows.iter().any(|r| &ec2_id(Kind::InternetGateway, r.0) == w) {
            return Err(bad("InvalidInternetGatewayID.NotFound", format!("The internet gateway '{w}' does not exist")));
        }
    }
    let mut items = String::new();
    for (id, vpc) in rows {
        let eid = ec2_id(Kind::InternetGateway, id);
        if !wanted.is_empty() && !wanted.contains(&eid) {
            continue;
        }
        let vpc_eid = vpc.map(|v| ec2_id(Kind::Vpc, v));
        if !passes(p, &tags_of_item(&tags, Kind::InternetGateway, id), |n| match n {
            "internet-gateway-id" => Some(vec![eid.clone()]),
            "attachment.vpc-id" => Some(vpc_eid.iter().cloned().collect()),
            "attachment.state" => Some(vpc_eid.iter().map(|_| "available".to_string()).collect()),
            "owner-id" => Some(vec![OWNER.to_string()]),
            _ => None,
        }) {
            continue;
        }
        let attachment = vpc_eid.map(|v| format!("<item><vpcId>{v}</vpcId><state>available</state></item>")).unwrap_or_default();
        items.push_str(&format!(
            "<item><internetGatewayId>{eid}</internetGatewayId><ownerId>{OWNER}</ownerId><attachmentSet>{attachment}</attachmentSet>{}</item>",
            tags_xml(&tags, Kind::InternetGateway, id)
        ));
    }
    Ok(format!("<internetGatewaySet>{items}</internetGatewaySet>"))
}

pub async fn attach_internet_gateway(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let igw = resolve(state, Kind::InternetGateway, &need(p, "InternetGatewayId")?, "InvalidInternetGatewayID.NotFound").await?;
    let vpc = resolve(state, Kind::Vpc, &need(p, "VpcId")?, "InvalidVpcID.NotFound").await?;
    authorize_vpc(state, actor, vpc, true).await?;
    let current: Option<Uuid> = crate::db::query_scalar("SELECT vpc_id FROM ec2_internet_gateways WHERE id = ?").bind(igw).fetch_one(&state.pool).await?;
    if current.is_some() {
        return Err(bad("Resource.AlreadyAssociated", "the internet gateway is already attached to a VPC"));
    }
    let taken: Option<i64> = crate::db::query_scalar("SELECT 1 FROM ec2_internet_gateways WHERE vpc_id = ?").bind(vpc).fetch_optional(&state.pool).await?;
    if taken.is_some() {
        return Err(bad("InvalidParameterValue", "the VPC already has an internet gateway attached"));
    }
    crate::db::query("UPDATE ec2_internet_gateways SET vpc_id = ? WHERE id = ?").bind(vpc).bind(igw).execute(&state.pool).await?;
    Ok("<return>true</return>".into())
}

pub async fn detach_internet_gateway(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let igw = resolve(state, Kind::InternetGateway, &need(p, "InternetGatewayId")?, "InvalidInternetGatewayID.NotFound").await?;
    let vpc = resolve(state, Kind::Vpc, &need(p, "VpcId")?, "InvalidVpcID.NotFound").await?;
    authorize_vpc(state, actor, vpc, true).await?;
    let done = crate::db::query("UPDATE ec2_internet_gateways SET vpc_id = NULL WHERE id = ? AND vpc_id = ?")
        .bind(igw)
        .bind(vpc)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if done == 0 {
        return Err(bad("Gateway.NotAttached", "the internet gateway is not attached to that VPC"));
    }
    Ok("<return>true</return>".into())
}

pub async fn delete_internet_gateway(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let igw = resolve(state, Kind::InternetGateway, &need(p, "InternetGatewayId")?, "InvalidInternetGatewayID.NotFound").await?;
    let (vpc, project): (Option<Uuid>, Option<Uuid>) =
        crate::db::query_as("SELECT vpc_id, project_id FROM ec2_internet_gateways WHERE id = ?").bind(igw).fetch_one(&state.pool).await?;
    if vpc.is_some() {
        return Err(bad("DependencyViolation", "the internet gateway is attached to a VPC; detach it first"));
    }
    if let Some(project) = project {
        let mut conn = state.pool.acquire().await?;
        crate::api::cloud::access(&mut conn, actor, project, true).await.map_err(super::more::api_err)?;
    }
    crate::db::query("DELETE FROM ec2_internet_gateways WHERE id = ?").bind(igw).execute(&state.pool).await?;
    crate::db::query("UPDATE ec2_routes SET target_kind = 'blackhole', target_id = NULL WHERE target_kind = 'igw' AND target_id = ?")
        .bind(igw)
        .execute(&state.pool)
        .await?;
    Ok("<return>true</return>".into())
}

// ---- NAT gateways ------------------------------------------------------------------------------------------------

async fn allocation_exists(state: &AppState, allocation: &str) -> Result<bool, Ec2Error> {
    let ids: Vec<Uuid> = crate::db::query_scalar("SELECT id FROM elastic_ips").fetch_all(&state.pool).await?;
    Ok(ids.into_iter().any(|id| crate::api::elastic_ips::allocation_id(id) == allocation))
}

fn nat_xml(id: Uuid, vpc: Uuid, subnet: Uuid, state_name: &str, allocation: &str, created: &str, tags: &str) -> String {
    let address = if allocation.is_empty() { String::new() } else { format!("<item><allocationId>{}</allocationId></item>", xml_escape(allocation)) };
    format!(
        "<natGatewayId>{}</natGatewayId><vpcId>{}</vpcId><subnetId>{}</subnetId><state>{state_name}</state><createTime>{}</createTime><natGatewayAddressSet>{address}</natGatewayAddressSet><connectivityType>public</connectivityType>{tags}",
        ec2_id(Kind::NatGateway, id),
        ec2_id(Kind::Vpc, vpc),
        ec2_id(Kind::Subnet, subnet),
        xml_escape(&iso(created)),
    )
}

pub async fn create_nat_gateway(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let subnet = resolve(state, Kind::Subnet, &need(p, "SubnetId")?, "InvalidSubnetID.NotFound").await?;
    let vpc = vpc_of_subnet(state, subnet).await?;
    authorize_vpc(state, actor, vpc, true).await?;
    if let Some(kind) = p.get("ConnectivityType") {
        if kind != "public" {
            return Err(bad("UnsupportedOperation", "only public NAT gateways are supported"));
        }
    }
    let allocation = need(p, "AllocationId")?;
    if !allocation_exists(state, &allocation).await? {
        return Err(bad("InvalidAllocationID.NotFound", format!("The allocation '{allocation}' does not exist")));
    }
    let id = Uuid::new_v4();
    crate::db::query("INSERT INTO ec2_nat_gateways (id, vpc_id, subnet_id, allocation_id) VALUES (?, ?, ?, ?)")
        .bind(id)
        .bind(vpc)
        .bind(subnet)
        .bind(&allocation)
        .execute(&state.pool)
        .await?;
    apply_tags(state, Kind::NatGateway, id, &spec_tags(p, "natgateway")).await?;
    let created: String = crate::db::query_scalar("SELECT created_at FROM ec2_nat_gateways WHERE id = ?").bind(id).fetch_one(&state.pool).await?;
    let tags = all_tags(state).await?;
    let body = nat_xml(id, vpc, subnet, "available", &allocation, &created, &tags_xml(&tags, Kind::NatGateway, id));
    let token = p.get("ClientToken").map(|t| format!("<clientToken>{}</clientToken>", xml_escape(t))).unwrap_or_default();
    Ok(format!("{token}<natGateway>{body}</natGateway>"))
}

pub async fn describe_nat_gateways(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let wanted = indexed(p, "NatGatewayId");
    let tags = all_tags(state).await?;
    type Row = (Uuid, Uuid, Uuid, String, String, String);
    let rows: Vec<Row> = crate::db::query_as("SELECT id, vpc_id, subnet_id, allocation_id, state, created_at FROM ec2_nat_gateways ORDER BY created_at, id")
        .fetch_all(&state.pool)
        .await?;
    for w in &wanted {
        if !rows.iter().any(|r| &ec2_id(Kind::NatGateway, r.0) == w) {
            return Err(bad("NatGatewayNotFound", format!("The NAT gateway '{w}' does not exist")));
        }
    }
    let mut items = String::new();
    for (id, vpc, subnet, allocation, st, created) in rows {
        let eid = ec2_id(Kind::NatGateway, id);
        if !wanted.is_empty() && !wanted.contains(&eid) {
            continue;
        }
        if !passes(p, &tags_of_item(&tags, Kind::NatGateway, id), |n| match n {
            "nat-gateway-id" => Some(vec![eid.clone()]),
            "vpc-id" => Some(vec![ec2_id(Kind::Vpc, vpc)]),
            "subnet-id" => Some(vec![ec2_id(Kind::Subnet, subnet)]),
            "state" => Some(vec![st.clone()]),
            _ => None,
        }) {
            continue;
        }
        items.push_str(&format!("<item>{}</item>", nat_xml(id, vpc, subnet, &st, &allocation, &created, &tags_xml(&tags, Kind::NatGateway, id))));
    }
    Ok(format!("<natGatewaySet>{items}</natGatewaySet>"))
}

pub async fn delete_nat_gateway(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let nat = resolve(state, Kind::NatGateway, &need(p, "NatGatewayId")?, "NatGatewayNotFound").await?;
    let vpc: Uuid = crate::db::query_scalar("SELECT vpc_id FROM ec2_nat_gateways WHERE id = ?").bind(nat).fetch_one(&state.pool).await?;
    authorize_vpc(state, actor, vpc, true).await?;
    let tables: Vec<Uuid> = crate::db::query_scalar("SELECT DISTINCT route_table_id FROM ec2_routes WHERE target_kind = 'nat' AND target_id = ?")
        .bind(nat)
        .fetch_all(&state.pool)
        .await?;
    crate::db::query("UPDATE ec2_routes SET target_kind = 'blackhole', target_id = NULL WHERE target_kind = 'nat' AND target_id = ?")
        .bind(nat)
        .execute(&state.pool)
        .await?;
    crate::db::query("DELETE FROM ec2_nat_gateways WHERE id = ?").bind(nat).execute(&state.pool).await?;
    for t in tables {
        super::route_tables::reconcile_table(state, actor, t).await;
    }
    Ok(format!("<natGatewayId>{}</natGatewayId>", ec2_id(Kind::NatGateway, nat)))
}

// ---- DHCP options ------------------------------------------------------------------------------------------------

fn dhcp_config(p: &Params) -> Result<BTreeMap<String, Vec<String>>, Ec2Error> {
    let mut out = BTreeMap::new();
    for n in 1..=20 {
        let Some(key) = p.get(&format!("DhcpConfiguration.{n}.Key")) else { break };
        let values: Vec<String> = (1..=20).map_while(|m| p.get(&format!("DhcpConfiguration.{n}.Value.{m}")).cloned()).collect();
        if values.is_empty() {
            return Err(bad("InvalidParameterValue", format!("DhcpConfiguration {key} needs at least one value")));
        }
        out.insert(key.clone(), values);
    }
    if out.is_empty() {
        return Err(bad("MissingParameter", "The request must contain the parameter DhcpConfiguration"));
    }
    Ok(out)
}

fn dhcp_xml(id: Uuid, config: &BTreeMap<String, Vec<String>>, tags: &str) -> String {
    let set: String = config
        .iter()
        .map(|(k, vs)| {
            let values: String = vs.iter().map(|v| format!("<item><value>{}</value></item>", xml_escape(v))).collect();
            format!("<item><key>{}</key><valueSet>{values}</valueSet></item>", xml_escape(k))
        })
        .collect();
    format!(
        "<dhcpOptionsId>{}</dhcpOptionsId><ownerId>{OWNER}</ownerId><dhcpConfigurationSet>{set}</dhcpConfigurationSet>{tags}",
        ec2_id(Kind::DhcpOptions, id)
    )
}

pub async fn create_dhcp_options(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let config = dhcp_config(p)?;
    let id = Uuid::new_v4();
    crate::db::query("INSERT INTO ec2_dhcp_options (id, project_id, config) VALUES (?, ?, ?)")
        .bind(id)
        .bind(project_param(p)?)
        .bind(serde_json::to_string(&config).unwrap_or_else(|_| "{}".into()))
        .execute(&state.pool)
        .await?;
    apply_tags(state, Kind::DhcpOptions, id, &spec_tags(p, "dhcp-options")).await?;
    let tags = all_tags(state).await?;
    Ok(format!("<dhcpOptions>{}</dhcpOptions>", dhcp_xml(id, &config, &tags_xml(&tags, Kind::DhcpOptions, id))))
}

pub async fn describe_dhcp_options(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let wanted = indexed(p, "DhcpOptionsId");
    let tags = all_tags(state).await?;
    let rows: Vec<(Uuid, String)> = crate::db::query_as("SELECT id, config FROM ec2_dhcp_options ORDER BY created_at, id").fetch_all(&state.pool).await?;
    for w in &wanted {
        if !rows.iter().any(|r| &ec2_id(Kind::DhcpOptions, r.0) == w) {
            return Err(bad("InvalidDhcpOptionID.NotFound", format!("The DHCP options set '{w}' does not exist")));
        }
    }
    let mut items = String::new();
    for (id, raw) in rows {
        let eid = ec2_id(Kind::DhcpOptions, id);
        if !wanted.is_empty() && !wanted.contains(&eid) {
            continue;
        }
        let config: BTreeMap<String, Vec<String>> = serde_json::from_str(&raw).unwrap_or_default();
        if !passes(p, &tags_of_item(&tags, Kind::DhcpOptions, id), |n| match n {
            "dhcp-options-id" => Some(vec![eid.clone()]),
            "key" => Some(config.keys().cloned().collect()),
            "value" => Some(config.values().flatten().cloned().collect()),
            "owner-id" => Some(vec![OWNER.to_string()]),
            _ => None,
        }) {
            continue;
        }
        items.push_str(&format!("<item>{}</item>", dhcp_xml(id, &config, &tags_xml(&tags, Kind::DhcpOptions, id))));
    }
    Ok(format!("<dhcpOptionsSet>{items}</dhcpOptionsSet>"))
}

pub async fn associate_dhcp_options(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let vpc = resolve(state, Kind::Vpc, &need(p, "VpcId")?, "InvalidVpcID.NotFound").await?;
    authorize_vpc(state, actor, vpc, true).await?;
    let wanted = need(p, "DhcpOptionsId")?;
    if wanted == "default" {
        crate::db::query("DELETE FROM ec2_dhcp_assocs WHERE vpc_id = ?").bind(vpc).execute(&state.pool).await?;
    } else {
        let dopt = resolve(state, Kind::DhcpOptions, &wanted, "InvalidDhcpOptionID.NotFound").await?;
        crate::db::query(
            "INSERT INTO ec2_dhcp_assocs (vpc_id, dhcp_options_id) VALUES (?, ?) ON CONFLICT (vpc_id) DO UPDATE SET dhcp_options_id = excluded.dhcp_options_id",
        )
        .bind(vpc)
        .bind(dopt)
        .execute(&state.pool)
        .await?;
    }
    Ok("<return>true</return>".into())
}

pub async fn delete_dhcp_options(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let dopt = resolve(state, Kind::DhcpOptions, &need(p, "DhcpOptionsId")?, "InvalidDhcpOptionID.NotFound").await?;
    let project: Option<Uuid> = crate::db::query_scalar("SELECT project_id FROM ec2_dhcp_options WHERE id = ?").bind(dopt).fetch_one(&state.pool).await?;
    if let Some(project) = project {
        let mut conn = state.pool.acquire().await?;
        crate::api::cloud::access(&mut conn, actor, project, true).await.map_err(super::more::api_err)?;
    }
    let used: Option<i64> = crate::db::query_scalar("SELECT 1 FROM ec2_dhcp_assocs WHERE dhcp_options_id = ?").bind(dopt).fetch_optional(&state.pool).await?;
    if used.is_some() {
        return Err(bad("DependencyViolation", "the DHCP options set is associated with a VPC"));
    }
    crate::db::query("DELETE FROM ec2_dhcp_options WHERE id = ?").bind(dopt).execute(&state.pool).await?;
    Ok("<return>true</return>".into())
}

/// The DHCP options set a VPC is associated with (`default` when none).
pub async fn dhcp_id_of_vpc(state: &AppState) -> Result<BTreeMap<Uuid, String>, Ec2Error> {
    let rows: Vec<(Uuid, Uuid)> = crate::db::query_as("SELECT vpc_id, dhcp_options_id FROM ec2_dhcp_assocs").fetch_all(&state.pool).await?;
    Ok(rows.into_iter().map(|(v, d)| (v, ec2_id(Kind::DhcpOptions, d))).collect())
}

// ---- VPC attributes ----------------------------------------------------------------------------------------------

async fn vpc_attrs(state: &AppState, vpc: Uuid) -> Result<(bool, bool), Ec2Error> {
    let row: Option<(bool, bool)> = crate::db::query_as("SELECT dns_support, dns_hostnames FROM ec2_vpc_attrs WHERE vpc_id = ?")
        .bind(vpc)
        .fetch_optional(&state.pool)
        .await?;
    Ok(row.unwrap_or((true, false)))
}

pub async fn describe_vpc_attribute(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let vpc = resolve(state, Kind::Vpc, &need(p, "VpcId")?, "InvalidVpcID.NotFound").await?;
    let (support, hostnames) = vpc_attrs(state, vpc).await?;
    let vpc_eid = ec2_id(Kind::Vpc, vpc);
    let body = match need(p, "Attribute")?.as_str() {
        "enableDnsSupport" => format!("<enableDnsSupport><value>{support}</value></enableDnsSupport>"),
        "enableDnsHostnames" => format!("<enableDnsHostnames><value>{hostnames}</value></enableDnsHostnames>"),
        "enableNetworkAddressUsageMetrics" => "<enableNetworkAddressUsageMetrics><value>false</value></enableNetworkAddressUsageMetrics>".to_string(),
        other => return Err(bad("InvalidParameterValue", format!("The attribute '{other}' is not valid for a VPC"))),
    };
    Ok(format!("<vpcId>{vpc_eid}</vpcId>{body}"))
}

pub async fn modify_vpc_attribute(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let vpc = resolve(state, Kind::Vpc, &need(p, "VpcId")?, "InvalidVpcID.NotFound").await?;
    authorize_vpc(state, actor, vpc, true).await?;
    if p.keys().any(|k| k.starts_with("EnableNetworkAddressUsageMetrics")) {
        return Err(bad("UnsupportedOperation", "network address usage metrics are not supported"));
    }
    let support = super::netcommon::bool_param(p, "EnableDnsSupport.Value")?;
    let hostnames = super::netcommon::bool_param(p, "EnableDnsHostnames.Value")?;
    if support.is_none() && hostnames.is_none() {
        return Err(bad("MissingParameter", "The request must contain EnableDnsSupport.Value or EnableDnsHostnames.Value"));
    }
    if support == Some(false) {
        // the subnets' libvirt networks always run dnsmasq, so DNS cannot be switched off
        return Err(bad("UnsupportedOperation", "DNS resolution cannot be disabled: the VPC's networks always provide it"));
    }
    let (_, cur_hostnames) = vpc_attrs(state, vpc).await?;
    crate::db::query(
        "INSERT INTO ec2_vpc_attrs (vpc_id, dns_support, dns_hostnames) VALUES (?, TRUE, ?) \
         ON CONFLICT (vpc_id) DO UPDATE SET dns_hostnames = excluded.dns_hostnames",
    )
    .bind(vpc)
    .bind(hostnames.unwrap_or(cur_hostnames))
    .execute(&state.pool)
    .await?;
    Ok("<return>true</return>".into())
}

// ---- read-only describes and refusals ------------------------------------------------------------------------------

pub async fn describe_egress_only_internet_gateways(_state: &AppState, _p: &Params) -> Result<String, Ec2Error> {
    Ok("<egressOnlyInternetGatewaySet/>".into())
}

pub async fn describe_prefix_lists(_state: &AppState, _p: &Params) -> Result<String, Ec2Error> {
    Ok("<prefixListSet/>".into())
}

pub async fn describe_vpc_endpoints(_state: &AppState, _p: &Params) -> Result<String, Ec2Error> {
    Ok("<vpcEndpointSet/>".into())
}

pub async fn create_vpc_endpoint(_state: &AppState, actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    Err(bad("UnsupportedOperation", "VPC endpoints are not supported by this backend"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_time_has_a_zone() {
        assert_eq!(iso("2026-10-07 14:03:11"), "2026-10-07T14:03:11.000Z");
        assert_eq!(iso("2026-10-07T14:03:11Z"), "2026-10-07T14:03:11Z");
    }

    #[test]
    fn dhcp_configuration_needs_values_and_a_key() {
        let mut p = Params::new();
        assert!(dhcp_config(&p).is_err());
        p.insert("DhcpConfiguration.1.Key".into(), "domain-name-servers".into());
        assert!(dhcp_config(&p).is_err());
        p.insert("DhcpConfiguration.1.Value.1".into(), "10.0.0.2".into());
        p.insert("DhcpConfiguration.1.Value.2".into(), "10.0.0.3".into());
        let c = dhcp_config(&p).unwrap();
        assert_eq!(c["domain-name-servers"], vec!["10.0.0.2", "10.0.0.3"]);
    }
}
