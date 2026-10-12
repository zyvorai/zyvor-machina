// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! More EC2 actions: volumes, security groups, images, VPCs and subnets, network interfaces, key pairs, reboot and
//! instance-type changes. Each one is a thin mapping onto the controller's own handlers, so authorization, validation and
//! audit behave exactly as in the REST API.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::{Extension, Json};
use serde_json::json;
use uuid::Uuid;

use super::{indexed, parse_filters, xml_escape, Ec2Error, OWNER};
use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::{self, ec2_id, Kind, Lookup};
use crate::state::AppState;

type Params = BTreeMap<String, String>;

fn bad(code: &'static str, msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad(code, msg)
}

fn need(p: &Params, k: &str) -> Result<String, Ec2Error> {
    p.get(k).cloned().ok_or_else(|| bad("MissingParameter", format!("The request must contain the parameter {k}")))
}

pub(super) fn api_err(e: crate::api::ApiError) -> Ec2Error {
    let code = match e.status.as_u16() {
        404 => "InvalidParameterValue",
        409 => "IncorrectState",
        403 => "UnauthorizedOperation",
        400 => "InvalidParameterValue",
        _ => "InternalError",
    };
    Ec2Error::new(e.status, code, e.message)
}

pub(super) async fn resolve(state: &AppState, kind: Kind, id: &str, code: &'static str) -> Result<Uuid, Ec2Error> {
    let (k, hex) = resource_ids::parse(id).ok_or_else(|| bad("InvalidID", format!("The id '{id}' is not valid")))?;
    if k != kind {
        return Err(bad("InvalidID", format!("The id '{id}' is not a {} id", kind.type_name())));
    }
    let mut conn = state.pool.acquire().await?;
    match resource_ids::resolve(&mut conn, kind, &hex).await? {
        Lookup::Found(u) => Ok(u),
        _ => Err(bad(code, format!("The {} '{id}' does not exist", kind.type_name()))),
    }
}

pub(super) fn tag_set(tags: &[(String, String)]) -> String {
    let items: String = tags
        .iter()
        .map(|(k, v)| format!("<item><key>{}</key><value>{}</value></item>", xml_escape(k), xml_escape(v)))
        .collect();
    format!("<tagSet>{items}</tagSet>")
}

pub(super) async fn tags_of(state: &AppState) -> Result<BTreeMap<(String, String), Vec<(String, String)>>, Ec2Error> {
    let rows: Vec<(String, String, String, String)> = crate::db::query_as("SELECT resource_type, resource_id, key, value FROM resource_tags")
        .fetch_all(&state.pool)
        .await?;
    let mut out: BTreeMap<(String, String), Vec<(String, String)>> = BTreeMap::new();
    for (t, r, k, v) in rows {
        out.entry((t, r)).or_default().push((k, v));
    }
    Ok(out)
}

pub(super) fn matches(filters: &[(String, Vec<String>)], tags: &[(String, String)], mut field: impl FnMut(&str) -> Option<Vec<String>>) -> bool {
    filters.iter().all(|(name, values)| {
        if let Some(r) = super::foundation::tag_filter_matches(tags, name, values) {
            return r;
        }
        // unknown names are refused before the action runs (`foundation::validate_filters`); one that gets here matches nothing
        field(name).is_some_and(|have| have.iter().any(|h| super::foundation::any_match(values, h)))
    })
}

// ---- volumes ---------------------------------------------------------------------------------------------------

