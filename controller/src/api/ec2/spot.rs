// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Spot requests, instant fleets and the spot price history.
//!
//! A spot instance is the cluster's *preemptible* instance: it runs until the cluster needs its room for a regular
//! instance, then it is stopped (see `engine::preempt`). There is no spot market, so
//!
//! * a request is fulfilled at once or fails; it never waits "open" for capacity or a price,
//! * `SpotPrice`/`MaxPrice` are validated and never lose (no bid can be outbid),
//! * the price history is a flat synthetic price (`0.005` per vCPU-hour), labelled as such in the reply,
//! * only `one-time` requests and `instant` fleets exist; `persistent` requests and `maintain`/`request` fleets (which
//!   would need a loop that keeps capacity) are refused.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::Extension;
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

fn unsupported(msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad("UnsupportedOperation", msg)
}

/// Dollars per vCPU-hour of the synthetic price.
const PRICE_PER_VCPU: f64 = 0.005;

fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S.000Z").to_string()
}

fn check_price(name: &str, v: &str) -> Result<(), Ec2Error> {
    match v.parse::<f64>() {
        Ok(n) if n > 0.0 && n.is_finite() => Ok(()),
        _ => Err(bad("InvalidParameterValue", format!("{name} must be a positive number"))),
    }
}

// ---- spot requests ---------------------------------------------------------------------------------------------------

/// `RequestSpotInstances` → the `RunInstances` parameters it stands for. Refuses what a market would be needed for.
pub fn spot_to_run(p: &Params) -> Result<Params, Ec2Error> {
    if p.get("Type").is_some_and(|t| t != "one-time") {
        return Err(unsupported("only one-time spot requests are supported: a persistent request would have to wait for capacity and relaunch"));
    }
    for k in ["ValidFrom", "ValidUntil", "LaunchGroup", "AvailabilityZoneGroup", "BlockDurationMinutes"] {
        if p.contains_key(k) {
            return Err(unsupported(format!("{k} is not supported")));
        }
    }
    if p.get("InstanceInterruptionBehavior").is_some_and(|v| v != "terminate") {
        return Err(unsupported("a preempted instance is terminated: InstanceInterruptionBehavior must be terminate"));
    }
    if let Some(price) = p.get("SpotPrice") {
        check_price("SpotPrice", price)?;
    }
    let count: u32 = match p.get("InstanceCount") {
        Some(v) => v.parse().ok().filter(|n| (1..=20).contains(n)).ok_or_else(|| bad("InvalidParameterValue", "InstanceCount must be between 1 and 20"))?,
        None => 1,
    };
    let mut q = Params::new();
    for (k, v) in p {
        if let Some(rest) = k.strip_prefix("LaunchSpecification.") {
            q.insert(rest.to_string(), v.clone());
        } else if k.starts_with("TagSpecification.") {
            q.insert(k.clone(), v.clone());
        }
    }
    if !q.keys().any(|k| k == "ImageId") {
        return Err(bad("MissingParameter", "LaunchSpecification.ImageId is required"));
    }
    q.insert("MinCount".into(), count.to_string());
    q.insert("MaxCount".into(), count.to_string());
    q.insert("InstanceMarketOptions.MarketType".into(), "spot".into());
    Ok(q)
}

/// The state of a request from the stored state and what became of its instance.
pub fn derive_state(stored: &str, vm: Option<bool>) -> (&'static str, &'static str, &'static str) {
    match (stored, vm) {
        ("cancelled", Some(_)) => ("cancelled", "request-canceled-and-instance-running", "Request canceled and instance running"),
        ("cancelled", None) => ("cancelled", "instance-terminated-by-user", "Request canceled and the instance is gone"),
        (_, None) => ("closed", "instance-terminated-by-user", "The instance was terminated"),
        (_, Some(true)) => ("closed", "instance-terminated-no-capacity", "The instance was stopped to make room for a regular instance"),
        (_, Some(false)) => ("active", "fulfilled", "Your spot request is fulfilled."),
    }
}

type RequestRow = (Uuid, String, String, Option<Uuid>, String, String, String);

async fn vm_status(state: &AppState, vm: Option<Uuid>) -> Result<Option<bool>, Ec2Error> {
    let Some(vm) = vm else { return Ok(None) };
    let row: Option<bool> = crate::db::query_scalar("SELECT preempted_at IS NOT NULL FROM vms WHERE id = ?").bind(vm).fetch_optional(&state.pool).await?;
    Ok(row)
}

async fn request_item(state: &AppState, r: &RequestRow, tags: &BTreeMap<String, String>) -> Result<String, Ec2Error> {
    let (id, stored, _kind, vm, price, itype, created) = r;
    let (st, code, msg) = derive_state(stored, vm_status(state, *vm).await?);
    let instance = vm.map(|v| format!("<instanceId>{}</instanceId>", ec2_id(Kind::Vm, v))).unwrap_or_default();
    let spot_price = if price.is_empty() { String::new() } else { format!("<spotPrice>{}</spotPrice>", xml_escape(price)) };
    Ok(format!(
        "<spotInstanceRequestId>{}</spotInstanceRequestId><state>{st}</state><status><code>{code}</code><message>{msg}</message><updateTime>{}</updateTime></status><type>one-time</type>{spot_price}<launchSpecification><instanceType>{}</instanceType></launchSpecification>{instance}<createTime>{}</createTime><productDescription>Linux/UNIX</productDescription>{}",
        ec2_id(Kind::SpotRequest, *id),
        now(),
        xml_escape(itype),
        xml_escape(created),
        tagspec::tag_set_xml(tags)
    ))
}

