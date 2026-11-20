// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! EC2-style resource ids (`i-0123456789abcdef0`, `vol-…`, `sg-…`) for Machina's UUID-keyed resources.
//!
//! An id is `<prefix>-` plus the first 17 hex digits of the resource's UUID, so it is **deterministic** (the same
//! resource always has the same id, with nothing to store or migrate) and looks like AWS's. Going back from an id to a
//! resource scans the resource's table for that hex prefix; tables are small and a clash among 68 bits is negligible, but an
//! ambiguous prefix is reported rather than guessed.

use crate::db::DbConn;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Vm,
    Volume,
    Snapshot,
    SecurityGroup,
    KeyPair,
    Image,
    Port,
    Vpc,
    Subnet,
    InstanceGroup,
    LaunchTemplate,
    InternetGateway,
    NatGateway,
    RouteTable,
    RouteTableAssociation,
    NetworkAcl,
    NetworkAclAssociation,
    DhcpOptions,
    SecurityGroupRule,
    PlacementGroup,
    SpotRequest,
    Fleet,
}

pub const ALL: &[Kind] = &[
    Kind::Vm,
    Kind::Volume,
    Kind::Snapshot,
    Kind::SecurityGroup,
    Kind::KeyPair,
    Kind::Image,
    Kind::Port,
    Kind::Vpc,
    Kind::Subnet,
    Kind::InstanceGroup,
    Kind::LaunchTemplate,
    Kind::InternetGateway,
    Kind::NatGateway,
    Kind::RouteTable,
    Kind::RouteTableAssociation,
    Kind::NetworkAcl,
    Kind::NetworkAclAssociation,
    Kind::DhcpOptions,
    Kind::SecurityGroupRule,
    Kind::PlacementGroup,
    Kind::SpotRequest,
    Kind::Fleet,
];

impl Kind {
    pub fn prefix(self) -> &'static str {
        match self {
            Kind::Vm => "i",
            Kind::Volume => "vol",
            Kind::Snapshot => "snap",
            Kind::SecurityGroup => "sg",
            Kind::KeyPair => "key",
            Kind::Image => "ami",
            Kind::Port => "eni",
            Kind::Vpc => "vpc",
            Kind::Subnet => "subnet",
            Kind::InstanceGroup => "asg",
            Kind::LaunchTemplate => "lt",
            Kind::InternetGateway => "igw",
            Kind::NatGateway => "nat",
            Kind::RouteTable => "rtb",
            Kind::RouteTableAssociation => "rtbassoc",
            Kind::NetworkAcl => "acl",
            Kind::NetworkAclAssociation => "aclassoc",
            Kind::DhcpOptions => "dopt",
            Kind::SecurityGroupRule => "sgr",
            Kind::PlacementGroup => "pg",
            Kind::SpotRequest => "sir",
            Kind::Fleet => "fleet",
        }
    }

    /// The name used in API paths and in `resource_tags.resource_type`.
    pub fn type_name(self) -> &'static str {
        match self {
            Kind::Vm => "vm",
            Kind::Volume => "volume",
            Kind::Snapshot => "snapshot",
            Kind::SecurityGroup => "security_group",
            Kind::KeyPair => "keypair",
            Kind::Image => "image",
            Kind::Port => "port",
            Kind::Vpc => "vpc",
            Kind::Subnet => "subnet",
            Kind::InstanceGroup => "instance_group",
            Kind::LaunchTemplate => "launch_template",
            Kind::InternetGateway => "internet_gateway",
            Kind::NatGateway => "nat_gateway",
            Kind::RouteTable => "route_table",
            Kind::RouteTableAssociation => "route_table_association",
            Kind::NetworkAcl => "network_acl",
            Kind::NetworkAclAssociation => "network_acl_association",
            Kind::DhcpOptions => "dhcp_options",
            Kind::SecurityGroupRule => "security_group_rule",
            Kind::PlacementGroup => "placement_group",
            Kind::SpotRequest => "spot_instances_request",
            Kind::Fleet => "fleet",
        }
    }

    /// The table holding this kind (a fixed list, never user input, so it is safe to put in SQL).
    pub fn table(self) -> &'static str {
        match self {
            Kind::Vm => "vms",
            Kind::Volume => "volumes",
            Kind::Snapshot => "volume_snapshots",
            Kind::SecurityGroup => "security_groups",
            Kind::KeyPair => "keypairs",
            Kind::Image => "templates",
            Kind::Port => "ports",
            Kind::Vpc => "cloud_vpcs",
            Kind::Subnet => "cloud_subnets",
            Kind::InstanceGroup => "cloud_instance_groups",
            Kind::LaunchTemplate => "cloud_launch_templates",
            Kind::InternetGateway => "ec2_internet_gateways",
            Kind::NatGateway => "ec2_nat_gateways",
            Kind::RouteTable => "ec2_route_tables",
            Kind::RouteTableAssociation => "ec2_route_table_assocs",
            Kind::NetworkAcl => "ec2_network_acls",
            Kind::NetworkAclAssociation => "ec2_acl_assocs",
            Kind::DhcpOptions => "ec2_dhcp_options",
            Kind::SecurityGroupRule => "security_group_rules",
            Kind::PlacementGroup => "ec2_placement_groups",
            Kind::SpotRequest => "ec2_spot_requests",
            Kind::Fleet => "ec2_fleets",
        }
    }

    pub fn from_type(name: &str) -> Option<Kind> {
        ALL.iter().copied().find(|k| k.type_name() == name)
    }

    /// Kinds whose rows are owned by a cloud project and always need membership to touch.
    pub fn is_cloud(self) -> bool {
        matches!(
            self,
            Kind::Vpc
                | Kind::Subnet
                | Kind::InstanceGroup
                | Kind::LaunchTemplate
                | Kind::InternetGateway
                | Kind::NatGateway
                | Kind::RouteTable
                | Kind::RouteTableAssociation
                | Kind::NetworkAcl
                | Kind::NetworkAclAssociation
                | Kind::DhcpOptions
        )
    }
}

