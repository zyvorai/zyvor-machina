// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Instance attributes: `ModifyInstanceAttribute` / `DescribeInstanceAttribute`, monitoring, the metadata service
//! options, console output and the small stubs Terraform asks for (`GetPasswordData`, credit specifications).
//!
//! Attributes without a column of their own live in `ec2_instance_attrs`. Each one is either **enforced** (termination
//! protection, the metadata endpoint switch, vCPUs, memory, instance type), **recorded** because it is a hint that has no
//! meaning here and a client would otherwise show a permanent difference (`ebsOptimized`, the hop limit, monitoring), or
//! **refused** with `UnsupportedOperation` (source/dest check off, a terminate-on-shutdown policy, user data changes after
//! launch, required metadata tokens). Nothing is accepted and then ignored without saying so.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::{Extension, Json};
use base64::Engine;
use uuid::Uuid;

use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::{ec2_id, Kind};
use crate::state::AppState;

use super::more::{api_err, resolve};
use super::{indexed, xml_escape, Ec2Error};

type Params = BTreeMap<String, String>;

fn bad(code: &'static str, msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad(code, msg)
}

fn unsupported(msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad("UnsupportedOperation", msg)
}

fn need(p: &Params, k: &str) -> Result<String, Ec2Error> {
    p.get(k).cloned().ok_or_else(|| bad("MissingParameter", format!("The request must contain the parameter {k}")))
}

/// The attributes kept in `ec2_instance_attrs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attrs {
    pub disable_api_termination: bool,
    pub ebs_optimized: bool,
    pub monitoring: bool,
    pub metadata_endpoint: bool,
    pub metadata_hop_limit: i64,
    pub placement_group: Option<String>,
}

impl Default for Attrs {
    fn default() -> Self {
        Self { disable_api_termination: false, ebs_optimized: false, monitoring: false, metadata_endpoint: true, metadata_hop_limit: 1, placement_group: None }
    }
}

pub async fn load(state: &AppState, vm: Uuid) -> Result<Attrs, Ec2Error> {
    type Row = (bool, bool, bool, bool, i64, Option<String>);
    let row: Option<Row> = crate::db::query_as(
        "SELECT disable_api_termination, ebs_optimized, monitoring, metadata_endpoint, metadata_hop_limit, placement_group \
         FROM ec2_instance_attrs WHERE vm_id = ?",
    )
    .bind(vm)
    .fetch_optional(&state.pool)
    .await?;
    Ok(row
        .map(|(d, e, m, me, h, g)| Attrs { disable_api_termination: d, ebs_optimized: e, monitoring: m, metadata_endpoint: me, metadata_hop_limit: h, placement_group: g })
        .unwrap_or_default())
}

pub async fn save(state: &AppState, vm: Uuid, a: &Attrs) -> Result<(), Ec2Error> {
    crate::db::query(
        "INSERT INTO ec2_instance_attrs (vm_id, disable_api_termination, ebs_optimized, monitoring, metadata_endpoint, metadata_hop_limit, placement_group) \
         VALUES (?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (vm_id) DO UPDATE SET disable_api_termination = excluded.disable_api_termination, ebs_optimized = excluded.ebs_optimized, \
         monitoring = excluded.monitoring, metadata_endpoint = excluded.metadata_endpoint, metadata_hop_limit = excluded.metadata_hop_limit, \
         placement_group = excluded.placement_group",
    )
    .bind(vm)
    .bind(a.disable_api_termination)
    .bind(a.ebs_optimized)
    .bind(a.monitoring)
    .bind(a.metadata_endpoint)
    .bind(a.metadata_hop_limit)
    .bind(&a.placement_group)
    .execute(&state.pool)
    .await?;
    Ok(())
}

/// `DisableApiTermination`: refuse to terminate until the attribute is cleared.
pub async fn terminate_guard(state: &AppState, vm: Uuid, eid: &str) -> Result<(), Ec2Error> {
    if load(state, vm).await?.disable_api_termination {
        return Err(Ec2Error::bad(
            "OperationNotPermitted",
            format!("The instance '{eid}' may not be terminated. Modify its 'disableApiTermination' instance attribute and try again."),
        ));
    }
    Ok(())
}

