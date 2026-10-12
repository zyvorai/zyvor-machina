// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Security group rules as objects (`sgr-` ids): the describe, in-place modify and description updates that Terraform's
//! `aws_vpc_security_group_*_rule` resources use. Authorize/Revoke stay in `more.rs`; both read and write the same
//! `security_group_rules` rows, so the rules the REST API and the web UI show are the same ones.

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::{ec2_id, Kind};
use crate::state::AppState;

use super::more::{parse_permissions, protocol_out, resolve};
use super::netcommon::{all_tags, bad, need, passes, tags_of_item, tags_xml, valid_cidr};
use super::{indexed, xml_escape, Ec2Error, OWNER};

type Params = BTreeMap<String, String>;

type Row = (Uuid, Uuid, String, Option<String>, Option<i64>, Option<i64>, Option<String>, Option<String>, String);

fn rule_item(row: &Row, tags: &str) -> String {
    let (id, group, direction, protocol, lo, hi, cidr, remote_sg, description) = row;
    let proto = protocol_out(protocol.as_deref());
    let (from, to) = if proto == "-1" { (-1, -1) } else { (lo.unwrap_or(-1), hi.or(*lo).unwrap_or(-1)) };
    let peer = match (remote_sg.as_deref().filter(|s| s.len() >= 17), cidr.as_deref()) {
        (Some(g), _) => format!("<referencedGroupInfo><groupId>sg-{}</groupId></referencedGroupInfo>", &g[..17]),
        (None, Some(c)) => format!("<cidrIpv4>{}</cidrIpv4>", xml_escape(c)),
        (None, None) => "<cidrIpv4>0.0.0.0/0</cidrIpv4>".to_string(),
    };
    format!(
        "<item><securityGroupRuleId>{}</securityGroupRuleId><groupId>{}</groupId><groupOwnerId>{OWNER}</groupOwnerId><isEgress>{}</isEgress><ipProtocol>{proto}</ipProtocol><fromPort>{from}</fromPort><toPort>{to}</toPort>{peer}<description>{}</description>{tags}</item>",
        ec2_id(Kind::SecurityGroupRule, *id),
        ec2_id(Kind::SecurityGroup, *group),
        direction == "egress",
        xml_escape(description)
    )
}

pub async fn describe_security_group_rules(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let wanted = indexed(p, "SecurityGroupRuleId");
    let tags = all_tags(state).await?;
    let rows: Vec<Row> = crate::db::query_as(
        "SELECT id, security_group_id, direction, protocol, port_min, port_max, remote_cidr, remote_sg_id, description FROM security_group_rules ORDER BY created_at, id",
    )
    .fetch_all(&state.pool)
    .await?;
    for w in &wanted {
        if !rows.iter().any(|r| &ec2_id(Kind::SecurityGroupRule, r.0) == w) {
            return Err(bad("InvalidSecurityGroupRuleId.NotFound", format!("The security group rule '{w}' does not exist")));
        }
    }
    let mut items = String::new();
    for row in &rows {
        let eid = ec2_id(Kind::SecurityGroupRule, row.0);
        if !wanted.is_empty() && !wanted.contains(&eid) {
            continue;
        }
        if !passes(p, &tags_of_item(&tags, Kind::SecurityGroupRule, row.0), |n| match n {
            "security-group-rule-id" => Some(vec![eid.clone()]),
            "group-id" => Some(vec![ec2_id(Kind::SecurityGroup, row.1)]),
            _ => None,
        }) {
            continue;
        }
        items.push_str(&rule_item(row, &tags_xml(&tags, Kind::SecurityGroupRule, row.0)));
    }
    Ok(format!("<securityGroupRuleSet>{items}</securityGroupRuleSet>"))
}

/// The protocol as `security_group_rules.protocol` stores it (None = all).
fn stored_protocol(raw: &str) -> Result<Option<String>, Ec2Error> {
    match raw.to_ascii_lowercase().as_str() {
        "-1" | "all" => Ok(None),
        x @ ("tcp" | "udp" | "icmp") => Ok(Some(x.to_string())),
        "58" | "icmpv6" => Ok(Some("icmpv6".into())),
        other => Err(bad("InvalidParameterValue", format!("unsupported IpProtocol '{other}'"))),
    }
}

/// The new values of one rule from `SecurityGroupRule.N.SecurityGroupRule.*`.
struct NewRule {
    protocol: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    cidr: Option<String>,
    description: String,
}