pub async fn request_spot_instances(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let run = spot_to_run(p)?;
    tagspec::only_types(p, &["spot-instances-request", "instance", "volume"])?;
    let ids = super::launch(state, actor, &run).await?;
    let req_tags = tagspec::tags_for(p, "spot-instances-request");
    let price = p.get("SpotPrice").cloned().unwrap_or_default();
    let mut items = String::new();
    for vm in ids {
        let id = Uuid::new_v4();
        let itype: String = crate::db::query_scalar::<_, Option<String>>("SELECT f.name FROM vms v LEFT JOIN flavors f ON f.id = v.flavor_id WHERE v.id = ?")
            .bind(vm)
            .fetch_optional(&state.pool)
            .await?
            .flatten()
            .unwrap_or_default();
        crate::db::query("INSERT INTO ec2_spot_requests (id, request_type, state, status_code, status_message, vm_id, spot_price, instance_type, created_by) VALUES (?, 'one-time', 'active', 'fulfilled', 'Your spot request is fulfilled.', ?, ?, ?, ?)")
            .bind(id)
            .bind(vm)
            .bind(&price)
            .bind(&itype)
            .bind(&actor.username)
            .execute(&state.pool)
            .await?;
        tagspec::apply(state, Kind::SpotRequest, id, &req_tags).await?;
        let row: RequestRow = (id, "active".into(), "one-time".into(), Some(vm), price.clone(), itype, now());
        items.push_str(&format!("<item>{}</item>", request_item(state, &row, &req_tags).await?));
    }
    Ok(format!("<spotInstanceRequestSet>{items}</spotInstanceRequestSet>"))
}

pub async fn describe_spot_instance_requests(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let wanted = indexed(p, "SpotInstanceRequestId");
    let mut filters = Vec::new();
    for (name, values) in super::parse_filters(p) {
        if values.is_empty() || !matches!(name.as_str(), "state" | "instance-id" | "spot-instance-request-id" | "type") && !name.starts_with("tag") {
            return Err(bad("InvalidParameterValue", format!("The filter '{name}' is not valid for DescribeSpotInstanceRequests")));
        }
        filters.push((name, values));
    }
    let rows: Vec<RequestRow> = crate::db::query_as(
        "SELECT id, state, request_type, vm_id, spot_price, instance_type, created_at FROM ec2_spot_requests ORDER BY created_at, id",
    )
    .fetch_all(&state.pool)
    .await?;
    for w in &wanted {
        if !rows.iter().any(|r| &ec2_id(Kind::SpotRequest, r.0) == w) {
            return Err(bad("InvalidSpotInstanceRequestID.NotFound", format!("The spot instance request ID '{w}' does not exist")));
        }
    }
    let mut items = String::new();
    for r in rows {
        let eid = ec2_id(Kind::SpotRequest, r.0);
        if !wanted.is_empty() && !wanted.contains(&eid) {
            continue;
        }
        let tags = tagspec::tags_of(state, Kind::SpotRequest, r.0).await?;
        let tag_list: Vec<(String, String)> = tags.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let (st, _, _) = derive_state(&r.1, vm_status(state, r.3).await?);
        let vm_eid = r.3.map(|v| ec2_id(Kind::Vm, v)).unwrap_or_default();
        let keep = filters.iter().all(|(n, v)| {
            if let Some(m) = super::foundation::tag_filter_matches(&tag_list, n, v) {
                return m;
            }
            match n.as_str() {
                "state" => super::foundation::any_match(v, st),
                "instance-id" => super::foundation::any_match(v, &vm_eid),
                "spot-instance-request-id" => super::foundation::any_match(v, &eid),
                "type" => super::foundation::any_match(v, "one-time"),
                _ => true,
            }
        });
        if keep {
            items.push_str(&format!("<item>{}</item>", request_item(state, &r, &tags).await?));
        }
    }
    Ok(format!("<spotInstanceRequestSet>{items}</spotInstanceRequestSet>"))
}

pub async fn cancel_spot_instance_requests(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let wanted = indexed(p, "SpotInstanceRequestId");
    if wanted.is_empty() {
        return Err(bad("MissingParameter", "The request must contain the parameter SpotInstanceRequestId"));
    }
    let mut ids = Vec::new();
    for w in &wanted {
        ids.push((w.clone(), resolve(state, Kind::SpotRequest, w, "InvalidSpotInstanceRequestID.NotFound").await?));
    }
    let mut items = String::new();
    for (eid, id) in ids {
        crate::db::query("UPDATE ec2_spot_requests SET state = 'cancelled' WHERE id = ?").bind(id).execute(&state.pool).await?;
        items.push_str(&format!("<item><spotInstanceRequestId>{eid}</spotInstanceRequestId><state>cancelled</state></item>"));
    }
    Ok(format!("<spotInstanceRequestSet>{items}</spotInstanceRequestSet>"))
}

