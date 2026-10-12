// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! EC2-compatible Query API (`POST /ec2`), signed with AWS SigV4 so awscli / boto3 work with `--endpoint-url`.
//!
//! v1 actions: DescribeInstances, DescribeInstanceTypes, DescribeTags, DescribeKeyPairs, StartInstances,
//! StopInstances, RunInstances, TerminateInstances, CreateTags, DeleteTags. Everything else answers `UnsupportedOperation`. Access keys are managed under
//! `/api/v1/ec2/access-keys` (admin).

pub mod addresses;
pub mod autoscaling;
pub mod capacity;
pub mod eni;
pub mod elbv2;
pub mod elbv2_model;
pub mod fleet;
pub mod foundation;
pub mod gameday;
pub mod gateways;
pub mod groups;
pub mod images;
pub mod images_ext;
pub mod instance_attrs;
pub mod launch_templates;
pub mod lb_members;
pub mod machina;
pub mod monitoring;
pub mod more;
pub mod nacls;
pub mod netcommon;
pub mod ops;
pub mod page;
pub mod peering;
pub mod placement_groups;
pub mod platform;
pub mod route_tables;
pub mod run_options;
pub mod schedules;
pub mod services;
pub mod sg_rules;
pub mod sigv4;
pub mod spot;
pub mod status;
pub mod tagspec;
pub mod volume_attrs;
pub mod volumes_ext;
pub mod vpc;

use std::collections::BTreeMap;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::Serialize;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::resource_ids::{ec2_id, Kind};
use crate::state::AppState;

const XMLNS: &str = "http://ec2.amazonaws.com/doc/2016-11-15/";
const OWNER: &str = "000000000000";

// ---- errors ---------------------------------------------------------------

#[derive(Debug)]
pub struct Ec2Error {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl Ec2Error {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self { status, code, message: message.into() }
    }
    fn bad(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }
}

impl From<sqlx::Error> for Ec2Error {
    fn from(e: sqlx::Error) -> Self {
        tracing::warn!("ec2 api database error: {e}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", "internal error")
    }
}

impl From<ApiError> for Ec2Error {
    fn from(e: ApiError) -> Self {
        Self::new(e.status, "UnauthorizedOperation", e.message)
    }
}

pub(crate) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

fn error_xml(e: &Ec2Error, request_id: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Response><Errors><Error><Code>{}</Code><Message>{}</Message></Error></Errors><RequestID>{request_id}</RequestID></Response>",
        e.code,
        xml_escape(&e.message)
    )
}

fn xml_response(action: &str, request_id: &str, body: &str) -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><{action}Response xmlns=\"{XMLNS}\"><requestId>{request_id}</requestId>{body}</{action}Response>")
}

// ---- pure helpers ----------------------------------------------------------

/// libvirt-style `observed_state` → EC2 (code, name).
pub(crate) fn instance_state(observed: &str) -> (u16, &'static str) {
    match observed {
        "running" | "blocked" => (16, "running"),
        "shutoff" | "stopped" | "paused" | "suspended" | "pmsuspended" | "crashed" => (80, "stopped"),
        "shutdown" => (64, "stopping"),
        "terminated" => (48, "terminated"),
        _ => (0, "pending"),
    }
}

/// `Filter.N.Name` / `Filter.N.Value.M` → (name, values).
pub(crate) fn parse_filters(p: &BTreeMap<String, String>) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for n in 1..=50 {
        let Some(name) = p.get(&format!("Filter.{n}.Name")) else { break };
        let values: Vec<String> = (1..=50).map_while(|m| p.get(&format!("Filter.{n}.Value.{m}")).cloned()).collect();
        out.push((name.clone(), values));
    }
    out
}

/// `{prefix}.1`, `{prefix}.2`, … values.
pub(crate) fn indexed(p: &BTreeMap<String, String>, prefix: &str) -> Vec<String> {
    (1..=200).map_while(|n| p.get(&format!("{prefix}.{n}")).cloned()).collect()
}

pub(crate) fn parse_form(body: &str) -> BTreeMap<String, String> {
    url::form_urlencoded::parse(body.as_bytes()).map(|(k, v)| (k.into_owned(), v.into_owned())).collect()
}

struct Inst {
    id: Uuid,
    name: String,
    state: String,
    vcpus: i64,
    memory_mib: i64,
    flavor: Option<String>,
    ip: Option<String>,
    created: String,
    tags: Vec<(String, String)>,
    /// Why the launch task (`vm.apply`) failed. Set only while the host still reports no state for the VM; such an
    /// instance is shown as `terminated` instead of `pending` forever.
    launch_failure: Option<String>,
}

impl Inst {
    fn eid(&self) -> String {
        ec2_id(Kind::Vm, self.id)
    }
    fn itype(&self) -> String {
        self.flavor.clone().unwrap_or_else(|| format!("custom.{}x{}", self.vcpus, self.memory_mib))
    }
}

fn matches_filter(i: &Inst, name: &str, values: &[String]) -> bool {
    let any = |v: &str| foundation::any_match(values, v);
    if let Some(r) = foundation::tag_filter_matches(&i.tags, name, values) {
        return r;
    }
    match name {
        "instance-id" => any(&i.eid()),
        "instance-state-name" => any(instance_state(&i.state).1),
        "instance-type" => any(&i.itype()),
        "private-ip-address" => i.ip.as_deref().is_some_and(any),
        // Unknown filters are refused before the action runs (`foundation::validate_filters`); one that gets here matches nothing.
        _ => false,
    }
}

/// One instance as an `<item>` of an `instancesSet`.
fn instance_core(i: &Inst) -> String {
    let (code, name) = instance_state(&i.state);
    let tags: String = i
        .tags
        .iter()
        .map(|(k, v)| format!("<item><key>{}</key><value>{}</value></item>", xml_escape(k), xml_escape(v)))
        .collect();
    let ip = i.ip.as_deref().map(|a| format!("<privateIpAddress>{}</privateIpAddress>", xml_escape(a))).unwrap_or_default();
    let reason = i.launch_failure.as_deref().map(status::state_reason_xml).unwrap_or_default();
    format!(
        "<item><instanceId>{}</instanceId><imageId/><instanceState><code>{code}</code><name>{name}</name></instanceState><privateDnsName>{}</privateDnsName>{ip}{reason}<instanceType>{}</instanceType><launchTime>{}</launchTime><tagSet>{tags}</tagSet></item>",
        i.eid(),
        xml_escape(&i.name),
        xml_escape(&i.itype()),
        xml_escape(&i.created),
    )
}

/// One instance as a one-instance reservation (DescribeInstances).
fn instance_item(i: &Inst) -> String {
    format!(
        "<item><reservationId>r-{}</reservationId><ownerId>{OWNER}</ownerId><groupSet/><instancesSet>{}</instancesSet></item>",
        &i.id.simple().to_string()[..17],
        instance_core(i)
    )
}