// ---- parameters ------------------------------------------------------------------------------------------------

/// The value of one attribute in either wire form: `DisableApiTermination.Value=true` (SDKs) or
/// `Attribute=disableApiTermination&Value=true` (the CLI's `--attribute/--value`).
pub fn attr_value(p: &Params, pascal: &str, camel: &str) -> Option<String> {
    if let Some(v) = p.get(&format!("{pascal}.Value")) {
        return Some(v.clone());
    }
    (p.get("Attribute").map(String::as_str) == Some(camel)).then(|| p.get("Value").cloned()).flatten()
}

fn parse_bool(name: &str, v: &str) -> Result<bool, Ec2Error> {
    match v {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(bad("InvalidParameterValue", format!("{name} must be true or false"))),
    }
}

/// What a `ModifyInstanceAttribute` call asks for, validated before anything is changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Modify {
    pub instance_type: Option<String>,
    pub disable_api_termination: Option<bool>,
    pub ebs_optimized: Option<bool>,
    pub vcpus: Option<u32>,
    pub memory_mib: Option<u64>,
}

/// `groups` is true when the call also replaces the security groups (`GroupId.N`), which alone is a valid change.
pub fn parse_modify(p: &Params, groups: bool) -> Result<Modify, Ec2Error> {
    let mut m = Modify::default();
    let mut any = false;
    if let Some(v) = attr_value(p, "InstanceType", "instanceType") {
        m.instance_type = Some(v);
        any = true;
    }
    if let Some(v) = attr_value(p, "DisableApiTermination", "disableApiTermination") {
        m.disable_api_termination = Some(parse_bool("DisableApiTermination", &v)?);
        any = true;
    }
    if let Some(v) = attr_value(p, "EbsOptimized", "ebsOptimized") {
        m.ebs_optimized = Some(parse_bool("EbsOptimized", &v)?);
        any = true;
    }
    if let Some(v) = attr_value(p, "SourceDestCheck", "sourceDestCheck") {
        if parse_bool("SourceDestCheck", &v)? {
            any = true;
        } else {
            return Err(unsupported("SourceDestCheck=false is not supported: the instance's network edge always drops frames that do not carry its own address"));
        }
    }
    if let Some(v) = attr_value(p, "InstanceInitiatedShutdownBehavior", "instanceInitiatedShutdownBehavior") {
        match v.as_str() {
            "stop" => any = true,
            "terminate" => return Err(unsupported("InstanceInitiatedShutdownBehavior=terminate is not supported: a guest shutdown always leaves the instance stopped")),
            _ => return Err(bad("InvalidParameterValue", "InstanceInitiatedShutdownBehavior must be stop or terminate")),
        }
    }
    if p.contains_key("UserData.Value") || p.get("Attribute").is_some_and(|a| a == "userData") {
        return Err(unsupported("UserData cannot be changed after launch: the cloud-init seed is built when the instance is created"));
    }
    // Machina-specific resize attributes (not in AWS): `VCpus.Value` and `MemoryMiB.Value`.
    if let Some(v) = attr_value(p, "VCpus", "vcpus") {
        m.vcpus = Some(v.parse().ok().filter(|n| *n >= 1).ok_or_else(|| bad("InvalidParameterValue", "VCpus must be a positive number"))?);
        any = true;
    }
    if let Some(v) = attr_value(p, "MemoryMiB", "memoryMiB") {
        m.memory_mib = Some(v.parse().ok().filter(|n| *n >= 128).ok_or_else(|| bad("InvalidParameterValue", "MemoryMiB must be a number of at least 128"))?);
        any = true;
    }
    if !any && !groups {
        return Err(bad(
            "InvalidParameterValue",
            "give one of InstanceType, DisableApiTermination, EbsOptimized, SourceDestCheck (true), InstanceInitiatedShutdownBehavior (stop), VCpus, MemoryMiB, GroupId.N or preemptible",
        ));
    }
    Ok(m)
}