/// `DescribeSpotPriceHistory`: one flat synthetic price per instance type (see the module notes).
pub async fn describe_spot_price_history(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let types = indexed(p, "InstanceType");
    let products = indexed(p, "ProductDescription");
    let rows: Vec<(String, i64)> = crate::db::query_as("SELECT name, vcpus FROM flavors ORDER BY name").fetch_all(&state.pool).await?;
    if !products.is_empty() && !products.iter().any(|d| super::foundation::wild_match(d, "Linux/UNIX")) {
        return Ok("<spotPriceHistorySet/>".into());
    }
    let items: String = rows
        .iter()
        .filter(|(n, _)| types.is_empty() || types.contains(n))
        .map(|(n, v)| {
            format!(
                "<item><instanceType>{}</instanceType><productDescription>Linux/UNIX</productDescription><spotPrice>{:.6}</spotPrice><timestamp>{}</timestamp><availabilityZone>machina-a</availabilityZone><machinaNote>flat synthetic price, {PRICE_PER_VCPU} per vCPU-hour: there is no spot market</machinaNote></item>",
                xml_escape(n),
                PRICE_PER_VCPU * (*v as f64),
                now()
            )
        })
        .collect();
    Ok(format!("<spotPriceHistorySet>{items}</spotPriceHistorySet>"))
}

// ---- fleets ----------------------------------------------------------------------------------------------------------

/// One launch of an instant fleet: how many, spot or on demand, and the override it uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FleetLaunch {
    pub count: u32,
    pub spot: bool,
    pub instance_type: Option<String>,
    pub subnet: Option<String>,
    pub zone: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct FleetPlan {
    pub total: u32,
    pub default_spot: bool,
    pub launches: Vec<FleetLaunch>,
}

/// `n` spread over `slots` as evenly as possible (the first ones take the remainder).
pub fn distribute(n: u32, slots: usize) -> Vec<u32> {
    if slots == 0 {
        return Vec::new();
    }
    let base = n / slots as u32;
    let rem = (n % slots as u32) as usize;
    (0..slots).map(|i| base + u32::from(i < rem)).collect()
}

/// Validates a `CreateFleet` request and turns it into launches.
pub fn plan_fleet(p: &Params) -> Result<FleetPlan, Ec2Error> {
    match p.get("Type").map(String::as_str) {
        Some("instant") => {}
        Some("maintain") | Some("request") | None => {
            return Err(unsupported("only Type=instant fleets are supported: keeping a fleet's capacity needs a loop that relaunches instances (use an instance group)"))
        }
        Some(_) => return Err(bad("InvalidParameterValue", "Type must be instant, request or maintain")),
    }
    for k in p.keys() {
        let k = k.as_str();
        let refused = k.starts_with("ValidFrom")
            || k.starts_with("ValidUntil")
            || k == "ReplaceUnhealthyInstances"
            || k == "TerminateInstancesWithExpiration"
            || k == "ExcessCapacityTerminationPolicy"
            || k.starts_with("Context")
            || k.starts_with("ReservedCapacityOptions")
            || k.starts_with("TargetCapacitySpecification.TargetCapacityUnitType")
            || (k.starts_with("SpotOptions.") && k != "SpotOptions.AllocationStrategy")
            || (k.starts_with("OnDemandOptions.") && k != "OnDemandOptions.AllocationStrategy");
        if refused {
            return Err(unsupported(format!("{k} is not supported")));
        }
    }
    let total: u32 = p
        .get("TargetCapacitySpecification.TotalTargetCapacity")
        .ok_or_else(|| bad("MissingParameter", "TargetCapacitySpecification.TotalTargetCapacity is required"))?
        .parse()
        .ok()
        .filter(|n| (1..=20).contains(n))
        .ok_or_else(|| bad("InvalidParameterValue", "TotalTargetCapacity must be between 1 and 20"))?;
    let default_spot = match p.get("TargetCapacitySpecification.DefaultTargetCapacityType").map(String::as_str) {
        None | Some("on-demand") => false,
        Some("spot") => true,
        Some(_) => return Err(bad("InvalidParameterValue", "DefaultTargetCapacityType must be on-demand or spot")),
    };
    let num = |k: &str| -> Result<Option<u32>, Ec2Error> {
        p.get(k).map(|v| v.parse::<u32>().map_err(|_| bad("InvalidParameterValue", format!("{k} must be a number")))).transpose()
    };
    let (spot_n, od_n) = match (num("TargetCapacitySpecification.SpotTargetCapacity")?, num("TargetCapacitySpecification.OnDemandTargetCapacity")?) {
        (None, None) => if default_spot { (total, 0) } else { (0, total) },
        (s, o) => (s.unwrap_or(0), o.unwrap_or(0)),
    };
    if spot_n + od_n != total {
        return Err(bad("InvalidParameterValue", "SpotTargetCapacity + OnDemandTargetCapacity must equal TotalTargetCapacity"));
    }
    let configs = (1..=10).filter(|n| p.keys().any(|k| k.starts_with(&format!("LaunchTemplateConfigs.{n}.")))).count();
    if configs != 1 {
        return Err(if configs == 0 {
            bad("MissingParameter", "LaunchTemplateConfigs.1.LaunchTemplateSpecification is required")
        } else {
            unsupported("only one LaunchTemplateConfigs entry is supported: list the alternatives as Overrides of it")
        });
    }
    // overrides: instance type, subnet, zone only
    let mut overrides: Vec<(Option<String>, Option<String>, Option<String>)> = Vec::new();
    for n in 1..=50 {
        let pre = format!("LaunchTemplateConfigs.1.Overrides.{n}.");
        if !p.keys().any(|k| k.starts_with(&pre)) {
            break;
        }
        for k in p.keys().filter(|k| k.starts_with(&pre)) {
            let field = &k[pre.len()..];
            match field {
                "InstanceType" | "SubnetId" | "AvailabilityZone" => {}
                "MaxPrice" => check_price("Overrides.MaxPrice", &p[k])?,
                other => return Err(unsupported(format!("Overrides.{other} is not supported (instance type, subnet, zone and max price are)"))),
            }
        }
        overrides.push((p.get(&format!("{pre}InstanceType")).cloned(), p.get(&format!("{pre}SubnetId")).cloned(), p.get(&format!("{pre}AvailabilityZone")).cloned()));
    }
    if overrides.is_empty() {
        overrides.push((None, None, None));
    }
    let mut launches = Vec::new();
    for (spot, n) in [(true, spot_n), (false, od_n)] {
        for (count, (instance_type, subnet, zone)) in distribute(n, overrides.len()).into_iter().zip(&overrides) {
            if count > 0 {
                launches.push(FleetLaunch { count, spot, instance_type: instance_type.clone(), subnet: subnet.clone(), zone: zone.clone() });
            }
        }
    }
    Ok(FleetPlan { total, default_spot, launches })
}

