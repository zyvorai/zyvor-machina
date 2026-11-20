// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! The Auto Scaling service (`autoscaling` credential scope) on top of the instance groups.
//!
//! An Auto Scaling group is a `cloud_instance_groups` row that also has an `asg_groups` row. The group row
//! keeps owning capacity (`policy_json`: min, max, desired, cooldown, target CPU) and its members, and the
//! reconciler in `engine/cloud.rs` keeps converging them: it launches into empty slots below `desired`, starts
//! stopped members, stops (or sleeps) members above it, and adjusts `desired` for a CPU target. Nothing here
//! launches or stops an instance on its own; this module changes the group's numbers and membership through the
//! same REST handlers the instance-group API uses, so project access and audit stay in one place.
//!
//! What the Auto Scaling API adds lives in `asg_*` tables: launch configurations, tags, scaling policies,
//! scaling activities, health-check settings and suspended processes.
//!
//! A parameter this service cannot honour is refused with `UnsupportedOperation` (never ignored), so a client's
//! state cannot drift from what the controller does. Honest limits:
//! - one subnet per group (a group lives on one host; the "availability zone" is that host);
//! - capacity is the `desired` number of slots; standby, lifecycle hooks, warm pools, mixed instances,
//!   instance refresh, scheduled actions and metrics collection are not implemented;
//! - `Launch` and `Terminate` can only be suspended together (the reconciler pauses as a whole);
//! - scaling policies run when executed or when an alarm fires; target tracking is the reconciler's CPU target
//!   (`ASGAverageCPUUtilization`, 10 to 90 percent);
//! - activities are recorded for API-driven changes, not for launches the reconciler does by itself.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use base64::Engine;
use machina_spec::ScalingPolicy;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::{ec2_id, Kind};
use crate::state::AppState;

use super::more::{api_err, resolve};
use super::{xml_escape, Ec2Error};

type Params = BTreeMap<String, String>;

const ACCOUNT: &str = "000000000000";

/// Every process name `SuspendProcesses` accepts.
const PROCESSES: &[&str] = &[
    "Launch",
    "Terminate",
    "AddToLoadBalancer",
    "AlarmNotification",
    "AZRebalance",
    "HealthCheck",
    "InstanceRefresh",
    "ReplaceUnhealthy",
    "ScheduledActions",
];

fn validation(msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad("ValidationError", msg)
}

fn unsupported(msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad("UnsupportedOperation", msg)
}

fn need(p: &Params, k: &str) -> Result<String, Ec2Error> {
    p.get(k).filter(|v| !v.is_empty()).cloned().ok_or_else(|| Ec2Error::bad("MissingParameter", format!("The request must contain the parameter {k}")))
}

fn flag(p: &Params, k: &str) -> bool {
    matches!(p.get(k).map(String::as_str), Some("true") | Some("1"))
}

fn int(p: &Params, k: &str) -> Result<Option<i64>, Ec2Error> {
    p.get(k).map(|v| v.parse::<i64>().map_err(|_| validation(format!("{k} must be a number")))).transpose()
}

/// `{prefix}.member.1`, `{prefix}.member.2`, … (the query encoding of a list), falling back to `{prefix}.1`, …
pub(crate) fn members(p: &Params, prefix: &str) -> Vec<String> {
    let listed: Vec<String> = (1..=200).map_while(|n| p.get(&format!("{prefix}.member.{n}")).cloned()).collect();
    if listed.is_empty() {
        super::indexed(p, prefix)
    } else {
        listed
    }
}

/// Refuses every parameter the action does not implement.
fn allow_only(p: &Params, exact: &[&str], prefixes: &[&str]) -> Result<(), Ec2Error> {
    for key in p.keys() {
        if key == "Action" || key == "Version" || exact.contains(&key.as_str()) || prefixes.iter().any(|x| key.starts_with(x)) {
            continue;
        }
        return Err(unsupported(format!("the parameter {key} is not supported by this Auto Scaling service")));
    }
    Ok(())
}

fn iso(ts: &str) -> String {
    let t = ts.trim();
    if t.len() >= 19 && t.is_char_boundary(10) && t.is_char_boundary(11) && t.is_char_boundary(19) {
        format!("{}T{}Z", &t[..10], &t[11..19])
    } else {
        t.to_string()
    }
}

fn now_text() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub fn group_arn(id: Uuid, name: &str) -> String {
    format!("arn:aws:autoscaling:machina:{ACCOUNT}:autoScalingGroup:{id}:autoScalingGroupName/{name}")
}

pub fn policy_arn(id: Uuid, group: &str, name: &str) -> String {
    format!("arn:aws:autoscaling:machina:{ACCOUNT}:scalingPolicy:{id}:autoScalingGroupName/{group}:policyName/{name}")
}

fn launch_config_arn(name: &str) -> String {
    format!("arn:aws:autoscaling:machina:{ACCOUNT}:launchConfiguration:{}:launchConfigurationName/{name}", Uuid::nil())
}

/// The policy id inside a policy ARN, if `arn` is one.
pub fn policy_id_from_arn(arn: &str) -> Option<Uuid> {
    let rest = arn.split_once(":scalingPolicy:")?.1;
    Uuid::parse_str(rest.split(':').next()?).ok()
}

// ---- scaling arithmetic (pure) -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjustmentType {
    ChangeInCapacity,
    ExactCapacity,
    PercentChangeInCapacity,
}

impl AdjustmentType {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ChangeInCapacity" => Some(Self::ChangeInCapacity),
            "ExactCapacity" => Some(Self::ExactCapacity),
            "PercentChangeInCapacity" => Some(Self::PercentChangeInCapacity),
            _ => None,
        }
    }
}

/// The group's new desired capacity after one adjustment, clamped to `min..=max`. A percentage truncates toward
/// zero and then moves by at least `min_magnitude` instances (never when the adjustment is zero).
pub fn target_capacity(kind: AdjustmentType, adjustment: i64, min_magnitude: i64, desired: u32, min: u32, max: u32) -> u32 {
    let current = i64::from(desired);
    let want = match kind {
        AdjustmentType::ExactCapacity => adjustment,
        AdjustmentType::ChangeInCapacity => current + adjustment,
        AdjustmentType::PercentChangeInCapacity => {
            let mut delta = current * adjustment / 100;
            if adjustment != 0 && delta.abs() < min_magnitude {
                delta = if adjustment < 0 { -min_magnitude } else { min_magnitude };
            }
            current + delta
        }
    };
    want.clamp(i64::from(min), i64::from(max)) as u32
}

/// One step of a step-scaling policy. Bounds are relative to the alarm threshold; the lower bound is inclusive,
/// the upper bound exclusive, and a missing bound is unbounded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub lower: Option<f64>,
    pub upper: Option<f64>,
    pub adjustment: i64,
}

/// The step matching `diff` (metric value minus alarm threshold).
pub fn pick_step(steps: &[Step], diff: f64) -> Option<&Step> {
    steps.iter().find(|s| s.lower.is_none_or(|l| diff >= l) && s.upper.is_none_or(|u| diff < u))
}

/// Why a policy adjustment may not run right now: the cooldown since the last scaling is still running.
pub fn cooling_down(secs_since_last_scaling: i64, cooldown_secs: i64) -> bool {
    secs_since_last_scaling < cooldown_secs
}

/// Cooldown is the new target's own cooldown; a group's default applies when the policy has none.
fn effective_cooldown(policy_cooldown: i64, group_default: u32) -> i64 {
    if policy_cooldown > 0 {
        policy_cooldown
    } else {
        i64::from(group_default)
    }
}

// ---- paging ------------------------------------------------------------------------------------------------

fn max_records(p: &Params) -> Result<usize, Ec2Error> {
    match p.get("MaxRecords") {
        None => Ok(50),
        Some(v) => match v.parse::<usize>() {
            Ok(n) if (1..=100).contains(&n) => Ok(n),
            _ => Err(validation("MaxRecords must be between 1 and 100")),
        },
    }
}

fn token_encode(last: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(last.as_bytes())
}

fn token_decode(token: &str) -> Result<String, Ec2Error> {
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token.trim())
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .ok_or_else(|| Ec2Error::bad("InvalidNextToken", "The NextToken value is not valid"))?;
    Ok(raw)
}

/// `keys` is sorted ascending; returns the page and the token for the next one.
fn paginate(keys: &[String], token: Option<&str>, limit: usize) -> Result<(Vec<String>, Option<String>), Ec2Error> {
    let start = match token {
        None => 0,
        Some(t) => {
            let last = token_decode(t)?;
            keys.iter().position(|k| k.as_str() > last.as_str()).unwrap_or(keys.len())
        }
    };
    let rest = &keys[start..];
    if rest.len() <= limit {
        return Ok((rest.to_vec(), None));
    }
    let page = rest[..limit].to_vec();
    let next = token_encode(&page[page.len() - 1]);
    Ok((page, Some(next)))
}

fn next_token_xml(token: &Option<String>) -> String {
    token.as_ref().map(|t| format!("<NextToken>{}</NextToken>", xml_escape(t))).unwrap_or_default()
}

// ---- group rows ---------------------------------------------------------------------------------------------

struct Group {
    id: Uuid,
    project: Uuid,
    template: Uuid,
    subnet: Uuid,
    name: String,
    policy: ScalingPolicy,
    paused: bool,
    launch_config: String,
    version: String,
    health_type: String,
    grace: i64,
    suspended: Vec<String>,
    created_at: String,
}

type GroupRow = (Uuid, Uuid, Uuid, Uuid, String, String, bool, String, String, String, i64, String, String);

const GROUP_SELECT: &str = "SELECT g.id, g.project_id, g.template_id, g.subnet_id, a.name, g.policy_json, g.paused, \
a.launch_config_name, a.launch_template_version, a.health_check_type, a.health_check_grace, a.suspended, a.created_at \
FROM asg_groups a JOIN cloud_instance_groups g ON g.id = a.group_id";

impl Group {
    fn from_row(r: GroupRow) -> Result<Group, Ec2Error> {
        let policy: ScalingPolicy = serde_json::from_str(&r.5).map_err(|e| Ec2Error::new(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", format!("group policy unreadable: {e}")))?;
        Ok(Group {
            id: r.0,
            project: r.1,
            template: r.2,
            subnet: r.3,
            name: r.4,
            policy,
            paused: r.6,
            launch_config: r.7,
            version: r.8,
            health_type: r.9,
            grace: r.10,
            suspended: r.11.split(',').filter(|s| !s.is_empty()).map(String::from).collect(),
            created_at: r.12,
        })
    }
}

async fn load_group(state: &AppState, name: &str) -> Result<Group, Ec2Error> {
    let row: Option<GroupRow> = crate::db::query_as(&format!("{GROUP_SELECT} WHERE a.name = ?")).bind(name).fetch_optional(&state.pool).await?;
    match row {
        Some(r) => Group::from_row(r),
        None => Err(validation(format!("AutoScalingGroup name not found - AutoScalingGroup: {name}"))),
    }
}

async fn all_group_names(state: &AppState) -> Result<Vec<String>, Ec2Error> {
    Ok(crate::db::query_scalar("SELECT name FROM asg_groups ORDER BY name").fetch_all(&state.pool).await?)
}

async fn group_of_vm(state: &AppState, vm: Uuid) -> Result<Option<String>, Ec2Error> {
    Ok(crate::db::query_scalar("SELECT a.name FROM cloud_group_members m JOIN asg_groups a ON a.group_id = m.group_id WHERE m.vm_id = ?")
        .bind(vm)
        .fetch_optional(&state.pool)
        .await?)
}

async fn zone_of_subnet(state: &AppState, subnet: Uuid) -> Result<String, Ec2Error> {
    let zone: Option<String> = crate::db::query_scalar(
        "SELECT h.hostname FROM cloud_subnets s JOIN cloud_vpcs v ON v.id = s.vpc_id JOIN hosts h ON h.id = v.host_id WHERE s.id = ?",
    )
    .bind(subnet)
    .fetch_optional(&state.pool)
    .await?;
    Ok(zone.unwrap_or_default())
}

/// A group's instances: the members in the slots the group wants running. Stopped and sleeping members above
/// `desired` are scaled in and are not part of the group any more.
struct Inst {
    vm: Uuid,
    observed: String,
}

async fn instances(state: &AppState, g: &Group) -> Result<Vec<Inst>, Ec2Error> {
    let rows: Vec<(Uuid, String)> = crate::db::query_as(
        "SELECT v.id, COALESCE(v.observed_state, '') FROM cloud_group_members m JOIN vms v ON v.id = m.vm_id \
         WHERE m.group_id = ? AND m.slot < ? ORDER BY m.slot",
    )
    .bind(g.id)
    .bind(i64::from(g.policy.desired))
    .fetch_all(&state.pool)
    .await?;
    Ok(rows.into_iter().map(|(vm, observed)| Inst { vm, observed }).collect())
}

fn lifecycle_state(observed: &str) -> &'static str {
    if observed == "running" {
        "InService"
    } else {
        "Pending"
    }
}

fn health_status(observed: &str) -> &'static str {
    if matches!(observed, "error" | "failed" | "crashed" | "missing") {
        "Unhealthy"
    } else {
        "Healthy"
    }
}