async fn load_instances(state: &AppState) -> Result<Vec<Inst>, Ec2Error> {
    type Row = (Uuid, String, String, i64, i64, Option<String>, Option<String>, String);
    let rows: Vec<Row> = crate::db::query_as(
        "SELECT v.id, v.name, v.observed_state, v.vcpus, v.memory_mib, f.name, v.guest_ip, v.created_at \
         FROM vms v LEFT JOIN flavors f ON f.id = v.flavor_id WHERE COALESCE(v.inventory_source, 'libvirt') != 'kubevirt' ORDER BY v.name",
    )
    .fetch_all(&state.pool)
    .await?;
    // Terminated instances stay visible for an hour; then the tombstone and its tags go.
    let _ = crate::db::query(
        "DELETE FROM resource_tags WHERE resource_type = 'vm' AND resource_id IN \
         (SELECT lower(hex(id)) FROM terminated_instances WHERE terminated_at < datetime('now', '-1 hour'))",
    )
    .execute(&state.pool)
    .await;
    let _ = crate::db::query("DELETE FROM terminated_instances WHERE terminated_at < datetime('now', '-1 hour')")
        .execute(&state.pool)
        .await;
    let gone: Vec<(Uuid, String, Option<String>, i64, i64, String)> = crate::db::query_as(
        "SELECT id, name, instance_type, vcpus, memory_mib, terminated_at FROM terminated_instances \
         WHERE id NOT IN (SELECT id FROM vms) ORDER BY name",
    )
    .fetch_all(&state.pool)
    .await?;
    let tags: Vec<(String, String, String)> =
        crate::db::query_as("SELECT resource_id, key, value FROM resource_tags WHERE resource_type = 'vm'")
            .fetch_all(&state.pool)
            .await?;
    let failures = status::failed_launches(state).await?;
    let mut out: Vec<Inst> = rows
        .into_iter()
        .map(|(id, name, mut state, vcpus, memory_mib, flavor, ip, created)| {
            let launch_failure = (instance_state(&state).0 == 0).then(|| failures.get(&id).cloned()).flatten();
            if launch_failure.is_some() {
                state = "terminated".into();
            }
            Inst {
            launch_failure,
            tags: tags.iter().filter(|(r, _, _)| *r == id.simple().to_string()).map(|(_, k, v)| (k.clone(), v.clone())).collect(),
            id,
            name,
            state,
            vcpus,
            memory_mib,
            flavor,
            ip,
            created,
            }
        })
        .collect();
    out.extend(gone.into_iter().map(|(id, name, flavor, vcpus, memory_mib, at)| Inst {
        tags: tags.iter().filter(|(r, _, _)| *r == id.simple().to_string()).map(|(_, k, v)| (k.clone(), v.clone())).collect(),
        id,
        name,
        state: "terminated".into(),
        vcpus,
        memory_mib,
        flavor,
        ip: None,
        created: at,
        launch_failure: None,
    }));
    Ok(out)
}

// ---- actions ---------------------------------------------------------------

async fn describe_instances(state: &AppState, p: &BTreeMap<String, String>) -> Result<String, Ec2Error> {
    let wanted = indexed(p, "InstanceId");
    let filters = parse_filters(p);
    let all = load_instances(state).await?;
    for w in &wanted {
        if !all.iter().any(|i| &i.eid() == w) {
            return Err(Ec2Error::bad("InvalidInstanceID.NotFound", format!("The instance ID '{w}' does not exist")));
        }
    }
    let mut items = String::new();
    for i in all
        .iter()
        .filter(|i| wanted.is_empty() || wanted.contains(&i.eid()))
        .filter(|i| filters.iter().all(|(n, v)| matches_filter(i, n, v)))
    {
        let extra = status::instance_extra(state, i.id).await?;
        items.push_str(&status::splice(&instance_item(i), &extra));
    }
    Ok(format!("<reservationSet>{items}</reservationSet>"))
}

async fn describe_instance_types(state: &AppState, p: &BTreeMap<String, String>) -> Result<String, Ec2Error> {
    let wanted = indexed(p, "InstanceType");
    let rows: Vec<(String, i64, i64)> = crate::db::query_as("SELECT name, vcpus, memory_mib FROM flavors ORDER BY name")
        .fetch_all(&state.pool)
        .await?;
    let items: String = rows
        .iter()
        .filter(|(n, _, _)| wanted.is_empty() || wanted.contains(n))
        .map(|(n, v, m)| {
            format!(
                "<item><instanceType>{}</instanceType><currentGeneration>true</currentGeneration><freeTierEligible>false</freeTierEligible>\
<supportedUsageClasses><item>on-demand</item><item>spot</item></supportedUsageClasses><supportedRootDeviceTypes><item>ebs</item></supportedRootDeviceTypes>\
<supportedVirtualizationTypes><item>hvm</item></supportedVirtualizationTypes><bareMetal>false</bareMetal><hypervisor>kvm</hypervisor>\
<instanceStorageSupported>false</instanceStorageSupported><burstablePerformanceSupported>false</burstablePerformanceSupported>\
<vCpuInfo><defaultVCpus>{v}</defaultVCpus><defaultCores>{v}</defaultCores><defaultThreadsPerCore>1</defaultThreadsPerCore></vCpuInfo>\
<memoryInfo><sizeInMiB>{m}</sizeInMiB></memoryInfo><ebsInfo><ebsOptimizedSupport>unsupported</ebsOptimizedSupport></ebsInfo></item>",
                xml_escape(n)
            )
        })
        .collect();
    Ok(format!("<instanceTypeSet>{items}</instanceTypeSet>"))
}

async fn describe_tags(state: &AppState, p: &BTreeMap<String, String>) -> Result<String, Ec2Error> {
    let filters = parse_filters(p);
    let rows: Vec<(String, String, String, String)> =
        crate::db::query_as("SELECT resource_type, resource_id, key, value FROM resource_tags ORDER BY resource_type, resource_id, key")
            .fetch_all(&state.pool)
            .await?;
    let mut items = String::new();
    for (rt, rid, k, v) in rows {
        let kind = Kind::from_type(&rt);
        let id = Uuid::parse_str(&rid).map(|u| u.simple().to_string()).unwrap_or(rid.clone());
        let ec2 = kind.map(|k| format!("{}-{}", k.prefix(), &id[..17.min(id.len())])).unwrap_or(id);
        let rtype = foundation::ec2_resource_type(&rt);
        // the tag filters look at this one tag; `tag:K` / `tag-key` / `tag-value` select by its own key and value
        let this = [(k.clone(), v.clone())];
        let ok = filters.iter().all(|(n, vals)| match n.as_str() {
            "key" => foundation::any_match(vals, &k),
            "value" => foundation::any_match(vals, &v),
            "resource-type" => foundation::any_match(vals, &rtype),
            "resource-id" => foundation::any_match(vals, &ec2),
            _ => foundation::tag_filter_matches(&this, n, vals).unwrap_or(false),
        });
        if !ok {
            continue;
        }
        items.push_str(&format!(
            "<item><resourceId>{ec2}</resourceId><resourceType>{rtype}</resourceType><key>{}</key><value>{}</value></item>",
            xml_escape(&k),
            xml_escape(&v)
        ));
    }
    Ok(format!("<tagSet>{items}</tagSet>"))
}

async fn describe_key_pairs(state: &AppState, p: &BTreeMap<String, String>) -> Result<String, Ec2Error> {
    let wanted = indexed(p, "KeyName");
    let rows: Vec<(Uuid, String, String)> = crate::db::query_as("SELECT id, name, fingerprint FROM keypairs ORDER BY name")
        .fetch_all(&state.pool)
        .await?;
    let items: String = rows
        .iter()
        .filter(|(_, n, _)| wanted.is_empty() || wanted.contains(n))
        .map(|(id, n, f)| {
            format!(
                "<item><keyPairId>{}</keyPairId><keyName>{}</keyName><keyFingerprint>{}</keyFingerprint></item>",
                ec2_id(Kind::KeyPair, *id),
                xml_escape(n),
                xml_escape(f)
            )
        })
        .collect();
    Ok(format!("<keySet>{items}</keySet>"))
}

async fn power(state: &AppState, actor: &AuthUser, p: &BTreeMap<String, String>, start: bool) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let ids = indexed(p, "InstanceId");
    if ids.is_empty() {
        return Err(Ec2Error::bad("MissingParameter", "The request must contain the parameter InstanceId"));
    }
    let all = load_instances(state).await?;
    // termination protection is checked for every instance before any is deleted
    for want in &ids {
        if let Some(i) = all.iter().find(|i| &i.eid() == want && i.state != "terminated") {
            instance_attrs::terminate_guard(state, i.id, want).await?;
        }
    }
    let mut items = String::new();
    for want in &ids {
        let i = all
            .iter()
            .find(|i| &i.eid() == want)
            .ok_or_else(|| Ec2Error::bad("InvalidInstanceID.NotFound", format!("The instance ID '{want}' does not exist")))?;
        let (pc, pn) = instance_state(&i.state);
        if let Some(cause) = &i.launch_failure {
            return Err(Ec2Error::bad("IncorrectInstanceState", status::launch_failed_message(want, cause)));
        }
        if i.state == "terminated" {
            return Err(Ec2Error::bad("IncorrectInstanceState", format!("The instance '{want}' is terminated")));
        }
        let r = if start {
            crate::api::vms::start_vm(State(state.clone()), Extension(actor.clone()), Path(i.id)).await
        } else {
            crate::api::vms::stop_vm(State(state.clone()), Extension(actor.clone()), Path(i.id)).await
        };
        let _ = r?;
        let (nc, nn) = if start { (0, "pending") } else { (64, "stopping") };
        items.push_str(&format!(
            "<item><instanceId>{want}</instanceId><currentState><code>{nc}</code><name>{nn}</name></currentState><previousState><code>{pc}</code><name>{pn}</name></previousState></item>"
        ));
    }
    Ok(format!("<instancesSet>{items}</instancesSet>"))
}