pub async fn modify_instance_attribute(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let id = resolve(state, Kind::Vm, &need(p, "InstanceId")?, "InvalidInstanceID.NotFound").await?;
    let groups = p.keys().any(|k| k.starts_with("GroupId."));
    let m = parse_modify(p, groups)?;
    // resolve the flavor before changing anything
    let flavor = match &m.instance_type {
        Some(t) => Some(
            crate::db::query_scalar::<_, Uuid>("SELECT id FROM flavors WHERE name = ?")
                .bind(t)
                .fetch_optional(&state.pool)
                .await?
                .ok_or_else(|| bad("InvalidParameterValue", format!("Unknown instance type '{t}'")))?,
        ),
        None => None,
    };
    if m.disable_api_termination.is_some() || m.ebs_optimized.is_some() {
        let mut a = load(state, id).await?;
        if let Some(v) = m.disable_api_termination {
            a.disable_api_termination = v;
        }
        if let Some(v) = m.ebs_optimized {
            a.ebs_optimized = v;
        }
        save(state, id, &a).await?;
    }
    if let Some(flavor_id) = flavor {
        let _ = crate::api::vms::change_vm_type(
            State(state.clone()),
            Extension(actor.clone()),
            Path(id),
            Json(crate::api::vms::ChangeTypeBody { flavor_id }),
        )
        .await
        .map_err(api_err)?;
    }
    if let Some(count) = m.vcpus {
        let _ = crate::api::vms::set_vm_vcpus(State(state.clone()), Extension(actor.clone()), Path(id), Json(crate::api::vms::SetVcpusBody { count }))
            .await
            .map_err(api_err)?;
    }
    if let Some(memory_mb) = m.memory_mib {
        let _ = crate::api::vms::set_vm_memory(State(state.clone()), Extension(actor.clone()), Path(id), Json(crate::api::vms::SetMemoryBody { memory_mb }))
            .await
            .map_err(api_err)?;
    }
    if groups {
        super::groups::modify_group_set(state, actor, p).await?;
    }
    Ok("<return>true</return>".into())
}

// ---- DescribeInstanceAttribute -----------------------------------------------------------------------------------

fn value_el(name: &str, v: &str) -> String {
    format!("<{name}><value>{}</value></{name}>", xml_escape(v))
}

pub async fn describe_instance_attribute(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let want = need(p, "InstanceId")?;
    let id = resolve(state, Kind::Vm, &want, "InvalidInstanceID.NotFound").await?;
    let attribute = need(p, "Attribute")?;
    let body = match attribute.as_str() {
        "instanceType" => {
            let flavor: Option<String> = crate::db::query_scalar("SELECT f.name FROM vms v LEFT JOIN flavors f ON f.id = v.flavor_id WHERE v.id = ?")
                .bind(id)
                .fetch_optional(&state.pool)
                .await?
                .flatten();
            value_el("instanceType", flavor.as_deref().unwrap_or(""))
        }
        "groupSet" => {
            let groups: Vec<(Uuid, String)> = crate::db::query_as(
                "SELECT g.id, g.name FROM security_groups g JOIN instance_security_groups i ON i.sg_id = g.id WHERE i.vm_id = ? ORDER BY g.name",
            )
            .bind(id)
            .fetch_all(&state.pool)
            .await?;
            let items: String = groups
                .iter()
                .map(|(gid, name)| format!("<item><groupId>{}</groupId><groupName>{}</groupName></item>", ec2_id(Kind::SecurityGroup, *gid), xml_escape(name)))
                .collect();
            format!("<groupSet>{items}</groupSet>")
        }
        "disableApiTermination" => value_el("disableApiTermination", &load(state, id).await?.disable_api_termination.to_string()),
        "ebsOptimized" => value_el("ebsOptimized", &load(state, id).await?.ebs_optimized.to_string()),
        "sourceDestCheck" => value_el("sourceDestCheck", "true"),
        "instanceInitiatedShutdownBehavior" => value_el("instanceInitiatedShutdownBehavior", "stop"),
        "userData" => {
            let ud: Option<String> = crate::db::query_scalar("SELECT json_extract(spec_json, '$.spec.cloud_init.user_data') FROM vms WHERE id = ?")
                .bind(id)
                .fetch_optional(&state.pool)
                .await?
                .flatten();
            value_el("userData", &ud.map(|u| base64::engine::general_purpose::STANDARD.encode(u)).unwrap_or_default())
        }
        "rootDeviceName" => value_el("rootDeviceName", "/dev/vda"),
        "blockDeviceMapping" => {
            let volumes: Vec<(Uuid, Option<String>, bool)> =
                crate::db::query_as("SELECT id, attached_device, delete_on_termination FROM volumes WHERE attached_vm_id = ? ORDER BY attached_device")
                    .bind(id)
                    .fetch_all(&state.pool)
                    .await?;
            let items: String = volumes
                .iter()
                .map(|(vid, dev, del)| {
                    format!(
                        "<item><deviceName>/dev/{}</deviceName><ebs><volumeId>{}</volumeId><status>attached</status><deleteOnTermination>{del}</deleteOnTermination></ebs></item>",
                        xml_escape(dev.as_deref().unwrap_or("vda")),
                        ec2_id(Kind::Volume, *vid)
                    )
                })
                .collect();
            format!("<blockDeviceMapping>{items}</blockDeviceMapping>")
        }
        "kernel" | "ramdisk" | "sriovNetSupport" => value_el(&attribute, ""),
        "enaSupport" => value_el("enaSupport", "false"),
        "productCodes" => "<productCodes/>".to_string(),
        other => return Err(bad("InvalidParameterValue", format!("Attribute '{other}' is not a valid instance attribute"))),
    };
    Ok(format!("<instanceId>{want}</instanceId>{body}"))
}

