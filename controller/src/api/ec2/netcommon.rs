// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Helpers shared by the VPC networking actions (`gateways`, `route_tables`, `nacls`, `sg_rules`): project
//! authorization through the VPC, the default objects every VPC has, `TagSpecification` handling and filter matching.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::state::AppState;

use super::more::{api_err, matches, tag_set, tags_of};
use super::{parse_filters, Ec2Error};

pub type Params = BTreeMap<String, String>;

pub fn bad(code: &'static str, msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad(code, msg)
}

pub fn need(p: &Params, k: &str) -> Result<String, Ec2Error> {
    p.get(k).cloned().ok_or_else(|| bad("MissingParameter", format!("The request must contain the parameter {k}")))
}

/// A UUID derived from `parent` and a purpose, so an object every VPC has (its main route table, its default network ACL)
/// always has the same id and can be created with `INSERT OR IGNORE` from any request.
pub fn derived_id(parent: Uuid, purpose: &str) -> Uuid {
    let mut h = Sha256::new();
    h.update(parent.as_bytes());
    h.update(purpose.as_bytes());
    let d = h.finalize();
    let mut b = [0u8; 16];
    b.copy_from_slice(&d[..16]);
    b[6] = (b[6] & 0x0f) | 0x50;
    b[8] = (b[8] & 0x3f) | 0x80;
    Uuid::from_bytes(b)
}

pub fn main_route_table_id(vpc: Uuid) -> Uuid {
    derived_id(vpc, "main-route-table")
}

pub fn default_acl_id(vpc: Uuid) -> Uuid {
    derived_id(vpc, "default-network-acl")
}

/// The project that owns a VPC.
pub async fn vpc_project(state: &AppState, vpc: Uuid) -> Result<Uuid, Ec2Error> {
    let project: Option<Uuid> = crate::db::query_scalar("SELECT project_id FROM cloud_vpcs WHERE id = ?")
        .bind(vpc)
        .fetch_optional(&state.pool)
        .await?;
    project.ok_or_else(|| bad("InvalidVpcID.NotFound", "the VPC does not exist"))
}

/// Cloud resources need project membership even when the legacy project RBAC is off (the same rule the REST API applies).
pub async fn authorize_vpc(state: &AppState, actor: &AuthUser, vpc: Uuid, write: bool) -> Result<(), Ec2Error> {
    let project = vpc_project(state, vpc).await?;
    let mut conn = state.pool.acquire().await?;
    crate::api::cloud::access(&mut conn, actor, project, write).await.map_err(api_err)
}

pub async fn vpc_of_subnet(state: &AppState, subnet: Uuid) -> Result<Uuid, Ec2Error> {
    let vpc: Option<Uuid> = crate::db::query_scalar("SELECT vpc_id FROM cloud_subnets WHERE id = ?")
        .bind(subnet)
        .fetch_optional(&state.pool)
        .await?;
    vpc.ok_or_else(|| bad("InvalidSubnetID.NotFound", "the subnet does not exist"))
}