fn parse_new_rule(p: &Params, base: &str) -> Result<NewRule, Ec2Error> {
    for k in p.keys().filter(|k| k.starts_with(base)) {
        let name = &k[base.len()..];
        if !matches!(name, "IpProtocol" | "FromPort" | "ToPort" | "CidrIpv4" | "ReferencedGroupId" | "Description") {
            return Err(bad("UnsupportedOperation", format!("{name} is not supported (IPv4 CIDR or security group peers only)")));
        }
    }
    let protocol = stored_protocol(&need(p, &format!("{base}IpProtocol"))?)?;
    let num = |k: &str| -> Result<Option<i64>, Ec2Error> {
        p.get(&format!("{base}{k}")).map(|v| v.parse::<i64>().map_err(|_| bad("InvalidParameterValue", format!("{k} must be a number")))).transpose()
    };
    let (from, to) = (num("FromPort")?, num("ToPort")?);
    if matches!(protocol.as_deref(), Some("tcp") | Some("udp")) {
        match (from, to) {
            (Some(f), Some(t)) if (0..=65535).contains(&f) && (0..=65535).contains(&t) && f <= t => {}
            _ => return Err(bad("InvalidParameterValue", "FromPort and ToPort must be 0-65535 with FromPort <= ToPort for TCP and UDP")),
        }
    }
    let cidr = p.get(&format!("{base}CidrIpv4")).cloned();
    let group = p.get(&format!("{base}ReferencedGroupId")).cloned();
    if cidr.is_some() == group.is_some() {
        return Err(bad("InvalidParameterCombination", "specify exactly one of CidrIpv4 and ReferencedGroupId"));
    }
    if cidr.as_deref().is_some_and(|c| !valid_cidr(c)) {
        return Err(bad("InvalidParameterValue", "CidrIpv4 must be an IPv4 network address with a prefix"));
    }
    let description = p.get(&format!("{base}Description")).cloned().unwrap_or_default();
    if description.len() > 255 {
        return Err(bad("InvalidParameterValue", "Description is limited to 255 characters"));
    }
    Ok(NewRule { protocol, from, to, cidr, description })
}

fn resync(state: &AppState) {
    let pool = state.pool.clone();
    tokio::spawn(async move {
        crate::engine::vm_netpol::reconcile(&pool, false).await;
    });
}

pub async fn modify_security_group_rules(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let gid = resolve(state, Kind::SecurityGroup, &need(p, "GroupId")?, "InvalidGroup.NotFound").await?;
    let mut changed = 0;
    for n in 1..=50 {
        let Some(raw) = p.get(&format!("SecurityGroupRule.{n}.SecurityGroupRuleId")) else { break };
        let rule = resolve(state, Kind::SecurityGroupRule, raw, "InvalidSecurityGroupRuleId.NotFound").await?;
        let owner: Uuid = crate::db::query_scalar("SELECT security_group_id FROM security_group_rules WHERE id = ?").bind(rule).fetch_one(&state.pool).await?;
        if owner != gid {
            return Err(bad("InvalidSecurityGroupRuleId.NotFound", format!("The rule '{raw}' is not in that security group")));
        }
        let base = format!("SecurityGroupRule.{n}.SecurityGroupRule.");
        let new = parse_new_rule(p, &base)?;
        let remote_sg = match p.get(&format!("{base}ReferencedGroupId")) {
            Some(g) => Some(resolve(state, Kind::SecurityGroup, g, "InvalidGroup.NotFound").await?.simple().to_string()),
            None => None,
        };
        crate::db::query(
            "UPDATE security_group_rules SET protocol = ?, port_min = ?, port_max = ?, remote_cidr = ?, remote_sg_id = ?, description = ? WHERE id = ?",
        )
        .bind(&new.protocol)
        .bind(new.from)
        .bind(new.to.or(new.from))
        .bind(&new.cidr)
        .bind(&remote_sg)
        .bind(&new.description)
        .bind(rule)
        .execute(&state.pool)
        .await?;
        changed += 1;
    }
    if changed == 0 {
        return Err(bad("MissingParameter", "The request must contain SecurityGroupRule.1.SecurityGroupRuleId"));
    }
    resync(state);
    Ok("<return>true</return>".into())
}

