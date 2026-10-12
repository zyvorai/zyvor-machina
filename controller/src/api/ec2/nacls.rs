// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Network ACLs on `POST /ec2`. They are **stored plans**: the entries are kept, listed and validated exactly as AWS does,
//! but no host filters traffic by them (the XML carries `forwardingActive=false`). Every VPC has a default ACL (rule 100
//! allow, rule 32767 deny, both directions) that each subnet starts out associated with.

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::{ec2_id, Kind};
use crate::state::AppState;

use super::more::resolve;
use super::netcommon::{all_tags, apply_tags, authorize_vpc, bad, ensure_defaults, need, passes, spec_tags, tags_of_item, tags_xml, valid_cidr};
use super::{indexed, xml_escape, Ec2Error, OWNER};

type Params = BTreeMap<String, String>;

/// The IP protocol as stored: `-1` (all) or a protocol number.
pub fn normalize_protocol(raw: &str) -> Result<String, Ec2Error> {
    match raw.to_ascii_lowercase().as_str() {
        "-1" | "all" => Ok("-1".into()),
        "tcp" => Ok("6".into()),
        "udp" => Ok("17".into()),
        "icmp" => Ok("1".into()),
        "icmpv6" | "58" => Err(bad("UnsupportedOperation", "IPv6 is not supported")),
        n => match n.parse::<u8>() {
            Ok(v) => Ok(v.to_string()),
            Err(_) => Err(bad("InvalidParameterValue", format!("Protocol '{raw}' is not valid"))),
        },
    }
}

struct Entry {
    number: i64,
    egress: bool,
    protocol: String,
    action: String,
    cidr: String,
    from: Option<i64>,
    to: Option<i64>,
}

fn parse_entry(p: &Params) -> Result<Entry, Ec2Error> {
    for k in ["Ipv6CidrBlock", "Icmp.Type", "Icmp.Code"] {
        if p.contains_key(k) {
            return Err(bad("UnsupportedOperation", format!("{k} is not supported (IPv4 TCP, UDP and protocol-number entries only)")));
        }
    }
    let number: i64 = need(p, "RuleNumber")?.parse().map_err(|_| bad("InvalidParameterValue", "RuleNumber must be a number"))?;
    if !(1..=32766).contains(&number) {
        return Err(bad("InvalidParameterValue", "RuleNumber must be between 1 and 32766"));
    }
    let egress = match need(p, "Egress")?.as_str() {
        "true" => true,
        "false" => false,
        _ => return Err(bad("InvalidParameterValue", "Egress must be true or false")),
    };
    let protocol = normalize_protocol(&need(p, "Protocol")?)?;
    let action = need(p, "RuleAction")?;
    if action != "allow" && action != "deny" {
        return Err(bad("InvalidParameterValue", "RuleAction must be allow or deny"));
    }
    let cidr = need(p, "CidrBlock")?;
    if !valid_cidr(&cidr) {
        return Err(bad("InvalidParameterValue", "CidrBlock must be an IPv4 network address with a prefix"));
    }
    let port = |k: &str| -> Result<Option<i64>, Ec2Error> {
        p.get(k).map(|v| v.parse::<i64>().map_err(|_| bad("InvalidParameterValue", format!("{k} must be a number")))).transpose()
    };
    let (from, to) = (port("PortRange.From")?, port("PortRange.To")?);
    if protocol == "6" || protocol == "17" {
        let (Some(f), Some(t)) = (from, to) else {
            return Err(bad("MissingParameter", "PortRange is required for TCP and UDP entries"));
        };
        if !(0..=65535).contains(&f) || !(0..=65535).contains(&t) || f > t {
            return Err(bad("InvalidParameterValue", "PortRange must be 0-65535 with From <= To"));
        }
    } else if from.is_some() || to.is_some() {
        return Err(bad("InvalidParameterValue", "PortRange applies only to TCP and UDP entries"));
    }
    Ok(Entry { number, egress, protocol, action, cidr, from, to })
}

fn entry_xml(number: i64, egress: bool, protocol: &str, action: &str, cidr: &str, from: Option<i64>, to: Option<i64>) -> String {
    let ports = match (from, to) {
        (Some(f), Some(t)) => format!("<portRange><from>{f}</from><to>{t}</to></portRange>"),
        _ => String::new(),
    };
    format!(
        "<item><ruleNumber>{number}</ruleNumber><protocol>{}</protocol><ruleAction>{}</ruleAction><egress>{egress}</egress><cidrBlock>{}</cidrBlock>{ports}</item>",
        xml_escape(protocol),
        xml_escape(action),
        xml_escape(cidr)
    )
}

async fn acl_vpc(state: &AppState, acl: Uuid) -> Result<(Uuid, bool), Ec2Error> {
    Ok(crate::db::query_as("SELECT vpc_id, is_default FROM ec2_network_acls WHERE id = ?").bind(acl).fetch_one(&state.pool).await?)
}