/// `Tag.N.Key/Value` and `TagSpecification.N.Tag.M.Key/Value` (instance specs only) → tags.
pub(crate) fn parse_tags(p: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut add = |kp: String, vp: String| {
        if let Some(k) = p.get(&kp) {
            out.insert(k.clone(), p.get(&vp).cloned().unwrap_or_default());
        }
    };
    for n in 1..=50 {
        add(format!("Tag.{n}.Key"), format!("Tag.{n}.Value"));
    }
    for sidx in 1..=5 {
        let rt = p.get(&format!("TagSpecification.{sidx}.ResourceType")).map(String::as_str);
        if rt.is_some_and(|r| r != "instance") {
            continue;
        }
        for n in 1..=50 {
            add(format!("TagSpecification.{sidx}.Tag.{n}.Key"), format!("TagSpecification.{sidx}.Tag.{n}.Value"));
        }
    }
    out
}

/// `UserData` is base64 in the EC2 API.
pub(crate) fn decode_user_data(b64: &str) -> Result<String, String> {
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|_| "UserData must be base64".to_string())?;
    String::from_utf8(raw).map_err(|_| "UserData must be UTF-8 text".to_string())
}

async fn wait_for_vm(state: &AppState, name: &str) -> Option<Uuid> {
    for _ in 0..40 {
        if let Ok(Some(id)) = crate::db::query_scalar::<_, Uuid>("SELECT id FROM vms WHERE name = ?")
            .bind(name)
            .fetch_optional(&state.pool)
            .await
        {
            return Some(id);
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    None
}

/// Wait until the instance's domain exists, so disks can be attached to it.
async fn wait_until_defined(state: &AppState, id: Uuid) -> bool {
    for _ in 0..60 {
        let st: Option<String> = crate::db::query_scalar("SELECT observed_state FROM vms WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
        if st.as_deref().is_some_and(|s| matches!(s, "running" | "shutoff" | "stopped" | "paused")) {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    false
}

/// `BlockDeviceMapping` extra volumes: create, tag and attach each to a new instance.
async fn attach_extra_volumes(state: &AppState, actor: &AuthUser, vm: Uuid, vm_name: &str, opts: &run_options::RunOptions) -> Result<(), Ec2Error> {
    if opts.volumes.is_empty() {
        return Ok(());
    }
    if !wait_until_defined(state, vm).await {
        return Err(Ec2Error::new(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", format!("instance {vm_name} did not become ready to attach its volumes")));
    }
    for v in &opts.volumes {
        let body: crate::api::volumes::CreateVolumeBody = serde_json::from_value(serde_json::json!({
            "name": format!("{vm_name}-{}", v.device), "size_gib": v.size_gib, "delete_on_termination": v.delete_on_termination
        }))
        .map_err(|e| Ec2Error::bad("InvalidParameterValue", e.to_string()))?;
        let Json(row) = crate::api::volumes::create_volume(State(state.clone()), Extension(actor.clone()), Json(body)).await.map_err(more::api_err)?;
        if !opts.volume_tags.is_empty() {
            let mut tx = state.pool.begin().await?;
            let _ = crate::api::tags::put_tag_map(&mut tx, Kind::Volume, row.id, &opts.volume_tags).await?;
            tx.commit().await?;
        }
        let attach: crate::api::volumes::AttachVolumeBody = serde_json::from_value(
            serde_json::json!({ "vm_id": vm, "target_dev": v.device, "delete_on_termination": v.delete_on_termination }),
        )
        .map_err(|e| Ec2Error::bad("InvalidParameterValue", e.to_string()))?;
        let _ = crate::api::volumes::attach_volume(State(state.clone()), Extension(actor.clone()), Path(row.id), Json(attach)).await.map_err(more::api_err)?;
    }
    Ok(())
}

/// Launches the instances of a `RunInstances` call (a launch template merged in, every option applied or refused) and
/// returns their ids. Also used by spot requests and fleets.
pub(crate) async fn launch(state: &AppState, actor: &AuthUser, params: &BTreeMap<String, String>) -> Result<Vec<Uuid>, Ec2Error> {
    require_operator(actor)?;
    // a launch template supplies defaults; every option is then applied or refused (see `run_options`)
    let merged = run_options::with_launch_template(state, params).await?;
    let p = &merged;
    let opts = run_options::parse(p)?;
    let group = placement_groups::group_for_run(state, p).await?;
    let host_id = run_options::host_for_zone(state, p).await?;
    if group.is_some() && host_id.is_some() {
        return Err(Ec2Error::bad("InvalidParameterCombination", "give Placement.AvailabilityZone or Placement.GroupName, not both"));
    }
    let need = |k: &str| p.get(k).cloned().ok_or_else(|| Ec2Error::bad("MissingParameter", format!("The request must contain the parameter {k}")));
    let image = need("ImageId")?;
    let max: u32 = need("MaxCount")?.parse().map_err(|_| Ec2Error::bad("InvalidParameterValue", "MaxCount must be a number"))?;
    let min: u32 = p.get("MinCount").map_or(Ok(max), |v| v.parse()).map_err(|_| Ec2Error::bad("InvalidParameterValue", "MinCount must be a number"))?;

    // ImageId: an ami- id, or an image (template) name.
    let template_ref: String = if let Some((Kind::Image, hex)) = crate::resource_ids::parse(&image) {
        let mut conn = state.pool.acquire().await?;
        match crate::resource_ids::resolve(&mut conn, Kind::Image, &hex).await? {
            crate::resource_ids::Lookup::Found(id) => crate::db::query_scalar("SELECT name FROM templates WHERE id = ?").bind(id).fetch_one(&mut *conn).await?,
            _ => return Err(Ec2Error::bad("InvalidAMIID.NotFound", format!("The image id '{image}' does not exist"))),
        }
    } else {
        image.clone()
    };
    let flavor: Option<Uuid> = match p.get("InstanceType") {
        Some(t) => Some(
            crate::db::query_scalar("SELECT id FROM flavors WHERE name = ?")
                .bind(t)
                .fetch_optional(&state.pool)
                .await?
                .ok_or_else(|| Ec2Error::bad("InvalidParameterValue", format!("Unknown instance type '{t}'")))?,
        ),
        None => None,
    };
    let tags = parse_tags(p);
    let name = tags.get("Name").cloned().unwrap_or_else(|| format!("ec2-{}", &Uuid::new_v4().simple().to_string()[..8]));
    let mut base = serde_json::json!({ "template_ref": template_ref });
    if let Some(f) = flavor {
        base["flavor_id"] = serde_json::json!(f);
    }
    if opts.preemptible {
        base["preemptible"] = serde_json::json!(true);
    }
    // SubnetId: launch on the network the subnet's host defined (`mc-<subnet uuid>`), on that subnet's VPC host
    let subnet = fleet::subnet_for_run(state, p).await?;
    if let Some(t) = &subnet {
        if group.is_some() {
            return Err(Ec2Error::bad("InvalidParameterCombination", "a subnet's network exists on one host: give SubnetId or Placement.GroupName, not both"));
        }
        if host_id.is_some_and(|h| h != t.host_id) {
            return Err(Ec2Error::bad("InvalidParameterCombination", "Placement.AvailabilityZone is not the zone of the subnet"));
        }
        base["network"] = serde_json::json!(t.network);
    }
    let host_id = host_id.or(subnet.as_ref().map(|t| t.host_id));
    if let Some(k) = p.get("KeyName") {
        base["key_name"] = serde_json::json!(k);
    }
    if let Some(u) = p.get("UserData") {
        base["cloud_init_user_data"] = serde_json::json!(decode_user_data(u).map_err(|m| Ec2Error::bad("InvalidParameterValue", m))?);
    }
    // one call for the whole count, or (placement group) one call per instance with the host the group's strategy chose
    let calls: Vec<(String, u32, u32, Option<Uuid>)> = match &group {
        Some(g) => {
            let memory_mib: i64 = match flavor {
                Some(f) => crate::db::query_scalar("SELECT memory_mib FROM flavors WHERE id = ?").bind(f).fetch_one(&state.pool).await?,
                None => 4096,
            };
            let hosts = placement_groups::plan_hosts(state, g, max as usize, memory_mib).await?;
            crate::api::vms::run_names(&name, max).into_iter().zip(hosts).map(|(n, h)| (n, 1, 1, Some(h))).collect()
        }
        None => vec![(name.clone(), max, min, host_id)],
    };
    let mut names: Vec<String> = Vec::new();
    for (call_name, count, min_count, host) in calls {
        let mut spec = base.clone();
        spec["name"] = serde_json::json!(call_name);
        spec["count"] = serde_json::json!(count);
        spec["min_count"] = serde_json::json!(min_count);
        if let Some(h) = host {
            spec["host_id"] = serde_json::json!(h);
        }
        let body: crate::api::vms::RunInstancesBody =
            serde_json::from_value(spec).map_err(|e| Ec2Error::bad("InvalidParameterValue", e.to_string()))?;
        let Json(done) = crate::api::vms::run_instances(State(state.clone()), Extension(actor.clone()), Json(body))
            .await
            .map_err(|e| Ec2Error::bad("InsufficientInstanceCapacity", e.message))?;
        names.extend(done["instances"].as_array().into_iter().flatten().filter_map(|i| i["name"].as_str().map(String::from)));
    }

    let mut launched: Vec<(Uuid, String)> = Vec::new();
    for n in &names {
        if let Some(id) = wait_for_vm(state, n).await {
            if !tags.is_empty() {
                let mut tx = state.pool.begin().await?;
                let _ = crate::api::tags::put_tag_map(&mut tx, Kind::Vm, id, &tags).await?;
                tx.commit().await?;
            }
            // attributes the call set (termination protection, monitoring, metadata endpoint, placement group…)
            let attrs = instance_attrs::Attrs { placement_group: group.as_ref().map(|g| g.name.clone()), ..opts.attrs.clone() };
            if attrs != instance_attrs::Attrs::default() {
                instance_attrs::save(state, id, &attrs).await?;
            }
            launched.push((id, n.clone()));
        }
    }
    let ids: Vec<Uuid> = launched.iter().map(|(id, _)| *id).collect();
    volume_attrs::attach_run_groups(state, actor, p, &ids).await?;
    for (id, n) in &launched {
        attach_extra_volumes(state, actor, *id, n, &opts).await?;
    }
    Ok(ids)
}

async fn run_instances(state: &AppState, actor: &AuthUser, params: &BTreeMap<String, String>) -> Result<String, Ec2Error> {
    let ids = launch(state, actor, params).await?;
    let all = load_instances(state).await?;
    let items: String = all.iter().filter(|i| ids.contains(&i.id)).map(instance_core).collect();
    Ok(format!(
        "<reservationId>r-{}</reservationId><ownerId>{OWNER}</ownerId><groupSet/><instancesSet>{items}</instancesSet>",
        &Uuid::new_v4().simple().to_string()[..17]
    ))
}

async fn terminate(state: &AppState, actor: &AuthUser, p: &BTreeMap<String, String>) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let ids = indexed(p, "InstanceId");
    if ids.is_empty() {
        return Err(Ec2Error::bad("MissingParameter", "The request must contain the parameter InstanceId"));
    }
    let all = load_instances(state).await?;
    let mut items = String::new();
    for want in &ids {
        let i = all
            .iter()
            .find(|i| &i.eid() == want)
            .ok_or_else(|| Ec2Error::bad("InvalidInstanceID.NotFound", format!("The instance ID '{want}' does not exist")))?;
        let (pc, pn) = instance_state(&i.state);
        // A launch that failed is shown as terminated but still has its VM row: terminating it removes that row.
        if i.state == "terminated" && i.launch_failure.is_none() {
            items.push_str(&format!("<item><instanceId>{want}</instanceId><currentState><code>48</code><name>terminated</name></currentState><previousState><code>48</code><name>terminated</name></previousState></item>"));
            continue;
        }
        // No `confirmed`: a cluster that requires approval for deletions keeps requiring it here.
        let _ = crate::api::vms::delete_vm(State(state.clone()), Extension(actor.clone()), Path(i.id), None).await?;
        items.push_str(&format!(
            "<item><instanceId>{want}</instanceId><currentState><code>32</code><name>shutting-down</name></currentState><previousState><code>{pc}</code><name>{pn}</name></previousState></item>"
        ));
    }
    Ok(format!("<instancesSet>{items}</instancesSet>"))
}

async fn tag_resources(state: &AppState, actor: &AuthUser, p: &BTreeMap<String, String>, delete: bool) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let resources = indexed(p, "ResourceId");
    if resources.is_empty() {
        return Err(Ec2Error::bad("MissingParameter", "The request must contain the parameter ResourceId"));
    }
    let tags = parse_tags(p);
    if tags.is_empty() {
        return Err(Ec2Error::bad("MissingParameter", "The request must contain the parameter Tag"));
    }
    if !delete {
        crate::api::tags::validate_tags(&tags).map_err(|m| Ec2Error::bad("InvalidParameterValue", m))?;
    }
    for r in &resources {
        let (kind, hex) = crate::resource_ids::parse(r).ok_or_else(|| Ec2Error::bad("InvalidID", format!("The id '{r}' is not valid")))?;
        let mut tx = crate::db::begin_write(&state.pool).await?;
        let id = match crate::resource_ids::resolve(&mut tx, kind, &hex).await? {
            crate::resource_ids::Lookup::Found(id) => id,
            _ => return Err(Ec2Error::bad("InvalidResourceID.NotFound", format!("The resource '{r}' does not exist"))),
        };
        if delete {
            let keys: Vec<String> = tags.keys().cloned().collect();
            crate::api::tags::delete_tag_keys(&mut tx, kind, id, &keys).await?;
        } else if let crate::api::tags::PutOutcome::TooMany(n) = crate::api::tags::put_tag_map(&mut tx, kind, id, &tags).await? {
            return Err(Ec2Error::bad("TagLimitExceeded", format!("that would give the resource {n} tags")));
        }
        tx.commit().await?;
    }
    Ok("<return>true</return>".into())
}

// ---- the endpoint ----------------------------------------------------------

fn respond(status: StatusCode, xml: String) -> Response {
    (status, [("content-type", "text/xml;charset=UTF-8")], xml).into_response()
}

/// `POST /ec2` — form-encoded `Action=…`, SigV4-signed.
pub async fn query(State(state): State<AppState>, headers: HeaderMap, uri: Uri, body: Bytes) -> Response {
    let request_id = Uuid::new_v4().to_string();
    // The service the request is signed for picks the response and error shape; until the
    // Authorization header is parsed it is ec2's.
    let mut service = services::Service::Ec2;
    match handle(&state, &headers, &uri, &body, &request_id, &mut service).await {
        Ok(xml) => respond(StatusCode::OK, xml),
        Err(e) => respond(e.status, service.error_xml(&e, &request_id)),
    }
}

async fn handle(
    state: &AppState,
    headers: &HeaderMap,
    uri: &Uri,
    body: &Bytes,
    request_id: &str,
    service: &mut services::Service,
) -> Result<String, Ec2Error> {
    let denied = |m: &str| Ec2Error::new(StatusCode::FORBIDDEN, "AuthFailure", m.to_string());
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| denied("missing Authorization header"))?;
    let auth = sigv4::parse_authorization(auth_header).map_err(|_| denied("malformed Authorization header"))?;
    let Some(scope) = services::Service::from_scope(&auth.service) else {
        return Err(denied("the credential scope must be for the ec2, monitoring, autoscaling or elasticloadbalancing service"));
    };
    *service = scope;
    let key: Option<(String, String, String, bool)> = crate::db::query_as(
        "SELECT secret_enc, username, role, revoked FROM ec2_access_keys WHERE access_key_id = ?",
    )
    .bind(&auth.access_key)
    .fetch_optional(&state.pool)
    .await?;
    // One message for unknown, revoked and wrong-secret keys: no oracle for valid access key ids.
    let Some((secret_enc, username, role, false)) = key else {
        return Err(denied("the security token included in the request is invalid"));
    };
    let secret = crate::engine::ai::crypto::load_api_key(&secret_enc)
        .map_err(|_| Ec2Error::new(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", "access key cannot be read"))?;
    let hv: Vec<(String, String)> = headers
        .iter()
        .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().to_string(), v.to_string())))
        .collect();
    let amz_date = headers.get("x-amz-date").and_then(|v| v.to_str().ok()).unwrap_or_default();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    match sigv4::verify(&auth, &secret, "POST", uri.path(), uri.query().unwrap_or(""), &hv, body, amz_date, now) {
        Ok(()) => {}
        Err(sigv4::SigError::Skew) => {
            return Err(Ec2Error::new(StatusCode::FORBIDDEN, "RequestExpired", "the request timestamp is too far from the server time"))
        }
        Err(_) => return Err(denied("the request signature we calculated does not match the signature you provided")),
    }
    let _ = crate::db::query("UPDATE ec2_access_keys SET last_used_at = CURRENT_TIMESTAMP WHERE access_key_id = ?")
        .bind(&auth.access_key)
        .execute(&state.pool)
        .await;

    let actor = AuthUser { username, role, auth_source: Some("ec2-access-key".into()) };
    let params = parse_form(std::str::from_utf8(body).map_err(|_| Ec2Error::bad("MalformedQueryString", "body is not UTF-8"))?);
    let action = params.get("Action").map(String::as_str).unwrap_or_default();
    if scope != services::Service::Ec2 {
        services::check_version(scope, &params)?;
        let inner = services::dispatch_other(scope, state, &actor, &params, action).await?;
        return Ok(scope.response_xml(action, request_id, &inner));
    }
    foundation::dry_run_gate(action, &params, &actor)?;
    foundation::validate_filters(action, &params)?;
    let token = match params.get("ClientToken").filter(|_| foundation::IDEMPOTENT_ACTIONS.contains(&action)) {
        Some(t) => {
            foundation::validate_token(t)?;
            let hash = foundation::request_hash(action, &params);
            match foundation::claim_token(state, &actor.username, action, t, &hash).await? {
                foundation::TokenClaim::Fresh => Some(t.clone()),
                foundation::TokenClaim::Replay(inner) => return Ok(xml_response(action, request_id, &inner)),
                foundation::TokenClaim::Mismatch => return Err(foundation::mismatch_error()),
                foundation::TokenClaim::InFlight => return Err(foundation::in_flight_error()),
            }
        }
        None => None,
    };
    let result = dispatch(state, &actor, &params, action).await;
    let inner = match (result, &token) {
        (Ok(inner), Some(t)) => {
            foundation::finish_token(state, &actor.username, action, t, &inner).await?;
            inner
        }
        (Ok(inner), None) => inner,
        (Err(e), Some(t)) => {
            foundation::release_token(state, &actor.username, action, t).await;
            return Err(e);
        }
        (Err(e), None) => return Err(e),
    };
    let inner = foundation::post_process(action, &params, inner)?;
    Ok(xml_response(action, request_id, &inner))
}

async fn dispatch(state: &AppState, actor: &AuthUser, params: &BTreeMap<String, String>, action: &str) -> Result<String, Ec2Error> {
    let actor = actor.clone();
    let params = params.clone();
    let inner = match action {
        "DescribeInstances" => describe_instances(state, &params).await?,
        "DescribeInstanceTypes" => describe_instance_types(state, &params).await?,
        "DescribeTags" => describe_tags(state, &params).await?,
        // instances, volumes, images, launch templates, placement groups, spot and fleets (see the modules)
        "CreateLaunchTemplateVersion" => launch_templates::create_launch_template_version(state, &actor, &params).await?,
        "ModifyLaunchTemplate" => launch_templates::modify_launch_template(state, &actor, &params).await?,
        "GetLaunchTemplateData" => launch_templates::get_launch_template_data(state, &params).await?,
        "MonitorInstances" => instance_attrs::set_monitoring(state, &actor, &params, true).await?,
        "UnmonitorInstances" => instance_attrs::set_monitoring(state, &actor, &params, false).await?,
        "ModifyInstanceMetadataOptions" => instance_attrs::modify_instance_metadata_options(state, &actor, &params).await?,
        "GetConsoleOutput" => instance_attrs::get_console_output(state, &actor, &params).await?,
        "GetConsoleScreenshot" => instance_attrs::get_console_screenshot()?,
        "GetPasswordData" => instance_attrs::get_password_data(state, &params).await?,
        "DescribeInstanceCreditSpecifications" => instance_attrs::describe_instance_credit_specifications(state, &params).await?,
        "DescribeVolumesModifications" => volumes_ext::describe_volumes_modifications(state, &params).await?,
        "DescribeVolumeStatus" => volumes_ext::describe_volume_status(state, &params).await?,
        "CreateSnapshots" => volumes_ext::create_snapshots(state, &actor, &params).await?,
        "CopySnapshot" => volumes_ext::copy_snapshot()?,
        "CreateKeyPair" => images_ext::create_key_pair(state, &actor, &params).await?,
        "RegisterImage" => images_ext::register_image(state, &actor, &params).await?,
        "CopyImage" => images_ext::copy_image(state, &actor, &params).await?,
        "DescribeImageAttribute" => images_ext::describe_image_attribute(state, &params).await?,
        "DescribeInstanceTypeOfferings" => images_ext::describe_instance_type_offerings(state, &params).await?,
        "CreatePlacementGroup" => placement_groups::create_placement_group(state, &actor, &params).await?,
        "DescribePlacementGroups" => placement_groups::describe_placement_groups(state, &params).await?,
        "DeletePlacementGroup" => placement_groups::delete_placement_group(state, &actor, &params).await?,
        "RequestSpotInstances" => spot::request_spot_instances(state, &actor, &params).await?,
        "DescribeSpotInstanceRequests" => spot::describe_spot_instance_requests(state, &params).await?,
        "CancelSpotInstanceRequests" => spot::cancel_spot_instance_requests(state, &actor, &params).await?,
        "DescribeSpotPriceHistory" => spot::describe_spot_price_history(state, &params).await?,
        "CreateFleet" => spot::create_fleet(state, &actor, &params).await?,
        "DescribeFleets" => spot::describe_fleets(state, &params).await?,
        "DescribeFleetInstances" => spot::describe_fleet_instances(state, &params).await?,
        "DeleteFleets" => spot::delete_fleets(state, &actor, &params).await?,
        "DescribeKeyPairs" => describe_key_pairs(state, &params).await?,
        "StartInstances" => power(state, &actor, &params, true).await?,
        "StopInstances" => power(state, &actor, &params, false).await?,
        "RunInstances" => run_instances(state, &actor, &params).await?,
        "TerminateInstances" => terminate(state, &actor, &params).await?,
        "DescribeVolumes" => more::describe_volumes(state, &params).await?,
        "CreateVolume" if params.contains_key("SnapshotId") => images::create_volume_from_snapshot(state, &actor, &params).await?,
        "CreateVolume" => more::create_volume(state, &actor, &params).await?,
        "DeleteVolume" => more::delete_volume(state, &actor, &params).await?,
        "AttachVolume" => more::attach_volume(state, &actor, &params).await?,
        "DetachVolume" => more::detach_volume(state, &actor, &params).await?,
        "ImportKeyPair" => more::import_key_pair(state, &actor, &params).await?,
        "DeleteKeyPair" => more::delete_key_pair(state, &actor, &params).await?,
        "DescribeSecurityGroups" => more::describe_security_groups(state, &params).await?,
        "CreateSecurityGroup" => more::create_security_group(state, &actor, &params).await?,
        "DeleteSecurityGroup" => more::delete_security_group(state, &actor, &params).await?,
        "AuthorizeSecurityGroupIngress" => more::security_group_rules(state, &actor, &params, false, false).await?,
        "AuthorizeSecurityGroupEgress" => more::security_group_rules(state, &actor, &params, true, false).await?,
        "RevokeSecurityGroupIngress" => more::security_group_rules(state, &actor, &params, false, true).await?,
        "RevokeSecurityGroupEgress" => more::security_group_rules(state, &actor, &params, true, true).await?,
        "DescribeImages" => more::describe_images(state, &params).await?,
        "DescribeVpcs" => more::describe_vpcs(state, &params).await?,
        "DescribeSubnets" => more::describe_subnets(state, &params).await?,
        "DescribeNetworkInterfaces" => more::describe_network_interfaces(state, &params).await?,
        "RebootInstances" => more::reboot_instances(state, &actor, &params).await?,
        "ModifyInstanceAttribute" if params.get("Attribute").is_some_and(|a| a == "preemptible") => {
            machina::modify_preemptible(state, &actor, &params).await?
        }
        "ModifyInstanceAttribute" if params.get("Attribute").is_some_and(|a| a == "groupSet") => {
            groups::modify_group_set(state, &actor, &params).await?
        }
        "DescribeInstanceStatus" => status::describe_instance_status(state, &params).await?,
        "DescribeAlarms" => monitoring::describe_alarms(state, &actor, &params).await?,
        "PutMetricAlarm" => monitoring::put_metric_alarm(state, &actor, &params).await?,
        "DeleteAlarms" => monitoring::delete_alarms(state, &actor, &params).await?,
        "EnableAlarmActions" => monitoring::set_alarm_actions(state, &actor, &params, true).await?,
        "DisableAlarmActions" => monitoring::set_alarm_actions(state, &actor, &params, false).await?,
        "GetMetricStatistics" => monitoring::get_metric_statistics(state, &actor, &params).await?,
        "DescribeInstanceGroups" => groups::describe_instance_groups(state, &actor, &params).await?,
        "UpdateInstanceGroup" => groups::update_instance_group(state, &actor, &params).await?,
        "ModifySubnetAttribute" => groups::modify_subnet_attribute(state, &actor, &params).await?,
        "CreateVpc" => vpc::create_vpc(state, &actor, &params).await?,
        "DeleteVpc" => vpc::delete_vpc(state, &actor, &params).await?,
        "CreateSubnet" => vpc::create_subnet(state, &actor, &params).await?,
        "DeleteSubnet" => vpc::delete_subnet(state, &actor, &params).await?,
        "DescribeRouteTables" => route_tables::describe_route_tables(state, &params).await?,
        "CreateRoute" => route_tables::create_route(state, &actor, &params).await?,
        "DeleteRoute" => route_tables::delete_route(state, &actor, &params).await?,
        "ReplaceRoute" => route_tables::replace_route(state, &actor, &params).await?,
        "CreateRouteTable" => route_tables::create_route_table(state, &actor, &params).await?,
        "DeleteRouteTable" => route_tables::delete_route_table(state, &actor, &params).await?,
        "AssociateRouteTable" => route_tables::associate_route_table(state, &actor, &params).await?,
        "DisassociateRouteTable" => route_tables::disassociate_route_table(state, &actor, &params).await?,
        "ReplaceRouteTableAssociation" => route_tables::replace_route_table_association(state, &actor, &params).await?,
        "CreateInternetGateway" => gateways::create_internet_gateway(state, &actor, &params).await?,
        "DescribeInternetGateways" => gateways::describe_internet_gateways(state, &params).await?,
        "AttachInternetGateway" => gateways::attach_internet_gateway(state, &actor, &params).await?,
        "DetachInternetGateway" => gateways::detach_internet_gateway(state, &actor, &params).await?,
        "DeleteInternetGateway" => gateways::delete_internet_gateway(state, &actor, &params).await?,
        "CreateNatGateway" => gateways::create_nat_gateway(state, &actor, &params).await?,
        "DescribeNatGateways" => gateways::describe_nat_gateways(state, &params).await?,
        "DeleteNatGateway" => gateways::delete_nat_gateway(state, &actor, &params).await?,
        "CreateDhcpOptions" => gateways::create_dhcp_options(state, &actor, &params).await?,
        "DescribeDhcpOptions" => gateways::describe_dhcp_options(state, &params).await?,
        "AssociateDhcpOptions" => gateways::associate_dhcp_options(state, &actor, &params).await?,
        "DeleteDhcpOptions" => gateways::delete_dhcp_options(state, &actor, &params).await?,
        "DescribeVpcAttribute" => gateways::describe_vpc_attribute(state, &params).await?,
        "ModifyVpcAttribute" => gateways::modify_vpc_attribute(state, &actor, &params).await?,
        "DescribeEgressOnlyInternetGateways" => gateways::describe_egress_only_internet_gateways(state, &params).await?,
        "DescribePrefixLists" | "DescribeManagedPrefixLists" => gateways::describe_prefix_lists(state, &params).await?,
        "DescribeVpcEndpoints" => gateways::describe_vpc_endpoints(state, &params).await?,
        "CreateVpcEndpoint" => gateways::create_vpc_endpoint(state, &actor, &params).await?,
        "CreateNetworkAcl" => nacls::create_network_acl(state, &actor, &params).await?,
        "DescribeNetworkAcls" => nacls::describe_network_acls(state, &params).await?,
        "DeleteNetworkAcl" => nacls::delete_network_acl(state, &actor, &params).await?,
        "CreateNetworkAclEntry" => nacls::create_network_acl_entry(state, &actor, &params).await?,
        "ReplaceNetworkAclEntry" => nacls::replace_network_acl_entry(state, &actor, &params).await?,
        "DeleteNetworkAclEntry" => nacls::delete_network_acl_entry(state, &actor, &params).await?,
        "ReplaceNetworkAclAssociation" => nacls::replace_network_acl_association(state, &actor, &params).await?,
        "DescribeSecurityGroupRules" => sg_rules::describe_security_group_rules(state, &params).await?,
        "ModifySecurityGroupRules" => sg_rules::modify_security_group_rules(state, &actor, &params).await?,
        "UpdateSecurityGroupRuleDescriptionsIngress" => sg_rules::update_rule_descriptions(state, &actor, &params, false).await?,
        "UpdateSecurityGroupRuleDescriptionsEgress" => sg_rules::update_rule_descriptions(state, &actor, &params, true).await?,
        "DetachNetworkInterface" => eni::detach_network_interface(state, &actor, &params).await?,
        "ModifyNetworkInterfaceAttribute" => eni::modify_network_interface_attribute(state, &actor, &params).await?,
        "DescribeNetworkInterfaceAttribute" => eni::describe_network_interface_attribute(state, &params).await?,
        "AssignPrivateIpAddresses" => eni::assign_private_ip_addresses(state, &actor, &params).await?,
        "UnassignPrivateIpAddresses" => eni::unassign_private_ip_addresses(state, &actor, &params).await?,
        "DescribeRegions" => vpc::describe_regions(state, &actor, &params).await?,
        "DescribeVolumeAttribute" => volume_attrs::describe_volume_attribute(state, &params).await?,
        "ModifyVolumeAttribute" => volume_attrs::modify_volume_attribute(state, &actor, &params).await?,
        "DescribeLoadBalancers" => volume_attrs::describe_load_balancers(state, &actor, &params).await?,
        "CreateLoadBalancer" => volume_attrs::create_load_balancer(state, &actor, &params).await?,
        "DescribeVpcPeeringConnections" => peering::describe_vpc_peering_connections(state, &actor, &params).await?,
        "CreateVpcPeeringConnection" => peering::create_vpc_peering_connection(state, &actor, &params).await?,
        "AcceptVpcPeeringConnection" => peering::accept_vpc_peering_connection(state, &actor, &params).await?,
        "AllocateSubnetAddress" => peering::allocate_subnet_address(state, &actor, &params).await?,
        "ReleaseSubnetAddress" => peering::release_subnet_address(state, &actor, &params).await?,
        "DescribeLoadBalancerMembers" => lb_members::describe_load_balancer_members(state, &actor, &params).await?,
        "RegisterInstancesWithLoadBalancer" => lb_members::register_instances(state, &actor, &params).await?,
        "DeregisterInstancesFromLoadBalancer" => lb_members::deregister_instances(state, &actor, &params).await?,
        "ConfigureHealthCheck" => lb_members::configure_health_check(state, &actor, &params).await?,
        "CreateNetworkInterface" => eni::create_network_interface(state, &actor, &params).await?,
        "DeleteNetworkInterface" => eni::delete_network_interface(state, &actor, &params).await?,
        "AttachNetworkInterface" => eni::attach_network_interface(state, &actor, &params).await?,
        "ModifyVolume" => volumes_ext::modify_volume(state, &actor, &params).await?,
        "CreateBackup" => ops::create_backup(state, &actor, &params).await?,
        "DescribeBackups" => ops::describe_backups(state, &actor, &params).await?,
        "RestoreBackup" => ops::restore_backup(state, &actor, &params).await?,
        "DescribeInstanceAttribute" => instance_attrs::describe_instance_attribute(state, &params).await?,
        "CreateBackupSchedule" => schedules::create_backup_schedule(state, &actor, &params).await?,
        "DescribeBackupSchedules" => schedules::describe_backup_schedules(state, &actor, &params).await?,
        "DeleteBackupSchedule" => schedules::delete_backup_schedule(state, &actor, &params).await?,
        "VerifyBackup" => schedules::verify_backup(state, &actor, &params).await?,
        "CreateVmSchedule" => schedules::create_vm_schedule(state, &actor, &params).await?,
        "DescribeVmSchedules" => schedules::describe_vm_schedules(state, &actor, &params).await?,
        "DeleteVmSchedule" => schedules::delete_vm_schedule(state, &actor, &params).await?,
        "CreateMaintenanceSchedule" => schedules::create_maintenance_schedule(state, &actor, &params).await?,
        "DescribeMaintenanceSchedules" => schedules::describe_maintenance_schedules(state, &actor, &params).await?,
        "DeleteMaintenanceSchedule" => schedules::delete_maintenance_schedule(state, &actor, &params).await?,
        "DescribeExperiments" => gameday::describe_experiments(state, &actor, &params).await?,
        "RunExperiment" => gameday::run_experiment(state, &actor, &params).await?,
        "AbortExperiment" => gameday::abort_experiment(state, &actor, &params).await?,
        "DescribeStacks" => platform::describe_stacks(state, &actor, &params).await?,
        "DescribeStackDrift" => platform::describe_stack_drift(state, &actor, &params).await?,
        "ConvergeStack" => platform::converge_stack(state, &actor, &params).await?,
        "SetStackAutoHeal" => platform::set_stack_auto_heal(state, &actor, &params).await?,
        "DeleteStack" => platform::delete_stack(state, &actor, &params).await?,
        "DescribeMigrationJobs" => platform::describe_migration_jobs(state, &actor, &params).await?,
        "DescribeHaStatus" => platform::describe_ha_status(state, &actor, &params).await?,
        "DescribeAudit" => platform::describe_audit(state, &actor, &params).await?,
        "DescribeNotifications" => platform::describe_notifications(state, &actor, &params).await?,
        "DescribeProjects" => platform::describe_projects(state, &actor, &params).await?,
        "CreateProject" => platform::create_project(state, &actor, &params).await?,
        "DescribeScheduledJobs" => platform::describe_scheduled_jobs(state, &actor, &params).await?,
        "CreateScheduledJob" => platform::create_scheduled_job(state, &actor, &params).await?,
        "DeleteScheduledJob" => platform::delete_scheduled_job(state, &actor, &params).await?,
        "DescribeCapacity" => capacity::describe_capacity(state, &actor, &params).await?,
        "DescribeCostEstimate" => capacity::describe_cost_estimate(state, &actor, &params).await?,
        "DescribeRightsizing" => capacity::describe_rightsizing(state, &actor, &params).await?,
        "ProposeResize" => capacity::propose_resize(state, &actor, &params).await?,
        "DescribeConsolidation" => capacity::describe_consolidation(state, &actor, &params).await?,
        "ProposeConsolidation" => capacity::propose_consolidation(state, &actor, &params).await?,
        "DescribePlacement" => capacity::describe_placement(state, &actor, &params).await?,
        "CreateWebhook" => capacity::create_webhook(state, &actor, &params).await?,
        "DescribeWebhooks" => capacity::describe_webhooks(state, &actor, &params).await?,
        "DeleteWebhook" => capacity::delete_webhook(state, &actor, &params).await?,
        "FenceHost" => capacity::fence_host(state, &actor, &params).await?,
        "DeleteLoadBalancer" => volume_attrs::delete_load_balancer(state, &actor, &params).await?,
        "ModifyInstanceAttribute" => instance_attrs::modify_instance_attribute(state, &actor, &params).await?,
        "DescribeAddresses" => addresses::describe_addresses(state, &actor, &params).await?,
        "AllocateAddress" => addresses::allocate_address(state, &actor, &params).await?,
        "AssociateAddress" => addresses::associate_address(state, &actor, &params).await?,
        "DisassociateAddress" => addresses::disassociate_address(state, &actor, &params).await?,
        "ReleaseAddress" => addresses::release_address(state, &actor, &params).await?,
        "DescribeSnapshots" => images::describe_snapshots(state, &actor, &params).await?,
        "CreateSnapshot" => images::create_snapshot(state, &actor, &params).await?,
        "DeleteSnapshot" => images::delete_snapshot(state, &actor, &params).await?,
        "CreateImage" => images::create_image(state, &actor, &params).await?,
        "DeregisterImage" => images::deregister_image(state, &actor, &params).await?,
        "ModifyImageAttribute" => images::modify_image_attribute(state, &actor, &params).await?,
        "DescribeAvailabilityZones" => fleet::describe_availability_zones(state, &actor, &params).await?,
        "DescribeAccountAttributes" => fleet::describe_account_attributes(state, &actor, &params).await?,
        "DescribeLaunchTemplates" => launch_templates::describe_launch_templates(state, &actor, &params).await?,
        "DescribeLaunchTemplateVersions" => launch_templates::describe_launch_template_versions(state, &actor, &params).await?,
        "CreateLaunchTemplate" => launch_templates::create_launch_template(state, &actor, &params).await?,
        "DeleteLaunchTemplate" => fleet::delete_launch_template(state, &actor, &params).await?,
        "SleepInstances" => machina::sleep_instances(state, &actor, &params).await?,
        "WakeInstances" => machina::wake_instances(state, &actor, &params).await?,
        "DescribeSleepPolicies" => machina::describe_sleep_policies(state, &actor, &params).await?,
        "ModifySleepPolicy" => machina::modify_sleep_policy(state, &actor, &params).await?,
        "CreateRestorePoint" => machina::create_restore_point(state, &actor, &params).await?,
        "DescribeRestorePoints" => machina::describe_restore_points(state, &actor, &params).await?,
        "RewindInstance" => machina::rewind_instance(state, &actor, &params).await?,
        "ForkInstance" => machina::fork_instance(state, &actor, &params).await?,
        "CreateTags" => tag_resources(state, &actor, &params, false).await?,
        "DeleteTags" => tag_resources(state, &actor, &params, true).await?,
        "" => return Err(Ec2Error::bad("MissingAction", "No action was specified")),
        other => {
            return Err(Ec2Error::bad("UnsupportedOperation", format!("The action {other} is not supported by this endpoint")))
        }
    };
    Ok(inner)
}

// ---- access key management (admin) -----------------------------------------

#[derive(Serialize)]
pub struct AccessKeyRow {
    access_key_id: String,
    username: String,
    role: String,
    description: String,
    created_at: String,
    last_used_at: Option<String>,
    revoked: bool,
}

fn random_string(n: usize) -> String {
    rand::thread_rng().sample_iter(&Alphanumeric).take(n).map(char::from).collect()
}

#[derive(serde::Deserialize, Default)]
pub struct NewKey {
    #[serde(default)]
    pub description: String,
}

/// The secret is returned once and never again.
pub async fn create_access_key(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<NewKey>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    if body.description.len() > 200 {
        return Err(ApiError::bad_request("description is limited to 200 characters"));
    }
    let id = format!("MCAK{}", random_string(16).to_ascii_uppercase());
    let secret = random_string(40);
    let stored = crate::engine::ai::crypto::store_api_key(&secret).map_err(|e| ApiError::internal(e.to_string()))?;
    crate::db::query("INSERT INTO ec2_access_keys (access_key_id, secret_enc, username, role, description) VALUES (?, ?, ?, ?, ?)")
        .bind(&id)
        .bind(stored)
        .bind(&actor.username)
        .bind(&actor.role)
        .bind(&body.description)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({
        "access_key_id": id,
        "secret_access_key": secret,
        "username": actor.username,
        "role": actor.role,
        "note": "store the secret now; it cannot be shown again",
    })))
}

pub async fn list_access_keys(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<AccessKeyRow>>, ApiError> {
    require_admin(&actor)?;
    let rows: Vec<(String, String, String, String, String, Option<String>, bool)> = crate::db::query_as(
        "SELECT access_key_id, username, role, description, created_at, last_used_at, revoked FROM ec2_access_keys ORDER BY created_at DESC",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(access_key_id, username, role, description, created_at, last_used_at, revoked)| AccessKeyRow {
                access_key_id, username, role, description, created_at, last_used_at, revoked,
            })
            .collect(),
    ))
}