pub async fn update_rule_descriptions(state: &AppState, actor: &AuthUser, p: &Params, egress: bool) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let gid = resolve(state, Kind::SecurityGroup, &need(p, "GroupId")?, "InvalidGroup.NotFound").await?;
    let perms = parse_permissions(p).map_err(|m| bad("InvalidParameterValue", m))?;
    let direction = if egress { "egress" } else { "ingress" };
    for perm in perms {
        let mut peers: Vec<(Option<String>, Option<String>, String)> =
            perm.cidrs.iter().zip(&perm.cidr_descriptions).map(|(c, d)| (Some(c.clone()), None, d.clone())).collect();
        for (g, d) in perm.groups.iter().zip(&perm.group_descriptions) {
            let sg = resolve(state, Kind::SecurityGroup, g, "InvalidGroup.NotFound").await?;
            peers.push((None, Some(sg.simple().to_string()), d.clone()));
        }
        for (cidr, remote_sg, description) in peers {
            if description.len() > 255 {
                return Err(bad("InvalidParameterValue", "Description is limited to 255 characters"));
            }
            let done = crate::db::query(
                "UPDATE security_group_rules SET description = ? WHERE security_group_id = ? AND direction = ? AND COALESCE(protocol,'') = ? \
                 AND COALESCE(port_min,-2) = ? AND COALESCE(port_max,-2) = ? AND COALESCE(remote_cidr,'') = ? AND COALESCE(remote_sg_id,'') = ?",
            )
            .bind(&description)
            .bind(gid)
            .bind(direction)
            .bind(perm.protocol.clone().unwrap_or_default())
            .bind(perm.from.unwrap_or(-2))
            .bind(perm.to.or(perm.from).unwrap_or(-2))
            .bind(cidr.unwrap_or_default())
            .bind(remote_sg.unwrap_or_default())
            .execute(&state.pool)
            .await?
            .rows_affected();
            if done == 0 {
                return Err(bad("InvalidPermission.NotFound", "The specified rule does not exist in this security group"));
            }
        }
    }
    Ok("<return>true</return>".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> Params {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    const BASE: &str = "SecurityGroupRule.1.SecurityGroupRule.";

    #[test]
    fn a_rule_needs_exactly_one_peer() {
        let ok = params(&[("SecurityGroupRule.1.SecurityGroupRule.IpProtocol", "tcp"), ("SecurityGroupRule.1.SecurityGroupRule.FromPort", "22"), ("SecurityGroupRule.1.SecurityGroupRule.ToPort", "22"), ("SecurityGroupRule.1.SecurityGroupRule.CidrIpv4", "10.0.0.0/8")]);
        let r = parse_new_rule(&ok, BASE).unwrap();
        assert_eq!((r.protocol.as_deref(), r.from, r.to, r.cidr.as_deref()), (Some("tcp"), Some(22), Some(22), Some("10.0.0.0/8")));
        let mut both = ok.clone();
        both.insert(format!("{BASE}ReferencedGroupId"), "sg-0123456789abcdef0".into());
        assert_eq!(parse_new_rule(&both, BASE).err().unwrap().code, "InvalidParameterCombination");
        let mut none = ok.clone();
        none.remove(&format!("{BASE}CidrIpv4"));
        assert!(parse_new_rule(&none, BASE).is_err());
    }

    #[test]
    fn unsupported_fields_and_bad_ports_are_refused() {
        let mut p = params(&[("SecurityGroupRule.1.SecurityGroupRule.IpProtocol", "tcp"), ("SecurityGroupRule.1.SecurityGroupRule.FromPort", "22"), ("SecurityGroupRule.1.SecurityGroupRule.ToPort", "22"), ("SecurityGroupRule.1.SecurityGroupRule.CidrIpv4", "10.0.0.0/8")]);
        p.insert(format!("{BASE}CidrIpv6"), "::/0".into());
        assert_eq!(parse_new_rule(&p, BASE).err().unwrap().code, "UnsupportedOperation");
        p.remove(&format!("{BASE}CidrIpv6"));
        p.insert(format!("{BASE}ToPort"), "21".into());
        assert!(parse_new_rule(&p, BASE).is_err());
    }

    #[test]
    fn rule_xml_shows_all_protocol_ports_as_minus_one() {
        let id = Uuid::parse_str("0123456789abcdef0123456789abcdef").unwrap();
        let row: Row = (id, id, "egress".into(), None, None, None, Some("0.0.0.0/0".into()), None, "all out".into());
        let x = rule_item(&row, "");
        assert!(x.contains("<securityGroupRuleId>sgr-0123456789abcdef0</securityGroupRuleId>"));
        assert!(x.contains("<isEgress>true</isEgress><ipProtocol>-1</ipProtocol><fromPort>-1</fromPort><toPort>-1</toPort>"));
        assert!(x.contains("<description>all out</description>"));
    }
}