// ---- monitoring ------------------------------------------------------------------------------------------------

/// `MonitorInstances` / `UnmonitorInstances`. Machina always collects the same metrics at one granularity; the flag is
/// recorded so the instance reports the state the client asked for.
pub async fn set_monitoring(state: &AppState, actor: &AuthUser, p: &Params, on: bool) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let ids = indexed(p, "InstanceId");
    if ids.is_empty() {
        return Err(bad("MissingParameter", "The request must contain the parameter InstanceId"));
    }
    let mut vms = Vec::new();
    for s in &ids {
        vms.push((s.clone(), resolve(state, Kind::Vm, s, "InvalidInstanceID.NotFound").await?));
    }
    let mut items = String::new();
    for (s, id) in vms {
        let mut a = load(state, id).await?;
        a.monitoring = on;
        save(state, id, &a).await?;
        items.push_str(&format!("<item><instanceId>{s}</instanceId><monitoring><state>{}</state></monitoring></item>", if on { "enabled" } else { "disabled" }));
    }
    Ok(format!("<instancesSet>{items}</instancesSet>"))
}

// ---- metadata options --------------------------------------------------------------------------------------------

/// The values of `ModifyInstanceMetadataOptions`, validated.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MetadataChange {
    pub endpoint: Option<bool>,
    pub hop_limit: Option<i64>,
}

pub fn parse_metadata(p: &Params) -> Result<MetadataChange, Ec2Error> {
    let mut c = MetadataChange::default();
    match p.get("HttpTokens").map(String::as_str) {
        None | Some("optional") => {}
        Some("required") => return Err(unsupported("HttpTokens=required is not supported: the metadata service answers without a session token")),
        Some(_) => return Err(bad("InvalidParameterValue", "HttpTokens must be optional or required")),
    }
    match p.get("HttpEndpoint").map(String::as_str) {
        None => {}
        Some("enabled") => c.endpoint = Some(true),
        Some("disabled") => c.endpoint = Some(false),
        Some(_) => return Err(bad("InvalidParameterValue", "HttpEndpoint must be enabled or disabled")),
    }
    if let Some(v) = p.get("HttpPutResponseHopLimit") {
        let n: i64 = v.parse().map_err(|_| bad("InvalidParameterValue", "HttpPutResponseHopLimit must be a number"))?;
        if !(1..=64).contains(&n) {
            return Err(bad("InvalidParameterValue", "HttpPutResponseHopLimit must be between 1 and 64"));
        }
        c.hop_limit = Some(n);
    }
    if p.get("HttpProtocolIpv6").is_some_and(|v| v != "disabled") || p.get("InstanceMetadataTags").is_some_and(|v| v != "disabled") {
        return Err(unsupported("HttpProtocolIpv6 and InstanceMetadataTags are not supported"));
    }
    Ok(c)
}