pub async fn describe_volumes(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let wanted = indexed(p, "VolumeId");
    let filters = parse_filters(p);
    let tags = tags_of(state).await?;
    let shapes = super::volumes_ext::shapes(state).await?;
    type Row = (Uuid, String, i64, String, Option<Uuid>, Option<String>, String);
    let rows: Vec<Row> = crate::db::query_as("SELECT id, name, size_gib, status, attached_vm_id, attached_device, created_at FROM volumes ORDER BY name")
        .fetch_all(&state.pool)
        .await?;
    for w in &wanted {
        if !rows.iter().any(|r| &ec2_id(Kind::Volume, r.0) == w) {
            return Err(bad("InvalidVolume.NotFound", format!("The volume '{w}' does not exist")));
        }
    }
    let mut items = String::new();
    for (id, _name, size, status, vm, dev, created) in rows {
        let eid = ec2_id(Kind::Volume, id);
        if !wanted.is_empty() && !wanted.contains(&eid) {
            continue;
        }
        let t = tags.get(&("volume".to_string(), id.simple().to_string())).cloned().unwrap_or_default();
        let state_name = match status.as_str() {
            "in-use" => "in-use",
            "available" => "available",
            "error" => "error",
            _ => "creating",
        };
        if !matches(&filters, &t, |n| match n {
            "volume-id" => Some(vec![eid.clone()]),
            "status" => Some(vec![state_name.to_string()]),
            "size" => Some(vec![size.to_string()]),
            "attachment.instance-id" => vm.map(|v| vec![ec2_id(Kind::Vm, v)]),
            _ => None,
        }) {
            continue;
        }
        let attach = match (vm, &dev) {
            (Some(v), Some(d)) => format!(
                "<item><volumeId>{eid}</volumeId><instanceId>{}</instanceId><device>/dev/{}</device><status>attached</status><deleteOnTermination>false</deleteOnTermination></item>",
                ec2_id(Kind::Vm, v),
                xml_escape(d)
            ),
            _ => String::new(),
        };
        items.push_str(&format!(
            "<item><volumeId>{eid}</volumeId><size>{size}</size><availabilityZone>machina-a</availabilityZone><status>{state_name}</status><createTime>{}</createTime><attachmentSet>{attach}</attachmentSet>{}{}<encrypted>false</encrypted></item>",
            xml_escape(&created),
            tag_set(&t),
            super::volumes_ext::shape_xml(&shapes.get(&id).cloned().unwrap_or_default())
        ));
    }
    Ok(format!("<volumeSet>{items}</volumeSet>"))
}

pub async fn create_volume(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let size: i64 = need(p, "Size")?.parse().map_err(|_| bad("InvalidParameterValue", "Size must be a number of GiB"))?;
    super::volumes_ext::reject_create_options(p)?;
    let shape = super::volumes_ext::parse_shape(p, &super::volumes_ext::VolumeShape::default())?;
    super::tagspec::only_types(p, &["volume"])?;
    // the volume's own tags: `Tag.N` and the `TagSpecification` of type volume
    let mut tags: BTreeMap<String, String> = BTreeMap::new();
    for n in 1..=50 {
        let Some(k) = p.get(&format!("Tag.{n}.Key")) else { break };
        tags.insert(k.clone(), p.get(&format!("Tag.{n}.Value")).cloned().unwrap_or_default());
    }
    tags.extend(super::tagspec::tags_for(p, "volume"));
    let name = tags.get("Name").cloned().unwrap_or_else(|| format!("vol-{}", &Uuid::new_v4().simple().to_string()[..8]));
    let body: crate::api::volumes::CreateVolumeBody =
        serde_json::from_value(json!({ "name": name, "size_gib": size })).map_err(|e| bad("InvalidParameterValue", e.to_string()))?;
    let Json(v) = crate::api::volumes::create_volume(State(state.clone()), Extension(actor.clone()), Json(body)).await.map_err(api_err)?;
    if shape != super::volumes_ext::VolumeShape::default() {
        super::volumes_ext::save_shape(state, v.id, &shape).await?;
    }
    if !tags.is_empty() {
        let mut tx = state.pool.begin().await?;
        let _ = crate::api::tags::put_tag_map(&mut tx, Kind::Volume, v.id, &tags).await?;
        tx.commit().await?;
    }
    Ok(format!(
        "<volumeId>{}</volumeId><size>{size}</size><availabilityZone>machina-a</availabilityZone><status>{}</status>{}",
        ec2_id(Kind::Volume, v.id),
        if v.status == "available" { "available" } else { "creating" },
        super::volumes_ext::shape_xml(&shape)
    ))
}