/// Create what every VPC has (a main route table, a default network ACL with its two standard entries and an association
/// to each subnet) and drop rows that point at VPCs and subnets that are gone. Idempotent; run before the describes.
pub async fn ensure_defaults(state: &AppState) -> Result<(), Ec2Error> {
    let vpcs: Vec<Uuid> = crate::db::query_scalar("SELECT id FROM cloud_vpcs").fetch_all(&state.pool).await?;
    for vpc in vpcs {
        crate::db::query("INSERT OR IGNORE INTO ec2_route_tables (id, vpc_id, is_main) VALUES (?, ?, TRUE)")
            .bind(main_route_table_id(vpc))
            .bind(vpc)
            .execute(&state.pool)
            .await?;
        let acl = default_acl_id(vpc);
        crate::db::query("INSERT OR IGNORE INTO ec2_network_acls (id, vpc_id, is_default) VALUES (?, ?, TRUE)")
            .bind(acl)
            .bind(vpc)
            .execute(&state.pool)
            .await?;
        for egress in [false, true] {
            for (number, action) in [(100_i64, "allow"), (32767_i64, "deny")] {
                crate::db::query(
                    "INSERT OR IGNORE INTO ec2_network_acl_entries (acl_id, rule_number, egress, protocol, rule_action, cidr) VALUES (?, ?, ?, '-1', ?, '0.0.0.0/0')",
                )
                .bind(acl)
                .bind(number)
                .bind(egress)
                .bind(action)
                .execute(&state.pool)
                .await?;
            }
        }
        let subnets: Vec<Uuid> = crate::db::query_scalar("SELECT id FROM cloud_subnets WHERE vpc_id = ?").bind(vpc).fetch_all(&state.pool).await?;
        for s in subnets {
            crate::db::query("INSERT OR IGNORE INTO ec2_acl_assocs (id, acl_id, subnet_id) VALUES (?, ?, ?)")
                .bind(derived_id(s, "default-acl-association"))
                .bind(acl)
                .bind(s)
                .execute(&state.pool)
                .await?;
        }
    }
    for sql in [
        "DELETE FROM ec2_acl_assocs WHERE subnet_id NOT IN (SELECT id FROM cloud_subnets)",
        "DELETE FROM ec2_route_table_assocs WHERE subnet_id NOT IN (SELECT id FROM cloud_subnets)",
        "DELETE FROM ec2_routes WHERE route_table_id NOT IN (SELECT id FROM ec2_route_tables WHERE vpc_id IN (SELECT id FROM cloud_vpcs))",
        "DELETE FROM ec2_route_table_assocs WHERE route_table_id NOT IN (SELECT id FROM ec2_route_tables WHERE vpc_id IN (SELECT id FROM cloud_vpcs))",
        "DELETE FROM ec2_route_tables WHERE vpc_id NOT IN (SELECT id FROM cloud_vpcs)",
        "DELETE FROM ec2_network_acl_entries WHERE acl_id NOT IN (SELECT id FROM ec2_network_acls WHERE vpc_id IN (SELECT id FROM cloud_vpcs))",
        "DELETE FROM ec2_acl_assocs WHERE acl_id NOT IN (SELECT id FROM ec2_network_acls WHERE vpc_id IN (SELECT id FROM cloud_vpcs))",
        "DELETE FROM ec2_network_acls WHERE vpc_id NOT IN (SELECT id FROM cloud_vpcs)",
        "DELETE FROM ec2_nat_gateways WHERE vpc_id NOT IN (SELECT id FROM cloud_vpcs)",
    ] {
        crate::db::query(sql).execute(&state.pool).await?;
    }
    Ok(())
}

/// Tags from the `TagSpecification.N` entries whose `ResourceType` is `resource_type` (`internet-gateway`, `route-table`…).
pub fn spec_tags(p: &Params, resource_type: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for s in 1..=5 {
        if p.get(&format!("TagSpecification.{s}.ResourceType")).map(String::as_str) != Some(resource_type) {
            continue;
        }
        for n in 1..=50 {
            if let Some(k) = p.get(&format!("TagSpecification.{s}.Tag.{n}.Key")) {
                out.insert(k.clone(), p.get(&format!("TagSpecification.{s}.Tag.{n}.Value")).cloned().unwrap_or_default());
            }
        }
    }
    out
}

pub async fn apply_tags(state: &AppState, kind: crate::resource_ids::Kind, id: Uuid, tags: &BTreeMap<String, String>) -> Result<(), Ec2Error> {
    if tags.is_empty() {
        return Ok(());
    }
    crate::api::tags::validate_tags(tags).map_err(|m| bad("InvalidParameterValue", m))?;
    let mut tx = crate::db::begin_write(&state.pool).await?;
    if let crate::api::tags::PutOutcome::TooMany(n) = crate::api::tags::put_tag_map(&mut tx, kind, id, tags).await? {
        return Err(bad("TagLimitExceeded", format!("that would give the resource {n} tags")));
    }
    tx.commit().await?;
    Ok(())
}

