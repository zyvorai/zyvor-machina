// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Launch templates with versions. `CreateLaunchTemplate` stores `LaunchTemplateData.*` as version 1, further versions
//! are added with `CreateLaunchTemplateVersion`, `ModifyLaunchTemplate` moves the default, and `RunInstances` resolves
//! `$Latest`, `$Default` or a number (`run_options::with_launch_template`).
//!
//! The data is kept as the flat `RunInstances` parameters it stands for (`InstanceType`, `BlockDeviceMapping.1.DeviceName`…),
//! so applying a template is a merge of parameters and the same validation that refuses an unsupported `RunInstances`
//! option refuses it when the template is written, not when it is used.

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::{ec2_id, Kind};
use crate::state::AppState;

use super::more::resolve;
use super::{indexed, tagspec, xml_escape, Ec2Error};

type Params = BTreeMap<String, String>;

fn bad(code: &'static str, msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad(code, msg)
}

fn need(p: &Params, k: &str) -> Result<String, Ec2Error> {
    p.get(k).cloned().ok_or_else(|| bad("MissingParameter", format!("The request must contain the parameter {k}")))
}

/// `LaunchTemplateData` members a template can hold: the `RunInstances` parameters Machina applies.
const DATA_BASES: &[&str] = &[
    "ImageId",
    "InstanceType",
    "KeyName",
    "UserData",
    "SecurityGroupId",
    "BlockDeviceMapping",
    "Placement",
    "Monitoring",
    "MetadataOptions",
    "TagSpecification",
    "InstanceMarketOptions",
    "EbsOptimized",
    "DisableApiTermination",
    "InstanceInitiatedShutdownBehavior",
];

/// `LaunchTemplateData.*` of a request → flat run parameters, validated like a `RunInstances` call. Anything Machina cannot
/// apply is refused here instead of being stored and ignored at launch.
pub fn parse_data(p: &Params) -> Result<Params, Ec2Error> {
    let mut data = Params::new();
    for (k, v) in p {
        let Some(rest) = k.strip_prefix("LaunchTemplateData.") else { continue };
        let base = rest.split('.').next().unwrap_or(rest);
        if !DATA_BASES.contains(&base) {
            return Err(bad("UnsupportedOperation", format!("LaunchTemplateData.{base} is not supported")));
        }
        data.insert(rest.to_string(), v.clone());
    }
    // the older call shape: ImageId at the top level
    if let Some(image) = p.get("ImageId") {
        data.entry("ImageId".into()).or_insert_with(|| image.clone());
    }
    super::run_options::parse(&data)?;
    super::tagspec::only_types(&data, &["instance", "volume"])?;
    Ok(data)
}