pub async fn delete_volume(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let id = resolve(state, Kind::Volume, &need(p, "VolumeId")?, "InvalidVolume.NotFound").await?;
    let _ = crate::api::volumes::delete_volume(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
    Ok("<return>true</return>".into())
}

/// `/dev/vdb` or `vdb` → `vdb`.
pub(crate) fn device_name(dev: &str) -> Option<String> {
    let d = dev.strip_prefix("/dev/").unwrap_or(dev);
    (d.len() == 3 && d.starts_with("vd") && d.as_bytes()[2].is_ascii_lowercase()).then(|| d.to_string())
}

pub async fn attach_volume(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let vol_s = need(p, "VolumeId")?;
    let vol = resolve(state, Kind::Volume, &vol_s, "InvalidVolume.NotFound").await?;
    let vm = resolve(state, Kind::Vm, &need(p, "InstanceId")?, "InvalidInstanceID.NotFound").await?;
    let dev = device_name(&need(p, "Device")?).ok_or_else(|| bad("InvalidParameterValue", "Device must look like /dev/vdb"))?;
    if let Some(cause) = super::status::launch_failure_of(state, vm).await? {
        return Err(bad("IncorrectInstanceState", super::status::launch_failed_message(&ec2_id(Kind::Vm, vm), &cause)));
    }
    let body: crate::api::volumes::AttachVolumeBody =
        serde_json::from_value(json!({ "vm_id": vm, "target_dev": dev })).map_err(|e| bad("InvalidParameterValue", e.to_string()))?;
    let _ = crate::api::volumes::attach_volume(State(state.clone()), Extension(actor.clone()), Path(vol), Json(body)).await.map_err(api_err)?;
    Ok(format!(
        "<volumeId>{vol_s}</volumeId><instanceId>{}</instanceId><device>/dev/{dev}</device><status>attached</status>",
        ec2_id(Kind::Vm, vm)
    ))
}

pub async fn detach_volume(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let vol_s = need(p, "VolumeId")?;
    let vol = resolve(state, Kind::Volume, &vol_s, "InvalidVolume.NotFound").await?;
    let _ = crate::api::volumes::detach_volume(State(state.clone()), Extension(actor.clone()), Path(vol)).await.map_err(api_err)?;
    Ok(format!("<volumeId>{vol_s}</volumeId><status>detaching</status>"))
}

// ---- key pairs ---------------------------------------------------------------------------------------------------

pub async fn import_key_pair(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let name = need(p, "KeyName")?;
    let material = need(p, "PublicKeyMaterial")?;
    let public_key = super::decode_user_data(&material).map_err(|_| bad("InvalidKey.Format", "PublicKeyMaterial must be the base64 of an OpenSSH public key"))?;
    let body: crate::api::keypairs::CreateKeypairBody =
        serde_json::from_value(json!({ "name": name, "public_key": public_key.trim() })).map_err(|e| bad("InvalidParameterValue", e.to_string()))?;
    let Json(k) = crate::api::keypairs::create_keypair(State(state.clone()), Extension(actor.clone()), Json(body)).await.map_err(api_err)?;
    Ok(format!(
        "<keyName>{}</keyName><keyFingerprint>{}</keyFingerprint><keyPairId>{}</keyPairId>",
        xml_escape(&k.name),
        xml_escape(&k.fingerprint),
        ec2_id(Kind::KeyPair, k.id)
    ))
}

pub async fn delete_key_pair(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let id: Option<Uuid> = if let Some(n) = p.get("KeyName") {
        crate::db::query_scalar("SELECT id FROM keypairs WHERE name = ?").bind(n).fetch_optional(&state.pool).await?
    } else {
        Some(resolve(state, Kind::KeyPair, &need(p, "KeyPairId")?, "InvalidKeyPair.NotFound").await?)
    };
    // EC2 answers success for a key that is already gone.
    if let Some(id) = id {
        let _ = crate::api::keypairs::delete_keypair(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
    }
    Ok("<return>true</return>".into())
}

// ---- security groups ---------------------------------------------------------------------------------------------

pub(super) fn protocol_out(p: Option<&str>) -> &str {
    match p {
        Some(x) if x.eq_ignore_ascii_case("tcp") => "tcp",
        Some(x) if x.eq_ignore_ascii_case("udp") => "udp",
        Some(x) if x.eq_ignore_ascii_case("icmp") => "icmp",
        Some(x) if x.eq_ignore_ascii_case("icmpv6") => "58",
        _ => "-1",
    }
}

pub(crate) fn permission_item(protocol: Option<&str>, lo: Option<i64>, hi: Option<i64>, cidr: Option<&str>, group: Option<&str>, description: &str) -> String {
    let proto = protocol_out(protocol);
    let ports = match (proto, lo) {
        ("-1", _) => String::new(),
        (_, Some(l)) => format!("<fromPort>{l}</fromPort><toPort>{}</toPort>", hi.unwrap_or(l)),
        _ => String::new(),
    };
    let desc = if description.is_empty() { String::new() } else { format!("<description>{}</description>", xml_escape(description)) };
    let peer = match group {
        Some(g) => format!("<groups><item><groupId>{}</groupId>{desc}</item></groups><ipRanges/>", xml_escape(g)),
        None => format!("<groups/><ipRanges><item><cidrIp>{}</cidrIp>{desc}</item></ipRanges>", xml_escape(cidr.unwrap_or("0.0.0.0/0"))),
    };
    format!("<item><ipProtocol>{proto}</ipProtocol>{ports}{peer}<ipv6Ranges/><prefixListIds/></item>")
}

pub async fn describe_security_groups(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let wanted = indexed(p, "GroupId");
    let names = indexed(p, "GroupName");
    let filters = parse_filters(p);
    let tags = tags_of(state).await?;
    let groups: Vec<(Uuid, String, String)> = crate::db::query_as("SELECT id, name, description FROM security_groups ORDER BY name").fetch_all(&state.pool).await?;
    type Rule = (Uuid, String, Option<String>, Option<i64>, Option<i64>, Option<String>, Option<String>, String);
    let rules: Vec<Rule> = crate::db::query_as("SELECT security_group_id, direction, protocol, port_min, port_max, remote_cidr, remote_sg_id, description FROM security_group_rules ORDER BY created_at, id")
        .fetch_all(&state.pool)
        .await?;
    for w in &wanted {
        if !groups.iter().any(|g| &ec2_id(Kind::SecurityGroup, g.0) == w) {
            return Err(bad("InvalidGroup.NotFound", format!("The security group '{w}' does not exist")));
        }
    }
    let mut items = String::new();
    for (id, name, desc) in groups {
        let eid = ec2_id(Kind::SecurityGroup, id);
        if (!wanted.is_empty() && !wanted.contains(&eid)) || (!names.is_empty() && !names.contains(&name)) {
            continue;
        }
        let t = tags.get(&("security_group".to_string(), id.simple().to_string())).cloned().unwrap_or_default();
        if !matches(&filters, &t, |n| match n {
            "group-id" => Some(vec![eid.clone()]),
            "group-name" => Some(vec![name.clone()]),
            _ => None,
        }) {
            continue;
        }
        let perm = |dir: &str| -> String {
            rules
                .iter()
                .filter(|r| r.0 == id && r.1 == dir)
                .map(|r| {
                    let g = r.6.as_deref().filter(|s| s.len() >= 17).map(|s| format!("sg-{}", &s[..17]));
                    permission_item(r.2.as_deref(), r.3, r.4, r.5.as_deref(), g.as_deref(), &r.7)
                })
                .collect()
        };
        items.push_str(&format!(
            "<item><ownerId>{OWNER}</ownerId><groupId>{eid}</groupId><groupName>{}</groupName><groupDescription>{}</groupDescription><ipPermissions>{}</ipPermissions><ipPermissionsEgress>{}</ipPermissionsEgress>{}</item>",
            xml_escape(&name),
            xml_escape(&desc),
            perm("ingress"),
            perm("egress"),
            tag_set(&t)
        ));
    }
    Ok(format!("<securityGroupInfo>{items}</securityGroupInfo>"))
}

pub async fn create_security_group(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let body: crate::api::networking::CreateSecurityGroupBody = serde_json::from_value(
        json!({ "name": need(p, "GroupName")?, "description": need(p, "GroupDescription")? }),
    )
    .map_err(|e| bad("InvalidParameterValue", e.to_string()))?;
    let Json(g) = crate::api::networking::create_security_group(State(state.clone()), Extension(actor.clone()), Json(body)).await.map_err(api_err)?;
    Ok(format!("<return>true</return><groupId>{}</groupId>", ec2_id(Kind::SecurityGroup, g.id)))
}

pub async fn delete_security_group(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let id = resolve(state, Kind::SecurityGroup, &need(p, "GroupId")?, "InvalidGroup.NotFound").await?;
    let _ = crate::api::networking::delete_security_group(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
    Ok("<return>true</return>".into())
}

/// One `IpPermissions.N` entry.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Permission {
    pub protocol: Option<String>,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub cidrs: Vec<String>,
    pub groups: Vec<String>,
    /// `IpRanges.M.Description` / `Groups.M.Description`, aligned with `cidrs` / `groups` ("" when none was given).
    pub cidr_descriptions: Vec<String>,
    pub group_descriptions: Vec<String>,
}

pub(crate) fn parse_permissions(p: &Params) -> Result<Vec<Permission>, String> {
    let mut out = Vec::new();
    for n in 1..=50 {
        let Some(proto) = p.get(&format!("IpPermissions.{n}.IpProtocol")) else { break };
        let protocol = match proto.as_str() {
            "-1" | "all" => None,
            x @ ("tcp" | "udp" | "icmp") => Some(x.to_string()),
            "58" | "icmpv6" => Some("icmpv6".to_string()),
            other => return Err(format!("unsupported IpProtocol '{other}'")),
        };
        let num = |k: &str| -> Result<Option<i64>, String> {
            p.get(&format!("IpPermissions.{n}.{k}")).map(|v| v.parse::<i64>().map_err(|_| format!("{k} must be a number"))).transpose()
        };
        let cidrs: Vec<String> = (1..=50).map_while(|m| p.get(&format!("IpPermissions.{n}.IpRanges.{m}.CidrIp")).cloned()).collect();
        let groups: Vec<String> = (1..=50).map_while(|m| p.get(&format!("IpPermissions.{n}.Groups.{m}.GroupId")).cloned()).collect();
        if cidrs.is_empty() && groups.is_empty() {
            return Err("each permission needs an IpRanges or Groups entry".into());
        }
        let desc = |kind: &str, count: usize| -> Vec<String> {
            (1..=count).map(|m| p.get(&format!("IpPermissions.{n}.{kind}.{m}.Description")).cloned().unwrap_or_default()).collect()
        };
        let cidr_descriptions = desc("IpRanges", cidrs.len());
        let group_descriptions = desc("Groups", groups.len());
        out.push(Permission { protocol, from: num("FromPort")?, to: num("ToPort")?, cidrs, groups, cidr_descriptions, group_descriptions });
    }
    if out.is_empty() {
        return Err("The request must contain the parameter IpPermissions".into());
    }
    Ok(out)
}

pub async fn security_group_rules(state: &AppState, actor: &AuthUser, p: &Params, egress: bool, revoke: bool) -> Result<String, Ec2Error> {
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
            if revoke {
                let id: Option<Uuid> = crate::db::query_scalar(
                    "SELECT id FROM security_group_rules WHERE security_group_id = ? AND direction = ? AND COALESCE(protocol,'') = ? \
                     AND COALESCE(port_min,-2) = ? AND COALESCE(port_max,-2) = ? AND COALESCE(remote_cidr,'') = ? AND COALESCE(remote_sg_id,'') = ? LIMIT 1",
                )
                .bind(gid)
                .bind(direction)
                .bind(perm.protocol.clone().unwrap_or_default())
                .bind(perm.from.unwrap_or(-2))
                .bind(perm.to.or(perm.from).unwrap_or(-2))
                .bind(cidr.clone().unwrap_or_default())
                .bind(remote_sg.clone().unwrap_or_default())
                .fetch_optional(&state.pool)
                .await?;
                let id = id.ok_or_else(|| bad("InvalidPermission.NotFound", "The specified rule does not exist in this security group"))?;
                let _ = crate::api::networking::delete_security_group_rule(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
            } else {
                let body: crate::api::networking::CreateSecurityGroupRuleBody = serde_json::from_value(json!({
                    "direction": direction, "protocol": perm.protocol, "port_min": perm.from, "port_max": perm.to.or(perm.from),
                    "remote_cidr": cidr, "remote_sg_id": remote_sg, "description": description,
                }))
                .map_err(|e| bad("InvalidParameterValue", e.to_string()))?;
                let _ = crate::api::networking::create_security_group_rule(State(state.clone()), Extension(actor.clone()), Path(gid), Json(body))
                    .await
                    .map_err(api_err)?;
            }
        }
    }
    Ok("<return>true</return>".into())
}

// ---- images, VPCs, subnets, network interfaces --------------------------------------------------------------------

pub async fn describe_images(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let wanted = indexed(p, "ImageId");
    let filters = parse_filters(p);
    let tags = tags_of(state).await?;
    type Row = (Uuid, String, String, Option<String>, String, String, String);
    let rows: Vec<Row> = crate::db::query_as(
        "SELECT id, name, version, os_family, description, COALESCE(visibility,'public'), COALESCE(project,'') FROM templates ORDER BY name, version",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut items = String::new();
    for (id, name, version, os, desc, vis, _owner) in rows {
        let eid = ec2_id(Kind::Image, id);
        if !wanted.is_empty() && !wanted.contains(&eid) {
            continue;
        }
        let t = tags.get(&("image".to_string(), id.simple().to_string())).cloned().unwrap_or_default();
        if !matches(&filters, &t, |n| match n {
            "image-id" => Some(vec![eid.clone()]),
            "name" => Some(vec![name.clone()]),
            "is-public" => Some(vec![(vis == "public").to_string()]),
            _ => None,
        }) {
            continue;
        }
        items.push_str(&format!(
            "<item><imageId>{eid}</imageId><imageLocation>{}@{}</imageLocation><imageState>available</imageState><imageOwnerId>{OWNER}</imageOwnerId><isPublic>{}</isPublic><architecture>x86_64</architecture><imageType>machine</imageType><name>{}</name><description>{}</description><platform>{}</platform><rootDeviceType>ebs</rootDeviceType><virtualizationType>hvm</virtualizationType>{}</item>",
            xml_escape(&name),
            xml_escape(&version),
            vis == "public",
            xml_escape(&name),
            xml_escape(&desc),
            if os.as_deref().is_some_and(|o| o.to_ascii_lowercase().contains("windows")) { "windows" } else { "linux" },
            tag_set(&t)
        ));
    }
    Ok(format!("<imagesSet>{items}</imagesSet>"))
}

pub async fn describe_vpcs(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let wanted = indexed(p, "VpcId");
    let tags = tags_of(state).await?;
    let rows: Vec<(Uuid, String, String)> = crate::db::query_as("SELECT id, name, cidr FROM cloud_vpcs ORDER BY name").fetch_all(&state.pool).await?;
    let dhcp = super::gateways::dhcp_id_of_vpc(state).await?;
    let mut items = String::new();
    for (id, _name, cidr) in rows {
        let eid = ec2_id(Kind::Vpc, id);
        let dhcp_id = dhcp.get(&id).cloned().unwrap_or_else(|| "default".to_string());
        if !wanted.is_empty() && !wanted.contains(&eid) {
            continue;
        }
        let t = tags.get(&("vpc".to_string(), id.simple().to_string())).cloned().unwrap_or_default();
        items.push_str(&format!(
            "<item><vpcId>{eid}</vpcId><state>available</state><cidrBlock>{}</cidrBlock><dhcpOptionsId>{dhcp_id}</dhcpOptionsId><instanceTenancy>default</instanceTenancy><isDefault>false</isDefault><ownerId>{OWNER}</ownerId>{}</item>",
            xml_escape(&cidr),
            tag_set(&t)
        ));
    }
    Ok(format!("<vpcSet>{items}</vpcSet>"))
}

pub async fn describe_subnets(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let wanted = indexed(p, "SubnetId");
    let tags = tags_of(state).await?;
    let rows: Vec<(Uuid, Uuid, String, String)> = crate::db::query_as("SELECT id, vpc_id, cidr, status FROM cloud_subnets ORDER BY name").fetch_all(&state.pool).await?;
    let mut items = String::new();
    for (id, vpc, cidr, status) in rows {
        let eid = ec2_id(Kind::Subnet, id);
        if !wanted.is_empty() && !wanted.contains(&eid) {
            continue;
        }
        let t = tags.get(&("subnet".to_string(), id.simple().to_string())).cloned().unwrap_or_default();
        items.push_str(&format!(
            "<item><subnetId>{eid}</subnetId><state>{}</state><vpcId>{}</vpcId><cidrBlock>{}</cidrBlock><availableIpAddressCount>0</availableIpAddressCount><availabilityZone>machina-a</availabilityZone><mapPublicIpOnLaunch>false</mapPublicIpOnLaunch><ownerId>{OWNER}</ownerId>{}</item>",
            if status == "ready" { "available" } else { "pending" },
            ec2_id(Kind::Vpc, vpc),
            xml_escape(&cidr),
            tag_set(&t)
        ));
    }
    Ok(format!("<subnetSet>{items}</subnetSet>"))
}

pub async fn describe_network_interfaces(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let wanted = indexed(p, "NetworkInterfaceId");
    let tags = tags_of(state).await?;
    type Row = (Uuid, Option<String>, Option<String>, Option<String>, Option<Uuid>, String, Option<String>);
    let rows: Vec<Row> = crate::db::query_as("SELECT id, subnet_id, mac_address, private_ip, vm_id, status, description FROM ports ORDER BY created_at")
        .fetch_all(&state.pool)
        .await?;
    let secondary: Vec<(Uuid, String)> = crate::db::query_as("SELECT port_id, address FROM ec2_eni_ips ORDER BY address").fetch_all(&state.pool).await?;
    let mut items = String::new();
    for (id, subnet, mac, ip, vm, status, desc) in rows {
        let eid = ec2_id(Kind::Port, id);
        if !wanted.is_empty() && !wanted.contains(&eid) {
            continue;
        }
        let t = tags.get(&("port".to_string(), id.simple().to_string())).cloned().unwrap_or_default();
        let subnet_id = subnet.and_then(|s| Uuid::parse_str(&s).ok()).map(|u| ec2_id(Kind::Subnet, u)).unwrap_or_default();
        let attach = vm
            .map(|v| format!("<attachment><attachmentId>eni-attach-{}</attachmentId><instanceId>{}</instanceId><status>attached</status><deviceIndex>1</deviceIndex></attachment>", &id.simple().to_string()[..17], ec2_id(Kind::Vm, v)))
            .unwrap_or_default();
        let mut addresses = String::new();
        if let Some(primary) = ip.as_deref().filter(|a| !a.is_empty()) {
            addresses.push_str(&format!("<item><privateIpAddress>{}</privateIpAddress><primary>true</primary></item>", xml_escape(primary)));
        }
        for (_, address) in secondary.iter().filter(|(port, _)| *port == id) {
            addresses.push_str(&format!("<item><privateIpAddress>{}</privateIpAddress><primary>false</primary></item>", xml_escape(address)));
        }
        items.push_str(&format!(
            "<item><networkInterfaceId>{eid}</networkInterfaceId><subnetId>{subnet_id}</subnetId><description>{}</description><ownerId>{OWNER}</ownerId><status>{}</status><macAddress>{}</macAddress><privateIpAddress>{}</privateIpAddress><privateIpAddressesSet>{addresses}</privateIpAddressesSet>{attach}{}</item>",
            xml_escape(desc.as_deref().unwrap_or("")),
            if vm.is_some() || status == "ACTIVE" { "in-use" } else { "available" },
            xml_escape(mac.as_deref().unwrap_or("")),
            xml_escape(ip.as_deref().unwrap_or("")),
            tag_set(&t)
        ));
    }
    Ok(format!("<networkInterfaceSet>{items}</networkInterfaceSet>"))
}

// ---- instance operations ------------------------------------------------------------------------------------------

pub async fn reboot_instances(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let ids = indexed(p, "InstanceId");
    if ids.is_empty() {
        return Err(bad("MissingParameter", "The request must contain the parameter InstanceId"));
    }
    for s in &ids {
        let id = resolve(state, Kind::Vm, s, "InvalidInstanceID.NotFound").await?;
        let _ = crate::api::vms::reboot_vm(State(state.clone()), Extension(actor.clone()), Path(id), None).await.map_err(api_err)?;
    }
    Ok("<return>true</return>".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(s: &str) -> Params {
        super::super::parse_form(s)
    }

    #[test]
    fn device_names() {
        assert_eq!(device_name("/dev/vdb").as_deref(), Some("vdb"));
        assert_eq!(device_name("vdc").as_deref(), Some("vdc"));
        assert!(device_name("/dev/sda").is_none());
        assert!(device_name("/dev/vdbb").is_none());
        assert!(device_name("/dev/vd1").is_none());
    }

    #[test]
    fn permissions_parse_ranges_and_groups() {
        let p = form("IpPermissions.1.IpProtocol=tcp&IpPermissions.1.FromPort=22&IpPermissions.1.ToPort=22&IpPermissions.1.IpRanges.1.CidrIp=10.0.0.0/8&IpPermissions.1.IpRanges.2.CidrIp=192.168.0.0/16&IpPermissions.2.IpProtocol=-1&IpPermissions.2.Groups.1.GroupId=sg-0123456789abcdef0");
        let perms = parse_permissions(&p).unwrap();
        assert_eq!(perms.len(), 2);
        assert_eq!(perms[0], Permission { protocol: Some("tcp".into()), from: Some(22), to: Some(22), cidrs: vec!["10.0.0.0/8".into(), "192.168.0.0/16".into()], groups: vec![], cidr_descriptions: vec![String::new(), String::new()], group_descriptions: vec![] });
        assert_eq!(perms[1].protocol, None);
        assert_eq!(perms[1].groups, ["sg-0123456789abcdef0"]);
    }

    #[test]
    fn bad_permissions_are_refused() {
        assert!(parse_permissions(&form("Action=X")).is_err());
        assert!(parse_permissions(&form("IpPermissions.1.IpProtocol=gre&IpPermissions.1.IpRanges.1.CidrIp=1.2.3.4/32")).is_err());
        assert!(parse_permissions(&form("IpPermissions.1.IpProtocol=tcp&IpPermissions.1.FromPort=x&IpPermissions.1.IpRanges.1.CidrIp=1.2.3.4/32")).is_err());
        assert!(parse_permissions(&form("IpPermissions.1.IpProtocol=tcp")).is_err());
    }

    #[test]
    fn permission_xml() {
        let x = permission_item(Some("tcp"), Some(80), Some(90), Some("10.0.0.0/8"), None, "");
        assert!(x.contains("<ipProtocol>tcp</ipProtocol><fromPort>80</fromPort><toPort>90</toPort>"));
        assert!(x.contains("<cidrIp>10.0.0.0/8</cidrIp>"));
        let all = permission_item(None, None, None, None, Some("sg-abc"), "");
        assert!(all.contains("<ipProtocol>-1</ipProtocol>") && !all.contains("fromPort") && all.contains("<groupId>sg-abc</groupId>"));
    }

    #[test]
    fn filters_match_fields_and_tags_and_unknown_names_match_nothing() {
        let f = vec![("status".to_string(), vec!["available".to_string()])];
        assert!(matches(&f, &[], |n| (n == "status").then(|| vec!["available".to_string()])));
        assert!(!matches(&f, &[], |n| (n == "status").then(|| vec!["in-use".to_string()])));
        let f = vec![("nonsense".to_string(), vec!["x".to_string()])];
        assert!(!matches(&f, &[], |_| None));
        let f = vec![("tag:Env".to_string(), vec!["prod".to_string()])];
        assert!(matches(&f, &[("Env".into(), "prod".into())], |_| None));
    }
}