/// The tags of every resource, by (resource type, simple uuid).
pub async fn all_tags(state: &AppState) -> Result<BTreeMap<(String, String), Vec<(String, String)>>, Ec2Error> {
    tags_of(state).await
}

pub fn tags_xml(tags: &BTreeMap<(String, String), Vec<(String, String)>>, kind: crate::resource_ids::Kind, id: Uuid) -> String {
    let t = tags.get(&(kind.type_name().to_string(), id.simple().to_string())).cloned().unwrap_or_default();
    tag_set(&t)
}

pub fn tags_of_item(tags: &BTreeMap<(String, String), Vec<(String, String)>>, kind: crate::resource_ids::Kind, id: Uuid) -> Vec<(String, String)> {
    tags.get(&(kind.type_name().to_string(), id.simple().to_string())).cloned().unwrap_or_default()
}

/// Does an item pass every filter of the request? `field(name)` answers the filter names the action understands natively
/// with the values the item has; tag filters are answered here.
pub fn passes(p: &Params, tags: &[(String, String)], field: impl FnMut(&str) -> Option<Vec<String>>) -> bool {
    matches(&parse_filters(p), tags, field)
}

/// `true`/`false` text of a parameter, if present.
pub fn bool_param(p: &Params, key: &str) -> Result<Option<bool>, Ec2Error> {
    match p.get(key).map(String::as_str) {
        None => Ok(None),
        Some("true") => Ok(Some(true)),
        Some("false") => Ok(Some(false)),
        Some(_) => Err(bad("InvalidParameterValue", format!("{key} must be true or false"))),
    }
}

/// `a.b.c` CIDR check shared by the routes and ACL entries.
pub fn valid_cidr(c: &str) -> bool {
    c.parse::<machina_spec::CloudCidr>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_ids_are_stable_distinct_and_uuid_shaped() {
        let vpc = Uuid::parse_str("0123456789abcdef0123456789abcdef").unwrap();
        assert_eq!(main_route_table_id(vpc), main_route_table_id(vpc));
        assert_ne!(main_route_table_id(vpc), default_acl_id(vpc));
        assert_ne!(main_route_table_id(vpc), main_route_table_id(Uuid::new_v4()));
        assert_eq!(main_route_table_id(vpc).get_version_num(), 5);
    }

    #[test]
    fn spec_tags_reads_only_the_named_resource_type() {
        let mut p = Params::new();
        p.insert("TagSpecification.1.ResourceType".into(), "route-table".into());
        p.insert("TagSpecification.1.Tag.1.Key".into(), "Name".into());
        p.insert("TagSpecification.1.Tag.1.Value".into(), "private".into());
        p.insert("TagSpecification.2.ResourceType".into(), "instance".into());
        p.insert("TagSpecification.2.Tag.1.Key".into(), "x".into());
        let t = spec_tags(&p, "route-table");
        assert_eq!(t.len(), 1);
        assert_eq!(t["Name"], "private");
        assert!(spec_tags(&p, "natgateway").is_empty());
    }

    #[test]
    fn bool_param_is_strict() {
        let mut p = Params::new();
        assert_eq!(bool_param(&p, "X").unwrap(), None);
        p.insert("X".into(), "true".into());
        assert_eq!(bool_param(&p, "X").unwrap(), Some(true));
        p.insert("X".into(), "yes".into());
        assert!(bool_param(&p, "X").is_err());
    }

    #[test]
    fn cidr_check() {
        assert!(valid_cidr("10.0.0.0/16"));
        assert!(!valid_cidr("10.0.0.0/33"));
        assert!(!valid_cidr("nonsense"));
    }
}