pub async fn create_network_acl(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let vpc = resolve(state, Kind::Vpc, &need(p, "VpcId")?, "InvalidVpcID.NotFound").await?;
    authorize_vpc(state, actor, vpc, true).await?;
    ensure_defaults(state).await?;
    let id = Uuid::new_v4();
    crate::db::query("INSERT INTO ec2_network_acls (id, vpc_id, is_default) VALUES (?, ?, FALSE)").bind(id).bind(vpc).execute(&state.pool).await?;
    // a new ACL starts with only the final deny rule, as in AWS
    for egress in [false, true] {
        crate::db::query("INSERT INTO ec2_network_acl_entries (acl_id, rule_number, egress, protocol, rule_action, cidr) VALUES (?, 32767, ?, '-1', 'deny', '0.0.0.0/0')")
            .bind(id)
            .bind(egress)
            .execute(&state.pool)
            .await?;
    }
    apply_tags(state, Kind::NetworkAcl, id, &spec_tags(p, "network-acl")).await?;
    let tags = all_tags(state).await?;
    let entries: String = [false, true].iter().map(|e| entry_xml(32767, *e, "-1", "deny", "0.0.0.0/0", None, None)).collect();
    Ok(format!(
        "<networkAcl><networkAclId>{}</networkAclId><vpcId>{}</vpcId><default>false</default><entrySet>{entries}</entrySet><associationSet/>{}<ownerId>{OWNER}</ownerId></networkAcl>",
        ec2_id(Kind::NetworkAcl, id),
        ec2_id(Kind::Vpc, vpc),
        tags_xml(&tags, Kind::NetworkAcl, id)
    ))
}

pub async fn describe_network_acls(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    ensure_defaults(state).await?;
    let wanted = indexed(p, "NetworkAclId");
    let tags = all_tags(state).await?;
    let rows: Vec<(Uuid, Uuid, bool)> = crate::db::query_as("SELECT id, vpc_id, is_default FROM ec2_network_acls ORDER BY vpc_id, is_default DESC, created_at, id").fetch_all(&state.pool).await?;
    for w in &wanted {
        if !rows.iter().any(|r| &ec2_id(Kind::NetworkAcl, r.0) == w) {
            return Err(bad("InvalidNetworkAclID.NotFound", format!("The network ACL '{w}' does not exist")));
        }
    }
    let mut items = String::new();
    for (id, vpc, is_default) in rows {
        let eid = ec2_id(Kind::NetworkAcl, id);
        if !wanted.is_empty() && !wanted.contains(&eid) {
            continue;
        }
        type E = (i64, bool, String, String, String, Option<i64>, Option<i64>);
        let entries: Vec<E> = crate::db::query_as(
            "SELECT rule_number, egress, protocol, rule_action, cidr, port_from, port_to FROM ec2_network_acl_entries WHERE acl_id = ? ORDER BY egress, rule_number",
        )
        .bind(id)
        .fetch_all(&state.pool)
        .await?;
        let assocs: Vec<(Uuid, Uuid)> = crate::db::query_as("SELECT id, subnet_id FROM ec2_acl_assocs WHERE acl_id = ? ORDER BY id").bind(id).fetch_all(&state.pool).await?;
        let assoc_ids: Vec<String> = assocs.iter().map(|(a, _)| ec2_id(Kind::NetworkAclAssociation, *a)).collect();
        let assoc_subnets: Vec<String> = assocs.iter().map(|(_, s)| ec2_id(Kind::Subnet, *s)).collect();
        if !passes(p, &tags_of_item(&tags, Kind::NetworkAcl, id), |n| match n {
            "network-acl-id" => Some(vec![eid.clone()]),
            "vpc-id" => Some(vec![ec2_id(Kind::Vpc, vpc)]),
            "default" => Some(vec![is_default.to_string()]),
            "owner-id" => Some(vec![OWNER.to_string()]),
            "association.network-acl-id" => Some(if assocs.is_empty() { vec![] } else { vec![eid.clone()] }),
            "association.network-acl-association-id" => Some(assoc_ids.clone()),
            "association.subnet-id" => Some(assoc_subnets.clone()),
            "entry.cidr" => Some(entries.iter().map(|e| e.4.clone()).collect()),
            "entry.rule-action" => Some(entries.iter().map(|e| e.3.clone()).collect()),
            "entry.egress" => Some(entries.iter().map(|e| e.1.to_string()).collect()),
            "entry.protocol" => Some(entries.iter().map(|e| e.2.clone()).collect()),
            "entry.rule-number" => Some(entries.iter().map(|e| e.0.to_string()).collect()),
            _ => None,
        }) {
            continue;
        }
        let entry_set: String = entries.iter().map(|e| entry_xml(e.0, e.1, &e.2, &e.3, &e.4, e.5, e.6)).collect();
        let assoc_set: String = assocs
            .iter()
            .map(|(a, s)| {
                format!(
                    "<item><networkAclAssociationId>{}</networkAclAssociationId><networkAclId>{eid}</networkAclId><subnetId>{}</subnetId></item>",
                    ec2_id(Kind::NetworkAclAssociation, *a),
                    ec2_id(Kind::Subnet, *s)
                )
            })
            .collect();
        items.push_str(&format!(
            "<item><networkAclId>{eid}</networkAclId><vpcId>{}</vpcId><default>{is_default}</default><entrySet>{entry_set}</entrySet><associationSet>{assoc_set}</associationSet>{}<ownerId>{OWNER}</ownerId><forwardingActive>false</forwardingActive></item>",
            ec2_id(Kind::Vpc, vpc),
            tags_xml(&tags, Kind::NetworkAcl, id)
        ));
    }
    Ok(format!("<networkAclSet>{items}</networkAclSet>"))
}