/// Template parameters merged under a request's own: a parameter family the request sets (`BlockDeviceMapping.*`) replaces
/// the template's whole family, as in EC2.
pub fn merge_under(request: &Params, template: &Params) -> Params {
    let mut out = request.clone();
    let families: std::collections::HashSet<&str> = request.keys().map(|k| k.split('.').next().unwrap_or(k)).collect();
    for (k, v) in template {
        let base = k.split('.').next().unwrap_or(k);
        if !families.contains(base) {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

// ---- storage -------------------------------------------------------------------------------------------------------

pub struct Template {
    pub id: Uuid,
    pub name: String,
    pub project: Uuid,
    pub created_at: String,
    pub default_version: i64,
    spec_json: String,
}

pub async fn find(state: &AppState, p: &Params) -> Result<Template, Ec2Error> {
    type Row = (Uuid, String, Uuid, String, i64, String);
    let by_id = p.get("LaunchTemplateId");
    let by_name = p.get("LaunchTemplateName");
    let (id, name) = match (by_id, by_name) {
        (Some(_), Some(_)) => return Err(bad("InvalidParameterCombination", "give LaunchTemplateId or LaunchTemplateName, not both")),
        (None, None) => return Err(bad("MissingParameter", "LaunchTemplateId or LaunchTemplateName is required")),
        (Some(id), None) => (Some(resolve(state, Kind::LaunchTemplate, id, "InvalidLaunchTemplateId.NotFound").await?), None),
        (None, Some(n)) => (None, Some(n.clone())),
    };
    let rows: Vec<Row> = match (id, &name) {
        (Some(id), _) => crate::db::query_as("SELECT id, name, project_id, created_at, default_version, spec_json FROM cloud_launch_templates WHERE id = ?")
            .bind(id)
            .fetch_all(&state.pool)
            .await?,
        (None, Some(n)) => crate::db::query_as("SELECT id, name, project_id, created_at, default_version, spec_json FROM cloud_launch_templates WHERE name = ?")
            .bind(n)
            .fetch_all(&state.pool)
            .await?,
        _ => Vec::new(),
    };
    match rows.len() {
        0 => Err(bad(
            if by_id.is_some() { "InvalidLaunchTemplateId.NotFound" } else { "InvalidLaunchTemplateName.NotFoundException" },
            "The launch template does not exist",
        )),
        1 => {
            let (id, name, project, created_at, default_version, spec_json) = rows.into_iter().next().unwrap_or_else(|| unreachable!());
            Ok(Template { id, name, project, created_at, default_version, spec_json })
        }
        _ => Err(bad("InvalidParameterValue", "the name exists in several projects: use LaunchTemplateId")),
    }
}

pub async fn latest_version(state: &AppState, id: Uuid) -> Result<i64, Ec2Error> {
    let v: Option<i64> = crate::db::query_scalar("SELECT MAX(version) FROM ec2_launch_template_versions WHERE template_id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    Ok(v.unwrap_or(1))
}

/// A version that predates versioning has no row: its data is the image its spec names.
fn legacy_data(spec_json: &str) -> Params {
    let v: serde_json::Value = serde_json::from_str(spec_json).unwrap_or_default();
    let image = v.pointer("/spec/template_ref").and_then(|x| x.as_str()).unwrap_or_default();
    let mut d = Params::new();
    if !image.is_empty() {
        d.insert("ImageId".into(), image.to_string());
    }
    d
}

pub struct Version {
    pub number: i64,
    pub description: String,
    pub data: Params,
    pub created_at: String,
}

pub async fn load_version(state: &AppState, t: &Template, number: i64) -> Result<Option<Version>, Ec2Error> {
    let row: Option<(String, String, String)> =
        crate::db::query_as("SELECT description, data_json, created_at FROM ec2_launch_template_versions WHERE template_id = ? AND version = ?")
            .bind(t.id)
            .bind(number)
            .fetch_optional(&state.pool)
            .await?;
    Ok(match row {
        Some((description, data_json, created_at)) => {
            Some(Version { number, description, data: serde_json::from_str(&data_json).unwrap_or_default(), created_at })
        }
        None if number == 1 && latest_version(state, t.id).await? == 1 => {
            Some(Version { number: 1, description: String::new(), data: legacy_data(&t.spec_json), created_at: t.created_at.clone() })
        }
        None => None,
    })
}

/// `$Latest`, `$Default` or a number → the version number, which must exist.
pub async fn select_version(state: &AppState, t: &Template, selector: &str) -> Result<i64, Ec2Error> {
    let n = match selector {
        "$Latest" => latest_version(state, t.id).await?,
        "$Default" => t.default_version,
        digits => digits
            .parse::<i64>()
            .map_err(|_| bad("InvalidLaunchTemplateId.VersionNotFound", format!("The launch template version '{digits}' does not exist")))?,
    };
    if load_version(state, t, n).await?.is_none() {
        return Err(bad("InvalidLaunchTemplateId.VersionNotFound", format!("The launch template version '{selector}' does not exist")));
    }
    Ok(n)
}

/// The run parameters of a template version, for `RunInstances` and `CreateFleet`.
pub async fn run_params(state: &AppState, p: &Params, prefix: &str) -> Result<Option<Params>, Ec2Error> {
    let id = p.get(&format!("{prefix}LaunchTemplateId"));
    let name = p.get(&format!("{prefix}LaunchTemplateName"));
    if id.is_none() && name.is_none() {
        if p.contains_key(&format!("{prefix}Version")) {
            return Err(bad("MissingParameter", "LaunchTemplateId or LaunchTemplateName is required"));
        }
        return Ok(None);
    }
    let mut q = Params::new();
    if let Some(i) = id {
        q.insert("LaunchTemplateId".into(), i.clone());
    }
    if let Some(n) = name {
        q.insert("LaunchTemplateName".into(), n.clone());
    }
    let t = find(state, &q).await?;
    let selector = p.get(&format!("{prefix}Version")).map(String::as_str).unwrap_or("$Default");
    let n = select_version(state, &t, selector).await?;
    Ok(load_version(state, &t, n).await?.map(|v| v.data))
}

// ---- XML -----------------------------------------------------------------------------------------------------------

fn created_by(actor: &str) -> String {
    format!("arn:aws:iam::000000000000:user/{actor}")
}

/// The `launchTemplateData` element for flat run parameters.
pub fn data_xml(d: &Params) -> String {
    let get = |k: &str| d.get(k).map(String::as_str);
    let mut x = String::new();
    let mut simple = |tag: &str, key: &str| {
        if let Some(v) = get(key) {
            x.push_str(&format!("<{tag}>{}</{tag}>", xml_escape(v)));
        }
    };
    simple("imageId", "ImageId");
    simple("instanceType", "InstanceType");
    simple("keyName", "KeyName");
    simple("userData", "UserData");
    simple("ebsOptimized", "EbsOptimized");
    simple("disableApiTermination", "DisableApiTermination");
    simple("instanceInitiatedShutdownBehavior", "InstanceInitiatedShutdownBehavior");
    let groups = indexed(d, "SecurityGroupId");
    if !groups.is_empty() {
        let items: String = groups.iter().map(|g| format!("<item>{}</item>", xml_escape(g))).collect();
        x.push_str(&format!("<securityGroupIdSet>{items}</securityGroupIdSet>"));
    }
    let mut bdm = String::new();
    for n in 1..=24 {
        let pre = format!("BlockDeviceMapping.{n}");
        let Some(dev) = d.get(&format!("{pre}.DeviceName")) else { break };
        let mut ebs = String::new();
        for (key, tag) in [("VolumeSize", "volumeSize"), ("DeleteOnTermination", "deleteOnTermination"), ("VolumeType", "volumeType")] {
            if let Some(v) = d.get(&format!("{pre}.Ebs.{key}")) {
                ebs.push_str(&format!("<{tag}>{}</{tag}>", xml_escape(v)));
            }
        }
        bdm.push_str(&format!("<item><deviceName>{}</deviceName><ebs>{ebs}</ebs></item>", xml_escape(dev)));
    }
    if !bdm.is_empty() {
        x.push_str(&format!("<blockDeviceMappingSet>{bdm}</blockDeviceMappingSet>"));
    }
    if let Some(v) = get("Monitoring.Enabled") {
        x.push_str(&format!("<monitoring><enabled>{}</enabled></monitoring>", xml_escape(v)));
    }
    let mut placement = String::new();
    for (key, tag) in [("AvailabilityZone", "availabilityZone"), ("GroupName", "groupName"), ("Tenancy", "tenancy")] {
        if let Some(v) = d.get(&format!("Placement.{key}")) {
            placement.push_str(&format!("<{tag}>{}</{tag}>", xml_escape(v)));
        }
    }
    if !placement.is_empty() {
        x.push_str(&format!("<placement>{placement}</placement>"));
    }
    let mut meta = String::new();
    for (key, tag) in [("HttpTokens", "httpTokens"), ("HttpEndpoint", "httpEndpoint"), ("HttpPutResponseHopLimit", "httpPutResponseHopLimit")] {
        if let Some(v) = d.get(&format!("MetadataOptions.{key}")) {
            meta.push_str(&format!("<{tag}>{}</{tag}>", xml_escape(v)));
        }
    }
    if !meta.is_empty() {
        x.push_str(&format!("<metadataOptions>{meta}</metadataOptions>"));
    }
    let mut specs = String::new();
    for n in 1..=10 {
        let Some(rt) = d.get(&format!("TagSpecification.{n}.ResourceType")) else { break };
        let tags: BTreeMap<String, String> = tagspec::tags_for(d, rt);
        specs.push_str(&format!("<item><resourceType>{}</resourceType>{}</item>", xml_escape(rt), tagspec::tag_set_xml(&tags)));
    }
    if !specs.is_empty() {
        x.push_str(&format!("<tagSpecificationSet>{specs}</tagSpecificationSet>"));
    }
    if let Some(mt) = get("InstanceMarketOptions.MarketType") {
        x.push_str(&format!("<instanceMarketOptions><marketType>{}</marketType></instanceMarketOptions>", xml_escape(mt)));
    }
    format!("<launchTemplateData>{x}</launchTemplateData>")
}

async fn template_xml(state: &AppState, t: &Template, actor: &str, wrap: &str) -> Result<String, Ec2Error> {
    let latest = latest_version(state, t.id).await?;
    let tags = tagspec::tags_of(state, Kind::LaunchTemplate, t.id).await?;
    Ok(format!(
        "<{wrap}><launchTemplateId>{}</launchTemplateId><launchTemplateName>{}</launchTemplateName><createTime>{}</createTime><createdBy>{}</createdBy><defaultVersionNumber>{}</defaultVersionNumber><latestVersionNumber>{latest}</latestVersionNumber><projectId>{}</projectId>{}</{wrap}>",
        ec2_id(Kind::LaunchTemplate, t.id),
        xml_escape(&t.name),
        xml_escape(&t.created_at),
        xml_escape(&created_by(actor)),
        t.default_version,
        t.project,
        tagspec::tag_set_xml(&tags)
    ))
}

async fn version_xml(t: &Template, v: &Version, actor: &str) -> String {
    format!(
        "<launchTemplateId>{}</launchTemplateId><launchTemplateName>{}</launchTemplateName><versionNumber>{}</versionNumber><versionDescription>{}</versionDescription><createTime>{}</createTime><createdBy>{}</createdBy><defaultVersion>{}</defaultVersion>{}",
        ec2_id(Kind::LaunchTemplate, t.id),
        xml_escape(&t.name),
        v.number,
        xml_escape(&v.description),
        xml_escape(&v.created_at),
        xml_escape(&created_by(actor)),
        v.number == t.default_version,
        data_xml(&v.data)
    )
}

// ---- actions -------------------------------------------------------------------------------------------------------

async fn default_project(state: &AppState, p: &Params) -> Result<Uuid, Ec2Error> {
    if let Some(s) = p.get("ProjectId") {
        return s.parse().map_err(|_| bad("InvalidParameterValue", "ProjectId must be a project UUID"));
    }
    crate::db::query_scalar::<_, Uuid>("SELECT id FROM projects WHERE name = 'default'")
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| bad("MissingParameter", "ProjectId is required (there is no default project)"))
}

pub async fn create_launch_template(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    use axum::extract::{Path, State};
    use axum::{Extension, Json};
    require_operator(actor)?;
    let name = need(p, "LaunchTemplateName")?;
    let project = default_project(state, p).await?;
    let data = parse_data(p)?;
    tagspec::only_types(p, &["launch-template"])?;
    // built from `VirtualMachine::new` so it carries the storage volume and API version the validator requires
    let vm = super::fleet::template_vm(&name, data.get("ImageId").map(String::as_str).unwrap_or_default());
    let body: crate::api::cloud::elastic::CreateTemplate =
        serde_json::from_value(serde_json::json!({ "name": name, "vm": vm })).map_err(|e| bad("InvalidParameterValue", e.to_string()))?;
    let Json(row) = crate::api::cloud::elastic::create_template(State(state.clone()), Extension(actor.clone()), Path(project), Json(body))
        .await
        .map_err(super::more::api_err)?;
    crate::db::query("INSERT INTO ec2_launch_template_versions (template_id, version, description, data_json) VALUES (?, 1, ?, ?)")
        .bind(row.id)
        .bind(p.get("VersionDescription").cloned().unwrap_or_default())
        .bind(serde_json::to_string(&data).unwrap_or_else(|_| "{}".into()))
        .execute(&state.pool)
        .await?;
    tagspec::apply(state, Kind::LaunchTemplate, row.id, &tagspec::tags_for(p, "launch-template")).await?;
    let t = find(state, &BTreeMap::from([("LaunchTemplateId".to_string(), ec2_id(Kind::LaunchTemplate, row.id))])).await?;
    template_xml(state, &t, &actor.username, "launchTemplate").await
}

pub async fn create_launch_template_version(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let t = find(state, p).await?;
    let mut data = parse_data(p)?;
    if let Some(src) = p.get("SourceVersion") {
        let n = select_version(state, &t, src).await?;
        let base = load_version(state, &t, n).await?.map(|v| v.data).unwrap_or_default();
        // the new data replaces whole families of the source, as in EC2
        data = merge_under(&data, &base);
    } else if data.is_empty() {
        return Err(bad("MissingParameter", "LaunchTemplateData is required"));
    }
    super::run_options::parse(&data)?;
    let next = latest_version(state, t.id).await? + 1;
    // a template that predates versioning gets its version 1 row first, so it keeps its image
    if load_version(state, &t, 1).await?.is_some()
        && crate::db::query_scalar::<_, i64>("SELECT COUNT(*) FROM ec2_launch_template_versions WHERE template_id = ?").bind(t.id).fetch_one(&state.pool).await? == 0
    {
        crate::db::query("INSERT INTO ec2_launch_template_versions (template_id, version, description, data_json) VALUES (?, 1, '', ?)")
            .bind(t.id)
            .bind(serde_json::to_string(&legacy_data(&t.spec_json)).unwrap_or_else(|_| "{}".into()))
            .execute(&state.pool)
            .await?;
    }
    let description = p.get("VersionDescription").cloned().unwrap_or_default();
    crate::db::query("INSERT INTO ec2_launch_template_versions (template_id, version, description, data_json) VALUES (?, ?, ?, ?)")
        .bind(t.id)
        .bind(next)
        .bind(&description)
        .bind(serde_json::to_string(&data).unwrap_or_else(|_| "{}".into()))
        .execute(&state.pool)
        .await?;
    let v = load_version(state, &t, next).await?.ok_or_else(|| Ec2Error::bad("InternalError", "version was not stored"))?;
    Ok(format!("<launchTemplateVersion>{}</launchTemplateVersion>", version_xml(&t, &v, &actor.username).await))
}

pub async fn modify_launch_template(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let mut t = find(state, p).await?;
    if let Some(d) = p.get("DefaultVersion") {
        let n = select_version(state, &t, d).await?;
        crate::db::query("UPDATE cloud_launch_templates SET default_version = ? WHERE id = ?").bind(n).bind(t.id).execute(&state.pool).await?;
        t.default_version = n;
    } else {
        return Err(bad("MissingParameter", "DefaultVersion is required"));
    }
    template_xml(state, &t, &actor.username, "launchTemplate").await
}

pub async fn describe_launch_templates(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let project = p.get("ProjectId").and_then(|s| Uuid::parse_str(s).ok());
    let rows: Vec<(Uuid, String, Uuid, String, i64, String)> = match project {
        Some(project) => {
            crate::db::query_as("SELECT id, name, project_id, created_at, default_version, spec_json FROM cloud_launch_templates WHERE project_id = ? ORDER BY name")
                .bind(project)
                .fetch_all(&state.pool)
                .await?
        }
        None => crate::db::query_as("SELECT id, name, project_id, created_at, default_version, spec_json FROM cloud_launch_templates ORDER BY name")
            .fetch_all(&state.pool)
            .await?,
    };
    let ids = indexed(p, "LaunchTemplateId");
    let names = indexed(p, "LaunchTemplateName");
    for w in &ids {
        if !rows.iter().any(|r| &ec2_id(Kind::LaunchTemplate, r.0) == w) {
            return Err(bad("InvalidLaunchTemplateId.NotFound", format!("The launch template '{w}' does not exist")));
        }
    }
    for w in &names {
        if !rows.iter().any(|r| &r.1 == w) {
            return Err(bad("InvalidLaunchTemplateName.NotFoundException", format!("The launch template '{w}' does not exist")));
        }
    }
    let mut items = String::new();
    for (id, name, project, created_at, default_version, spec_json) in rows {
        if (!ids.is_empty() && !ids.contains(&ec2_id(Kind::LaunchTemplate, id))) || (!names.is_empty() && !names.contains(&name)) {
            continue;
        }
        let t = Template { id, name, project, created_at, default_version, spec_json };
        items.push_str(&template_xml(state, &t, &actor.username, "item").await?);
    }
    Ok(format!("<launchTemplates>{items}</launchTemplates>"))
}

pub async fn describe_launch_template_versions(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let t = find(state, p).await?;
    let latest = latest_version(state, t.id).await?;
    let mut wanted: Vec<i64> = Vec::new();
    for sel in indexed(p, "LaunchTemplateVersion") {
        wanted.push(select_version(state, &t, &sel).await?);
    }
    let min: i64 = p.get("MinVersion").map(|v| v.parse().unwrap_or(1)).unwrap_or(1);
    let max: i64 = p.get("MaxVersion").map(|v| v.parse().unwrap_or(latest)).unwrap_or(latest);
    let mut items = String::new();
    for n in 1..=latest {
        if (!wanted.is_empty() && !wanted.contains(&n)) || n < min || n > max {
            continue;
        }
        if let Some(v) = load_version(state, &t, n).await? {
            items.push_str(&format!("<item>{}</item>", version_xml(&t, &v, &actor.username).await));
        }
    }
    Ok(format!("<launchTemplateVersionSet>{items}</launchTemplateVersionSet>"))
}

/// `GetLaunchTemplateData`: the template data that would reproduce a running instance.
pub async fn get_launch_template_data(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let want = need(p, "InstanceId")?;
    let id = resolve(state, Kind::Vm, &want, "InvalidInstanceID.NotFound").await?;
    let mut d = Params::new();
    let spec: Option<(Option<String>, Option<String>)> = crate::db::query_as(
        "SELECT json_extract(v.spec_json, '$.spec.template_ref'), f.name FROM vms v LEFT JOIN flavors f ON f.id = v.flavor_id WHERE v.id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    if let Some((image, flavor)) = spec {
        if let Some(name) = image.filter(|i| !i.is_empty()) {
            let tid: Option<Uuid> = crate::db::query_scalar("SELECT id FROM templates WHERE name = ? ORDER BY version LIMIT 1").bind(&name).fetch_optional(&state.pool).await?;
            d.insert("ImageId".into(), tid.map(|t| ec2_id(Kind::Image, t)).unwrap_or(name));
        }
        if let Some(f) = flavor {
            d.insert("InstanceType".into(), f);
        }
    }
    let groups: Vec<Uuid> = crate::db::query_scalar("SELECT sg_id FROM instance_security_groups WHERE vm_id = ? ORDER BY sg_id").bind(id).fetch_all(&state.pool).await?;
    for (n, g) in groups.iter().enumerate() {
        d.insert(format!("SecurityGroupId.{}", n + 1), ec2_id(Kind::SecurityGroup, *g));
    }
    let a = super::instance_attrs::load(state, id).await?;
    d.insert("Monitoring.Enabled".into(), a.monitoring.to_string());
    d.insert("MetadataOptions.HttpEndpoint".into(), if a.metadata_endpoint { "enabled" } else { "disabled" }.into());
    d.insert("MetadataOptions.HttpPutResponseHopLimit".into(), a.metadata_hop_limit.to_string());
    if let Some(g) = a.placement_group {
        d.insert("Placement.GroupName".into(), g);
    }
    Ok(data_xml(&d))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pairs: &[(&str, &str)]) -> Params {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn data_is_flattened_validated_and_unsupported_members_refused() {
        let d = parse_data(&p(&[
            ("LaunchTemplateName", "web"),
            ("LaunchTemplateData.ImageId", "ami-1"),
            ("LaunchTemplateData.InstanceType", "m1.small"),
            ("LaunchTemplateData.SecurityGroupId.1", "sg-1"),
            ("LaunchTemplateData.BlockDeviceMapping.1.DeviceName", "/dev/sdb"),
            ("LaunchTemplateData.BlockDeviceMapping.1.Ebs.VolumeSize", "20"),
        ]))
        .unwrap();
        assert_eq!(d.get("InstanceType").map(String::as_str), Some("m1.small"));
        assert_eq!(d.get("BlockDeviceMapping.1.Ebs.VolumeSize").map(String::as_str), Some("20"));
        assert!(!d.keys().any(|k| k.starts_with("LaunchTemplateData")));
        for (k, v) in [
            ("LaunchTemplateData.IamInstanceProfile.Name", "r"),
            ("LaunchTemplateData.NetworkInterface.1.DeviceIndex", "0"),
            ("LaunchTemplateData.CpuOptions.CoreCount", "2"),
        ] {
            assert_eq!(parse_data(&p(&[(k, v)])).unwrap_err().code, "UnsupportedOperation", "{k}");
        }
        // run-option validation applies to the data too
        assert_eq!(parse_data(&p(&[("LaunchTemplateData.MetadataOptions.HttpTokens", "required")])).unwrap_err().code, "UnsupportedOperation");
    }

    #[test]
    fn the_older_top_level_image_still_works() {
        let d = parse_data(&p(&[("LaunchTemplateName", "web"), ("ImageId", "ami-9")])).unwrap();
        assert_eq!(d.get("ImageId").map(String::as_str), Some("ami-9"));
    }

    #[test]
    fn a_request_family_replaces_the_templates_whole_family() {
        let tpl = p(&[("InstanceType", "m1.small"), ("SecurityGroupId.1", "sg-t"), ("SecurityGroupId.2", "sg-u"), ("KeyName", "k")]);
        let req = p(&[("InstanceType", "m1.large"), ("SecurityGroupId.1", "sg-r")]);
        let m = merge_under(&req, &tpl);
        assert_eq!(m.get("InstanceType").map(String::as_str), Some("m1.large"));
        assert_eq!(m.get("SecurityGroupId.1").map(String::as_str), Some("sg-r"));
        assert!(!m.contains_key("SecurityGroupId.2"), "the template's second group must not leak in");
        assert_eq!(m.get("KeyName").map(String::as_str), Some("k"));
    }

    #[test]
    fn data_xml_round_trips_the_fields_terraform_reads() {
        let d = p(&[
            ("ImageId", "ami-1"),
            ("InstanceType", "m1.small"),
            ("SecurityGroupId.1", "sg-1"),
            ("BlockDeviceMapping.1.DeviceName", "/dev/sdb"),
            ("BlockDeviceMapping.1.Ebs.VolumeSize", "20"),
            ("Monitoring.Enabled", "true"),
            ("Placement.GroupName", "pg1"),
            ("MetadataOptions.HttpEndpoint", "enabled"),
            ("TagSpecification.1.ResourceType", "instance"),
            ("TagSpecification.1.Tag.1.Key", "Env"),
            ("TagSpecification.1.Tag.1.Value", "prod"),
        ]);
        let x = data_xml(&d);
        for want in [
            "<imageId>ami-1</imageId>",
            "<securityGroupIdSet><item>sg-1</item></securityGroupIdSet>",
            "<deviceName>/dev/sdb</deviceName><ebs><volumeSize>20</volumeSize></ebs>",
            "<monitoring><enabled>true</enabled></monitoring>",
            "<placement><groupName>pg1</groupName></placement>",
            "<httpEndpoint>enabled</httpEndpoint>",
            "<resourceType>instance</resourceType><tagSet><item><key>Env</key><value>prod</value></item></tagSet>",
        ] {
            assert!(x.contains(want), "{want} missing from {x}");
        }
    }

    #[test]
    fn legacy_data_is_the_image() {
        let d = legacy_data(r#"{"spec":{"template_ref":"ubuntu"}}"#);
        assert_eq!(d.get("ImageId").map(String::as_str), Some("ubuntu"));
        assert!(legacy_data("{}").is_empty());
    }
}