const HEX_LEN: usize = 17;

/// `i-0123456789abcdef0` for a machine, `vol-…` for a volume, and so on.
pub fn ec2_id(kind: Kind, id: Uuid) -> String {
    format!("{}-{}", kind.prefix(), &id.simple().to_string()[..HEX_LEN])
}

/// Split `i-0123456789abcdef0` into its kind and the 17-digit hex prefix. None unless it is exactly that shape.
pub fn parse(s: &str) -> Option<(Kind, String)> {
    let (prefix, hex) = s.split_once('-')?;
    if hex.len() != HEX_LEN
        || !hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let kind = ALL.iter().copied().find(|k| k.prefix() == prefix)?;
    Some((kind, hex.to_string()))
}

#[derive(Debug, PartialEq, Eq)]
pub enum Lookup {
    Found(Uuid),
    NotFound,
    Ambiguous,
}

/// Resolve an EC2-style id of a known kind to the resource's UUID.
pub async fn resolve(
    conn: &mut DbConn,
    kind: Kind,
    hex_prefix: &str,
) -> Result<Lookup, sqlx::Error> {
    let rows: Vec<Uuid> = crate::db::query_scalar(&format!(
        "SELECT id FROM {} WHERE lower(hex(id)) LIKE ? LIMIT 2",
        kind.table()
    ))
    .bind(format!("{hex_prefix}%"))
    .fetch_all(conn)
    .await?;
    Ok(match rows.as_slice() {
        [] => Lookup::NotFound,
        [one] => Lookup::Found(*one),
        _ => Lookup::Ambiguous,
    })
}

/// Accept either a UUID or an EC2-style id for `kind`, and return the UUID if the resource exists.
pub async fn locate(
    conn: &mut DbConn,
    kind: Kind,
    id: &str,
) -> Result<Lookup, sqlx::Error> {
    if let Ok(u) = Uuid::parse_str(id) {
        let found: Option<i64> =
            crate::db::query_scalar(&format!("SELECT 1 FROM {} WHERE id = ?", kind.table()))
                .bind(u)
                .fetch_optional(&mut *conn)
                .await?;
        return Ok(if found.is_some() {
            Lookup::Found(u)
        } else {
            Lookup::NotFound
        });
    }
    match parse(id) {
        Some((k, hex)) if k == kind => resolve(conn, kind, &hex).await,
        _ => Ok(Lookup::NotFound),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_deterministic_and_aws_shaped() {
        let u = Uuid::parse_str("0123456789abcdef0123456789abcdef").unwrap();
        assert_eq!(ec2_id(Kind::Vm, u), "i-0123456789abcdef0");
        assert_eq!(ec2_id(Kind::Volume, u), "vol-0123456789abcdef0");
        assert_eq!(ec2_id(Kind::SecurityGroup, u), "sg-0123456789abcdef0");
        assert_eq!(ec2_id(Kind::Vm, u), ec2_id(Kind::Vm, u));
    }

    #[test]
    fn parse_accepts_only_the_exact_shape() {
        assert_eq!(
            parse("i-0123456789abcdef0"),
            Some((Kind::Vm, "0123456789abcdef0".into()))
        );
        assert_eq!(parse("vpc-0123456789abcdef0").unwrap().0, Kind::Vpc);
        for bad in [
            "",
            "i-",
            "i-123",
            "i-0123456789abcdef",
            "i-0123456789abcdef00",
            "i-0123456789ABCDEF0",
            "x-0123456789abcdef0",
            "0123456789abcdef0",
            "i-0123456789abcdeg0",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn every_kind_round_trips_through_its_names() {
        for k in ALL {
            assert_eq!(Kind::from_type(k.type_name()), Some(*k));
            let u = Uuid::new_v4();
            assert_eq!(parse(&ec2_id(*k, u)).unwrap().0, *k);
        }
        // prefixes are unique, so an id names exactly one kind
        let mut prefixes: Vec<_> = ALL.iter().map(|k| k.prefix()).collect();
        prefixes.sort_unstable();
        prefixes.dedup();
        assert_eq!(prefixes.len(), ALL.len());
        assert_eq!(Kind::from_type("nonsense"), None);
    }
}