pub async fn delete_network_acl(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let acl = resolve(state, Kind::NetworkAcl, &need(p, "NetworkAclId")?, "InvalidNetworkAclID.NotFound").await?;
    let (vpc, is_default) = acl_vpc(state, acl).await?;
    authorize_vpc(state, actor, vpc, true).await?;
    if is_default {
        return Err(bad("DependencyViolation", "the default network ACL of a VPC cannot be deleted"));
    }
    let used: Option<i64> = crate::db::query_scalar("SELECT 1 FROM ec2_acl_assocs WHERE acl_id = ?").bind(acl).fetch_optional(&state.pool).await?;
    if used.is_some() {
        return Err(bad("DependencyViolation", "the network ACL is associated with subnets"));
    }
    crate::db::query("DELETE FROM ec2_network_acl_entries WHERE acl_id = ?").bind(acl).execute(&state.pool).await?;
    crate::db::query("DELETE FROM ec2_network_acls WHERE id = ?").bind(acl).execute(&state.pool).await?;
    Ok("<return>true</return>".into())
}

pub async fn create_network_acl_entry(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let acl = resolve(state, Kind::NetworkAcl, &need(p, "NetworkAclId")?, "InvalidNetworkAclID.NotFound").await?;
    let (vpc, _) = acl_vpc(state, acl).await?;
    authorize_vpc(state, actor, vpc, true).await?;
    let e = parse_entry(p)?;
    let exists: Option<i64> = crate::db::query_scalar("SELECT 1 FROM ec2_network_acl_entries WHERE acl_id = ? AND rule_number = ? AND egress = ?")
        .bind(acl)
        .bind(e.number)
        .bind(e.egress)
        .fetch_optional(&state.pool)
        .await?;
    if exists.is_some() {
        return Err(bad("NetworkAclEntryAlreadyExists", format!("An entry with rule number {} already exists", e.number)));
    }
    crate::db::query("INSERT INTO ec2_network_acl_entries (acl_id, rule_number, egress, protocol, rule_action, cidr, port_from, port_to) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(acl)
        .bind(e.number)
        .bind(e.egress)
        .bind(&e.protocol)
        .bind(&e.action)
        .bind(&e.cidr)
        .bind(e.from)
        .bind(e.to)
        .execute(&state.pool)
        .await?;
    Ok("<return>true</return><forwardingActive>false</forwardingActive>".into())
}

pub async fn replace_network_acl_entry(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let acl = resolve(state, Kind::NetworkAcl, &need(p, "NetworkAclId")?, "InvalidNetworkAclID.NotFound").await?;
    let (vpc, _) = acl_vpc(state, acl).await?;
    authorize_vpc(state, actor, vpc, true).await?;
    let e = parse_entry(p)?;
    let done = crate::db::query("UPDATE ec2_network_acl_entries SET protocol = ?, rule_action = ?, cidr = ?, port_from = ?, port_to = ? WHERE acl_id = ? AND rule_number = ? AND egress = ?")
        .bind(&e.protocol)
        .bind(&e.action)
        .bind(&e.cidr)
        .bind(e.from)
        .bind(e.to)
        .bind(acl)
        .bind(e.number)
        .bind(e.egress)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if done == 0 {
        return Err(bad("InvalidNetworkAclEntry.NotFound", format!("There is no entry with rule number {}", e.number)));
    }
    Ok("<return>true</return>".into())
}

pub async fn delete_network_acl_entry(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let acl = resolve(state, Kind::NetworkAcl, &need(p, "NetworkAclId")?, "InvalidNetworkAclID.NotFound").await?;
    let (vpc, _) = acl_vpc(state, acl).await?;
    authorize_vpc(state, actor, vpc, true).await?;
    let number: i64 = need(p, "RuleNumber")?.parse().map_err(|_| bad("InvalidParameterValue", "RuleNumber must be a number"))?;
    if number == 32767 {
        return Err(bad("InvalidParameterValue", "the final deny rule (32767) cannot be deleted"));
    }
    let egress = match need(p, "Egress")?.as_str() {
        "true" => true,
        "false" => false,
        _ => return Err(bad("InvalidParameterValue", "Egress must be true or false")),
    };
    let done = crate::db::query("DELETE FROM ec2_network_acl_entries WHERE acl_id = ? AND rule_number = ? AND egress = ?")
        .bind(acl)
        .bind(number)
        .bind(egress)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if done == 0 {
        return Err(bad("InvalidNetworkAclEntry.NotFound", format!("There is no entry with rule number {number}")));
    }
    Ok("<return>true</return>".into())
}

pub async fn replace_network_acl_association(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    ensure_defaults(state).await?;
    let assoc = resolve(state, Kind::NetworkAclAssociation, &need(p, "AssociationId")?, "InvalidAssociationID.NotFound").await?;
    let acl = resolve(state, Kind::NetworkAcl, &need(p, "NetworkAclId")?, "InvalidNetworkAclID.NotFound").await?;
    let (old_acl, subnet): (Uuid, Uuid) = crate::db::query_as("SELECT acl_id, subnet_id FROM ec2_acl_assocs WHERE id = ?").bind(assoc).fetch_one(&state.pool).await?;
    let (vpc, _) = acl_vpc(state, acl).await?;
    authorize_vpc(state, actor, vpc, true).await?;
    if acl_vpc(state, old_acl).await?.0 != vpc {
        return Err(bad("InvalidParameterValue", "the network ACL is in another VPC"));
    }
    let id = Uuid::new_v4();
    crate::db::query("DELETE FROM ec2_acl_assocs WHERE id = ?").bind(assoc).execute(&state.pool).await?;
    crate::db::query("INSERT INTO ec2_acl_assocs (id, acl_id, subnet_id) VALUES (?, ?, ?)").bind(id).bind(acl).bind(subnet).execute(&state.pool).await?;
    Ok(format!("<newAssociationId>{}</newAssociationId>", ec2_id(Kind::NetworkAclAssociation, id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_params() -> Params {
        let mut p = Params::new();
        for (k, v) in [("RuleNumber", "100"), ("Egress", "false"), ("Protocol", "tcp"), ("RuleAction", "allow"), ("CidrBlock", "10.0.0.0/8"), ("PortRange.From", "22"), ("PortRange.To", "22")] {
            p.insert(k.into(), v.into());
        }
        p
    }

    #[test]
    fn protocols_become_numbers() {
        assert_eq!(normalize_protocol("tcp").unwrap(), "6");
        assert_eq!(normalize_protocol("UDP").unwrap(), "17");
        assert_eq!(normalize_protocol("all").unwrap(), "-1");
        assert_eq!(normalize_protocol("47").unwrap(), "47");
        assert!(normalize_protocol("icmpv6").is_err());
        assert!(normalize_protocol("bogus").is_err());
    }

    #[test]
    fn a_valid_entry_parses() {
        let e = parse_entry(&entry_params()).unwrap();
        assert_eq!((e.number, e.egress, e.protocol.as_str(), e.action.as_str()), (100, false, "6", "allow"));
        assert_eq!((e.from, e.to), (Some(22), Some(22)));
    }

    #[test]
    fn bad_entries_are_refused_not_ignored() {
        let mut p = entry_params();
        p.insert("RuleNumber".into(), "32767".into());
        assert!(parse_entry(&p).is_err());
        let mut p = entry_params();
        p.remove("PortRange.From");
        assert!(parse_entry(&p).is_err(), "tcp needs a port range");
        let mut p = entry_params();
        p.insert("Icmp.Type".into(), "8".into());
        assert_eq!(parse_entry(&p).err().unwrap().code, "UnsupportedOperation");
        let mut p = entry_params();
        p.insert("CidrBlock".into(), "10.0.0.1/8".into());
        assert!(parse_entry(&p).is_err(), "not a network address");
        let mut p = entry_params();
        p.insert("Protocol".into(), "-1".into());
        assert!(parse_entry(&p).is_err(), "ports only on tcp/udp");
    }

    #[test]
    fn entry_xml_shows_ports_only_when_there_are_some() {
        assert!(entry_xml(100, false, "6", "allow", "10.0.0.0/8", Some(22), Some(22)).contains("<portRange><from>22</from><to>22</to></portRange>"));
        assert!(!entry_xml(32767, true, "-1", "deny", "0.0.0.0/0", None, None).contains("portRange"));
    }
}