/// The `RunInstances` parameters of one fleet launch.
pub fn launch_params(p: &Params, l: &FleetLaunch) -> Params {
    let mut q = Params::new();
    for k in ["LaunchTemplateId", "LaunchTemplateName", "Version"] {
        if let Some(v) = p.get(&format!("LaunchTemplateConfigs.1.LaunchTemplateSpecification.{k}")) {
            q.insert(format!("LaunchTemplate.{k}"), v.clone());
        }
    }
    q.insert("MinCount".into(), l.count.to_string());
    q.insert("MaxCount".into(), l.count.to_string());
    if let Some(t) = &l.instance_type {
        q.insert("InstanceType".into(), t.clone());
    }
    if let Some(s) = &l.subnet {
        q.insert("SubnetId".into(), s.clone());
    }
    if let Some(z) = &l.zone {
        q.insert("Placement.AvailabilityZone".into(), z.clone());
    }
    if l.spot {
        q.insert("InstanceMarketOptions.MarketType".into(), "spot".into());
    }
    // tags for the instances come with the request (`TagSpecification` of type instance)
    for (k, v) in p {
        if k.starts_with("TagSpecification.") {
            q.insert(k.clone(), v.clone());
        }
    }
    q
}

fn spec_xml(p: &Params, l: &FleetLaunch) -> String {
    let (kind, v) = (if p.contains_key("LaunchTemplateConfigs.1.LaunchTemplateSpecification.LaunchTemplateId") { "launchTemplateId" } else { "launchTemplateName" }, {
        p.get("LaunchTemplateConfigs.1.LaunchTemplateSpecification.LaunchTemplateId").or_else(|| p.get("LaunchTemplateConfigs.1.LaunchTemplateSpecification.LaunchTemplateName")).cloned().unwrap_or_default()
    });
    let version = p.get("LaunchTemplateConfigs.1.LaunchTemplateSpecification.Version").cloned().unwrap_or_else(|| "$Default".into());
    let itype = l.instance_type.as_deref().map(|t| format!("<instanceType>{}</instanceType>", xml_escape(t))).unwrap_or_default();
    format!(
        "<launchTemplateAndOverrides><launchTemplateSpecification><{kind}>{}</{kind}><version>{}</version></launchTemplateSpecification><overrides>{itype}</overrides></launchTemplateAndOverrides>",
        xml_escape(&v),
        xml_escape(&version)
    )
}