pub fn metadata_xml(a: &Attrs) -> String {
    format!(
        "<instanceMetadataOptions><state>applied</state><httpTokens>optional</httpTokens><httpPutResponseHopLimit>{}</httpPutResponseHopLimit><httpEndpoint>{}</httpEndpoint><httpProtocolIpv6>disabled</httpProtocolIpv6><instanceMetadataTags>disabled</instanceMetadataTags></instanceMetadataOptions>",
        a.metadata_hop_limit,
        if a.metadata_endpoint { "enabled" } else { "disabled" }
    )
}

/// Switches the metadata endpoint for one instance. The agent's table is refreshed every 30 s, so the change reaches the
/// guest within that time.
pub async fn modify_instance_metadata_options(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let want = need(p, "InstanceId")?;
    let id = resolve(state, Kind::Vm, &want, "InvalidInstanceID.NotFound").await?;
    let c = parse_metadata(p)?;
    let mut a = load(state, id).await?;
    if let Some(e) = c.endpoint {
        a.metadata_endpoint = e;
    }
    if let Some(h) = c.hop_limit {
        a.metadata_hop_limit = h;
    }
    save(state, id, &a).await?;
    Ok(format!("<instanceId>{want}</instanceId>{}", metadata_xml(&a)))
}

// ---- console and stubs -------------------------------------------------------------------------------------------

/// `GetConsoleOutput`: the instance's QEMU log (the only console-side log Machina keeps; the guest's serial output is
/// not captured). Base64 as the API requires.
pub async fn get_console_output(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let want = need(p, "InstanceId")?;
    let id = resolve(state, Kind::Vm, &want, "InvalidInstanceID.NotFound").await?;
    let Json(v) = crate::api::vms::get_vm_qemu_logs(
        State(state.clone()),
        Extension(actor.clone()),
        Path(id),
        axum::extract::Query(crate::api::vms::QemuLogsQuery { lines: Some(500) }),
    )
    .await
    .map_err(api_err)?;
    let text = v["content"].as_str().unwrap_or_default();
    Ok(format!(
        "<instanceId>{want}</instanceId><timestamp>{}</timestamp><output>{}</output>",
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S.000Z"),
        base64::engine::general_purpose::STANDARD.encode(text)
    ))
}

pub fn get_console_screenshot() -> Result<String, Ec2Error> {
    Err(unsupported("GetConsoleScreenshot is not supported: the controller has no screenshot path to the host (open the instance's console instead)"))
}