async fn template_name(state: &AppState, id: Uuid) -> String {
    crate::db::query_scalar("SELECT name FROM cloud_launch_templates WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
}

async fn tags_of(state: &AppState, group: Uuid) -> Result<Vec<(String, String, bool)>, Ec2Error> {
    Ok(crate::db::query_as("SELECT tag_key, tag_value, propagate_at_launch FROM asg_tags WHERE group_id = ? ORDER BY tag_key")
        .bind(group)
        .fetch_all(&state.pool)
        .await?)
}

fn tag_member(group: &str, key: &str, value: &str, propagate: bool) -> String {
    format!(
        "<member><ResourceId>{}</ResourceId><ResourceType>auto-scaling-group</ResourceType><Key>{}</Key><Value>{}</Value><PropagateAtLaunch>{propagate}</PropagateAtLaunch></member>",
        xml_escape(group),
        xml_escape(key),
        xml_escape(value)
    )
}

async fn group_xml(state: &AppState, g: &Group) -> Result<String, Ec2Error> {
    let zone = zone_of_subnet(state, g.subnet).await?;
    let insts = instances(state, g).await?;
    let tags = tags_of(state, g.id).await?;
    let launch = if g.launch_config.is_empty() {
        format!(
            "<LaunchTemplate><LaunchTemplateId>{}</LaunchTemplateId><LaunchTemplateName>{}</LaunchTemplateName><Version>{}</Version></LaunchTemplate>",
            ec2_id(Kind::LaunchTemplate, g.template),
            xml_escape(&template_name(state, g.template).await),
            xml_escape(&g.version)
        )
    } else {
        format!("<LaunchConfigurationName>{}</LaunchConfigurationName>", xml_escape(&g.launch_config))
    };
    let instances_xml: String = insts
        .iter()
        .map(|i| {
            format!(
                "<member><InstanceId>{}</InstanceId><InstanceType></InstanceType><AvailabilityZone>{}</AvailabilityZone><LifecycleState>{}</LifecycleState><HealthStatus>{}</HealthStatus><ProtectedFromScaleIn>false</ProtectedFromScaleIn></member>",
                ec2_id(Kind::Vm, i.vm),
                xml_escape(&zone),
                lifecycle_state(&i.observed),
                health_status(&i.observed)
            )
        })
        .collect();
    let suspended: String = g
        .suspended
        .iter()
        .map(|s| format!("<member><ProcessName>{}</ProcessName><SuspensionReason>User suspended</SuspensionReason></member>", xml_escape(s)))
        .collect();
    let tags_xml: String = tags.iter().map(|(k, v, pr)| tag_member(&g.name, k, v, *pr)).collect();
    Ok(format!(
        "<member><AutoScalingGroupName>{name}</AutoScalingGroupName><AutoScalingGroupARN>{arn}</AutoScalingGroupARN>{launch}\
<MinSize>{min}</MinSize><MaxSize>{max}</MaxSize><DesiredCapacity>{desired}</DesiredCapacity><DefaultCooldown>{cool}</DefaultCooldown>\
<AvailabilityZones><member>{zone}</member></AvailabilityZones><LoadBalancerNames/><TargetGroupARNs/>\
<HealthCheckType>{hc}</HealthCheckType><HealthCheckGracePeriod>{grace}</HealthCheckGracePeriod>\
<Instances>{instances_xml}</Instances><CreatedTime>{created}</CreatedTime><SuspendedProcesses>{suspended}</SuspendedProcesses>\
<VPCZoneIdentifier>{subnet}</VPCZoneIdentifier><EnabledMetrics/><Tags>{tags_xml}</Tags>\
<TerminationPolicies><member>Default</member></TerminationPolicies><NewInstancesProtectedFromScaleIn>false</NewInstancesProtectedFromScaleIn>\
<ServiceLinkedRoleARN>arn:aws:iam::{ACCOUNT}:role/aws-service-role/autoscaling.amazonaws.com/AWSServiceRoleForAutoScaling</ServiceLinkedRoleARN></member>",
        name = xml_escape(&g.name),
        arn = xml_escape(&group_arn(g.id, &g.name)),
        min = g.policy.min,
        max = g.policy.max,
        desired = g.policy.desired,
        cool = g.policy.cooldown_secs,
        zone = xml_escape(&zone),
        hc = xml_escape(&g.health_type),
        grace = g.grace,
        created = iso(&g.created_at),
        subnet = ec2_id(Kind::Subnet, g.subnet),
    ))
}

async fn record(state: &AppState, group: Uuid, description: &str, cause: &str) {
    let _ = crate::db::query("INSERT INTO asg_activities (id, group_id, description, cause) VALUES (?, ?, ?, ?)")
        .bind(Uuid::new_v4())
        .bind(group)
        .bind(description)
        .bind(format!("At {} {cause}", now_text()))
        .execute(&state.pool)
        .await;
}

// ---- changing a group through the REST handlers --------------------------------------------------------------

/// Writes the group's policy (capacity numbers, CPU target) through the REST handler: access check, validation,
/// audit, cooldown timestamp. The `paused` flag stays as it is unless `paused` says otherwise.
async fn write_policy(state: &AppState, actor: &AuthUser, g: &Group, policy: &ScalingPolicy, paused: bool) -> Result<(), Ec2Error> {
    policy.validate().map_err(validation)?;
    let body: crate::api::cloud::elastic::UpdateGroup =
        serde_json::from_value(serde_json::json!({ "policy": policy, "paused": paused })).map_err(|e| validation(e.to_string()))?;
    let _ = crate::api::cloud::elastic::update_group(State(state.clone()), Extension(actor.clone()), Path(g.id), Json(body)).await.map_err(api_err)?;
    Ok(())
}

async fn set_paused(state: &AppState, group: Uuid, paused: bool) -> Result<(), Ec2Error> {
    crate::db::query("UPDATE cloud_instance_groups SET paused = ? WHERE id = ?").bind(paused).bind(group).execute(&state.pool).await?;
    Ok(())
}

// ---- launch configurations ----------------------------------------------------------------------------------

pub async fn create_launch_configuration(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(
        p,
        &["LaunchConfigurationName", "ImageId", "InstanceType", "KeyName", "UserData", "InstanceMonitoring.Enabled", "EbsOptimized", "AssociatePublicIpAddress"],
        &["SecurityGroups.member."],
    )?;
    let name = need(p, "LaunchConfigurationName")?;
    if name.len() > 255 {
        return Err(validation("LaunchConfigurationName must be 1 to 255 characters"));
    }
    let image = need(p, "ImageId")?;
    if flag(p, "InstanceMonitoring.Enabled") {
        return Err(unsupported("InstanceMonitoring.Enabled=true is not supported; set it to false (enable_monitoring = false in Terraform)"));
    }
    if flag(p, "EbsOptimized") || flag(p, "AssociatePublicIpAddress") {
        return Err(unsupported("EbsOptimized and AssociatePublicIpAddress are not supported"));
    }
    if !members(p, "SecurityGroups").is_empty() {
        return Err(unsupported("SecurityGroups on a launch configuration are not supported; attach security groups to the instances"));
    }
    let user_data = match p.get("UserData").filter(|v| !v.is_empty()) {
        Some(b64) => {
            let text = super::decode_user_data(b64).map_err(validation)?;
            machina_spec::validate_user_data(&text).map_err(|e| validation(e.to_string()))?;
            text
        }
        None => String::new(),
    };
    if let Some(flavor) = p.get("InstanceType").filter(|v| !v.is_empty()) {
        let known: Option<Uuid> = crate::db::query_scalar("SELECT id FROM flavors WHERE name = ?").bind(flavor).fetch_optional(&state.pool).await?;
        if known.is_none() {
            return Err(validation(format!("Unknown instance type '{flavor}'")));
        }
    }
    if let Some(key) = p.get("KeyName").filter(|v| !v.is_empty()) {
        let known: Option<Uuid> = crate::db::query_scalar("SELECT id FROM keypairs WHERE name = ?").bind(key).fetch_optional(&state.pool).await?;
        if known.is_none() {
            return Err(validation(format!("The key pair '{key}' does not exist")));
        }
    }
    let exists: Option<String> = crate::db::query_scalar("SELECT name FROM asg_launch_configs WHERE name = ?").bind(&name).fetch_optional(&state.pool).await?;
    if exists.is_some() {
        return Err(Ec2Error::bad("AlreadyExists", format!("Launch Configuration by this name already exists - A launch configuration already exists with the name {name}")));
    }
    crate::db::query("INSERT INTO asg_launch_configs (name, image_id, instance_type, key_name, user_data, created_by) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(&name)
        .bind(&image)
        .bind(p.get("InstanceType").cloned().unwrap_or_default())
        .bind(p.get("KeyName").cloned().unwrap_or_default())
        .bind(&user_data)
        .bind(&actor.username)
        .execute(&state.pool)
        .await?;
    Ok(String::new())
}

type LaunchConfigRow = (String, String, String, String, String, String);

pub async fn describe_launch_configurations(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(p, &["MaxRecords", "NextToken"], &["LaunchConfigurationNames.member."])?;
    let wanted = members(p, "LaunchConfigurationNames");
    let rows: Vec<LaunchConfigRow> = crate::db::query_as("SELECT name, image_id, instance_type, key_name, user_data, created_at FROM asg_launch_configs ORDER BY name")
        .fetch_all(&state.pool)
        .await?;
    let rows: Vec<LaunchConfigRow> = rows.into_iter().filter(|r| wanted.is_empty() || wanted.contains(&r.0)).collect();
    let names: Vec<String> = rows.iter().map(|r| r.0.clone()).collect();
    let (page, next) = paginate(&names, p.get("NextToken").map(String::as_str), max_records(p)?)?;
    let items: String = rows
        .iter()
        .filter(|r| page.contains(&r.0))
        .map(|(name, image, itype, key, user_data, created)| {
            let user_data_b64 = base64::engine::general_purpose::STANDARD.encode(user_data.as_bytes());
            format!(
                "<member><LaunchConfigurationName>{}</LaunchConfigurationName><LaunchConfigurationARN>{}</LaunchConfigurationARN><ImageId>{}</ImageId><KeyName>{}</KeyName>\
<SecurityGroups/><UserData>{}</UserData><InstanceType>{}</InstanceType><InstanceMonitoring><Enabled>false</Enabled></InstanceMonitoring>\
<EbsOptimized>false</EbsOptimized><BlockDeviceMappings/><ClassicLinkVPCSecurityGroups/><CreatedTime>{}</CreatedTime></member>",
                xml_escape(name),
                xml_escape(&launch_config_arn(name)),
                xml_escape(image),
                xml_escape(key),
                if user_data.is_empty() { String::new() } else { user_data_b64 },
                xml_escape(itype),
                iso(created)
            )
        })
        .collect();
    Ok(format!("<LaunchConfigurations>{items}</LaunchConfigurations>{}", next_token_xml(&next)))
}

pub async fn delete_launch_configuration(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(p, &["LaunchConfigurationName"], &[])?;
    let name = need(p, "LaunchConfigurationName")?;
    let used: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM asg_groups WHERE launch_config_name = ?").bind(&name).fetch_one(&state.pool).await?;
    if used > 0 {
        return Err(Ec2Error::bad("ResourceInUse", format!("Launch configuration name not found - Launch configuration {name} is still in use by {used} group(s)")));
    }
    let gone = crate::db::query("DELETE FROM asg_launch_configs WHERE name = ?").bind(&name).execute(&state.pool).await?.rows_affected();
    if gone == 0 {
        return Err(validation(format!("Launch configuration name not found - {name}")));
    }
    Ok(String::new())
}

/// The image a launch configuration names, as a template name `RunInstances`-style: an `ami-` id or a template name.
async fn image_template_name(state: &AppState, image: &str) -> Result<String, Ec2Error> {
    if let Some((Kind::Image, hex)) = crate::resource_ids::parse(image) {
        let mut conn = state.pool.acquire().await?;
        return match crate::resource_ids::resolve(&mut conn, Kind::Image, &hex).await? {
            crate::resource_ids::Lookup::Found(id) => Ok(crate::db::query_scalar("SELECT name FROM templates WHERE id = ?").bind(id).fetch_one(&mut *conn).await?),
            _ => Err(validation(format!("The image id '{image}' does not exist"))),
        };
    }
    Ok(image.to_string())
}

/// Makes (or reuses) the launch template a launch configuration stands for in the subnet's project.
async fn template_from_config(state: &AppState, actor: &AuthUser, project: Uuid, config: &str) -> Result<Uuid, Ec2Error> {
    let row: Option<(String, String, String, String)> =
        crate::db::query_as("SELECT image_id, instance_type, key_name, user_data FROM asg_launch_configs WHERE name = ?").bind(config).fetch_optional(&state.pool).await?;
    let Some((image, instance_type, key_name, user_data)) = row else {
        return Err(validation(format!("Launch configuration name not found - {config}")));
    };
    let template = format!("lc-{config}");
    let existing: Option<Uuid> = crate::db::query_scalar("SELECT id FROM cloud_launch_templates WHERE project_id = ? AND name = ?")
        .bind(project)
        .bind(&template)
        .fetch_optional(&state.pool)
        .await?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let mut vm = super::fleet::template_vm(&template, &image_template_name(state, &image).await?);
    if !instance_type.is_empty() {
        let flavor: Option<(i64, i64)> = crate::db::query_as("SELECT vcpus, memory_mib FROM flavors WHERE name = ?").bind(&instance_type).fetch_optional(&state.pool).await?;
        if let Some((vcpus, memory)) = flavor {
            vm["spec"]["cpu"] = serde_json::json!({ "sockets": 1, "cores": vcpus });
            vm["spec"]["memory"] = serde_json::json!(format!("{memory}Mi"));
        }
    }
    let mut cloud_init = serde_json::Map::new();
    if !user_data.is_empty() {
        cloud_init.insert("user_data".into(), serde_json::json!(user_data));
    }
    if !key_name.is_empty() {
        let key: Option<String> = crate::db::query_scalar("SELECT public_key FROM keypairs WHERE name = ?").bind(&key_name).fetch_optional(&state.pool).await?;
        if let Some(public) = key {
            cloud_init.insert("ssh_pubkey".into(), serde_json::json!(public));
        }
    }
    if !cloud_init.is_empty() {
        vm["spec"]["cloud_init"] = serde_json::Value::Object(cloud_init);
    }
    Ok(super::fleet::create_template_row(state, actor, project, &template, vm).await?.0)
}

// ---- launch source of a group -------------------------------------------------------------------------------

struct Source {
    template: Uuid,
    launch_config: String,
    version: String,
}

/// `LaunchTemplate.*` or `LaunchConfigurationName` (exactly one), turned into the template the group launches from.
async fn launch_source(state: &AppState, actor: &AuthUser, p: &Params, project: Uuid) -> Result<Option<Source>, Ec2Error> {
    let by_id = p.get("LaunchTemplate.LaunchTemplateId").filter(|v| !v.is_empty());
    let by_name = p.get("LaunchTemplate.LaunchTemplateName").filter(|v| !v.is_empty());
    let config = p.get("LaunchConfigurationName").filter(|v| !v.is_empty());
    let template_given = by_id.is_some() || by_name.is_some();
    if template_given && config.is_some() {
        return Err(validation("Specify either a launch configuration or a launch template, not both"));
    }
    if let Some(config) = config {
        let template = template_from_config(state, actor, project, config).await?;
        return Ok(Some(Source { template, launch_config: config.clone(), version: "$Default".into() }));
    }
    if !template_given {
        return Ok(None);
    }
    if by_id.is_some() && by_name.is_some() {
        return Err(validation("Specify LaunchTemplateId or LaunchTemplateName, not both"));
    }
    let version = p.get("LaunchTemplate.Version").cloned().unwrap_or_else(|| "$Default".into());
    if !matches!(version.as_str(), "1" | "$Latest" | "$Default") {
        return Err(validation(format!("You must use a valid fully-formed launch template. The launch template version '{version}' does not exist")));
    }
    let template: Uuid = match (by_id, by_name) {
        (Some(id), _) => resolve(state, Kind::LaunchTemplate, id, "ValidationError").await?,
        (None, Some(name)) => crate::db::query_scalar("SELECT id FROM cloud_launch_templates WHERE project_id = ? AND name = ?")
            .bind(project)
            .bind(name)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| validation(format!("The launch template '{name}' does not exist in the subnet's project")))?,
        (None, None) => return Ok(None),
    };
    Ok(Some(Source { template, launch_config: String::new(), version }))
}

// ---- groups -------------------------------------------------------------------------------------------------

const GROUP_PARAMS: &[&str] = &[
    "AutoScalingGroupName",
    "MinSize",
    "MaxSize",
    "DesiredCapacity",
    "DefaultCooldown",
    "LaunchConfigurationName",
    "LaunchTemplate.LaunchTemplateId",
    "LaunchTemplate.LaunchTemplateName",
    "LaunchTemplate.Version",
    "VPCZoneIdentifier",
    "HealthCheckType",
    "HealthCheckGracePeriod",
    "NewInstancesProtectedFromScaleIn",
];
const GROUP_PREFIXES: &[&str] = &["Tags.member.", "AvailabilityZones.member.", "TerminationPolicies.member."];

/// The refusals shared by create and update.
fn check_group_options(p: &Params) -> Result<(), Ec2Error> {
    if flag(p, "NewInstancesProtectedFromScaleIn") {
        return Err(unsupported("NewInstancesProtectedFromScaleIn is not supported"));
    }
    if let Some(t) = p.get("HealthCheckType") {
        if t != "EC2" {
            return Err(unsupported("HealthCheckType must be EC2; ELB health checks arrive with the load balancer service"));
        }
    }
    if members(p, "TerminationPolicies").iter().any(|t| t != "Default") {
        return Err(unsupported("only the Default termination policy is supported"));
    }
    Ok(())
}

fn single_subnet(p: &Params) -> Result<Option<String>, Ec2Error> {
    let Some(raw) = p.get("VPCZoneIdentifier").map(|v| v.trim()).filter(|v| !v.is_empty()) else { return Ok(None) };
    let list: Vec<&str> = raw.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    if list.len() != 1 {
        return Err(unsupported("a group uses exactly one subnet (VPCZoneIdentifier lists several)"));
    }
    Ok(Some(list[0].to_string()))
}

struct TagIn {
    key: String,
    value: String,
    propagate: bool,
    resource_id: Option<String>,
}

fn parse_group_tags(p: &Params) -> Result<Vec<TagIn>, Ec2Error> {
    let mut out = Vec::new();
    for n in 1..=50 {
        let Some(key) = p.get(&format!("Tags.member.{n}.Key")) else { break };
        if key.is_empty() || key.len() > 128 || key.starts_with("aws:") {
            return Err(validation("a tag key must be 1 to 128 characters and must not start with aws:"));
        }
        if let Some(t) = p.get(&format!("Tags.member.{n}.ResourceType")) {
            if t != "auto-scaling-group" {
                return Err(validation("ResourceType must be auto-scaling-group"));
            }
        }
        out.push(TagIn {
            key: key.clone(),
            value: p.get(&format!("Tags.member.{n}.Value")).cloned().unwrap_or_default(),
            propagate: flag(p, &format!("Tags.member.{n}.PropagateAtLaunch")),
            resource_id: p.get(&format!("Tags.member.{n}.ResourceId")).cloned(),
        });
    }
    Ok(out)
}

async fn put_tags(state: &AppState, group: Uuid, tags: &[TagIn]) -> Result<(), Ec2Error> {
    for t in tags {
        crate::db::query(
            "INSERT INTO asg_tags (group_id, tag_key, tag_value, propagate_at_launch) VALUES (?, ?, ?, ?) \
             ON CONFLICT (group_id, tag_key) DO UPDATE SET tag_value = excluded.tag_value, propagate_at_launch = excluded.propagate_at_launch",
        )
        .bind(group)
        .bind(&t.key)
        .bind(&t.value)
        .bind(t.propagate)
        .execute(&state.pool)
        .await?;
    }
    Ok(())
}

pub async fn create_auto_scaling_group(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(p, GROUP_PARAMS, GROUP_PREFIXES)?;
    check_group_options(p)?;
    let name = need(p, "AutoScalingGroupName")?;
    if name.len() > 255 {
        return Err(validation("AutoScalingGroupName must be 1 to 255 characters"));
    }
    machina_spec::validate_name(&name).map_err(|_| validation("AutoScalingGroupName may contain letters, digits, - and _ (at most 64 characters, starting with a letter or digit)"))?;
    let taken: Option<String> = crate::db::query_scalar("SELECT name FROM asg_groups WHERE name = ?").bind(&name).fetch_optional(&state.pool).await?;
    if taken.is_some() {
        return Err(Ec2Error::bad("AlreadyExists", format!("AutoScalingGroup by this name already exists - A group with the name {name} already exists")));
    }
    let min = int(p, "MinSize")?.ok_or_else(|| Ec2Error::bad("MissingParameter", "The request must contain the parameter MinSize"))?;
    let max = int(p, "MaxSize")?.ok_or_else(|| Ec2Error::bad("MissingParameter", "The request must contain the parameter MaxSize"))?;
    let desired = int(p, "DesiredCapacity")?.unwrap_or(min);
    let cooldown = int(p, "DefaultCooldown")?.unwrap_or(300);
    let grace = int(p, "HealthCheckGracePeriod")?.unwrap_or(0);
    if grace < 0 {
        return Err(validation("HealthCheckGracePeriod must not be negative"));
    }
    if min < 0 || max < min || desired < min || desired > max || max > 100 {
        return Err(validation("Desired capacity must be between the minimum and the maximum size, and the maximum size is at most 100"));
    }
    let subnet_id = single_subnet(p)?.ok_or_else(|| validation("At least one Availability Zone or VPC Subnet is required to create an AutoScalingGroup (VPCZoneIdentifier)"))?;
    let subnet = resolve(state, Kind::Subnet, &subnet_id, "ValidationError").await?;
    let placed: Option<(Uuid, String)> = crate::db::query_as(
        "SELECT v.project_id, h.hostname FROM cloud_subnets s JOIN cloud_vpcs v ON v.id = s.vpc_id JOIN hosts h ON h.id = v.host_id WHERE s.id = ?",
    )
    .bind(subnet)
    .fetch_optional(&state.pool)
    .await?;
    let (project, zone) = placed.ok_or_else(|| validation(format!("The subnet '{subnet_id}' does not exist")))?;
    for az in members(p, "AvailabilityZones") {
        if az != zone {
            return Err(validation(format!("The availability zone '{az}' is not the zone of the subnet ('{zone}')")));
        }
    }
    let tags = parse_group_tags(p)?;
    if tags.iter().any(|t| t.resource_id.as_deref().is_some_and(|r| r != name)) {
        return Err(validation("a tag's ResourceId must be the group's name"));
    }
    let source = launch_source(state, actor, p, project)
        .await?
        .ok_or_else(|| validation("Valid requests must contain either LaunchTemplate, LaunchConfigurationName, or InstanceId"))?;
    let policy: ScalingPolicy = serde_json::from_value(serde_json::json!({
        "min": min, "max": max, "desired": desired, "cooldown_secs": cooldown
    }))
    .map_err(|e| validation(e.to_string()))?;
    policy.validate().map_err(validation)?;
    let body: crate::api::cloud::elastic::CreateGroup = serde_json::from_value(serde_json::json!({
        "name": name, "template_id": source.template, "subnet_id": subnet, "policy": policy
    }))
    .map_err(|e| validation(e.to_string()))?;
    let Json(created) = crate::api::cloud::elastic::create_group(State(state.clone()), Extension(actor.clone()), Path(project), Json(body)).await.map_err(api_err)?;
    let id: Uuid = created.get("id").and_then(|v| v.as_str()).and_then(|s| Uuid::parse_str(s).ok()).ok_or_else(|| Ec2Error::new(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", "group id missing"))?;
    let stored = crate::db::query(
        "INSERT INTO asg_groups (group_id, name, launch_config_name, launch_template_version, health_check_type, health_check_grace) VALUES (?, ?, ?, ?, 'EC2', ?)",
    )
    .bind(id)
    .bind(&name)
    .bind(&source.launch_config)
    .bind(&source.version)
    .bind(grace)
    .execute(&state.pool)
    .await;
    if let Err(e) = stored {
        // do not leave an unmanaged group behind
        let _ = crate::api::cloud::elastic::delete_group(State(state.clone()), Extension(actor.clone()), Path(id)).await;
        return Err(e.into());
    }
    put_tags(state, id, &tags).await?;
    record(state, id, &format!("Group {name} created with desired capacity {desired}"), "a user request created the group.").await;
    Ok(String::new())
}

pub async fn update_auto_scaling_group(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(p, GROUP_PARAMS, GROUP_PREFIXES)?;
    check_group_options(p)?;
    let g = load_group(state, &need(p, "AutoScalingGroupName")?).await?;
    if let Some(subnet) = single_subnet(p)? {
        let wanted = resolve(state, Kind::Subnet, &subnet, "ValidationError").await?;
        if wanted != g.subnet {
            return Err(unsupported("moving a group to another subnet is not supported"));
        }
    }
    if !members(p, "AvailabilityZones").is_empty() {
        let zone = zone_of_subnet(state, g.subnet).await?;
        if members(p, "AvailabilityZones").iter().any(|az| az != &zone) {
            return Err(validation(format!("the group's zone is '{zone}'")));
        }
    }
    let mut policy = g.policy.clone();
    if let Some(v) = int(p, "MinSize")? {
        policy.min = u32::try_from(v).map_err(|_| validation("MinSize must not be negative"))?;
    }
    if let Some(v) = int(p, "MaxSize")? {
        policy.max = u32::try_from(v).map_err(|_| validation("MaxSize must not be negative"))?;
    }
    match int(p, "DesiredCapacity")? {
        Some(v) => policy.desired = u32::try_from(v).map_err(|_| validation("DesiredCapacity must not be negative"))?,
        // moving the limits past the current desired capacity moves the capacity with them
        None => policy.desired = policy.desired.clamp(policy.min, policy.max.max(policy.min)),
    }
    if let Some(v) = int(p, "DefaultCooldown")? {
        policy.cooldown_secs = u32::try_from(v).map_err(|_| validation("DefaultCooldown must not be negative"))?;
    }
    policy.validate().map_err(|e| validation(format!("{e}; Desired capacity must be between the minimum and the maximum size")))?;
    let grace = int(p, "HealthCheckGracePeriod")?;
    if grace.is_some_and(|v| v < 0) {
        return Err(validation("HealthCheckGracePeriod must not be negative"));
    }
    let source = launch_source(state, actor, p, g.project).await?;
    if let Some(s) = &source {
        crate::db::query("UPDATE cloud_instance_groups SET template_id = ? WHERE id = ?").bind(s.template).bind(g.id).execute(&state.pool).await?;
        crate::db::query("UPDATE asg_groups SET launch_config_name = ?, launch_template_version = ? WHERE group_id = ?")
            .bind(&s.launch_config)
            .bind(&s.version)
            .bind(g.id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(grace) = grace {
        crate::db::query("UPDATE asg_groups SET health_check_grace = ? WHERE group_id = ?").bind(grace).bind(g.id).execute(&state.pool).await?;
    }
    write_policy(state, actor, &g, &policy, g.paused).await?;
    if policy.desired != g.policy.desired {
        record(
            state,
            g.id,
            &format!("Changing capacity of {} from {} to {}", g.name, g.policy.desired, policy.desired),
            "a user request updated the group.",
        )
        .await;
    }
    Ok(String::new())
}

pub async fn delete_auto_scaling_group(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(p, &["AutoScalingGroupName", "ForceDelete"], &[])?;
    let g = load_group(state, &need(p, "AutoScalingGroupName")?).await?;
    if flag(p, "ForceDelete") {
        // stop the reconciler first, then take the members out and terminate them
        set_paused(state, g.id, true).await?;
        let vms: Vec<Uuid> = crate::db::query_scalar("SELECT vm_id FROM cloud_group_members WHERE group_id = ? AND vm_id IS NOT NULL").bind(g.id).fetch_all(&state.pool).await?;
        crate::db::query("DELETE FROM cloud_group_members WHERE group_id = ?").bind(g.id).execute(&state.pool).await?;
        for vm in vms {
            // an instance that is already gone is fine
            let _ = crate::api::vms::delete_vm(State(state.clone()), Extension(actor.clone()), Path(vm), None).await;
        }
    }
    match crate::api::cloud::elastic::delete_group(State(state.clone()), Extension(actor.clone()), Path(g.id)).await {
        Ok(_) => Ok(String::new()),
        Err(e) if e.status == StatusCode::CONFLICT => {
            Err(Ec2Error::bad("ResourceInUse", "You cannot delete an AutoScalingGroup while there are instances or pending Spot instance request(s) still in the group"))
        }
        Err(e) => {
            if flag(p, "ForceDelete") {
                let _ = set_paused(state, g.id, g.paused).await;
            }
            Err(api_err(e))
        }
    }
}

pub async fn describe_auto_scaling_groups(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(p, &["MaxRecords", "NextToken"], &["AutoScalingGroupNames.member."])?;
    let wanted = members(p, "AutoScalingGroupNames");
    let names: Vec<String> = all_group_names(state).await?.into_iter().filter(|n| wanted.is_empty() || wanted.contains(n)).collect();
    let (page, next) = paginate(&names, p.get("NextToken").map(String::as_str), max_records(p)?)?;
    let mut items = String::new();
    for name in page {
        let g = load_group(state, &name).await?;
        items.push_str(&group_xml(state, &g).await?);
    }
    Ok(format!("<AutoScalingGroups>{items}</AutoScalingGroups>{}", next_token_xml(&next)))
}

pub async fn describe_auto_scaling_instances(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(p, &["MaxRecords", "NextToken"], &["InstanceIds.member."])?;
    let wanted = members(p, "InstanceIds");
    let mut keyed: Vec<(String, String)> = Vec::new();
    for name in all_group_names(state).await? {
        let g = load_group(state, &name).await?;
        let zone = zone_of_subnet(state, g.subnet).await?;
        for i in instances(state, &g).await? {
            let id = ec2_id(Kind::Vm, i.vm);
            if !wanted.is_empty() && !wanted.contains(&id) {
                continue;
            }
            keyed.push((
                id.clone(),
                format!(
                    "<member><InstanceId>{id}</InstanceId><InstanceType></InstanceType><AutoScalingGroupName>{}</AutoScalingGroupName><AvailabilityZone>{}</AvailabilityZone><LifecycleState>{}</LifecycleState><HealthStatus>{}</HealthStatus><ProtectedFromScaleIn>false</ProtectedFromScaleIn></member>",
                    xml_escape(&g.name),
                    xml_escape(&zone),
                    lifecycle_state(&i.observed),
                    health_status(&i.observed)
                ),
            ));
        }
    }
    keyed.sort();
    let ids: Vec<String> = keyed.iter().map(|k| k.0.clone()).collect();
    let (page, next) = paginate(&ids, p.get("NextToken").map(String::as_str), max_records(p)?)?;
    let items: String = keyed.iter().filter(|k| page.contains(&k.0)).map(|k| k.1.as_str()).collect();
    Ok(format!("<AutoScalingInstances>{items}</AutoScalingInstances>{}", next_token_xml(&next)))
}

pub async fn set_desired_capacity(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(p, &["AutoScalingGroupName", "DesiredCapacity", "HonorCooldown"], &[])?;
    let g = load_group(state, &need(p, "AutoScalingGroupName")?).await?;
    let want = int(p, "DesiredCapacity")?.ok_or_else(|| Ec2Error::bad("MissingParameter", "The request must contain the parameter DesiredCapacity"))?;
    if want < i64::from(g.policy.min) {
        return Err(validation(format!("New SetDesiredCapacity value {want} is below min value {} for the AutoScalingGroup.", g.policy.min)));
    }
    if want > i64::from(g.policy.max) {
        return Err(validation(format!("New SetDesiredCapacity value {want} is above max value {} for the AutoScalingGroup.", g.policy.max)));
    }
    if flag(p, "HonorCooldown") && cooling_down(secs_since(state, &g).await?, i64::from(g.policy.cooldown_secs)) {
        return Err(Ec2Error::bad("ScalingActivityInProgress", "The group is in its cooldown period"));
    }
    let mut policy = g.policy.clone();
    policy.desired = want as u32;
    write_policy(state, actor, &g, &policy, g.paused).await?;
    record(state, g.id, &format!("Changing capacity of {} from {} to {want}", g.name, g.policy.desired), "a user request explicitly set the group desired capacity.").await;
    Ok(String::new())
}

async fn secs_since(state: &AppState, g: &Group) -> Result<i64, Ec2Error> {
    Ok(crate::db::query_scalar("SELECT CAST(strftime('%s','now') AS INTEGER) - CAST(strftime('%s', last_scaled_at) AS INTEGER) FROM cloud_instance_groups WHERE id = ?")
        .bind(g.id)
        .fetch_one(&state.pool)
        .await?)
}

pub async fn terminate_instance(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(p, &["InstanceId", "ShouldDecrementDesiredCapacity"], &[])?;
    let vm = resolve(state, Kind::Vm, &need(p, "InstanceId")?, "ValidationError").await?;
    let Some(group) = group_of_vm(state, vm).await? else {
        return Err(validation("Instance Id not found - No managed instance found for instance ID"));
    };
    let g = load_group(state, &group).await?;
    let decrement = p.get("ShouldDecrementDesiredCapacity").map(String::as_str);
    if !matches!(decrement, Some("true") | Some("false") | Some("1") | Some("0")) {
        return Err(Ec2Error::bad("MissingParameter", "The request must contain the parameter ShouldDecrementDesiredCapacity"));
    }
    let decrement = flag(p, "ShouldDecrementDesiredCapacity");
    if decrement && g.policy.desired <= g.policy.min {
        return Err(validation("Terminating instance without replacement will reduce the capacity below the minimum size of the group"));
    }
    let _ = crate::api::vms::delete_vm(State(state.clone()), Extension(actor.clone()), Path(vm), None).await?;
    // free the slot so the reconciler launches the replacement (or leaves it empty when the capacity shrinks)
    crate::db::query("UPDATE cloud_group_members SET vm_id = NULL WHERE group_id = ? AND vm_id = ?").bind(g.id).bind(vm).execute(&state.pool).await?;
    if decrement {
        let mut policy = g.policy.clone();
        policy.desired -= 1;
        write_policy(state, actor, &g, &policy, g.paused).await?;
    }
    record(state, g.id, &format!("Terminating EC2 instance: {}", ec2_id(Kind::Vm, vm)), "an instance was taken out of service in response to a user request.").await;
    Ok(format!(
        "<Activity><ActivityId>{}</ActivityId><AutoScalingGroupName>{}</AutoScalingGroupName><Description>Terminating EC2 instance: {}</Description><Cause>a user request</Cause><StartTime>{}</StartTime><StatusCode>InProgress</StatusCode><Progress>0</Progress></Activity>",
        Uuid::new_v4(),
        xml_escape(&g.name),
        ec2_id(Kind::Vm, vm),
        now_text()
    ))
}

/// Pauses the reconciler for the group while its membership changes, and restores the flag whatever happens.
async fn with_paused<T, F>(state: &AppState, g: &Group, f: F) -> Result<T, Ec2Error>
where
    F: std::future::Future<Output = Result<T, Ec2Error>>,
{
    set_paused(state, g.id, true).await?;
    let out = f.await;
    set_paused(state, g.id, g.paused).await?;
    out
}

pub async fn attach_instances(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(p, &["AutoScalingGroupName"], &["InstanceIds.member."])?;
    let g = load_group(state, &need(p, "AutoScalingGroupName")?).await?;
    let ids = members(p, "InstanceIds");
    if ids.is_empty() {
        return Err(Ec2Error::bad("MissingParameter", "The request must contain the parameter InstanceIds.member.1"));
    }
    let project_name: String = crate::db::query_scalar("SELECT name FROM projects WHERE id = ?").bind(g.project).fetch_one(&state.pool).await?;
    let group_host: Uuid = crate::db::query_scalar("SELECT v.host_id FROM cloud_subnets s JOIN cloud_vpcs v ON v.id = s.vpc_id WHERE s.id = ?").bind(g.subnet).fetch_one(&state.pool).await?;
    let mut vms = Vec::new();
    for id in &ids {
        let vm = resolve(state, Kind::Vm, id, "ValidationError").await?;
        let (project, host): (Option<String>, Option<Uuid>) = crate::db::query_as("SELECT project, host_id FROM vms WHERE id = ?").bind(vm).fetch_one(&state.pool).await?;
        if project.as_deref() != Some(project_name.as_str()) {
            return Err(validation(format!("The instance {id} is not in the group's project")));
        }
        if host != Some(group_host) {
            return Err(validation(format!("The instance {id} is not on the group's host (zone)")));
        }
        if group_of_vm(state, vm).await?.is_some() {
            return Err(validation(format!("The instance {id} is already part of an AutoScalingGroup")));
        }
        vms.push(vm);
    }
    let new_desired = g.policy.desired as usize + vms.len();
    if new_desired > g.policy.max as usize {
        return Err(validation(format!("Attaching {} instance(s) would exceed the maximum size ({}) of the group", vms.len(), g.policy.max)));
    }
    with_paused(state, &g, async {
        let taken: Vec<i64> = crate::db::query_scalar("SELECT slot FROM cloud_group_members WHERE group_id = ? AND vm_id IS NOT NULL").bind(g.id).fetch_all(&state.pool).await?;
        let free: Vec<i64> = (0..new_desired as i64).filter(|s| !taken.contains(s)).collect();
        if free.len() < vms.len() {
            return Err(validation("the group has no free slot for these instances"));
        }
        for (vm, slot) in vms.iter().zip(free) {
            crate::db::query(
                "INSERT INTO cloud_group_members (group_id, slot, vm_id) VALUES (?, ?, ?) \
                 ON CONFLICT (group_id, slot) DO UPDATE SET vm_id = excluded.vm_id",
            )
            .bind(g.id)
            .bind(slot)
            .bind(*vm)
            .execute(&state.pool)
            .await?;
        }
        let mut policy = g.policy.clone();
        policy.desired = new_desired as u32;
        write_policy(state, actor, &g, &policy, true).await
    })
    .await?;
    record(state, g.id, &format!("Attaching {} instance(s) to {}", vms.len(), g.name), "a user request attached instances.").await;
    Ok(String::new())
}

pub async fn detach_instances(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(p, &["AutoScalingGroupName", "ShouldDecrementDesiredCapacity"], &["InstanceIds.member."])?;
    let g = load_group(state, &need(p, "AutoScalingGroupName")?).await?;
    let ids = members(p, "InstanceIds");
    if ids.is_empty() {
        return Err(Ec2Error::bad("MissingParameter", "The request must contain the parameter InstanceIds.member.1"));
    }
    if !p.contains_key("ShouldDecrementDesiredCapacity") {
        return Err(Ec2Error::bad("MissingParameter", "The request must contain the parameter ShouldDecrementDesiredCapacity"));
    }
    let decrement = flag(p, "ShouldDecrementDesiredCapacity");
    let mut vms = Vec::new();
    for id in &ids {
        let vm = resolve(state, Kind::Vm, id, "ValidationError").await?;
        if group_of_vm(state, vm).await?.as_deref() != Some(g.name.as_str()) {
            return Err(validation(format!("The instance {id} is not part of the group {}", g.name)));
        }
        vms.push(vm);
    }
    if decrement && (g.policy.desired as usize) < vms.len() + g.policy.min as usize {
        return Err(validation("Detaching the instances would reduce the capacity below the minimum size of the group"));
    }
    with_paused(state, &g, async {
        for vm in &vms {
            crate::db::query("UPDATE cloud_group_members SET vm_id = NULL WHERE group_id = ? AND vm_id = ?").bind(g.id).bind(*vm).execute(&state.pool).await?;
        }
        if decrement {
            let mut policy = g.policy.clone();
            policy.desired -= vms.len() as u32;
            write_policy(state, actor, &g, &policy, true).await?;
        }
        Ok(())
    })
    .await?;
    record(state, g.id, &format!("Detaching {} instance(s) from {}", vms.len(), g.name), "a user request detached instances.").await;
    Ok(String::new())
}

// ---- processes ----------------------------------------------------------------------------------------------

async fn change_processes(state: &AppState, actor: &AuthUser, p: &Params, suspend: bool) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(p, &["AutoScalingGroupName"], &["ScalingProcesses.member."])?;
    let g = load_group(state, &need(p, "AutoScalingGroupName")?).await?;
    let mut named = members(p, "ScalingProcesses");
    if let Some(bad) = named.iter().find(|n| !PROCESSES.contains(&n.as_str())) {
        return Err(validation(format!("{bad} is not a valid process name")));
    }
    if named.is_empty() {
        named = PROCESSES.iter().map(|s| s.to_string()).collect();
    }
    let mut set = g.suspended.clone();
    if suspend {
        for n in named {
            if !set.contains(&n) {
                set.push(n);
            }
        }
    } else {
        set.retain(|n| !named.contains(n));
    }
    let launch = set.iter().any(|n| n == "Launch");
    let terminate = set.iter().any(|n| n == "Terminate");
    if launch != terminate {
        return Err(unsupported("Launch and Terminate can only be suspended together; the group reconciler pauses as a whole"));
    }
    set.sort();
    crate::db::query("UPDATE asg_groups SET suspended = ? WHERE group_id = ?").bind(set.join(",")).bind(g.id).execute(&state.pool).await?;
    set_paused(state, g.id, launch && terminate).await?;
    Ok(String::new())
}

pub async fn suspend_processes(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    change_processes(state, actor, p, true).await
}

pub async fn resume_processes(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    change_processes(state, actor, p, false).await
}

// ---- scaling policies ---------------------------------------------------------------------------------------

type PolicyRow = (Uuid, Uuid, String, String, String, i64, i64, i64, String, Option<f64>, bool);

const POLICY_SELECT: &str = "SELECT id, group_id, name, policy_type, adjustment_type, scaling_adjustment, min_adjustment_magnitude, cooldown, steps_json, target_value, enabled FROM asg_policies";

fn parse_steps(p: &Params) -> Result<Vec<Step>, Ec2Error> {
    let mut out = Vec::new();
    for n in 1..=20 {
        let Some(adj) = p.get(&format!("StepAdjustments.member.{n}.ScalingAdjustment")) else { break };
        let bound = |k: &str| -> Result<Option<f64>, Ec2Error> {
            p.get(&format!("StepAdjustments.member.{n}.{k}"))
                .map(|v| v.parse::<f64>().map_err(|_| validation(format!("{k} must be a number"))))
                .transpose()
        };
        out.push(Step {
            lower: bound("MetricIntervalLowerBound")?,
            upper: bound("MetricIntervalUpperBound")?,
            adjustment: adj.parse().map_err(|_| validation("ScalingAdjustment must be a number"))?,
        });
    }
    Ok(out)
}

pub async fn put_scaling_policy(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(
        p,
        &[
            "AutoScalingGroupName",
            "PolicyName",
            "PolicyType",
            "AdjustmentType",
            "ScalingAdjustment",
            "MinAdjustmentMagnitude",
            "Cooldown",
            "MetricAggregationType",
            "EstimatedInstanceWarmup",
            "Enabled",
            "TargetTrackingConfiguration.PredefinedMetricSpecification.PredefinedMetricType",
            "TargetTrackingConfiguration.TargetValue",
            "TargetTrackingConfiguration.DisableScaleIn",
        ],
        &["StepAdjustments.member."],
    )?;
    let g = load_group(state, &need(p, "AutoScalingGroupName")?).await?;
    let name = need(p, "PolicyName")?;
    let kind = p.get("PolicyType").map(String::as_str).unwrap_or("SimpleScaling");
    let mut adjustment_type = String::new();
    let mut adjustment = 0i64;
    let mut steps: Vec<Step> = Vec::new();
    let mut target: Option<f64> = None;
    match kind {
        "SimpleScaling" | "StepScaling" => {
            let at = need(p, "AdjustmentType")?;
            if AdjustmentType::parse(&at).is_none() {
                return Err(validation(format!("AdjustmentType must be ChangeInCapacity, ExactCapacity or PercentChangeInCapacity, not {at}")));
            }
            adjustment_type = at;
            if kind == "SimpleScaling" {
                adjustment = int(p, "ScalingAdjustment")?.ok_or_else(|| Ec2Error::bad("MissingParameter", "The request must contain the parameter ScalingAdjustment"))?;
            } else {
                steps = parse_steps(p)?;
                if steps.is_empty() {
                    return Err(validation("A StepScaling policy needs at least one StepAdjustments entry"));
                }
            }
        }
        "TargetTrackingScaling" => {
            let metric = p.get("TargetTrackingConfiguration.PredefinedMetricSpecification.PredefinedMetricType").map(String::as_str);
            if metric != Some("ASGAverageCPUUtilization") {
                return Err(unsupported("target tracking supports only the predefined metric ASGAverageCPUUtilization"));
            }
            if flag(p, "TargetTrackingConfiguration.DisableScaleIn") {
                return Err(unsupported("DisableScaleIn is not supported for target tracking"));
            }
            let value: f64 = p
                .get("TargetTrackingConfiguration.TargetValue")
                .ok_or_else(|| Ec2Error::bad("MissingParameter", "The request must contain the parameter TargetTrackingConfiguration.TargetValue"))?
                .parse()
                .map_err(|_| validation("TargetValue must be a number"))?;
            if !value.is_finite() || !(10.0..=90.0).contains(&value) {
                return Err(validation("TargetValue must be between 10 and 90 percent CPU"));
            }
            target = Some(value);
        }
        other => return Err(unsupported(format!("PolicyType {other} is not supported (SimpleScaling, StepScaling or TargetTrackingScaling)"))),
    }
    let cooldown = int(p, "Cooldown")?.unwrap_or(0);
    let magnitude = int(p, "MinAdjustmentMagnitude")?.unwrap_or(0);
    if cooldown < 0 || magnitude < 0 {
        return Err(validation("Cooldown and MinAdjustmentMagnitude must not be negative"));
    }
    let enabled = p.get("Enabled").map(|v| v != "false").unwrap_or(true);
    if let Some(value) = target {
        // the reconciler has one CPU target per group
        let other: Option<String> = crate::db::query_scalar("SELECT name FROM asg_policies WHERE group_id = ? AND policy_type = 'TargetTrackingScaling' AND name <> ?")
            .bind(g.id)
            .bind(&name)
            .fetch_optional(&state.pool)
            .await?;
        if let Some(other) = other {
            return Err(unsupported(format!("a group has one target tracking policy (already {other})")));
        }
        let mut policy = g.policy.clone();
        policy.target_cpu = Some(value);
        write_policy(state, actor, &g, &policy, g.paused).await?;
    }
    crate::db::query(
        "INSERT INTO asg_policies (id, group_id, name, policy_type, adjustment_type, scaling_adjustment, min_adjustment_magnitude, cooldown, steps_json, target_value, enabled) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (group_id, name) DO UPDATE SET policy_type = excluded.policy_type, adjustment_type = excluded.adjustment_type, \
         scaling_adjustment = excluded.scaling_adjustment, min_adjustment_magnitude = excluded.min_adjustment_magnitude, cooldown = excluded.cooldown, \
         steps_json = excluded.steps_json, target_value = excluded.target_value, enabled = excluded.enabled",
    )
    .bind(Uuid::new_v4())
    .bind(g.id)
    .bind(&name)
    .bind(kind)
    .bind(&adjustment_type)
    .bind(adjustment)
    .bind(magnitude)
    .bind(cooldown)
    .bind(serde_json::to_string(&steps).map_err(|e| validation(e.to_string()))?)
    .bind(target)
    .bind(enabled)
    .execute(&state.pool)
    .await?;
    let id: Uuid = crate::db::query_scalar("SELECT id FROM asg_policies WHERE group_id = ? AND name = ?").bind(g.id).bind(&name).fetch_one(&state.pool).await?;
    Ok(format!("<PolicyARN>{}</PolicyARN><Alarms/>", xml_escape(&policy_arn(id, &g.name, &name))))
}

fn policy_xml(group: &str, r: &PolicyRow) -> String {
    let (id, _, name, kind, adjustment_type, adjustment, magnitude, cooldown, steps, target, enabled) = r;
    let mut out = format!(
        "<member><AutoScalingGroupName>{}</AutoScalingGroupName><PolicyName>{}</PolicyName><PolicyARN>{}</PolicyARN><PolicyType>{kind}</PolicyType>",
        xml_escape(group),
        xml_escape(name),
        xml_escape(&policy_arn(*id, group, name))
    );
    if !adjustment_type.is_empty() {
        out.push_str(&format!("<AdjustmentType>{adjustment_type}</AdjustmentType>"));
    }
    if kind == "SimpleScaling" {
        out.push_str(&format!("<ScalingAdjustment>{adjustment}</ScalingAdjustment>"));
        if *cooldown > 0 {
            out.push_str(&format!("<Cooldown>{cooldown}</Cooldown>"));
        }
    }
    if *magnitude > 0 {
        out.push_str(&format!("<MinAdjustmentMagnitude>{magnitude}</MinAdjustmentMagnitude>"));
    }
    if kind == "StepScaling" {
        let parsed: Vec<Step> = serde_json::from_str(steps).unwrap_or_default();
        let items: String = parsed
            .iter()
            .map(|s| {
                let lower = s.lower.map(|v| format!("<MetricIntervalLowerBound>{v}</MetricIntervalLowerBound>")).unwrap_or_default();
                let upper = s.upper.map(|v| format!("<MetricIntervalUpperBound>{v}</MetricIntervalUpperBound>")).unwrap_or_default();
                format!("<member>{lower}{upper}<ScalingAdjustment>{}</ScalingAdjustment></member>", s.adjustment)
            })
            .collect();
        out.push_str(&format!("<StepAdjustments>{items}</StepAdjustments>"));
    }
    if let Some(value) = target {
        out.push_str(&format!(
            "<TargetTrackingConfiguration><PredefinedMetricSpecification><PredefinedMetricType>ASGAverageCPUUtilization</PredefinedMetricType></PredefinedMetricSpecification><TargetValue>{value}</TargetValue><DisableScaleIn>false</DisableScaleIn></TargetTrackingConfiguration>"
        ));
    }
    out.push_str(&format!("<Alarms/><Enabled>{enabled}</Enabled></member>"));
    out
}

pub async fn describe_policies(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(p, &["AutoScalingGroupName", "MaxRecords", "NextToken"], &["PolicyNames.member.", "PolicyTypes.member."])?;
    let names = members(p, "PolicyNames");
    let types = members(p, "PolicyTypes");
    let groups: Vec<String> = match p.get("AutoScalingGroupName") {
        Some(g) => {
            load_group(state, g).await?;
            vec![g.clone()]
        }
        None => all_group_names(state).await?,
    };
    let mut keyed: Vec<(String, String)> = Vec::new();
    for group in groups {
        let g = load_group(state, &group).await?;
        let rows: Vec<PolicyRow> = crate::db::query_as(&format!("{POLICY_SELECT} WHERE group_id = ? ORDER BY name")).bind(g.id).fetch_all(&state.pool).await?;
        for r in rows {
            if (!names.is_empty() && !names.contains(&r.2) && !names.iter().any(|n| policy_id_from_arn(n) == Some(r.0)))
                || (!types.is_empty() && !types.contains(&r.3))
            {
                continue;
            }
            keyed.push((format!("{}/{}", g.name, r.2), policy_xml(&g.name, &r)));
        }
    }
    keyed.sort();
    let keys: Vec<String> = keyed.iter().map(|k| k.0.clone()).collect();
    let (page, next) = paginate(&keys, p.get("NextToken").map(String::as_str), max_records(p)?)?;
    let items: String = keyed.iter().filter(|k| page.contains(&k.0)).map(|k| k.1.as_str()).collect();
    Ok(format!("<ScalingPolicies>{items}</ScalingPolicies>{}", next_token_xml(&next)))
}

/// A policy by name (with its group) or by ARN.
async fn find_policy(state: &AppState, p: &Params) -> Result<(Group, PolicyRow), Ec2Error> {
    let reference = need(p, "PolicyName")?;
    if let Some(id) = policy_id_from_arn(&reference) {
        let row: Option<PolicyRow> = crate::db::query_as(&format!("{POLICY_SELECT} WHERE id = ?")).bind(id).fetch_optional(&state.pool).await?;
        let row = row.ok_or_else(|| validation(format!("No scaling policy found for {reference}")))?;
        let name: String = crate::db::query_scalar("SELECT name FROM asg_groups WHERE group_id = ?").bind(row.1).fetch_one(&state.pool).await?;
        return Ok((load_group(state, &name).await?, row));
    }
    let g = load_group(state, &need(p, "AutoScalingGroupName")?).await?;
    let row: Option<PolicyRow> = crate::db::query_as(&format!("{POLICY_SELECT} WHERE group_id = ? AND name = ?")).bind(g.id).bind(&reference).fetch_optional(&state.pool).await?;
    let row = row.ok_or_else(|| validation(format!("No scaling policy found for {reference} in group {}", g.name)))?;
    Ok((g, row))
}

pub async fn delete_policy(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(p, &["AutoScalingGroupName", "PolicyName"], &[])?;
    let (g, row) = find_policy(state, p).await?;
    if row.9.is_some() {
        let mut policy = g.policy.clone();
        policy.target_cpu = None;
        policy.predictive = false;
        write_policy(state, actor, &g, &policy, g.paused).await?;
    }
    crate::db::query("DELETE FROM asg_policies WHERE id = ?").bind(row.0).execute(&state.pool).await?;
    Ok(String::new())
}

pub async fn execute_policy(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(p, &["AutoScalingGroupName", "PolicyName", "HonorCooldown", "MetricValue", "BreachThreshold"], &[])?;
    let (g, row) = find_policy(state, p).await?;
    let (_, _, name, kind, adjustment_type, scaling_adjustment, magnitude, cooldown, steps_json, _, enabled) = &row;
    if !enabled {
        return Err(validation(format!("The policy {name} is disabled")));
    }
    let at = AdjustmentType::parse(adjustment_type).ok_or_else(|| validation("target tracking policies cannot be executed"))?;
    let adjustment = match kind.as_str() {
        "SimpleScaling" => *scaling_adjustment,
        "StepScaling" => {
            let value: f64 = p.get("MetricValue").and_then(|v| v.parse().ok()).ok_or_else(|| validation("MetricValue is required to execute a step scaling policy"))?;
            let threshold: f64 = p.get("BreachThreshold").and_then(|v| v.parse().ok()).ok_or_else(|| validation("BreachThreshold is required to execute a step scaling policy"))?;
            let steps: Vec<Step> = serde_json::from_str(steps_json).unwrap_or_default();
            match pick_step(&steps, value - threshold) {
                Some(step) => step.adjustment,
                None => return Err(validation("MetricValue does not fall into any step of the policy")),
            }
        }
        _ => return Err(validation("target tracking policies cannot be executed")),
    };
    if flag(p, "HonorCooldown") && cooling_down(secs_since(state, &g).await?, effective_cooldown(*cooldown, g.policy.cooldown_secs)) {
        return Err(Ec2Error::bad("ScalingActivityInProgress", "The group is in its cooldown period"));
    }
    let want = target_capacity(at, adjustment, *magnitude, g.policy.desired, g.policy.min, g.policy.max);
    if want != g.policy.desired {
        let mut policy = g.policy.clone();
        policy.desired = want;
        write_policy(state, actor, &g, &policy, g.paused).await?;
        record(state, g.id, &format!("Changing capacity of {} from {} to {want}", g.name, g.policy.desired), &format!("policy {name} was executed.")).await;
    }
    Ok(String::new())
}

/// An alarm action that names a scaling policy: the group and the step an alarm should apply. Only a policy that is
/// one fixed `ChangeInCapacity` can be an alarm action, because an alarm carries a single integer step.
pub async fn alarm_target(state: &AppState, arn: &str) -> Result<(Uuid, i64), Ec2Error> {
    let id = policy_id_from_arn(arn).ok_or_else(|| validation(format!("'{arn}' is not a scaling policy ARN")))?;
    let row: Option<PolicyRow> = crate::db::query_as(&format!("{POLICY_SELECT} WHERE id = ?")).bind(id).fetch_optional(&state.pool).await?;
    let (_, group, _, kind, adjustment_type, adjustment, _, _, steps, _, _) = row.ok_or_else(|| validation(format!("No scaling policy found for {arn}")))?;
    if adjustment_type != "ChangeInCapacity" {
        return Err(unsupported("an alarm can only run a scaling policy whose AdjustmentType is ChangeInCapacity"));
    }
    match kind.as_str() {
        "SimpleScaling" => Ok((group, adjustment)),
        "StepScaling" => {
            let parsed: Vec<Step> = serde_json::from_str(&steps).unwrap_or_default();
            match parsed.as_slice() {
                [only] => Ok((group, only.adjustment)),
                _ => Err(unsupported("an alarm can only run a step scaling policy with exactly one step")),
            }
        }
        _ => Err(unsupported("an alarm cannot run a target tracking policy")),
    }
}

// ---- activities and tags ------------------------------------------------------------------------------------

type ActivityRow = (Uuid, Uuid, String, String, String, String, String);

pub async fn describe_scaling_activities(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(p, &["AutoScalingGroupName", "MaxRecords", "NextToken"], &["ActivityIds.member."])?;
    let ids = members(p, "ActivityIds");
    let group = match p.get("AutoScalingGroupName") {
        Some(n) => Some(load_group(state, n).await?),
        None => None,
    };
    let rows: Vec<ActivityRow> = crate::db::query_as(
        "SELECT id, group_id, description, cause, status_code, status_message, start_time FROM asg_activities ORDER BY start_time DESC, id DESC LIMIT 500",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut names: BTreeMap<Uuid, String> = BTreeMap::new();
    let mut keyed: Vec<(String, String)> = Vec::new();
    for (id, gid, description, cause, status, message, start) in rows {
        if group.as_ref().is_some_and(|g| g.id != gid) || (!ids.is_empty() && !ids.contains(&id.to_string())) {
            continue;
        }
        if !names.contains_key(&gid) {
            let n: Option<String> = crate::db::query_scalar("SELECT name FROM asg_groups WHERE group_id = ?").bind(gid).fetch_optional(&state.pool).await?;
            names.insert(gid, n.unwrap_or_default());
        }
        let key = format!("{}/{id}", iso(&start));
        keyed.push((
            key,
            format!(
                "<member><ActivityId>{id}</ActivityId><AutoScalingGroupName>{}</AutoScalingGroupName><Description>{}</Description><Cause>{}</Cause><StartTime>{}</StartTime><EndTime>{}</EndTime><StatusCode>{status}</StatusCode>{}<Progress>100</Progress></member>",
                xml_escape(&names[&gid]),
                xml_escape(&description),
                xml_escape(&cause),
                iso(&start),
                iso(&start),
                if message.is_empty() { String::new() } else { format!("<StatusMessage>{}</StatusMessage>", xml_escape(&message)) }
            ),
        ));
    }
    // newest first; the token is the key of the last activity returned, so page on the reversed order
    keyed.sort_by(|a, b| b.0.cmp(&a.0));
    let limit = max_records(p)?;
    let start = match p.get("NextToken") {
        None => 0,
        Some(t) => {
            let last = token_decode(t)?;
            keyed.iter().position(|k| k.0 == last).map(|i| i + 1).unwrap_or(keyed.len())
        }
    };
    let rest = &keyed[start.min(keyed.len())..];
    let page = &rest[..rest.len().min(limit)];
    let next = (rest.len() > limit).then(|| token_encode(&page[page.len() - 1].0));
    let items: String = page.iter().map(|k| k.1.as_str()).collect();
    Ok(format!("<Activities>{items}</Activities>{}", next_token_xml(&next)))
}

pub async fn create_or_update_tags(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(p, &[], &["Tags.member."])?;
    let tags = parse_group_tags(p)?;
    if tags.is_empty() {
        return Err(Ec2Error::bad("MissingParameter", "The request must contain the parameter Tags.member.1.Key"));
    }
    for (n, t) in tags.iter().enumerate() {
        let group = t.resource_id.as_deref().ok_or_else(|| validation(format!("Tags.member.{}.ResourceId is required", n + 1)))?;
        let g = load_group(state, group).await?;
        put_tags(state, g.id, std::slice::from_ref(t)).await?;
    }
    Ok(String::new())
}

pub async fn delete_tags(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(p, &[], &["Tags.member."])?;
    let tags = parse_group_tags(p)?;
    if tags.is_empty() {
        return Err(Ec2Error::bad("MissingParameter", "The request must contain the parameter Tags.member.1.Key"));
    }
    for (n, t) in tags.iter().enumerate() {
        let group = t.resource_id.as_deref().ok_or_else(|| validation(format!("Tags.member.{}.ResourceId is required", n + 1)))?;
        let g = load_group(state, group).await?;
        crate::db::query("DELETE FROM asg_tags WHERE group_id = ? AND tag_key = ?").bind(g.id).bind(&t.key).execute(&state.pool).await?;
    }
    Ok(String::new())
}

pub async fn describe_tags(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    allow_only(p, &["MaxRecords", "NextToken"], &["Filters.member."])?;
    let mut filters: Vec<(String, Vec<String>)> = Vec::new();
    for n in 1..=20 {
        let Some(name) = p.get(&format!("Filters.member.{n}.Name")) else { break };
        if !matches!(name.as_str(), "auto-scaling-group" | "key" | "value" | "propagate-at-launch") {
            return Err(validation(format!("Unsupported filter name {name}")));
        }
        filters.push((name.clone(), members(p, &format!("Filters.member.{n}.Values"))));
    }
    let mut keyed: Vec<(String, String)> = Vec::new();
    for group in all_group_names(state).await? {
        let g = load_group(state, &group).await?;
        for (k, v, pr) in tags_of(state, g.id).await? {
            let keep = filters.iter().all(|(name, values)| {
                values.is_empty()
                    || match name.as_str() {
                        "auto-scaling-group" => values.contains(&g.name),
                        "key" => values.contains(&k),
                        "value" => values.contains(&v),
                        _ => values.contains(&pr.to_string()),
                    }
            });
            if keep {
                keyed.push((format!("{}/{k}", g.name), tag_member(&g.name, &k, &v, pr)));
            }
        }
    }
    keyed.sort();
    let keys: Vec<String> = keyed.iter().map(|k| k.0.clone()).collect();
    let (page, next) = paginate(&keys, p.get("NextToken").map(String::as_str), max_records(p)?)?;
    let items: String = keyed.iter().filter(|k| page.contains(&k.0)).map(|k| k.1.as_str()).collect();
    Ok(format!("<Tags>{items}</Tags>{}", next_token_xml(&next)))
}

// ---- small fixed answers --------------------------------------------------------------------------------------

fn static_list(tag: &str, items: &[&str]) -> String {
    let body: String = items.iter().map(|i| format!("<member>{i}</member>")).collect();
    format!("<{tag}>{body}</{tag}>")
}

async fn describe_account_limits(state: &AppState, actor: &AuthUser) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let groups: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM asg_groups").fetch_one(&state.pool).await?;
    let configs: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM asg_launch_configs").fetch_one(&state.pool).await?;
    Ok(format!(
        "<MaxNumberOfAutoScalingGroups>200</MaxNumberOfAutoScalingGroups><MaxNumberOfLaunchConfigurations>200</MaxNumberOfLaunchConfigurations>\
<NumberOfAutoScalingGroups>{groups}</NumberOfAutoScalingGroups><NumberOfLaunchConfigurations>{configs}</NumberOfLaunchConfigurations>"
    ))
}

/// Dispatches one Auto Scaling action; the result is the final body of `<Action>Result` (empty = no result element).
pub async fn dispatch(state: &AppState, actor: &AuthUser, p: &Params, action: &str) -> Result<String, Ec2Error> {
    Ok(match action {
        "CreateAutoScalingGroup" => create_auto_scaling_group(state, actor, p).await?,
        "UpdateAutoScalingGroup" => update_auto_scaling_group(state, actor, p).await?,
        "DeleteAutoScalingGroup" => delete_auto_scaling_group(state, actor, p).await?,
        "DescribeAutoScalingGroups" => describe_auto_scaling_groups(state, actor, p).await?,
        "DescribeAutoScalingInstances" => describe_auto_scaling_instances(state, actor, p).await?,
        "SetDesiredCapacity" => set_desired_capacity(state, actor, p).await?,
        "TerminateInstanceInAutoScalingGroup" => terminate_instance(state, actor, p).await?,
        "AttachInstances" => attach_instances(state, actor, p).await?,
        "DetachInstances" => detach_instances(state, actor, p).await?,
        "SuspendProcesses" => suspend_processes(state, actor, p).await?,
        "ResumeProcesses" => resume_processes(state, actor, p).await?,
        "CreateLaunchConfiguration" => create_launch_configuration(state, actor, p).await?,
        "DescribeLaunchConfigurations" => describe_launch_configurations(state, actor, p).await?,
        "DeleteLaunchConfiguration" => delete_launch_configuration(state, actor, p).await?,
        "PutScalingPolicy" => put_scaling_policy(state, actor, p).await?,
        "DescribePolicies" => describe_policies(state, actor, p).await?,
        "DeletePolicy" => delete_policy(state, actor, p).await?,
        "ExecutePolicy" => execute_policy(state, actor, p).await?,
        "DescribeScalingActivities" => describe_scaling_activities(state, actor, p).await?,
        "CreateOrUpdateTags" => create_or_update_tags(state, actor, p).await?,
        "DeleteTags" => delete_tags(state, actor, p).await?,
        "DescribeTags" => describe_tags(state, actor, p).await?,
        "DescribeAccountLimits" => describe_account_limits(state, actor).await?,
        "DescribeAdjustmentTypes" => "<AdjustmentTypes><member><AdjustmentType>ChangeInCapacity</AdjustmentType></member><member><AdjustmentType>ExactCapacity</AdjustmentType></member><member><AdjustmentType>PercentChangeInCapacity</AdjustmentType></member></AdjustmentTypes>".into(),
        "DescribeTerminationPolicyTypes" => static_list("TerminationPolicyTypes", &["Default"]),
        "DescribeAutoScalingNotificationTypes" => static_list("AutoScalingNotificationTypes", &["autoscaling:EC2_INSTANCE_LAUNCH", "autoscaling:EC2_INSTANCE_LAUNCH_ERROR", "autoscaling:EC2_INSTANCE_TERMINATE", "autoscaling:EC2_INSTANCE_TERMINATE_ERROR", "autoscaling:TEST_NOTIFICATION"]),
        "DescribeScalingProcessTypes" => {
            let items: String = PROCESSES.iter().map(|n| format!("<member><ProcessName>{n}</ProcessName></member>")).collect();
            format!("<Processes>{items}</Processes>")
        }
        "DescribeMetricCollectionTypes" => "<Metrics/><Granularities><member><Granularity>1Minute</Granularity></member></Granularities>".into(),
        // lists this service never fills, answered with valid empty lists (a client lists them while it reads a group)
        "DescribeLifecycleHooks" => "<LifecycleHooks/>".into(),
        "DescribeNotificationConfigurations" => "<NotificationConfigurations/>".into(),
        "DescribeScheduledActions" => "<ScheduledUpdateGroupActions/>".into(),
        "DescribeInstanceRefreshes" => "<InstanceRefreshes/>".into(),
        "DescribeLoadBalancers" => "<LoadBalancers/>".into(),
        "DescribeLoadBalancerTargetGroups" => "<LoadBalancerTargetGroups/>".into(),
        "DescribeWarmPool" => "<Instances/>".into(),
        "EnterStandby" | "ExitStandby" => return Err(unsupported("standby is not supported; detach the instance instead")),
        "EnableMetricsCollection" | "DisableMetricsCollection" => return Err(unsupported("group metrics collection is not supported")),
        "PutLifecycleHook" | "PutNotificationConfiguration" | "PutScheduledUpdateGroupAction" | "PutWarmPool" | "StartInstanceRefresh" | "AttachLoadBalancers"
        | "AttachLoadBalancerTargetGroups" | "SetInstanceProtection" | "SetInstanceHealth" => {
            return Err(unsupported(format!("{action} is not supported by this Auto Scaling service")))
        }
        _ => return Err(Ec2Error::new(StatusCode::BAD_REQUEST, "InvalidAction", format!("The action {action} is not valid for this web service."))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> Params {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn admin() -> AuthUser {
        AuthUser { username: "tester".into(), role: "admin".into(), auth_source: Some("ec2-access-key".into()) }
    }

    // ---- pure functions ----

    #[test]
    fn capacity_adjustments_clamp_to_the_limits() {
        use AdjustmentType::*;
        assert_eq!(target_capacity(ChangeInCapacity, 2, 0, 3, 1, 10), 5);
        assert_eq!(target_capacity(ChangeInCapacity, -9, 0, 3, 1, 10), 1, "never below the minimum");
        assert_eq!(target_capacity(ChangeInCapacity, 99, 0, 3, 1, 10), 10, "never above the maximum");
        assert_eq!(target_capacity(ExactCapacity, 7, 0, 3, 1, 10), 7);
        assert_eq!(target_capacity(ExactCapacity, 0, 0, 3, 2, 10), 2);
    }

    #[test]
    fn percent_changes_truncate_and_honour_the_minimum_magnitude() {
        use AdjustmentType::PercentChangeInCapacity as Pct;
        assert_eq!(target_capacity(Pct, 50, 0, 10, 0, 100), 15);
        assert_eq!(target_capacity(Pct, 10, 0, 4, 0, 100), 4, "10 percent of 4 truncates to nothing");
        assert_eq!(target_capacity(Pct, 10, 1, 4, 0, 100), 5, "a minimum magnitude of 1 moves it by one");
        assert_eq!(target_capacity(Pct, -10, 2, 20, 0, 100), 18);
        assert_eq!(target_capacity(Pct, 0, 5, 20, 0, 100), 20, "a zero adjustment stays zero");
    }

    #[test]
    fn steps_pick_by_distance_from_the_threshold() {
        let steps = vec![
            Step { lower: None, upper: Some(0.0), adjustment: -1 },
            Step { lower: Some(0.0), upper: Some(10.0), adjustment: 1 },
            Step { lower: Some(10.0), upper: None, adjustment: 3 },
        ];
        assert_eq!(pick_step(&steps, -5.0).map(|s| s.adjustment), Some(-1));
        assert_eq!(pick_step(&steps, 0.0).map(|s| s.adjustment), Some(1), "the lower bound is inclusive");
        assert_eq!(pick_step(&steps, 10.0).map(|s| s.adjustment), Some(3), "the upper bound is exclusive");
        assert_eq!(pick_step(&steps, 99.0).map(|s| s.adjustment), Some(3));
        assert!(pick_step(&[Step { lower: Some(5.0), upper: Some(9.0), adjustment: 1 }], 1.0).is_none());
    }

    #[test]
    fn cooldown_uses_the_policy_then_the_group_default() {
        assert!(cooling_down(10, 300));
        assert!(!cooling_down(300, 300));
        assert_eq!(effective_cooldown(0, 300), 300);
        assert_eq!(effective_cooldown(60, 300), 60);
    }

    #[test]
    fn arns_round_trip_and_member_lists_read_both_encodings() {
        let id = Uuid::new_v4();
        let arn = policy_arn(id, "web", "scale-out");
        assert_eq!(policy_id_from_arn(&arn), Some(id));
        assert_eq!(policy_id_from_arn("scale-out"), None);
        let q = params(&[("InstanceIds.member.1", "i-1"), ("InstanceIds.member.2", "i-2"), ("Other.1", "x")]);
        assert_eq!(members(&q, "InstanceIds"), vec!["i-1", "i-2"]);
        assert_eq!(members(&params(&[("Other.1", "x")]), "Other"), vec!["x"]);
        assert!(members(&q, "Missing").is_empty());
    }

    #[test]
    fn paging_walks_the_sorted_names_and_ends_without_a_token() {
        let names: Vec<String> = ["a", "b", "c", "d", "e"].iter().map(|s| s.to_string()).collect();
        let (first, token) = paginate(&names, None, 2).unwrap();
        assert_eq!(first, vec!["a", "b"]);
        let (second, token) = paginate(&names, token.as_deref(), 2).unwrap();
        assert_eq!(second, vec!["c", "d"]);
        let (last, end) = paginate(&names, token.as_deref(), 2).unwrap();
        assert_eq!(last, vec!["e"]);
        assert!(end.is_none());
        assert_eq!(paginate(&names, Some("***"), 2).unwrap_err().code, "InvalidNextToken");
        assert_eq!(max_records(&params(&[("MaxRecords", "0")])).unwrap_err().code, "ValidationError");
        assert_eq!(max_records(&params(&[])).unwrap(), 50);
    }

    #[test]
    fn unknown_parameters_are_refused_not_ignored() {
        let ok = params(&[("Action", "X"), ("Version", "2011-01-01"), ("MinSize", "1"), ("Tags.member.1.Key", "a")]);
        assert!(allow_only(&ok, &["MinSize"], &["Tags.member."]).is_ok());
        let bad = params(&[("MinSize", "1"), ("MixedInstancesPolicy.InstancesDistribution.OnDemandBaseCapacity", "1")]);
        let e = allow_only(&bad, &["MinSize"], &[]).unwrap_err();
        assert_eq!(e.code, "UnsupportedOperation");
        assert!(e.message.contains("MixedInstancesPolicy"), "{}", e.message);
    }

    #[test]
    fn timestamps_are_iso_8601() {
        assert_eq!(iso("2026-10-08 12:34:56"), "2026-10-08T12:34:56Z");
        assert_eq!(iso("short"), "short");
    }

    #[test]
    fn one_subnet_per_group() {
        assert_eq!(single_subnet(&params(&[("VPCZoneIdentifier", "subnet-1")])).unwrap(), Some("subnet-1".into()));
        assert_eq!(single_subnet(&params(&[("VPCZoneIdentifier", " ")])).unwrap(), None);
        assert_eq!(single_subnet(&params(&[("VPCZoneIdentifier", "subnet-1,subnet-2")])).unwrap_err().code, "UnsupportedOperation");
    }

    // ---- against a migrated database ----

    struct Env {
        state: AppState,
        _rx: tokio::sync::mpsc::UnboundedReceiver<crate::tasks::TaskMessage>,
        project: Uuid,
        subnet: String,
    }

    /// A project, an online host, a VPC and a ready subnet, the way the REST handlers build them.
    async fn env() -> Env {
        let (state, rx) = crate::engine::test_support::test_state().await;
        let cluster = Uuid::new_v4();
        let host = Uuid::new_v4();
        let project = Uuid::new_v4();
        crate::db::query("INSERT INTO clusters(id,name) VALUES (?,'asg-test')").bind(cluster).execute(&state.pool).await.unwrap();
        crate::db::query("INSERT INTO hosts(id,cluster_id,hostname,state) VALUES (?,?,'asg-host','online')").bind(host).bind(cluster).execute(&state.pool).await.unwrap();
        crate::db::query("INSERT INTO projects(id,name) VALUES (?,'asg-proj')").bind(project).execute(&state.pool).await.unwrap();
        let vpc_body: crate::api::cloud::network::CreateVpc = serde_json::from_value(serde_json::json!({"name":"private","cidr":"10.30.0.0/16","host_id":host})).unwrap();
        let Json(vpc) = crate::api::cloud::network::create_vpc(State(state.clone()), Extension(admin()), Path(project), Json(vpc_body)).await.unwrap();
        let vpc = serde_json::to_value(&vpc).unwrap()["id"].as_str().and_then(|s| Uuid::parse_str(s).ok()).unwrap();
        let subnet_body: crate::api::cloud::network::CreateSubnet = serde_json::from_value(serde_json::json!({"name":"apps","cidr":"10.30.1.0/24"})).unwrap();
        let Json(subnet) = crate::api::cloud::network::create_subnet(State(state.clone()), Extension(admin()), Path(vpc), Json(subnet_body)).await.unwrap();
        let subnet = subnet["id"].as_str().and_then(|s| Uuid::parse_str(s).ok()).unwrap();
        crate::db::query("UPDATE cloud_subnets SET status = 'ready' WHERE id = ?").bind(subnet).execute(&state.pool).await.unwrap();
        Env { state, _rx: rx, project, subnet: ec2_id(Kind::Subnet, subnet) }
    }

    async fn add_template(e: &Env, name: &str) -> String {
        let vm = super::super::fleet::template_vm(name, "ubuntu-24.04");
        let (id, _, _) = super::super::fleet::create_template_row(&e.state, &admin(), e.project, name, vm).await.unwrap();
        ec2_id(Kind::LaunchTemplate, id)
    }

    async fn create_group(e: &Env, name: &str, template: &str, min: u32, max: u32, desired: u32) {
        let p = params(&[
            ("AutoScalingGroupName", name),
            ("MinSize", &min.to_string()),
            ("MaxSize", &max.to_string()),
            ("DesiredCapacity", &desired.to_string()),
            ("VPCZoneIdentifier", &e.subnet),
            ("LaunchTemplate.LaunchTemplateId", template),
            ("LaunchTemplate.Version", "$Latest"),
            ("Tags.member.1.Key", "env"),
            ("Tags.member.1.Value", "test"),
            ("Tags.member.1.PropagateAtLaunch", "true"),
        ]);
        create_auto_scaling_group(&e.state, &admin(), &p).await.unwrap();
    }

    #[tokio::test]
    async fn a_group_is_created_described_updated_and_deleted() {
        let e = env().await;
        let lt = add_template(&e, "base").await;
        create_group(&e, "web", &lt, 1, 4, 2).await;

        let d = describe_auto_scaling_groups(&e.state, &admin(), &params(&[("AutoScalingGroupNames.member.1", "web")])).await.unwrap();
        for want in [
            "<AutoScalingGroupName>web</AutoScalingGroupName>",
            "<MinSize>1</MinSize>",
            "<MaxSize>4</MaxSize>",
            "<DesiredCapacity>2</DesiredCapacity>",
            "<AvailabilityZones><member>asg-host</member></AvailabilityZones>",
            "<Version>$Latest</Version>",
            "<Key>env</Key><Value>test</Value><PropagateAtLaunch>true</PropagateAtLaunch>",
            "<HealthCheckType>EC2</HealthCheckType>",
        ] {
            assert!(d.contains(want), "missing {want} in {d}");
        }
        assert!(d.contains("<Instances></Instances>"), "no member has been launched yet: {d}");

        // the same name twice
        let again = create_auto_scaling_group(
            &e.state,
            &admin(),
            &params(&[("AutoScalingGroupName", "web"), ("MinSize", "0"), ("MaxSize", "1"), ("VPCZoneIdentifier", &e.subnet), ("LaunchTemplate.LaunchTemplateId", &lt)]),
        )
        .await
        .unwrap_err();
        assert_eq!(again.code, "AlreadyExists");

        // raising the minimum past the desired capacity moves the capacity with it
        update_auto_scaling_group(&e.state, &admin(), &params(&[("AutoScalingGroupName", "web"), ("MinSize", "3"), ("MaxSize", "6")])).await.unwrap();
        let g = load_group(&e.state, "web").await.unwrap();
        assert_eq!((g.policy.min, g.policy.desired, g.policy.max), (3, 3, 6));

        // the REST view of the same group shows the same numbers
        let pol: String = crate::db::query_scalar("SELECT policy_json FROM cloud_instance_groups WHERE id = ?").bind(g.id).fetch_one(&e.state.pool).await.unwrap();
        assert!(pol.contains("\"desired\":3"), "{pol}");

        delete_auto_scaling_group(&e.state, &admin(), &params(&[("AutoScalingGroupName", "web")])).await.unwrap();
        assert!(describe_auto_scaling_groups(&e.state, &admin(), &params(&[])).await.unwrap().contains("<AutoScalingGroups></AutoScalingGroups>"));
        let leftovers: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM asg_tags").fetch_one(&e.state.pool).await.unwrap();
        assert_eq!(leftovers, 0, "tags go with the group");
    }

    #[tokio::test]
    async fn bad_group_requests_are_refused_with_the_right_codes() {
        let e = env().await;
        let lt = add_template(&e, "base").await;
        let base = |extra: &[(&str, &str)]| {
            let mut p = params(&[("AutoScalingGroupName", "g1"), ("MinSize", "0"), ("MaxSize", "2"), ("VPCZoneIdentifier", &e.subnet), ("LaunchTemplate.LaunchTemplateId", &lt)]);
            for (k, v) in extra {
                p.insert(k.to_string(), v.to_string());
            }
            p
        };
        let code = |r: Result<String, Ec2Error>| r.unwrap_err().code;
        let s = &e.state;
        let a = admin();
        assert_eq!(code(create_auto_scaling_group(s, &a, &base(&[("MixedInstancesPolicy.LaunchTemplate.LaunchTemplateSpecification.LaunchTemplateId", "x")])).await), "UnsupportedOperation");
        assert_eq!(code(create_auto_scaling_group(s, &a, &base(&[("TargetGroupARNs.member.1", "arn:x")])).await), "UnsupportedOperation");
        assert_eq!(code(create_auto_scaling_group(s, &a, &base(&[("HealthCheckType", "ELB")])).await), "UnsupportedOperation");
        assert_eq!(code(create_auto_scaling_group(s, &a, &base(&[("NewInstancesProtectedFromScaleIn", "true")])).await), "UnsupportedOperation");
        assert_eq!(code(create_auto_scaling_group(s, &a, &base(&[("TerminationPolicies.member.1", "NewestInstance")])).await), "UnsupportedOperation");
        assert_eq!(code(create_auto_scaling_group(s, &a, &base(&[("VPCZoneIdentifier", "subnet-1,subnet-2")])).await), "UnsupportedOperation");
        assert_eq!(code(create_auto_scaling_group(s, &a, &base(&[("DesiredCapacity", "5")])).await), "ValidationError");
        assert_eq!(code(create_auto_scaling_group(s, &a, &base(&[("MinSize", "3")])).await), "ValidationError");
        assert_eq!(code(create_auto_scaling_group(s, &a, &base(&[("AvailabilityZones.member.1", "elsewhere")])).await), "ValidationError");
        assert_eq!(code(create_auto_scaling_group(s, &a, &base(&[("LaunchConfigurationName", "lc")])).await), "ValidationError", "both a template and a configuration");
        let mut no_source = base(&[]);
        no_source.remove("LaunchTemplate.LaunchTemplateId");
        assert_eq!(code(create_auto_scaling_group(s, &a, &no_source).await), "ValidationError");
        let mut no_min = base(&[]);
        no_min.remove("MinSize");
        assert_eq!(code(create_auto_scaling_group(s, &a, &no_min).await), "MissingParameter");
        assert_eq!(code(set_desired_capacity(s, &a, &params(&[("AutoScalingGroupName", "nope"), ("DesiredCapacity", "1")])).await), "ValidationError");
        let viewer = AuthUser { username: "v".into(), role: "viewer".into(), auth_source: None };
        assert!(create_auto_scaling_group(s, &viewer, &base(&[])).await.is_err(), "a viewer cannot create groups");
    }

    #[tokio::test]
    async fn capacity_can_only_move_inside_the_limits() {
        let e = env().await;
        let lt = add_template(&e, "base").await;
        create_group(&e, "api", &lt, 1, 3, 1).await;
        let s = &e.state;
        let a = admin();
        let set = |n: &str| params(&[("AutoScalingGroupName", "api"), ("DesiredCapacity", n)]);
        set_desired_capacity(s, &a, &set("3")).await.unwrap();
        assert_eq!(load_group(s, "api").await.unwrap().policy.desired, 3);
        let over = set_desired_capacity(s, &a, &set("4")).await.unwrap_err();
        assert!(over.message.contains("above max value 3"), "{}", over.message);
        let under = set_desired_capacity(s, &a, &set("0")).await.unwrap_err();
        assert!(under.message.contains("below min value 1"), "{}", under.message);
        let busy = set_desired_capacity(s, &a, &params(&[("AutoScalingGroupName", "api"), ("DesiredCapacity", "2"), ("HonorCooldown", "true")])).await.unwrap_err();
        assert_eq!(busy.code, "ScalingActivityInProgress", "the capacity just changed, so the cooldown is running");
        let acts = describe_scaling_activities(s, &a, &params(&[("AutoScalingGroupName", "api")])).await.unwrap();
        assert!(acts.contains("Changing capacity of api from 1 to 3"), "{acts}");
    }

    #[tokio::test]
    async fn policies_scale_the_group_and_target_tracking_sets_the_cpu_target() {
        let e = env().await;
        let lt = add_template(&e, "base").await;
        create_group(&e, "svc", &lt, 1, 6, 2).await;
        let s = &e.state;
        let a = admin();

        let put = put_scaling_policy(
            s,
            &a,
            &params(&[
                ("AutoScalingGroupName", "svc"),
                ("PolicyName", "out"),
                ("AdjustmentType", "ChangeInCapacity"),
                ("ScalingAdjustment", "2"),
                ("Cooldown", "60"),
            ]),
        )
        .await
        .unwrap();
        assert!(put.contains("<PolicyARN>arn:aws:autoscaling:machina:000000000000:scalingPolicy:"), "{put}");
        execute_policy(s, &a, &params(&[("AutoScalingGroupName", "svc"), ("PolicyName", "out")])).await.unwrap();
        assert_eq!(load_group(s, "svc").await.unwrap().policy.desired, 4);
        // the policy's cooldown now blocks a polite caller
        let again = execute_policy(s, &a, &params(&[("AutoScalingGroupName", "svc"), ("PolicyName", "out"), ("HonorCooldown", "true")])).await.unwrap_err();
        assert_eq!(again.code, "ScalingActivityInProgress");
        // and an impolite one is clamped at the maximum
        for _ in 0..3 {
            execute_policy(s, &a, &params(&[("AutoScalingGroupName", "svc"), ("PolicyName", "out")])).await.unwrap();
        }
        assert_eq!(load_group(s, "svc").await.unwrap().policy.desired, 6);

        // step scaling needs the alarm numbers and picks the matching step
        put_scaling_policy(
            s,
            &a,
            &params(&[
                ("AutoScalingGroupName", "svc"),
                ("PolicyName", "steps"),
                ("PolicyType", "StepScaling"),
                ("AdjustmentType", "ChangeInCapacity"),
                ("StepAdjustments.member.1.MetricIntervalUpperBound", "0"),
                ("StepAdjustments.member.1.ScalingAdjustment", "-2"),
                ("StepAdjustments.member.2.MetricIntervalLowerBound", "0"),
                ("StepAdjustments.member.2.ScalingAdjustment", "1"),
            ]),
        )
        .await
        .unwrap();
        let missing = execute_policy(s, &a, &params(&[("AutoScalingGroupName", "svc"), ("PolicyName", "steps")])).await.unwrap_err();
        assert_eq!(missing.code, "ValidationError");
        execute_policy(s, &a, &params(&[("AutoScalingGroupName", "svc"), ("PolicyName", "steps"), ("MetricValue", "10"), ("BreachThreshold", "40")])).await.unwrap();
        assert_eq!(load_group(s, "svc").await.unwrap().policy.desired, 4, "a metric 30 below the threshold takes the -2 step");

        // target tracking is the reconciler's CPU target
        let tt = |value: &str, metric: &str| {
            params(&[
                ("AutoScalingGroupName", "svc"),
                ("PolicyName", "cpu"),
                ("PolicyType", "TargetTrackingScaling"),
                ("TargetTrackingConfiguration.PredefinedMetricSpecification.PredefinedMetricType", metric),
                ("TargetTrackingConfiguration.TargetValue", value),
            ])
        };
        assert_eq!(put_scaling_policy(s, &a, &tt("60", "ALBRequestCountPerTarget")).await.unwrap_err().code, "UnsupportedOperation");
        assert_eq!(put_scaling_policy(s, &a, &tt("95", "ASGAverageCPUUtilization")).await.unwrap_err().code, "ValidationError");
        put_scaling_policy(s, &a, &tt("60", "ASGAverageCPUUtilization")).await.unwrap();
        assert_eq!(load_group(s, "svc").await.unwrap().policy.target_cpu, Some(60.0));
        let cannot = execute_policy(s, &a, &params(&[("AutoScalingGroupName", "svc"), ("PolicyName", "cpu")])).await.unwrap_err();
        assert_eq!(cannot.code, "ValidationError");
        let d = describe_policies(s, &a, &params(&[("AutoScalingGroupName", "svc")])).await.unwrap();
        assert!(d.contains("<PolicyName>cpu</PolicyName>") && d.contains("<TargetValue>60</TargetValue>") && d.contains("<PolicyName>steps</PolicyName>"), "{d}");
        delete_policy(s, &a, &params(&[("AutoScalingGroupName", "svc"), ("PolicyName", "cpu")])).await.unwrap();
        assert_eq!(load_group(s, "svc").await.unwrap().policy.target_cpu, None, "deleting the policy clears the CPU target");
    }

    #[tokio::test]
    async fn a_policy_arn_can_be_an_alarm_action_only_when_one_step_is_enough() {
        let e = env().await;
        let lt = add_template(&e, "base").await;
        create_group(&e, "alarmed", &lt, 0, 5, 1).await;
        let s = &e.state;
        let a = admin();
        let arn_of = |out: String| out.split("<PolicyARN>").nth(1).and_then(|r| r.split("</PolicyARN>").next()).unwrap().to_string();
        let simple = arn_of(
            put_scaling_policy(s, &a, &params(&[("AutoScalingGroupName", "alarmed"), ("PolicyName", "one"), ("AdjustmentType", "ChangeInCapacity"), ("ScalingAdjustment", "-1")])).await.unwrap(),
        );
        let (group, step) = alarm_target(s, &simple).await.unwrap();
        assert_eq!(group, load_group(s, "alarmed").await.unwrap().id);
        assert_eq!(step, -1);
        let exact = arn_of(
            put_scaling_policy(s, &a, &params(&[("AutoScalingGroupName", "alarmed"), ("PolicyName", "exact"), ("AdjustmentType", "ExactCapacity"), ("ScalingAdjustment", "3")])).await.unwrap(),
        );
        assert_eq!(alarm_target(s, &exact).await.unwrap_err().code, "UnsupportedOperation");
        assert_eq!(alarm_target(s, "arn:aws:sns:x").await.unwrap_err().code, "ValidationError");
    }

    #[tokio::test]
    async fn suspending_launch_and_terminate_pauses_the_group() {
        let e = env().await;
        let lt = add_template(&e, "base").await;
        create_group(&e, "p", &lt, 0, 2, 0).await;
        let s = &e.state;
        let a = admin();
        let one = suspend_processes(s, &a, &params(&[("AutoScalingGroupName", "p"), ("ScalingProcesses.member.1", "Launch")])).await.unwrap_err();
        assert_eq!(one.code, "UnsupportedOperation");
        assert_eq!(
            suspend_processes(s, &a, &params(&[("AutoScalingGroupName", "p"), ("ScalingProcesses.member.1", "Bogus")])).await.unwrap_err().code,
            "ValidationError"
        );
        suspend_processes(s, &a, &params(&[("AutoScalingGroupName", "p"), ("ScalingProcesses.member.1", "Launch"), ("ScalingProcesses.member.2", "Terminate")])).await.unwrap();
        let g = load_group(s, "p").await.unwrap();
        assert!(g.paused, "the reconciler skips a paused group");
        assert_eq!(g.suspended, vec!["Launch", "Terminate"]);
        let d = describe_auto_scaling_groups(s, &a, &params(&[])).await.unwrap();
        assert!(d.contains("<ProcessName>Launch</ProcessName>"), "{d}");
        resume_processes(s, &a, &params(&[("AutoScalingGroupName", "p")])).await.unwrap();
        let g = load_group(s, "p").await.unwrap();
        assert!(!g.paused && g.suspended.is_empty());
    }

    #[tokio::test]
    async fn launch_configurations_are_stored_listed_and_turned_into_a_template_for_a_group() {
        let e = env().await;
        let s = &e.state;
        let a = admin();
        let create = |name: &str, extra: &[(&str, &str)]| {
            let mut p = params(&[("LaunchConfigurationName", name), ("ImageId", "ubuntu-24.04")]);
            for (k, v) in extra {
                p.insert(k.to_string(), v.to_string());
            }
            p
        };
        assert_eq!(create_launch_configuration(s, &a, &create("lc1", &[("InstanceMonitoring.Enabled", "true")])).await.unwrap_err().code, "UnsupportedOperation");
        assert_eq!(create_launch_configuration(s, &a, &create("lc1", &[("SecurityGroups.member.1", "sg-1")])).await.unwrap_err().code, "UnsupportedOperation");
        assert_eq!(create_launch_configuration(s, &a, &create("lc1", &[("InstanceType", "no-such-type")])).await.unwrap_err().code, "ValidationError");
        assert_eq!(create_launch_configuration(s, &a, &create("lc1", &[("BlockDeviceMappings.member.1.DeviceName", "/dev/sda1")])).await.unwrap_err().code, "UnsupportedOperation");
        // #!/bin/sh\necho hi
        create_launch_configuration(s, &a, &create("lc1", &[("UserData", "IyEvYmluL3NoCmVjaG8gaGk="), ("InstanceMonitoring.Enabled", "false")])).await.unwrap();
        assert_eq!(create_launch_configuration(s, &a, &create("lc1", &[])).await.unwrap_err().code, "AlreadyExists");
        let d = describe_launch_configurations(s, &a, &params(&[])).await.unwrap();
        assert!(d.contains("<LaunchConfigurationName>lc1</LaunchConfigurationName>") && d.contains("<UserData>IyEvYmluL3NoCmVjaG8gaGk=</UserData>"), "{d}");

        // a group built from it gets a template named after it, in the subnet's project
        let p = params(&[("AutoScalingGroupName", "fromlc"), ("MinSize", "0"), ("MaxSize", "1"), ("VPCZoneIdentifier", &e.subnet), ("LaunchConfigurationName", "lc1")]);
        create_auto_scaling_group(s, &a, &p).await.unwrap();
        let spec: String = crate::db::query_scalar("SELECT spec_json FROM cloud_launch_templates WHERE name = 'lc-lc1'").fetch_one(&s.pool).await.unwrap();
        assert!(spec.contains("echo hi") && spec.contains("ubuntu-24.04"), "{spec}");
        let g = describe_auto_scaling_groups(s, &a, &params(&[])).await.unwrap();
        assert!(g.contains("<LaunchConfigurationName>lc1</LaunchConfigurationName>") && !g.contains("<LaunchTemplate>"), "{g}");
        // in use, then free
        assert_eq!(delete_launch_configuration(s, &a, &params(&[("LaunchConfigurationName", "lc1")])).await.unwrap_err().code, "ResourceInUse");
        delete_auto_scaling_group(s, &a, &params(&[("AutoScalingGroupName", "fromlc")])).await.unwrap();
        delete_launch_configuration(s, &a, &params(&[("LaunchConfigurationName", "lc1")])).await.unwrap();
        assert_eq!(delete_launch_configuration(s, &a, &params(&[("LaunchConfigurationName", "lc1")])).await.unwrap_err().code, "ValidationError");
    }

    #[tokio::test]
    async fn tags_can_be_added_filtered_and_removed() {
        let e = env().await;
        let lt = add_template(&e, "base").await;
        create_group(&e, "tagged", &lt, 0, 1, 0).await;
        let s = &e.state;
        let a = admin();
        create_or_update_tags(
            s,
            &a,
            &params(&[
                ("Tags.member.1.ResourceId", "tagged"),
                ("Tags.member.1.ResourceType", "auto-scaling-group"),
                ("Tags.member.1.Key", "team"),
                ("Tags.member.1.Value", "infra"),
                ("Tags.member.1.PropagateAtLaunch", "false"),
            ]),
        )
        .await
        .unwrap();
        let all = describe_tags(s, &a, &params(&[])).await.unwrap();
        assert!(all.contains("<Key>env</Key>") && all.contains("<Key>team</Key>"), "{all}");
        let only = describe_tags(s, &a, &params(&[("Filters.member.1.Name", "key"), ("Filters.member.1.Values.member.1", "team")])).await.unwrap();
        assert!(only.contains("<Key>team</Key>") && !only.contains("<Key>env</Key>"), "{only}");
        assert_eq!(describe_tags(s, &a, &params(&[("Filters.member.1.Name", "colour")])).await.unwrap_err().code, "ValidationError");
        delete_tags(s, &a, &params(&[("Tags.member.1.ResourceId", "tagged"), ("Tags.member.1.Key", "team")])).await.unwrap();
        assert!(!describe_tags(s, &a, &params(&[])).await.unwrap().contains("<Key>team</Key>"));
        assert_eq!(
            create_or_update_tags(s, &a, &params(&[("Tags.member.1.Key", "x")])).await.unwrap_err().code,
            "ValidationError",
            "the resource id is required"
        );
    }

    #[tokio::test]
    async fn unsupported_actions_say_so_and_unknown_ones_are_invalid() {
        let e = env().await;
        let a = admin();
        for action in ["EnterStandby", "PutLifecycleHook", "StartInstanceRefresh", "EnableMetricsCollection", "AttachLoadBalancerTargetGroups"] {
            assert_eq!(dispatch(&e.state, &a, &params(&[]), action).await.unwrap_err().code, "UnsupportedOperation", "{action}");
        }
        assert_eq!(dispatch(&e.state, &a, &params(&[]), "NoSuchAction").await.unwrap_err().code, "InvalidAction");
        assert!(dispatch(&e.state, &a, &params(&[]), "DescribeLifecycleHooks").await.unwrap().contains("<LifecycleHooks/>"));
        assert!(dispatch(&e.state, &a, &params(&[]), "DescribeAdjustmentTypes").await.unwrap().contains("PercentChangeInCapacity"));
        assert!(dispatch(&e.state, &a, &params(&[]), "DescribeAccountLimits").await.unwrap().contains("<NumberOfAutoScalingGroups>0</NumberOfAutoScalingGroups>"));
    }
}