pub async fn create_fleet(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let plan = plan_fleet(p)?;
    tagspec::only_types(p, &["fleet", "instance", "volume"])?;
    // resolve the template once so a bad id fails before anything launches
    let template = Params::from_iter(
        ["LaunchTemplateId", "LaunchTemplateName"]
            .iter()
            .filter_map(|k| p.get(&format!("LaunchTemplateConfigs.1.LaunchTemplateSpecification.{k}")).map(|v| (k.to_string(), v.clone()))),
    );
    let t = super::launch_templates::find(state, &template).await?;
    let fleet = Uuid::new_v4();
    crate::db::query("INSERT INTO ec2_fleets (id, fleet_type, state, target_capacity, spot, template_id, template_version, created_by) VALUES (?, 'instant', 'active', ?, ?, ?, ?, ?)")
        .bind(fleet)
        .bind(plan.total as i64)
        .bind(plan.default_spot)
        .bind(t.id)
        .bind(p.get("LaunchTemplateConfigs.1.LaunchTemplateSpecification.Version").cloned().unwrap_or_else(|| "$Default".into()))
        .bind(&actor.username)
        .execute(&state.pool)
        .await?;
    tagspec::apply(state, Kind::Fleet, fleet, &tagspec::tags_for(p, "fleet")).await?;
    let mut instances = String::new();
    let mut errors = String::new();
    for l in &plan.launches {
        match super::launch(state, actor, &launch_params(p, l)).await {
            Ok(ids) => {
                for id in &ids {
                    crate::db::query("INSERT INTO ec2_fleet_instances (fleet_id, vm_id) VALUES (?, ?)").bind(fleet).bind(id).execute(&state.pool).await?;
                }
                let list: String = ids.iter().map(|i| format!("<item>{}</item>", ec2_id(Kind::Vm, *i))).collect();
                instances.push_str(&format!(
                    "<item>{}<lifecycle>{}</lifecycle><instanceIds>{list}</instanceIds>{}</item>",
                    spec_xml(p, l),
                    if l.spot { "spot" } else { "on-demand" },
                    l.instance_type.as_deref().map(|t| format!("<instanceType>{}</instanceType>", xml_escape(t))).unwrap_or_default()
                ));
            }
            Err(e) => errors.push_str(&format!(
                "<item>{}<lifecycle>{}</lifecycle><errorCode>{}</errorCode><errorMessage>{}</errorMessage></item>",
                spec_xml(p, l),
                if l.spot { "spot" } else { "on-demand" },
                e.code,
                xml_escape(&e.message)
            )),
        }
    }
    Ok(format!("<fleetId>{}</fleetId><errorSet>{errors}</errorSet><fleetInstanceSet>{instances}</fleetInstanceSet>", ec2_id(Kind::Fleet, fleet)))
}

type FleetRow = (Uuid, String, String, i64, bool, String);

pub async fn describe_fleets(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let wanted = indexed(p, "FleetId");
    let mut filters = Vec::new();
    for (name, values) in super::parse_filters(p) {
        if values.is_empty() || !matches!(name.as_str(), "fleet-state" | "type") {
            return Err(bad("InvalidParameterValue", format!("The filter '{name}' is not valid for DescribeFleets")));
        }
        filters.push((name, values));
    }
    let rows: Vec<FleetRow> = crate::db::query_as("SELECT id, fleet_type, state, target_capacity, spot, created_at FROM ec2_fleets ORDER BY created_at, id").fetch_all(&state.pool).await?;
    for w in &wanted {
        if !rows.iter().any(|r| &ec2_id(Kind::Fleet, r.0) == w) {
            return Err(bad("InvalidFleetId.NotFound", format!("The fleet id '{w}' does not exist")));
        }
    }
    let mut items = String::new();
    for (id, kind, st, target, spot, created) in rows {
        let eid = ec2_id(Kind::Fleet, id);
        if !wanted.is_empty() && !wanted.contains(&eid) {
            continue;
        }
        let fleet_state = if st == "deleted" { "deleted_running" } else { "active" };
        if !filters.iter().all(|(n, v)| super::foundation::any_match(v, if n == "type" { &kind } else { fleet_state })) {
            continue;
        }
        let fulfilled: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM ec2_fleet_instances f JOIN vms v ON v.id = f.vm_id WHERE f.fleet_id = ?").bind(id).fetch_one(&state.pool).await?;
        let tags = tagspec::tags_of(state, Kind::Fleet, id).await?;
        items.push_str(&format!(
            "<item><fleetId>{eid}</fleetId><fleetState>{fleet_state}</fleetState><createTime>{}</createTime><type>{kind}</type><targetCapacitySpecification><totalTargetCapacity>{target}</totalTargetCapacity><defaultTargetCapacityType>{}</defaultTargetCapacityType></targetCapacitySpecification><fulfilledCapacity>{fulfilled}</fulfilledCapacity>{}</item>",
            xml_escape(&created),
            if spot { "spot" } else { "on-demand" },
            tagspec::tag_set_xml(&tags)
        ));
    }
    Ok(format!("<fleetSet>{items}</fleetSet>"))
}