/// `GetPasswordData`: instances have no generated Windows password; an empty answer in the right shape.
pub async fn get_password_data(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let want = need(p, "InstanceId")?;
    let _ = resolve(state, Kind::Vm, &want, "InvalidInstanceID.NotFound").await?;
    Ok(format!("<instanceId>{want}</instanceId><timestamp>{}</timestamp><passwordData/>", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S.000Z")))
}

/// `DescribeInstanceCreditSpecifications`: every instance runs at standard credits (there is no burst-credit model).
pub async fn describe_instance_credit_specifications(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let wanted = indexed(p, "InstanceId");
    let rows: Vec<Uuid> = crate::db::query_scalar("SELECT id FROM vms WHERE COALESCE(inventory_source, 'libvirt') != 'kubevirt' ORDER BY name")
        .fetch_all(&state.pool)
        .await?;
    for w in &wanted {
        if !rows.iter().any(|r| &ec2_id(Kind::Vm, *r) == w) {
            return Err(bad("InvalidInstanceID.NotFound", format!("The instance ID '{w}' does not exist")));
        }
    }
    let items: String = rows
        .iter()
        .map(|r| ec2_id(Kind::Vm, *r))
        .filter(|e| wanted.is_empty() || wanted.contains(e))
        .map(|e| format!("<item><instanceId>{e}</instanceId><cpuCredits>standard</cpuCredits></item>"))
        .collect();
    Ok(format!("<instanceCreditSpecificationSet>{items}</instanceCreditSpecificationSet>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pairs: &[(&str, &str)]) -> Params {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn both_wire_forms_of_an_attribute_are_read() {
        let sdk = p(&[("DisableApiTermination.Value", "true")]);
        assert_eq!(attr_value(&sdk, "DisableApiTermination", "disableApiTermination").as_deref(), Some("true"));
        let cli = p(&[("Attribute", "disableApiTermination"), ("Value", "true")]);
        assert_eq!(attr_value(&cli, "DisableApiTermination", "disableApiTermination").as_deref(), Some("true"));
        // a Value meant for another attribute is not picked up
        let other = p(&[("Attribute", "instanceType"), ("Value", "m1.small")]);
        assert_eq!(attr_value(&other, "DisableApiTermination", "disableApiTermination"), None);
    }

    #[test]
    fn modify_accepts_what_it_enforces_and_records() {
        let m = parse_modify(&p(&[("DisableApiTermination.Value", "true"), ("EbsOptimized.Value", "false"), ("SourceDestCheck.Value", "true")]), false).unwrap();
        assert_eq!(m.disable_api_termination, Some(true));
        assert_eq!(m.ebs_optimized, Some(false));
        let m = parse_modify(&p(&[("Attribute", "instanceType"), ("Value", "m1.small")]), false).unwrap();
        assert_eq!(m.instance_type.as_deref(), Some("m1.small"));
        let m = parse_modify(&p(&[("VCpus.Value", "4"), ("MemoryMiB.Value", "4096")]), false).unwrap();
        assert_eq!((m.vcpus, m.memory_mib), (Some(4), Some(4096)));
        assert_eq!(parse_modify(&p(&[("InstanceInitiatedShutdownBehavior.Value", "stop")]), false).unwrap(), Modify::default());
    }

    #[test]
    fn modify_refuses_what_it_cannot_do() {
        for (k, v) in [
            ("SourceDestCheck.Value", "false"),
            ("InstanceInitiatedShutdownBehavior.Value", "terminate"),
            ("UserData.Value", "aGVsbG8="),
        ] {
            assert_eq!(parse_modify(&p(&[(k, v)]), false).unwrap_err().code, "UnsupportedOperation", "{k}");
        }
        assert_eq!(parse_modify(&p(&[("DisableApiTermination.Value", "maybe")]), false).unwrap_err().code, "InvalidParameterValue");
        assert_eq!(parse_modify(&p(&[("VCpus.Value", "0")]), false).unwrap_err().code, "InvalidParameterValue");
        assert_eq!(parse_modify(&p(&[]), false).unwrap_err().code, "InvalidParameterValue");
    }

    #[test]
    fn metadata_options_validate() {
        let c = parse_metadata(&p(&[("HttpEndpoint", "disabled"), ("HttpPutResponseHopLimit", "2"), ("HttpTokens", "optional")])).unwrap();
        assert_eq!(c, MetadataChange { endpoint: Some(false), hop_limit: Some(2) });
        assert_eq!(parse_metadata(&p(&[("HttpTokens", "required")])).unwrap_err().code, "UnsupportedOperation");
        assert_eq!(parse_metadata(&p(&[("HttpPutResponseHopLimit", "65")])).unwrap_err().code, "InvalidParameterValue");
        assert_eq!(parse_metadata(&p(&[("HttpEndpoint", "off")])).unwrap_err().code, "InvalidParameterValue");
        assert_eq!(parse_metadata(&p(&[("InstanceMetadataTags", "enabled")])).unwrap_err().code, "UnsupportedOperation");
    }

    #[test]
    fn metadata_xml_reports_the_stored_values() {
        let a = Attrs { metadata_endpoint: false, metadata_hop_limit: 3, ..Attrs::default() };
        let x = metadata_xml(&a);
        assert!(x.contains("<httpEndpoint>disabled</httpEndpoint>") && x.contains("<httpPutResponseHopLimit>3</httpPutResponseHopLimit>"));
    }

    async fn insert_vm(state: &AppState) -> Uuid {
        let id = Uuid::new_v4();
        let cluster = Uuid::new_v4();
        crate::db::query("INSERT INTO clusters (id, name) VALUES (?, 'c')").bind(cluster).execute(&state.pool).await.unwrap();
        crate::db::query("INSERT INTO vms (id, cluster_id, name, spec_json) VALUES (?, ?, 'web', '{}')")
            .bind(id)
            .bind(cluster)
            .execute(&state.pool)
            .await
            .unwrap();
        id
    }

    #[tokio::test]
    async fn attributes_default_then_round_trip_and_guard_termination() {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        let vm = insert_vm(&state).await;
        assert_eq!(load(&state, vm).await.unwrap(), Attrs::default());
        assert!(terminate_guard(&state, vm, "i-1").await.is_ok());
        let a = Attrs { disable_api_termination: true, ebs_optimized: true, monitoring: true, metadata_endpoint: false, metadata_hop_limit: 2, placement_group: Some("pg1".into()) };
        save(&state, vm, &a).await.unwrap();
        assert_eq!(load(&state, vm).await.unwrap(), a);
        assert_eq!(terminate_guard(&state, vm, "i-1").await.unwrap_err().code, "OperationNotPermitted");
        // saving again replaces the row
        save(&state, vm, &Attrs::default()).await.unwrap();
        assert_eq!(load(&state, vm).await.unwrap(), Attrs::default());
    }

    #[tokio::test]
    async fn describe_attribute_reads_protection_and_user_data() {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        let vm = insert_vm(&state).await;
        crate::db::query("UPDATE vms SET spec_json = ? WHERE id = ?")
            .bind(r##"{"spec":{"cloud_init":{"user_data":"#!/bin/sh\necho hi"}}}"##)
            .bind(vm)
            .execute(&state.pool)
            .await
            .unwrap();
        save(&state, vm, &Attrs { disable_api_termination: true, ..Attrs::default() }).await.unwrap();
        let eid = ec2_id(Kind::Vm, vm);
        let x = describe_instance_attribute(&state, &p(&[("InstanceId", &eid), ("Attribute", "disableApiTermination")])).await.unwrap();
        assert!(x.contains("<disableApiTermination><value>true</value>"), "{x}");
        let x = describe_instance_attribute(&state, &p(&[("InstanceId", &eid), ("Attribute", "userData")])).await.unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode("#!/bin/sh\necho hi");
        assert!(x.contains(&format!("<userData><value>{b64}</value>")), "{x}");
        let x = describe_instance_attribute(&state, &p(&[("InstanceId", &eid), ("Attribute", "sourceDestCheck")])).await.unwrap();
        assert!(x.contains("<sourceDestCheck><value>true</value>"));
        let e = describe_instance_attribute(&state, &p(&[("InstanceId", &eid), ("Attribute", "nope")])).await.unwrap_err();
        assert_eq!(e.code, "InvalidParameterValue");
    }

    #[tokio::test]
    async fn credit_specs_and_password_data_have_the_right_shape() {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        let vm = insert_vm(&state).await;
        let eid = ec2_id(Kind::Vm, vm);
        let x = describe_instance_credit_specifications(&state, &p(&[("InstanceId.1", &eid)])).await.unwrap();
        assert!(x.contains(&format!("<instanceId>{eid}</instanceId><cpuCredits>standard</cpuCredits>")));
        assert_eq!(describe_instance_credit_specifications(&state, &p(&[("InstanceId.1", "i-00000000000000000")])).await.unwrap_err().code, "InvalidInstanceID.NotFound");
        let x = get_password_data(&state, &p(&[("InstanceId", &eid)])).await.unwrap();
        assert!(x.contains("<passwordData/>"));
        assert_eq!(get_console_screenshot().unwrap_err().code, "UnsupportedOperation");
    }
}