pub async fn revoke_access_key(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    let r = crate::db::query("UPDATE ec2_access_keys SET revoked = 1 WHERE access_key_id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found("access key not found"));
    }
    Ok(Json(serde_json::json!({ "revoked": id })))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every SQL literal in the EC2 modules must prepare against the migrated schema. The statements are not compile-checked, and a
    /// wrong column name otherwise only fails when that action is first called.
    #[tokio::test]
    async fn every_sql_literal_prepares_against_the_schema() {
        use sqlx::Executor;
        let pool = crate::db::testing::pool().await;
        let sources = [
            include_str!("mod.rs"),
            include_str!("more.rs"),
            include_str!("addresses.rs"),
            include_str!("autoscaling.rs"),
            include_str!("capacity.rs"),
            include_str!("eni.rs"),
            include_str!("elbv2.rs"),
            include_str!("fleet.rs"),
            include_str!("foundation.rs"),
            include_str!("gameday.rs"),
            include_str!("gateways.rs"),
            include_str!("groups.rs"),
            include_str!("images.rs"),
            include_str!("images_ext.rs"),
            include_str!("instance_attrs.rs"),
            include_str!("launch_templates.rs"),
            include_str!("lb_members.rs"),
            include_str!("machina.rs"),
            include_str!("monitoring.rs"),
            include_str!("nacls.rs"),
            include_str!("netcommon.rs"),
            include_str!("ops.rs"),
            include_str!("peering.rs"),
            include_str!("placement_groups.rs"),
            include_str!("platform.rs"),
            include_str!("route_tables.rs"),
            include_str!("run_options.rs"),
            include_str!("schedules.rs"),
            include_str!("sg_rules.rs"),
            include_str!("spot.rs"),
            include_str!("status.rs"),
            include_str!("volume_attrs.rs"),
            include_str!("volumes_ext.rs"),
            include_str!("vpc.rs"),
        ];
        let mut checked = 0;
        for src in sources {
            for raw in src.split('"').skip(1).step_by(2) {
                // a Rust string continuation (backslash, newline, indent) is one space
                let flat = raw.split("\\\n").map(str::trim_start).collect::<Vec<_>>().join("");
                let lit = flat.as_str();
                let t = lit.trim_start();
                if t.len() < 25 || !["SELECT ", "INSERT ", "UPDATE ", "DELETE "].iter().any(|k| t.starts_with(k)) {
                    continue;
                }
                // the controller writes SQLite-form SQL and the PostgreSQL build rewrites it (`?` to `$n`, ...) before it runs
                #[cfg(feature = "postgres")]
                let owned = crate::db::dialect::to_postgres(lit);
                #[cfg(not(feature = "postgres"))]
                let owned = lit.to_string();
                if let Err(e) = (&pool).prepare(owned.as_str()).await {
                    panic!("SQL does not prepare: {lit}\n{e}");
                }
                checked += 1;
            }
        }
        assert!(checked > 20, "expected to check the EC2 statements, checked {checked}");
    }

    fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn states_map_to_ec2_codes() {
        assert_eq!(instance_state("running"), (16, "running"));
        assert_eq!(instance_state("shutoff"), (80, "stopped"));
        assert_eq!(instance_state("weird"), (0, "pending"));
        assert_eq!(instance_state("terminated"), (48, "terminated"));
    }

    #[test]
    fn form_filters_and_indexed_params() {
        let m = parse_form("Action=DescribeInstances&InstanceId.1=i-a&InstanceId.2=i-b&Filter.1.Name=tag%3AEnv&Filter.1.Value.1=prod&Filter.1.Value.2=stage");
        assert_eq!(indexed(&m, "InstanceId"), ["i-a", "i-b"]);
        assert_eq!(parse_filters(&m), vec![("tag:Env".to_string(), vec!["prod".to_string(), "stage".to_string()])]);
        assert!(indexed(&p(&[]), "InstanceId").is_empty());
    }

    fn inst() -> Inst {
        Inst {
            id: Uuid::parse_str("0123456789abcdef0123456789abcdef").unwrap(),
            name: "web<1>".into(),
            state: "running".into(),
            vcpus: 2,
            memory_mib: 4096,
            flavor: None,
            ip: Some("10.0.0.5".into()),
            created: "2026-10-05 10:00:00".into(),
            tags: vec![("Env".into(), "prod".into())],
            launch_failure: None,
        }
    }

    #[test]
    fn filters_match_by_id_state_type_and_tag_and_unknown_matches_nothing() {
        let i = inst();
        assert!(matches_filter(&i, "instance-id", &[i.eid()]));
        assert!(matches_filter(&i, "instance-state-name", &["running".into()]));
        assert!(!matches_filter(&i, "instance-state-name", &["stopped".into()]));
        assert!(matches_filter(&i, "instance-type", &["custom.2x4096".into()]));
        assert!(matches_filter(&i, "tag:Env", &["prod".into()]));
        assert!(!matches_filter(&i, "tag:Env", &["dev".into()]));
        assert!(matches_filter(&i, "tag-key", &["Env".into()]));
        assert!(!matches_filter(&i, "nonsense", &["x".into()]));
    }

    #[test]
    fn instance_xml_is_escaped_and_carries_state_and_tags() {
        let x = instance_item(&inst());
        assert!(x.contains("<name>running</name>"));
        assert!(x.contains("web&lt;1&gt;"));
        assert!(x.contains("<key>Env</key><value>prod</value>"));
        assert!(x.contains("<instanceId>i-0123456789abcdef0</instanceId>"));
    }

    #[test]
    fn tags_from_tag_and_tag_specification_params() {
        let m = parse_form("Tag.1.Key=A&Tag.1.Value=1&TagSpecification.1.ResourceType=instance&TagSpecification.1.Tag.1.Key=Name&TagSpecification.1.Tag.1.Value=web&TagSpecification.2.ResourceType=volume&TagSpecification.2.Tag.1.Key=Skip&TagSpecification.2.Tag.1.Value=x");
        let t = parse_tags(&m);
        assert_eq!(t.get("A").map(String::as_str), Some("1"));
        assert_eq!(t.get("Name").map(String::as_str), Some("web"));
        assert!(!t.contains_key("Skip"), "volume tag specs are not applied to instances");
    }

    #[test]
    fn user_data_is_base64_text() {
        assert_eq!(decode_user_data("I2Nsb3VkLWNvbmZpZw==").unwrap(), "#cloud-config");
        assert!(decode_user_data("not base64!!").is_err());
        assert!(decode_user_data("/w==").is_err(), "0xFF is not UTF-8");
    }

    #[test]
    fn error_xml_has_code_message_and_request_id() {
        let e = Ec2Error::bad("InvalidParameterValue", "bad <x>");
        let x = error_xml(&e, "rid");
        assert!(x.contains("<Code>InvalidParameterValue</Code>"));
        assert!(x.contains("bad &lt;x&gt;"));
        assert!(x.contains("<RequestID>rid</RequestID>"));
    }
}