pub async fn describe_fleet_instances(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let eid = p.get("FleetId").cloned().ok_or_else(|| bad("MissingParameter", "The request must contain the parameter FleetId"))?;
    let id = resolve(state, Kind::Fleet, &eid, "InvalidFleetId.NotFound").await?;
    let rows: Vec<(Uuid, Option<String>)> = crate::db::query_as(
        "SELECT f.vm_id, fl.name FROM ec2_fleet_instances f JOIN vms v ON v.id = f.vm_id LEFT JOIN flavors fl ON fl.id = v.flavor_id WHERE f.fleet_id = ? ORDER BY v.name",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let items: String = rows
        .iter()
        .map(|(vm, t)| {
            format!(
                "<item><instanceId>{}</instanceId><instanceType>{}</instanceType><instanceHealth>healthy</instanceHealth></item>",
                ec2_id(Kind::Vm, *vm),
                xml_escape(t.as_deref().unwrap_or(""))
            )
        })
        .collect();
    Ok(format!("<fleetId>{eid}</fleetId><activeInstanceSet>{items}</activeInstanceSet>"))
}

pub async fn delete_fleets(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let wanted = indexed(p, "FleetId");
    if wanted.is_empty() {
        return Err(bad("MissingParameter", "The request must contain the parameter FleetId"));
    }
    let terminate = match p.get("TerminateInstances").map(String::as_str) {
        Some("true") => true,
        Some("false") => false,
        _ => return Err(bad("MissingParameter", "TerminateInstances (true or false) is required")),
    };
    let mut ids = Vec::new();
    for w in &wanted {
        ids.push((w.clone(), resolve(state, Kind::Fleet, w, "InvalidFleetId.NotFound").await?));
    }
    let (mut ok, mut failed) = (String::new(), String::new());
    for (eid, id) in ids {
        let previous: String = crate::db::query_scalar("SELECT state FROM ec2_fleets WHERE id = ?").bind(id).fetch_one(&state.pool).await?;
        let mut error: Option<(String, String)> = None;
        if terminate {
            let members: Vec<Uuid> = crate::db::query_scalar("SELECT vm_id FROM ec2_fleet_instances WHERE fleet_id = ?").bind(id).fetch_all(&state.pool).await?;
            for vm in members {
                if let Err(e) = super::instance_attrs::terminate_guard(state, vm, &ec2_id(Kind::Vm, vm)).await {
                    error = Some((e.code.to_string(), e.message));
                    break;
                }
                // already-deleted members are fine: the fleet only remembers them
                let _ = crate::api::vms::delete_vm(State(state.clone()), Extension(actor.clone()), Path(vm), None).await;
            }
        }
        match error {
            Some((code, message)) => failed.push_str(&format!("<item><fleetId>{eid}</fleetId><error><code>{code}</code><message>{}</message></error></item>", xml_escape(&message))),
            None => {
                crate::db::query("UPDATE ec2_fleets SET state = 'deleted' WHERE id = ?").bind(id).execute(&state.pool).await?;
                ok.push_str(&format!(
                    "<item><currentFleetState>{}</currentFleetState><previousFleetState>{}</previousFleetState><fleetId>{eid}</fleetId></item>",
                    if terminate { "deleted_terminating" } else { "deleted_running" },
                    if previous == "deleted" { "deleted_running" } else { "active" }
                ));
            }
        }
    }
    Ok(format!("<successfulFleetDeletionSet>{ok}</successfulFleetDeletionSet><unsuccessfulFleetDeletionSet>{failed}</unsuccessfulFleetDeletionSet>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pairs: &[(&str, &str)]) -> Params {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn a_spot_request_becomes_a_spot_run() {
        let q = spot_to_run(&p(&[
            ("InstanceCount", "3"),
            ("SpotPrice", "0.05"),
            ("LaunchSpecification.ImageId", "ami-1"),
            ("LaunchSpecification.InstanceType", "m1.small"),
            ("LaunchSpecification.SecurityGroupId.1", "sg-1"),
            ("TagSpecification.1.ResourceType", "instance"),
            ("TagSpecification.1.Tag.1.Key", "a"),
        ]))
        .unwrap();
        assert_eq!(q.get("ImageId").map(String::as_str), Some("ami-1"));
        assert_eq!((q.get("MinCount").map(String::as_str), q.get("MaxCount").map(String::as_str)), (Some("3"), Some("3")));
        assert_eq!(q.get("InstanceMarketOptions.MarketType").map(String::as_str), Some("spot"));
        assert_eq!(q.get("SecurityGroupId.1").map(String::as_str), Some("sg-1"));
        assert!(q.contains_key("TagSpecification.1.ResourceType"));
        assert!(!q.contains_key("SpotPrice"));
    }

    #[test]
    fn what_needs_a_market_is_refused() {
        let base = [("LaunchSpecification.ImageId", "ami-1")];
        let with = |k: &'static str, v: &'static str| {
            let mut pairs = base.to_vec();
            pairs.push((k, v));
            spot_to_run(&p(&pairs))
        };
        for (k, v) in [("Type", "persistent"), ("ValidUntil", "2030-01-01T00:00:00Z"), ("BlockDurationMinutes", "60"), ("LaunchGroup", "g"), ("InstanceInterruptionBehavior", "hibernate")] {
            assert_eq!(with(k, v).unwrap_err().code, "UnsupportedOperation", "{k}");
        }
        assert_eq!(with("SpotPrice", "-1").unwrap_err().code, "InvalidParameterValue");
        assert_eq!(with("InstanceCount", "99").unwrap_err().code, "InvalidParameterValue");
        assert_eq!(spot_to_run(&p(&[])).unwrap_err().code, "MissingParameter");
        assert!(with("Type", "one-time").is_ok());
    }

    #[test]
    fn request_state_follows_the_instance() {
        assert_eq!(derive_state("active", Some(false)).0, "active");
        assert_eq!(derive_state("active", Some(true)), ("closed", "instance-terminated-no-capacity", "The instance was stopped to make room for a regular instance"));
        assert_eq!(derive_state("active", None).0, "closed");
        assert_eq!(derive_state("cancelled", Some(false)).1, "request-canceled-and-instance-running");
        assert_eq!(derive_state("cancelled", None).0, "cancelled");
    }

    #[test]
    fn capacity_is_spread_evenly() {
        assert_eq!(distribute(5, 2), vec![3, 2]);
        assert_eq!(distribute(4, 4), vec![1, 1, 1, 1]);
        assert_eq!(distribute(1, 3), vec![1, 0, 0]);
        assert!(distribute(3, 0).is_empty());
    }

    fn fleet(extra: &[(&str, &str)]) -> Params {
        let mut v = vec![
            ("Type", "instant"),
            ("TargetCapacitySpecification.TotalTargetCapacity", "4"),
            ("LaunchTemplateConfigs.1.LaunchTemplateSpecification.LaunchTemplateId", "lt-1"),
            ("LaunchTemplateConfigs.1.LaunchTemplateSpecification.Version", "$Latest"),
        ];
        v.extend_from_slice(extra);
        p(&v)
    }

    #[test]
    fn an_instant_fleet_is_planned_over_its_overrides() {
        let plan = plan_fleet(&fleet(&[
            ("TargetCapacitySpecification.DefaultTargetCapacityType", "spot"),
            ("LaunchTemplateConfigs.1.Overrides.1.InstanceType", "m1.small"),
            ("LaunchTemplateConfigs.1.Overrides.2.InstanceType", "m1.large"),
            ("LaunchTemplateConfigs.1.Overrides.2.MaxPrice", "0.5"),
        ]))
        .unwrap();
        assert_eq!(plan.total, 4);
        assert_eq!(plan.launches.len(), 2);
        assert!(plan.launches.iter().all(|l| l.spot && l.count == 2));
        assert_eq!(plan.launches[1].instance_type.as_deref(), Some("m1.large"));
        // no overrides: one launch using the template as is
        let one = plan_fleet(&fleet(&[])).unwrap();
        assert_eq!(one.launches, vec![FleetLaunch { count: 4, spot: false, instance_type: None, subnet: None, zone: None }]);
        // a split between spot and on demand
        let split = plan_fleet(&fleet(&[("TargetCapacitySpecification.SpotTargetCapacity", "1"), ("TargetCapacitySpecification.OnDemandTargetCapacity", "3")])).unwrap();
        assert_eq!(split.launches.iter().map(|l| (l.spot, l.count)).collect::<Vec<_>>(), vec![(true, 1), (false, 3)]);
    }

    #[test]
    fn fleets_that_would_need_a_capacity_loop_are_refused() {
        let mut maintain = fleet(&[]);
        maintain.insert("Type".into(), "maintain".into());
        assert_eq!(plan_fleet(&maintain).unwrap_err().code, "UnsupportedOperation");
        let mut none = fleet(&[]);
        none.remove("Type");
        assert_eq!(plan_fleet(&none).unwrap_err().code, "UnsupportedOperation");
        for (k, v) in [
            ("ReplaceUnhealthyInstances", "true"),
            ("ValidUntil", "2030-01-01T00:00:00Z"),
            ("SpotOptions.MaxTotalPrice", "5"),
            ("LaunchTemplateConfigs.1.Overrides.1.WeightedCapacity", "2"),
            ("LaunchTemplateConfigs.2.LaunchTemplateSpecification.LaunchTemplateId", "lt-2"),
        ] {
            assert_eq!(plan_fleet(&fleet(&[(k, v)])).unwrap_err().code, "UnsupportedOperation", "{k}");
        }
        assert!(plan_fleet(&fleet(&[("SpotOptions.AllocationStrategy", "lowest-price")])).is_ok());
        assert_eq!(plan_fleet(&fleet(&[("TargetCapacitySpecification.SpotTargetCapacity", "1")])).unwrap_err().code, "InvalidParameterValue");
        let mut big = fleet(&[]);
        big.insert("TargetCapacitySpecification.TotalTargetCapacity".into(), "50".into());
        assert_eq!(plan_fleet(&big).unwrap_err().code, "InvalidParameterValue");
    }

    #[test]
    fn a_fleet_launch_carries_the_template_override_and_market() {
        let params = fleet(&[("TagSpecification.1.ResourceType", "instance"), ("TagSpecification.1.Tag.1.Key", "k")]);
        let l = FleetLaunch { count: 2, spot: true, instance_type: Some("m1.large".into()), subnet: Some("subnet-1".into()), zone: Some("host-a".into()) };
        let q = launch_params(&params, &l);
        assert_eq!(q.get("LaunchTemplate.LaunchTemplateId").map(String::as_str), Some("lt-1"));
        assert_eq!(q.get("LaunchTemplate.Version").map(String::as_str), Some("$Latest"));
        assert_eq!((q.get("InstanceType").map(String::as_str), q.get("SubnetId").map(String::as_str), q.get("Placement.AvailabilityZone").map(String::as_str)), (Some("m1.large"), Some("subnet-1"), Some("host-a")));
        assert_eq!(q.get("InstanceMarketOptions.MarketType").map(String::as_str), Some("spot"));
        assert_eq!(q.get("MaxCount").map(String::as_str), Some("2"));
        assert!(q.contains_key("TagSpecification.1.Tag.1.Key"));
    }

    #[tokio::test]
    async fn requests_are_listed_filtered_and_cancelled() {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        let admin = AuthUser { username: "u".into(), role: "admin".into(), auth_source: None };
        let id = Uuid::new_v4();
        crate::db::query("INSERT INTO ec2_spot_requests (id, state, spot_price, instance_type, created_by) VALUES (?, 'active', '0.05', 'm1.small', 'u')").bind(id).execute(&state.pool).await.unwrap();
        let eid = ec2_id(Kind::SpotRequest, id);
        // its instance is gone, so the request reads as closed
        let x = describe_spot_instance_requests(&state, &p(&[("SpotInstanceRequestId.1", &eid)])).await.unwrap();
        assert!(x.contains("<state>closed</state>") && x.contains("<spotPrice>0.05</spotPrice>"), "{x}");
        let active = describe_spot_instance_requests(&state, &p(&[("Filter.1.Name", "state"), ("Filter.1.Value.1", "active")])).await.unwrap();
        assert!(!active.contains("<item>"), "{active}");
        assert_eq!(describe_spot_instance_requests(&state, &p(&[("Filter.1.Name", "price"), ("Filter.1.Value.1", "1")])).await.unwrap_err().code, "InvalidParameterValue");
        assert_eq!(describe_spot_instance_requests(&state, &p(&[("SpotInstanceRequestId.1", "sir-00000000000000000")])).await.unwrap_err().code, "InvalidSpotInstanceRequestID.NotFound");
        let c = cancel_spot_instance_requests(&state, &admin, &p(&[("SpotInstanceRequestId.1", &eid)])).await.unwrap();
        assert!(c.contains("<state>cancelled</state>"));
        let after = describe_spot_instance_requests(&state, &p(&[("SpotInstanceRequestId.1", &eid)])).await.unwrap();
        assert!(after.contains("<state>cancelled</state>"), "{after}");
    }

    #[tokio::test]
    async fn the_price_history_is_flat_and_says_so() {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        crate::db::query("INSERT INTO flavors (id, name, vcpus, memory_mib, disk_gib) VALUES (?, 'm1.big', 4, 8192, 20)").bind(Uuid::new_v4()).execute(&state.pool).await.unwrap();
        let x = describe_spot_price_history(&state, &p(&[("InstanceType.1", "m1.big")])).await.unwrap();
        assert!(x.contains("<spotPrice>0.020000</spotPrice>") && x.contains("there is no spot market"), "{x}");
        assert_eq!(describe_spot_price_history(&state, &p(&[("ProductDescription.1", "Windows")])).await.unwrap(), "<spotPriceHistorySet/>");
    }

    #[tokio::test]
    async fn fleets_are_described_and_deleted() {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        let admin = AuthUser { username: "u".into(), role: "admin".into(), auth_source: None };
        let id = Uuid::new_v4();
        crate::db::query("INSERT INTO ec2_fleets (id, fleet_type, state, target_capacity, spot, created_by) VALUES (?, 'instant', 'active', 3, TRUE, 'u')").bind(id).execute(&state.pool).await.unwrap();
        let eid = ec2_id(Kind::Fleet, id);
        let x = describe_fleets(&state, &p(&[("FleetId.1", &eid)])).await.unwrap();
        assert!(x.contains("<fleetState>active</fleetState>") && x.contains("<totalTargetCapacity>3</totalTargetCapacity>") && x.contains("<defaultTargetCapacityType>spot</defaultTargetCapacityType>"), "{x}");
        assert!(describe_fleet_instances(&state, &p(&[("FleetId", &eid)])).await.unwrap().contains("<activeInstanceSet></activeInstanceSet>"));
        assert_eq!(delete_fleets(&state, &admin, &p(&[("FleetId.1", &eid)])).await.unwrap_err().code, "MissingParameter");
        let d = delete_fleets(&state, &admin, &p(&[("FleetId.1", &eid), ("TerminateInstances", "false")])).await.unwrap();
        assert!(d.contains("<currentFleetState>deleted_running</currentFleetState>") && d.contains("<previousFleetState>active</previousFleetState>"), "{d}");
        let after = describe_fleets(&state, &p(&[("Filter.1.Name", "fleet-state"), ("Filter.1.Value.1", "active")])).await.unwrap();
        assert!(!after.contains("<item>"), "{after}");
    }
}
